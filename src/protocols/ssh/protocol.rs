use std::io::{Cursor, Read, Write};

use aes::{cipher::StreamCipher, Aes128};
use anyhow::bail;
use byteorder::{ReadBytesExt, WriteBytesExt, BE};
use ctr::Ctr128BE;

#[derive(Debug)]
#[repr(u8)]
pub enum Message {
    Disconnect {
        reason_code: u32,
        description: String,
        language_tag: String,
    } = 1,
    Ignore {
        data: Vec<u8>,
    } = 2,
    Unimplemented {
        packet_sequence_number: u32,
    } = 3,
    Debug {
        always_display: bool,
        message: String,
        language_tag: String,
    } = 4,
    ServiceRequest {
        service_name: String,
    } = 5,
    ServiceAccept {
        service_name: String,
    } = 6,
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
    UserauthRequest {
        username: String,
        service_name: String,
        authentication_method: String,
        // depends
    } = 50,
    UserauthFailure {
        authentication_methods: Vec<String>,
        partial_success: bool,
    } = 51,
    UserauthSuccess = 52,
    UserauthBanner {
        message: String,
        language_tag: String,
    } = 53,

    GlobalRequest {
        request_name: String,
        want_reply: bool,
        // depends
    } = 80,
    RequestSuccess {
        // depends
    } = 81,
    RequestFailure = 82,
    ChannelOpen {
        channel_type: String,
        sender_channel: u32,
        initial_window_size: u32,
        maximum_packet_size: u32,
        // depends
    } = 90,
    ChannelOpenConfirmation {
        recipient_channel: u32,
        sender_channel: u32,
        initial_window_size: u32,
        maximum_packet_size: u32,
        // depends
    } = 91,
    ChannelOpenFailure {
        recipient_channel: u32,
        reason_code: u32,
        description: String,
        language_tag: String,
    } = 92,
    ChannelWindowAdjust {
        recipient_channel: u32,
        bytes_to_add: u32,
    } = 93,
    ChannelData {
        recipient_channel: u32,
        data: Vec<u8>,
    } = 94,
    ChannelExtendedData {
        recipient_channel: u32,
        data_type_code: u32,
        data: Vec<u8>,
    } = 95,
    ChannelEof {
        recipient_channel: u32,
    } = 96,
    ChannelClose {
        recipient_channel: u32,
    } = 97,
    ChannelRequest {
        recipient_channel: u32,
        request_type: String,
        want_reply: bool,
        extra: ChannelRequestExtra,
    } = 98,
    ChannelSuccess {
        recipient_channel: u32,
    } = 99,
    ChannelFailure {
        recipient_channel: u32,
    } = 100,
}

#[derive(Debug)]
pub enum ChannelRequestExtra {
    Terminal {
        terminal_type: String,
        width_columns: u32,
        height_rows: u32,
        width_pixels: u32,
        height_pixels: u32,
        terminal_modes: Vec<u8>,
    },
    WindowChange {
        width_columns: u32,
        height_rows: u32,
        width_pixels: u32,
        height_pixels: u32,
    },
    None,
}

pub fn write_payload(
    payload: Vec<u8>,
    cipher_block_key_size: Option<usize>,
) -> anyhow::Result<Vec<u8>> {
    let mut data = Vec::new();

    let multiple_of = cipher_block_key_size.unwrap_or_default().max(8);

    // must be mod 8 and at least 4
    let mut padding_length = multiple_of - (payload.len() + 5) % multiple_of;
    if padding_length < 4 {
        padding_length += multiple_of;
    }

    let packet_length = payload.len() + padding_length + 1;
    data.write_u32::<BE>(packet_length as u32)?;
    data.write_u8(padding_length as u8)?;
    data.write_all(&payload)?;
    data.write_all(&vec![0; padding_length])?;

    Ok(data)
}

pub fn write_packet(
    packet: Message,
    cipher_block_key_size: Option<usize>,
) -> anyhow::Result<Vec<u8>> {
    let payload = write_message(packet)?;
    write_payload(payload, cipher_block_key_size)
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
        2 => {
            let data = read_bytes(&mut data)?;
            Ok(Message::Ignore { data })
        }
        3 => {
            let packet_sequence_number = data.read_u32::<BE>()?;
            Ok(Message::Unimplemented {
                packet_sequence_number,
            })
        }
        4 => {
            let always_display = data.read_u8()? != 0;
            let message = read_string(&mut data)?;
            let language_tag = read_string(&mut data)?;
            Ok(Message::Debug {
                always_display,
                message,
                language_tag,
            })
        }
        5 => {
            let service_name = read_string(&mut data)?;
            Ok(Message::ServiceRequest { service_name })
        }
        6 => {
            let service_name = read_string(&mut data)?;
            Ok(Message::ServiceAccept { service_name })
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
        50 => {
            let username = read_string(&mut data)?;
            let service_name = read_string(&mut data)?;
            let authentication_method = read_string(&mut data)?;
            Ok(Message::UserauthRequest {
                username,
                service_name,
                authentication_method,
            })
        }
        51 => {
            let authentication_methods = read_name_list(&mut data)?;
            let partial_success = data.read_u8()? != 0;
            Ok(Message::UserauthFailure {
                authentication_methods,
                partial_success,
            })
        }
        52 => Ok(Message::UserauthSuccess),
        53 => {
            let message = read_string(&mut data)?;
            let language_tag = read_string(&mut data)?;
            Ok(Message::UserauthBanner {
                message,
                language_tag,
            })
        }
        80 => {
            let request_name = read_string(&mut data)?;
            let want_reply = data.read_u8()? != 0;
            Ok(Message::GlobalRequest {
                request_name,
                want_reply,
            })
        }
        81 => Ok(Message::RequestSuccess {}),
        82 => Ok(Message::RequestFailure),
        90 => {
            let channel_type = read_string(&mut data)?;
            let sender_channel = data.read_u32::<BE>()?;
            let initial_window_size = data.read_u32::<BE>()?;
            let max_packet_size = data.read_u32::<BE>()?;
            Ok(Message::ChannelOpen {
                channel_type,
                sender_channel,
                initial_window_size,
                maximum_packet_size: max_packet_size,
            })
        }
        91 => {
            let recipient_channel = data.read_u32::<BE>()?;
            let sender_channel = data.read_u32::<BE>()?;
            let initial_window_size = data.read_u32::<BE>()?;
            let max_packet_size = data.read_u32::<BE>()?;
            Ok(Message::ChannelOpenConfirmation {
                recipient_channel,
                sender_channel,
                initial_window_size,
                maximum_packet_size: max_packet_size,
            })
        }
        92 => {
            let recipient_channel = data.read_u32::<BE>()?;
            let reason_code = data.read_u32::<BE>()?;
            let description = read_string(&mut data)?;
            let language_tag = read_string(&mut data)?;
            Ok(Message::ChannelOpenFailure {
                recipient_channel,
                reason_code,
                description,
                language_tag,
            })
        }
        93 => {
            let recipient_channel = data.read_u32::<BE>()?;
            let bytes_to_add = data.read_u32::<BE>()?;
            Ok(Message::ChannelWindowAdjust {
                recipient_channel,
                bytes_to_add,
            })
        }
        94 => {
            let recipient_channel = data.read_u32::<BE>()?;
            let data = read_bytes(&mut data)?;
            Ok(Message::ChannelData {
                recipient_channel,
                data,
            })
        }
        95 => {
            let recipient_channel = data.read_u32::<BE>()?;
            let data_type_code = data.read_u32::<BE>()?;
            let data = read_bytes(&mut data)?;
            Ok(Message::ChannelExtendedData {
                recipient_channel,
                data_type_code,
                data,
            })
        }
        96 => {
            let recipient_channel = data.read_u32::<BE>()?;
            Ok(Message::ChannelEof { recipient_channel })
        }
        97 => {
            let recipient_channel = data.read_u32::<BE>()?;
            Ok(Message::ChannelClose { recipient_channel })
        }
        98 => {
            let recipient_channel = data.read_u32::<BE>()?;
            let request_type = read_string(&mut data)?;
            let want_reply = data.read_u8()? != 0;

            let extra = match request_type.as_str() {
                "pty-req" => ChannelRequestExtra::Terminal {
                    terminal_type: read_string(&mut data)?,
                    width_columns: data.read_u32::<BE>()?,
                    height_rows: data.read_u32::<BE>()?,
                    width_pixels: data.read_u32::<BE>()?,
                    height_pixels: data.read_u32::<BE>()?,
                    terminal_modes: read_bytes(&mut data)?,
                },
                "window-change" => ChannelRequestExtra::WindowChange {
                    width_columns: data.read_u32::<BE>()?,
                    height_rows: data.read_u32::<BE>()?,
                    width_pixels: data.read_u32::<BE>()?,
                    height_pixels: data.read_u32::<BE>()?,
                },
                _ => ChannelRequestExtra::None,
            };

            Ok(Message::ChannelRequest {
                recipient_channel,
                request_type,
                want_reply,
                extra,
            })
        }
        99 => {
            let recipient_channel = data.read_u32::<BE>()?;
            Ok(Message::ChannelSuccess { recipient_channel })
        }
        100 => {
            let recipient_channel = data.read_u32::<BE>()?;
            Ok(Message::ChannelFailure { recipient_channel })
        }
        _ => bail!("unknown message type: {message_type} (0x{message_type:02x})"),
    }
}

pub fn write_message(message: Message) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    match message {
        Message::Disconnect {
            reason_code,
            description,
            language_tag,
        } => {
            buf.write_u8(1)?;
            buf.write_u32::<BE>(reason_code)?;
            write_string(&mut buf, &description)?;
            write_string(&mut buf, &language_tag)?;
        }
        Message::Ignore { data } => {
            buf.write_u8(2)?;
            write_bytes(&mut buf, &data)?;
        }
        Message::Unimplemented {
            packet_sequence_number,
        } => {
            buf.write_u8(3)?;
            buf.write_u32::<BE>(packet_sequence_number)?;
        }
        Message::Debug {
            always_display,
            message,
            language_tag,
        } => {
            buf.write_u8(4)?;
            buf.write_u8(if always_display { 1 } else { 0 })?;
            write_string(&mut buf, &message)?;
            write_string(&mut buf, &language_tag)?;
        }
        Message::ServiceRequest { service_name } => {
            buf.write_u8(5)?;
            write_string(&mut buf, &service_name)?;
        }
        Message::ServiceAccept { service_name } => {
            buf.write_u8(6)?;
            write_string(&mut buf, &service_name)?;
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
            buf.write_u8(20)?;
            buf.write_all(&cookie)?;
            write_name_list(&mut buf, &kex_algorithms)?;
            write_name_list(&mut buf, &server_host_key_algorithms)?;
            write_name_list(&mut buf, &encryption_algorithms_client_to_server)?;
            write_name_list(&mut buf, &encryption_algorithms_server_to_client)?;
            write_name_list(&mut buf, &mac_algorithms_client_to_server)?;
            write_name_list(&mut buf, &mac_algorithms_server_to_client)?;
            write_name_list(&mut buf, &compression_algorithms_client_to_server)?;
            write_name_list(&mut buf, &compression_algorithms_server_to_client)?;
            write_name_list(&mut buf, &languages_client_to_server)?;
            write_name_list(&mut buf, &languages_server_to_client)?;
            buf.write_u8(if first_kex_packet_follows { 1 } else { 0 })?;
            buf.write_u32::<BE>(reserved)?;
        }
        Message::NewKeys => {
            buf.write_u8(21)?;
        }
        Message::KexEcdhInit { client_public_key } => {
            buf.write_u8(30)?;
            write_bytes(&mut buf, &client_public_key)?;
        }
        Message::KexEcdhReply {
            server_public_host_key,
            server_public_key,
            signature,
        } => {
            buf.write_u8(31)?;
            write_bytes(&mut buf, &server_public_host_key)?;
            write_bytes(&mut buf, &server_public_key)?;
            write_bytes(&mut buf, &signature)?;
        }
        Message::UserauthRequest {
            username,
            service_name,
            authentication_method,
        } => {
            buf.write_u8(50)?;
            write_string(&mut buf, &username)?;
            write_string(&mut buf, &service_name)?;
            write_string(&mut buf, &authentication_method)?;
        }
        Message::UserauthFailure {
            authentication_methods,
            partial_success,
        } => {
            buf.write_u8(51)?;
            write_name_list(&mut buf, &authentication_methods)?;
            buf.write_u8(if partial_success { 1 } else { 0 })?;
        }
        Message::UserauthSuccess => {
            buf.write_u8(52)?;
        }
        Message::UserauthBanner {
            message,
            language_tag,
        } => {
            buf.write_u8(53)?;
            write_string(&mut buf, &message)?;
            write_string(&mut buf, &language_tag)?;
        }
        Message::GlobalRequest {
            request_name,
            want_reply,
        } => {
            buf.write_u8(80)?;
            write_string(&mut buf, &request_name)?;
            buf.write_u8(if want_reply { 1 } else { 0 })?;
        }
        Message::RequestSuccess {} => {
            buf.write_u8(81)?;
        }
        Message::RequestFailure => {
            buf.write_u8(82)?;
        }
        Message::ChannelOpen {
            channel_type,
            sender_channel,
            initial_window_size,
            maximum_packet_size: max_packet_size,
        } => {
            buf.write_u8(90)?;
            write_string(&mut buf, &channel_type)?;
            buf.write_u32::<BE>(sender_channel)?;
            buf.write_u32::<BE>(initial_window_size)?;
            buf.write_u32::<BE>(max_packet_size)?;
        }
        Message::ChannelOpenConfirmation {
            recipient_channel,
            sender_channel,
            initial_window_size,
            maximum_packet_size: max_packet_size,
        } => {
            buf.write_u8(91)?;
            buf.write_u32::<BE>(recipient_channel)?;
            buf.write_u32::<BE>(sender_channel)?;
            buf.write_u32::<BE>(initial_window_size)?;
            buf.write_u32::<BE>(max_packet_size)?;
        }
        Message::ChannelOpenFailure {
            recipient_channel,
            reason_code,
            description,
            language_tag,
        } => {
            buf.write_u8(92)?;
            buf.write_u32::<BE>(recipient_channel)?;
            buf.write_u32::<BE>(reason_code)?;
            write_string(&mut buf, &description)?;
            write_string(&mut buf, &language_tag)?;
        }
        Message::ChannelWindowAdjust {
            recipient_channel,
            bytes_to_add,
        } => {
            buf.write_u8(93)?;
            buf.write_u32::<BE>(recipient_channel)?;
            buf.write_u32::<BE>(bytes_to_add)?;
        }
        Message::ChannelData {
            recipient_channel,
            data,
        } => {
            buf.write_u8(94)?;
            buf.write_u32::<BE>(recipient_channel)?;
            write_bytes(&mut buf, &data)?;
        }
        Message::ChannelExtendedData {
            recipient_channel,
            data_type_code,
            data,
        } => {
            buf.write_u8(95)?;
            buf.write_u32::<BE>(recipient_channel)?;
            buf.write_u32::<BE>(data_type_code)?;
            write_bytes(&mut buf, &data)?;
        }
        Message::ChannelEof { recipient_channel } => {
            buf.write_u8(96)?;
            buf.write_u32::<BE>(recipient_channel)?;
        }
        Message::ChannelClose { recipient_channel } => {
            buf.write_u8(97)?;
            buf.write_u32::<BE>(recipient_channel)?;
        }
        Message::ChannelRequest {
            recipient_channel,
            request_type,
            want_reply,
            extra,
        } => {
            buf.write_u8(98)?;
            buf.write_u32::<BE>(recipient_channel)?;
            write_string(&mut buf, &request_type)?;
            buf.write_u8(if want_reply { 1 } else { 0 })?;
            match extra {
                ChannelRequestExtra::Terminal {
                    terminal_type,
                    width_columns,
                    height_rows,
                    width_pixels,
                    height_pixels,
                    terminal_modes,
                } => {
                    write_string(&mut buf, &terminal_type)?;
                    buf.write_u32::<BE>(width_columns)?;
                    buf.write_u32::<BE>(height_rows)?;
                    buf.write_u32::<BE>(width_pixels)?;
                    buf.write_u32::<BE>(height_pixels)?;
                    write_bytes(&mut buf, &terminal_modes)?;
                }
                ChannelRequestExtra::WindowChange {
                    width_columns,
                    height_rows,
                    width_pixels,
                    height_pixels,
                } => {
                    buf.write_u32::<BE>(width_columns)?;
                    buf.write_u32::<BE>(height_rows)?;
                    buf.write_u32::<BE>(width_pixels)?;
                    buf.write_u32::<BE>(height_pixels)?;
                }
                ChannelRequestExtra::None => todo!(),
            }
        }
        Message::ChannelSuccess { recipient_channel } => {
            buf.write_u8(99)?;
            buf.write_u32::<BE>(recipient_channel)?;
        }
        Message::ChannelFailure { recipient_channel } => {
            buf.write_u8(100)?;
            buf.write_u32::<BE>(recipient_channel)?;
        }
    }
    Ok(buf)
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
