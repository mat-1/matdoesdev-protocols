pub mod connection;
mod crypto;
mod protocol;

use std::io::Cursor;

use aes::{
    cipher::{IvSizeUser, KeyIvInit, KeySizeUser, StreamCipher},
    Aes128,
};
use anyhow::bail;
use ctr::Ctr128BE;
use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::{
    io::AsyncWriteExt,
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener,
    },
};
use tokio_util::codec::{BytesCodec, FramedRead};

use crate::{
    crawl::SiteData,
    protocols::ssh::{connection::ReadConnection, protocol::ChannelRequestExtra},
    terminal::TerminalSession,
};

use super::Protocol;

const BIND_HOST: &str = "[::]";
const BIND_PORT: u16 = 2222;

#[derive(Clone)]
pub struct Ssh {
    pub site_data: SiteData,
}

impl Protocol for Ssh {
    fn generate(data: &SiteData) -> Self {
        Ssh {
            site_data: data.clone(),
        }
    }

    async fn serve(self) {
        // start a tcp server

        let listener = TcpListener::bind(format!("{BIND_HOST}:{BIND_PORT}"))
            .await
            .unwrap();

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            println!("started tcp connection");

            let (read, write) = stream.into_split();
            let framed = FramedRead::new(read, BytesCodec::new());

            let site_data = self.site_data.clone();
            tokio::spawn(async move {
                match connection(framed, write, site_data).await {
                    Ok(_) => {}
                    Err(e) => {
                        println!("error: {}", e);
                    }
                }
            });

            // tokio::spawn(async move {
            //     let _ = tokio::io::AsyncWriteExt::write_all(&mut write, b"SSH-2.0-matssh_1.0\r\n")
            //         .await;

            //     while let Some(message) = framed.next().await {
            //         let bytes = match message {
            //             Ok(bytes) => bytes,
            //             Err(e) => {
            //                 println!("error reading from stream: {}", e);
            //                 return;
            //             }
            //         };
            //         println!("received message: {:?}", bytes);

            //         let mut data = Cursor::new(bytes);
            //         let packet_length = data.read_u32::<BE>();
            //         let padding_length = data.read_u8();

            //         let payload_length = packet_length - padding_length as u32 - 1;
            //         if payload_length > bytes {
            //             eprintln!("payload length is greater than packet length");
            //             return;
            //         }
            //         let mut payload = vec![0; payload_length];
            //         data.read_exact(&mut payload);

            //         println!("payload: {:?}", payload);
            //     }
            // });
        }
    }
}

pub struct EncryptedConnection {
    write: OwnedWriteHalf,

    cipher_server_to_client: Ctr128BE<Aes128>,
    integrity_key_server_to_client: Vec<u8>,
    sequence_number_server_to_client: u32,
}

impl EncryptedConnection {
    pub async fn new(
        write: OwnedWriteHalf,
        _exchange_hash: Vec<u8>,
        _session_id: Vec<u8>,
        encryption_keys: &crypto::EncryptionKeys,

        sequence_number_server_to_client: u32,
    ) -> anyhow::Result<Self> {
        let cipher_server_to_client = Ctr128BE::<Aes128>::new(
            &<[u8; 16]>::try_from(encryption_keys.encryption_key_server_to_client.clone())
                .unwrap()
                .into(),
            &<[u8; 16]>::try_from(encryption_keys.initial_iv_server_to_client.clone())
                .unwrap()
                .into(),
        );

        Ok(Self {
            write,
            cipher_server_to_client,
            integrity_key_server_to_client: encryption_keys.integrity_key_server_to_client.clone(),
            sequence_number_server_to_client,
        })
    }

    pub async fn write_packet(&mut self, packet: protocol::Message) -> anyhow::Result<()> {
        let mut bytes = protocol::write_packet(packet, Some(Ctr128BE::<Aes128>::key_size()))?;

        // write mac
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.integrity_key_server_to_client)?;
        mac.update(&self.sequence_number_server_to_client.to_be_bytes());
        mac.update(&bytes);

        self.cipher_server_to_client.apply_keystream(&mut bytes);
        self.write.write_all(&bytes).await?;
        self.write.write_all(&mac.finalize().into_bytes()).await?;
        self.sequence_number_server_to_client += 1;

        Ok(())
    }
}

async fn connection(
    mut read: FramedRead<OwnedReadHalf, BytesCodec>,
    mut write: OwnedWriteHalf,
    site_data: SiteData,
) -> anyhow::Result<()> {
    let server_id = "SSH-2.0-matssh_1.0";
    let keypair = crypto::ed25519::load_keypair();

    // the first message is the identification string
    write
        .write_all(format!("{server_id}\r\n").as_bytes())
        .await?;
    let bytes = read
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("connection closed"))??;
    println!("received message: {:?}", bytes);
    let client_id = String::from_utf8(bytes[..bytes.len() - 2].to_vec())?;

    let mut read = ReadConnection::new(read);
    let mut sequence_number_server_to_client = 0;

    // send key exchange
    let cookie = crypto::generate_cookie();
    let server_kex_init_payload = protocol::write_message(protocol::Message::KexInit {
        cookie,
        kex_algorithms: vec!["curve25519-sha256".to_string()],
        server_host_key_algorithms: vec!["ssh-ed25519".to_string()],
        encryption_algorithms_client_to_server: vec!["aes128-ctr".to_string()],
        encryption_algorithms_server_to_client: vec!["aes128-ctr".to_string()],
        mac_algorithms_client_to_server: vec!["hmac-sha2-256".to_string()],
        mac_algorithms_server_to_client: vec!["hmac-sha2-256".to_string()],
        compression_algorithms_client_to_server: vec!["none".to_string()],
        compression_algorithms_server_to_client: vec!["none".to_string()],
        languages_client_to_server: vec![],
        languages_server_to_client: vec![],
        first_kex_packet_follows: false,
        reserved: 0,
    })?;
    let server_kex_init_bytes = protocol::write_payload(server_kex_init_payload.clone(), None)?;
    write.write_all(&server_kex_init_bytes).await?;
    sequence_number_server_to_client += 1;

    // receive key exchange
    let client_kex_init_payload = read.read_payload().await?;
    let client_kex_init_message =
        protocol::read_message(Cursor::new(client_kex_init_payload.clone()))?;
    match client_kex_init_message {
        protocol::Message::KexInit { .. } => {
            // check to make sure we support the algorithms
        }
        _ => bail!("expected KexInit"),
    }

    // the session ID is the exchange hash from the first key exchange, and then never changes after that
    let session_id: Vec<u8>;
    // this one does change every key exchange
    let exchange_hash: Vec<u8>;
    let encryption_keys: crypto::EncryptionKeys;

    loop {
        let packet = read.read_packet().await?;
        println!("packet: {packet:?}");
        match packet {
            protocol::Message::Disconnect {
                reason_code,
                description,
                language_tag,
            } => {
                bail!(
                    "disconnect: reason_code: {reason_code}, description: {description}, language_tag: {language_tag}"
                );
            }
            protocol::Message::KexEcdhInit { client_public_key } => {
                let client_public_key = <[u8; 32]>::try_from(client_public_key)
                    .map_err(|_| anyhow::anyhow!("client public key is not 32 bytes long"))?;
                let client_public_key = curve25519_dalek::MontgomeryPoint(client_public_key);
                let server_secret =
                    curve25519_dalek::Scalar::from_bytes_mod_order(rand::random::<[u8; 32]>());
                let server_public_key = (ED25519_BASEPOINT_TABLE * &server_secret).to_montgomery();

                let shared_secret = server_secret * client_public_key;

                let mut server_public_host_key = Vec::new();
                protocol::write_string(&mut server_public_host_key, "ssh-ed25519")?;
                protocol::write_bytes(&mut server_public_host_key, keypair.public.as_bytes())?;

                exchange_hash = crypto::ed25519::compute_exchange_hash(
                    &server_public_host_key,
                    Some(shared_secret.as_bytes()),
                    &crypto::ed25519::Exchange {
                        client_id: client_id.as_bytes().to_vec(),
                        server_id: server_id.as_bytes().to_vec(),
                        client_kex_init: client_kex_init_payload.clone(),
                        server_kex_init: server_kex_init_payload.clone(),
                        client_ephemeral: client_public_key.as_bytes().to_vec(),
                        server_ephemeral: server_public_key.as_bytes().to_vec(),
                    },
                )?;

                write
                    .write_all(&protocol::write_packet(
                        protocol::Message::KexEcdhReply {
                            server_public_host_key,
                            server_public_key: server_public_key.as_bytes().to_vec(),
                            signature: crypto::ed25519::add_signature(&keypair, &exchange_hash)?,
                        },
                        None,
                    )?)
                    .await?;
                write
                    .write_all(&protocol::write_packet(protocol::Message::NewKeys, None)?)
                    .await?;
                sequence_number_server_to_client += 2;

                session_id = exchange_hash.clone();
                encryption_keys = crypto::compute_keys(
                    shared_secret.as_bytes(),
                    &exchange_hash,
                    &session_id,
                    Ctr128BE::<Aes128>::key_size(),
                    Ctr128BE::<Aes128>::iv_size(),
                    32,
                )?;
                break;
            }
            _ => bail!("unexpected message"),
        }
    }

    println!("waiting for newkeys");

    // wait for client to send us NewKeys, then we enable encryption
    loop {
        let packet = read.read_packet().await?;
        match packet {
            protocol::Message::NewKeys => {
                break;
            }
            _ => bail!("expected NewKeys"),
        }
    }

    // encryption is now enabled!
    println!("encryption is now enabled");
    read.set_cipher(
        &encryption_keys.encryption_key_client_to_server,
        &encryption_keys.initial_iv_client_to_server,
    );
    read.integrity_key = Some(encryption_keys.integrity_key_client_to_server.clone());
    let mut conn = EncryptedConnection::new(
        write,
        exchange_hash,
        session_id,
        &encryption_keys,
        sequence_number_server_to_client,
    )
    .await?;

    let mut terminal_session = TerminalSession::new(site_data);

    while let Ok(packet) = read.read_packet().await {
        println!("packet: {packet:?}");
        match packet {
            protocol::Message::ServiceRequest { service_name } => {
                if service_name == "ssh-userauth" {
                    conn.write_packet(protocol::Message::ServiceAccept { service_name })
                        .await?;
                    // conn.write_packet(protocol::Message::UserauthBanner {
                    //     message: format!("hi chat\n"),
                    //     language_tag: "english probably".to_string(),
                    // })
                    // .await?;
                } else {
                    bail!("unsupported service: {service_name}");
                }
            }
            protocol::Message::Disconnect {
                reason_code,
                description,
                language_tag,
            } => {
                println!(
                    "disconnect: reason_code: {reason_code}, description: {description}, language_tag: {language_tag}"
                );
                break;
            }
            protocol::Message::UserauthRequest {
                username,
                service_name: _,
                authentication_method: _,
            } => {
                println!("user {username} is connecting");
                conn.write_packet(protocol::Message::UserauthSuccess)
                    .await?;
            }
            protocol::Message::ChannelOpen {
                channel_type,
                sender_channel,
                initial_window_size,
                maximum_packet_size,
            } => {
                println!(
                    "channel open: channel_type: {channel_type}, sender_channel: {sender_channel}, initial_window_size: {initial_window_size}, maximum_packet_size: {maximum_packet_size}"
                );
                conn.write_packet(protocol::Message::ChannelOpenConfirmation {
                    recipient_channel: sender_channel,
                    sender_channel: 0,
                    initial_window_size,
                    maximum_packet_size,
                })
                .await?;
                conn.write_packet(protocol::Message::ChannelSuccess {
                    recipient_channel: sender_channel,
                })
                .await?;
            }
            protocol::Message::ChannelRequest {
                recipient_channel,
                request_type,
                want_reply,
                extra,
            } => {
                println!(
                    "channel request: recipient_channel: {recipient_channel}, request_type: {request_type}, want_reply: {want_reply}"
                );
                match extra {
                    ChannelRequestExtra::Terminal {
                        terminal_type: _,
                        width_columns,
                        height_rows,
                        width_pixels: _,
                        height_pixels: _,
                        terminal_modes: _,
                    } => {
                        let data = terminal_session.resize(width_columns, height_rows);
                        if !data.is_empty() {
                            conn.write_packet(protocol::Message::ChannelData {
                                recipient_channel,
                                data,
                            })
                            .await?;
                        }
                    }
                    ChannelRequestExtra::WindowChange {
                        width_columns,
                        height_rows,
                        width_pixels: _,
                        height_pixels: _,
                    } => {
                        let data = terminal_session.resize(width_columns, height_rows);
                        if !data.is_empty() {
                            conn.write_packet(protocol::Message::ChannelData {
                                recipient_channel,
                                data,
                            })
                            .await?;
                        }
                    }
                    ChannelRequestExtra::None => {}
                }
            }
            protocol::Message::ChannelData {
                recipient_channel,
                data,
            } => {
                println!("channel data: recipient_channel: {recipient_channel}, data: {data:?}");
                if data == [3] || data == [4] {
                    conn.write_packet(protocol::Message::ChannelData {
                        recipient_channel,
                        data: "Bye!\r\n".as_bytes().to_vec(),
                    })
                    .await?;
                    break;
                }
                let data = terminal_session.on_keystroke(&data);
                if !data.is_empty() {
                    conn.write_packet(protocol::Message::ChannelData {
                        recipient_channel,
                        data,
                    })
                    .await?;
                }
            }
            protocol::Message::ChannelWindowAdjust {
                recipient_channel,
                bytes_to_add,
            } => {
                println!(
                    "channel window adjust: recipient_channel: {recipient_channel}, bytes_to_add: {bytes_to_add}"
                );
            }
            _ => println!("unexpected message"),
        }
    }

    println!("connection closed");

    Ok(())
}
