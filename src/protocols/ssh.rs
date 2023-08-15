pub mod connection;
mod crypto;
mod protocol;

use std::io::Cursor;

use aes::{
    cipher::{IvSizeUser, KeySizeUser},
    Aes128,
};
use anyhow::bail;
use ctr::Ctr128BE;
use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener,
    },
};

use crate::{
    crawl::SiteData,
    protocols::ssh::{
        connection::{Channel, EncryptedConnection, ReadConnection},
        protocol::ChannelRequestExtra,
    },
    terminal::TerminalSession,
};

use super::Protocol;

const BIND_HOST: &str = "[::]";
const BIND_PORT: u16 = {
    #[cfg(debug_assertions)]
    {
        2222
    }
    #[cfg(not(debug_assertions))]
    22
};

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

            let site_data = self.site_data.clone();
            tokio::spawn(async move {
                match connection(read, write, site_data).await {
                    Ok(_) => {}
                    Err(e) => {
                        println!("error: {}", e);
                    }
                }
            });
        }
    }
}

async fn connection(
    mut read: OwnedReadHalf,
    mut write: OwnedWriteHalf,
    site_data: SiteData,
) -> anyhow::Result<()> {
    let server_id = "SSH-2.0-matssh_1.0";
    let keypair = crypto::ed25519::load_keypair();

    // the first message is the identification string
    write
        .write_all(format!("{server_id}\r\n").as_bytes())
        .await?;

    let mut bytes = Vec::new();
    loop {
        let byte = read.read_u8().await?;
        bytes.push(byte);
        if bytes.ends_with(&[b'\r', b'\n']) {
            break;
        }
    }

    let client_id = String::from_utf8(bytes[..bytes.len() - 2].to_vec())?;
    println!("client id: {client_id}");

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
                protocol::write_bytes(
                    &mut server_public_host_key,
                    keypair.verifying_key().as_bytes(),
                )?;

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
            _ => println!("unexpected message"),
        }
    }

    // wait for client to send us NewKeys, then we enable encryption
    loop {
        let packet = read.read_packet().await?;
        match packet {
            protocol::Message::NewKeys => {
                break;
            }
            _ => println!("expected NewKeys"),
        }
    }

    // encryption is now enabled!
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
        // println!("packet: {packet:?}");
        match packet {
            protocol::Message::ServiceRequest { service_name } => {
                if service_name == "ssh-userauth" {
                    conn.write_packet(protocol::Message::ServiceAccept { service_name })
                        .await?;
                    conn.write_packet(protocol::Message::UserauthBanner {
                        message: format!(
                            "welcome to mat does dev free preview no download required\n"
                        ),
                        language_tag: "".to_string(),
                    })
                    .await?;
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
                channel_type: _,
                sender_channel,
                initial_window_size,
                maximum_packet_size,
            } => {
                conn.channels.insert(
                    sender_channel,
                    Channel {
                        recipient_window_size: initial_window_size,
                        sender_window_size: 2097152,
                        recipient_maximum_packet_size: maximum_packet_size,
                        sender_maximum_packet_size: 32768,
                    },
                );
                conn.write_packet(protocol::Message::ChannelOpenConfirmation {
                    recipient_channel: sender_channel,
                    sender_channel,
                    initial_window_size: 2097152,
                    maximum_packet_size: 32768,
                })
                .await?;
                conn.write_packet(protocol::Message::ChannelSuccess {
                    recipient_channel: sender_channel,
                })
                .await?;

                conn.write_data(&terminal_session.on_open(), sender_channel)
                    .await?;
            }
            protocol::Message::ChannelRequest {
                recipient_channel,
                request_type: _,
                want_reply: _,
                extra,
            } => match extra {
                ChannelRequestExtra::Terminal {
                    terminal_type: _,
                    width_columns,
                    height_rows,
                    width_pixels: _,
                    height_pixels: _,
                    terminal_modes: _,
                } => {
                    let data = terminal_session.resize(width_columns, height_rows);
                    conn.write_data(&data, recipient_channel).await?;
                }
                ChannelRequestExtra::WindowChange {
                    width_columns,
                    height_rows,
                    width_pixels: _,
                    height_pixels: _,
                } => {
                    let data = terminal_session.resize(width_columns, height_rows);
                    conn.write_data(&data, recipient_channel).await?;
                }
                ChannelRequestExtra::Exec { command: _ } => {
                    conn.write_packet(protocol::Message::ChannelSuccess { recipient_channel })
                        .await?;
                }
                ChannelRequestExtra::Shell => {
                    conn.write_packet(protocol::Message::ChannelSuccess { recipient_channel })
                        .await?;
                }
                ChannelRequestExtra::None => {}
            },
            protocol::Message::ChannelData {
                recipient_channel,
                data,
            } => {
                if data == [3] || data == [4] {
                    // ^C or ^D

                    conn.write_data(&terminal_session.on_close(), recipient_channel)
                        .await?;
                    break;
                }
                let data = terminal_session.on_keystroke(&data);
                conn.write_data(&data, recipient_channel).await?;
            }
            protocol::Message::ChannelWindowAdjust {
                recipient_channel,
                bytes_to_add,
            } => {
                if let Some(channel) = conn.channels.get_mut(&recipient_channel) {
                    channel.recipient_window_size += bytes_to_add;
                }
            }
            protocol::Message::ChannelEof { recipient_channel } => {
                conn.write_packet(protocol::Message::ChannelClose { recipient_channel })
                    .await?;
            }
            protocol::Message::ChannelClose {
                recipient_channel: _,
            } => {}
            _ => println!("unexpected message"),
        }
    }

    println!("connection closed");

    Ok(())
}
