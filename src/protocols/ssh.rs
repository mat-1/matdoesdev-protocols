mod ed25519;
mod protocol;

use std::{
    collections::HashMap,
    io::{self, Cursor, Read, Write},
    path::Path,
    sync::Arc,
};

use anyhow::bail;
use byteorder::{ReadBytesExt, WriteBytesExt, BE};
use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
use ed25519_dalek::Signer;
use futures_util::StreamExt;
use tokio::{
    io::AsyncWriteExt,
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
};
use tokio_rustls::server::TlsStream;
use tokio_util::codec::{BytesCodec, FramedRead};
use url::Url;

use crate::{
    crawl::{ImageSource, PostPart, SiteData},
    HOSTNAME,
};

use super::Protocol;

const BIND_HOST: &str = "[::]";
const BIND_PORT: u16 = 2222;

#[derive(Clone)]
pub struct Ssh {}

impl Protocol for Ssh {
    fn generate(data: &SiteData) -> Self {
        Ssh {}
    }

    async fn serve(self) {
        // start a tcp server

        let ssh = Arc::new(self);

        let listener = TcpListener::bind(format!("{BIND_HOST}:{BIND_PORT}"))
            .await
            .unwrap();

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            println!("started tcp connection");

            let (read, mut write) = stream.into_split();
            let mut framed = FramedRead::new(read, BytesCodec::new());

            tokio::spawn(async move {
                match connection(framed, write).await {
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

async fn connection(
    mut read: FramedRead<OwnedReadHalf, BytesCodec>,
    mut write: OwnedWriteHalf,
) -> anyhow::Result<()> {
    let server_id = "SSH-2.0-matssh_1.0";
    let keypair = ed25519::load_keypair();

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

    // send key exchange
    let cookie = generate_cookie();
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
    let server_kex_init_bytes = protocol::write_payload(server_kex_init_payload.clone())?;
    write.write_all(&server_kex_init_bytes).await?;

    // receive key exchange
    let client_kex_init_bytes = read
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("connection closed"))??
        .to_vec();
    let client_kex_init_payload =
        protocol::read_payload(Cursor::new(client_kex_init_bytes.clone()))?;
    let client_kex_init_message =
        protocol::read_message(Cursor::new(client_kex_init_payload.clone()))?;
    match client_kex_init_message {
        protocol::Message::KexInit {
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
            // check to make sure we support the algorithms
        }
        _ => bail!("expected KexInit"),
    }

    while let Some(bytes) = read.next().await.transpose()? {
        println!("received message: {:?}", bytes);

        let packet = protocol::read_packet(Cursor::new(bytes.into()))?;
        println!("packet: {packet:?}");
        match packet.message {
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
            protocol::Message::KexEcdhInit { client_public_key } => {
                let client_public_key = <[u8; 32]>::try_from(client_public_key)
                    .map_err(|_| anyhow::anyhow!("client public key is not 32 bytes long"))?;
                let client_public_key = curve25519_dalek::MontgomeryPoint(client_public_key);
                let server_secret =
                    curve25519_dalek::Scalar::from_bytes_mod_order(rand::random::<[u8; 32]>());
                let server_public_key = (ED25519_BASEPOINT_TABLE * &server_secret).to_montgomery();

                let shared = server_secret * client_public_key;

                let mut server_public_host_key = Vec::new();
                protocol::write_string(&mut server_public_host_key, "ssh-ed25519")?;
                protocol::write_bytes(&mut server_public_host_key, keypair.public.as_bytes())?;

                let hash = ed25519::compute_exchange_hash(
                    &server_public_host_key,
                    Some(shared.as_bytes()),
                    &ed25519::Exchange {
                        client_id: client_id.as_bytes().to_vec(),
                        server_id: server_id.as_bytes().to_vec(),
                        client_kex_init: client_kex_init_payload.clone(),
                        server_kex_init: server_kex_init_payload.clone(),
                        client_ephemeral: client_public_key.as_bytes().to_vec(),
                        server_ephemeral: server_public_key.as_bytes().to_vec(),
                    },
                )?;

                write
                    .write_all(&protocol::write_packet(protocol::Packet {
                        message: protocol::Message::KexEcdhReply {
                            server_public_host_key,
                            server_public_key: server_public_key.as_bytes().to_vec(),
                            signature: ed25519::add_signature(&keypair, &hash)?,
                        },
                        mac: vec![],
                    })?)
                    .await?;
                write
                    .write_all(&protocol::write_packet(protocol::Packet {
                        message: protocol::Message::NewKeys,
                        mac: vec![],
                    })?)
                    .await?;
                break;
            }
            _ => bail!("unexpected message"),
        }
    }

    // wait for client to send us NewKeys, then we enable encryption
    while let Some(bytes) = read.next().await.transpose()? {
        let packet = protocol::read_packet(Cursor::new(bytes.into()))?;
        match packet.message {
            protocol::Message::NewKeys => {
                break;
            }
            _ => bail!("expected NewKeys"),
        }
    }

    // encryption is now enabled!

    while let Some(bytes) = read.next().await.transpose()? {
        println!("received message: {:?}", bytes);

        let packet = protocol::read_packet(Cursor::new(bytes.into()))?;
        println!("packet: {packet:?}");
        match packet.message {
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
            _ => bail!("unexpected message"),
        }
    }

    println!("connection closed");

    Ok(())
}

fn generate_cookie() -> [u8; 16] {
    rand::random()
}
