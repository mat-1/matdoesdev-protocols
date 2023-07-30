use std::io::{Cursor, Read, Write};

use anyhow::bail;
use byteorder::{ReadBytesExt, WriteBytesExt, BE};

#[derive(Debug)]
pub struct Packet {
    pub message: Message,
    pub mac: Vec<u8>,
}

#[derive(Debug)]
#[repr(u8)]
pub enum Message {
    Disconnect {
        reason_code: u32,
        description: String,
        language_tag: String,
    } = 1,
    KexInit {
        cookie: [u8; 16],
        kex_algorithms: Vec<String>,
        server_host_key_algorithms: Vec<String>,
        encryption_algorithms_client_to_server: Vec<String>,
        encryption_algorithms_server_to_client: Vec<String>,
        mac_algorithms_client_to_server: Vec<String>,
        mac_algorithms_server_to_client: Vec<String>,
        compression_algorithms_client_to_server: Vec<String>,
        compression_algorithms_server_to_client: Vec<String>,
        languages_client_to_server: Vec<String>,
        languages_server_to_client: Vec<String>,
        first_kex_packet_follows: bool,
        reserved: u32,
    } = 20,
    NewKeys = 21,
    KexEcdhInit {
        /// Q_C, client's ephemeral public key octet string
        client_public_key: Vec<u8>,
    } = 30,
    KexEcdhReply {
        /// K_S, server's public host key
        server_public_host_key: Vec<u8>,
        /// Q_S, server's ephemeral public key octet string
        server_public_key: Vec<u8>,
        /// the signature on the exchange hash
        signature: Vec<u8>,
    } = 31,
}

pub fn read_payload(mut data: Cursor<Vec<u8>>) -> anyhow::Result<Vec<u8>> {
    let packet_length = data.read_u32::<BE>()? as usize;
    let padding_length = data.read_u8()? as usize;

    let payload_length = packet_length - padding_length - 1;
    let mut payload = Vec::new();
    for _ in 0..payload_length {
        payload.push(data.read_u8()?);
    }
    let mut padding = vec![0; padding_length];
    data.read_exact(&mut padding)?;

    Ok(payload)
}

pub fn write_payload(payload: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let mut data = Vec::new();

    // must be mod 8 and at least 4
    let mut padding_length = 8 - (payload.len() + 5) % 8;
    if padding_length < 4 {
        padding_length += 8;
    }

    let packet_length = payload.len() + padding_length + 1;
    data.write_u32::<BE>(packet_length as u32)?;
    data.write_u8(padding_length as u8)?;
    data.write_all(&payload)?;
    data.write_all(&vec![0; padding_length])?;

    Ok(data)
}

pub fn read_packet(data: Cursor<Vec<u8>>) -> anyhow::Result<Packet> {
    let mac = data.get_ref()[data.position() as usize..].to_vec();

    let payload = read_payload(data)?;
    println!("payload: {:?}", String::from_utf8_lossy(&payload));

    let message = read_message(Cursor::new(payload))?;

    Ok(Packet { message, mac })
}

pub fn write_packet(packet: Packet) -> anyhow::Result<Vec<u8>> {
    println!("writing packet: {:?}", packet);
    let payload = write_message(packet.message)?;
    write_payload(payload)
}

pub fn read_message(mut data: impl Read) -> anyhow::Result<Message> {
    let message_type = data.read_u8()?;
    match message_type {
        1 => {
            let reason_code = data.read_u32::<BE>()?;
            let description = read_string(&mut data)?;
            let language_tag = read_string(&mut data)?;
            Ok(Message::Disconnect {
                reason_code,
                description,
                language_tag,
            })
        }
        20 => {
            let cookie = {
                let mut cookie = [0; 16];
                data.read_exact(&mut cookie)?;
                cookie
            };
            let kex_algorithms = read_name_list(&mut data)?;
            let server_host_key_algorithms = read_name_list(&mut data)?;
            let encryption_algorithms_client_to_server = read_name_list(&mut data)?;
            let encryption_algorithms_server_to_client = read_name_list(&mut data)?;
            let mac_algorithms_client_to_server = read_name_list(&mut data)?;
            let mac_algorithms_server_to_client = read_name_list(&mut data)?;
            let compression_algorithms_client_to_server = read_name_list(&mut data)?;
            let compression_algorithms_server_to_client = read_name_list(&mut data)?;
            let languages_client_to_server = read_name_list(&mut data)?;
            let languages_server_to_client = read_name_list(&mut data)?;
            let first_kex_packet_follows = data.read_u8()? != 0;
            let reserved = data.read_u32::<BE>()?;

            Ok(Message::KexInit {
                cookie,
                kex_algorithms,
                server_host_key_algorithms,
                encryption_algorithms_client_to_server,
                encryption_algorithms_server_to_client,
                mac_algorithms_client_to_server,
                mac_algorithms_server_to_client,
                compression_algorithms_client_to_server,
                compression_algorithms_server_to_client,
                languages_client_to_server,
                languages_server_to_client,
                first_kex_packet_follows,
                reserved,
            })
        }
        21 => Ok(Message::NewKeys),
        30 => {
            let client_public_key = read_bytes(&mut data)?;
            Ok(Message::KexEcdhInit { client_public_key })
        }
        31 => {
            let server_public_host_key = read_bytes(&mut data)?;
            let server_public_key = read_bytes(&mut data)?;
            let signature = read_bytes(&mut data)?;
            Ok(Message::KexEcdhReply {
                server_public_host_key,
                server_public_key,
                signature,
            })
        }
        _ => bail!("unknown message type: {message_type} (0x{message_type:02x})"),
    }
}

pub fn write_message(message: Message) -> anyhow::Result<Vec<u8>> {
    let mut data = Vec::new();
    match message {
        Message::Disconnect {
            reason_code,
            description,
            language_tag,
        } => {
            data.write_u8(1)?;
            data.write_u32::<BE>(reason_code)?;
            write_string(&mut data, &description)?;
            write_string(&mut data, &language_tag)?;
        }
        Message::KexInit {
            cookie,
            kex_algorithms,
            server_host_key_algorithms,
            encryption_algorithms_client_to_server,
            encryption_algorithms_server_to_client,
            mac_algorithms_client_to_server,
            mac_algorithms_server_to_client,
            compression_algorithms_client_to_server,
            compression_algorithms_server_to_client,
            languages_client_to_server,
            languages_server_to_client,
            first_kex_packet_follows,
            reserved,
        } => {
            data.write_u8(20)?;
            data.write_all(&cookie)?;
            write_name_list(&mut data, &kex_algorithms)?;
            write_name_list(&mut data, &server_host_key_algorithms)?;
            write_name_list(&mut data, &encryption_algorithms_client_to_server)?;
            write_name_list(&mut data, &encryption_algorithms_server_to_client)?;
            write_name_list(&mut data, &mac_algorithms_client_to_server)?;
            write_name_list(&mut data, &mac_algorithms_server_to_client)?;
            write_name_list(&mut data, &compression_algorithms_client_to_server)?;
            write_name_list(&mut data, &compression_algorithms_server_to_client)?;
            write_name_list(&mut data, &languages_client_to_server)?;
            write_name_list(&mut data, &languages_server_to_client)?;
            data.write_u8(if first_kex_packet_follows { 1 } else { 0 })?;
            data.write_u32::<BE>(reserved)?;
        }
        Message::NewKeys => {
            data.write_u8(21)?;
        }
        Message::KexEcdhInit { client_public_key } => {
            data.write_u8(30)?;
            write_bytes(&mut data, &client_public_key)?;
        }
        Message::KexEcdhReply {
            server_public_host_key,
            server_public_key,
            signature,
        } => {
            data.write_u8(31)?;
            write_bytes(&mut data, &server_public_host_key)?;
            write_bytes(&mut data, &server_public_key)?;
            write_bytes(&mut data, &signature)?;
        }
    }
    Ok(data)
}

pub fn read_bytes(mut data: impl Read) -> anyhow::Result<Vec<u8>> {
    let length = data.read_u32::<BE>()?;
    let mut bytes = Vec::new();
    for _ in 0..length {
        bytes.push(data.read_u8()?);
    }
    Ok(bytes)
}

pub fn write_bytes(data: &mut Vec<u8>, bytes: &[u8]) -> anyhow::Result<()> {
    data.write_u32::<BE>(bytes.len() as u32)?;
    data.write_all(bytes)?;
    Ok(())
}

pub fn read_string(mut data: impl Read) -> anyhow::Result<String> {
    let bytes = read_bytes(&mut data)?;
    Ok(String::from_utf8(bytes)?)
}

pub fn write_string(data: &mut Vec<u8>, string: &str) -> anyhow::Result<()> {
    write_bytes(data, string.as_bytes())
}

pub fn read_name_list(mut data: impl Read) -> anyhow::Result<Vec<String>> {
    let string = read_string(&mut data)?;
    if string.is_empty() {
        return Ok(vec![]);
    }
    Ok(string.split(',').map(|s| s.to_string()).collect())
}

pub fn write_name_list(data: &mut Vec<u8>, name_list: &[String]) -> anyhow::Result<()> {
    let string = name_list.join(",");
    write_string(data, &string)?;
    Ok(())
}

pub fn write_mpint(data: &mut Vec<u8>, s: &[u8]) -> anyhow::Result<()> {
    // Skip initial 0s.
    let mut i = 0;
    while i < s.len() && s[i] == 0 {
        i += 1
    }
    // If the first non-zero is >= 128, write its length (u32, BE), followed by 0.
    if s[i] & 0x80 != 0 {
        data.write_u32::<BE>((s.len() - i + 1) as u32)?;
        data.write_u8(0)?;
    } else {
        data.write_u32::<BE>((s.len() - i) as u32)?;
    }
    data.write_all(&s[i..])?;

    Ok(())
}
