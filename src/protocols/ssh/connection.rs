use std::io::{Cursor, Read};

use aes::{
    cipher::{KeyIvInit, StreamCipher},
    Aes128,
};
use byteorder::ReadBytesExt;
use ctr::Ctr128BE;
use tokio::{io::AsyncReadExt, net::tcp::OwnedReadHalf};

use super::protocol::{self, read_message};

pub struct ReadConnection {
    pub read: OwnedReadHalf,
    pub cipher: Option<Ctr128BE<Aes128>>,
    pub integrity_key: Option<Vec<u8>>,
}

impl ReadConnection {
    pub fn new(read: OwnedReadHalf) -> Self {
        Self {
            read,
            cipher: None,
            integrity_key: None,
        }
    }

    pub fn set_cipher(
        &mut self,
        encryption_key_client_to_server: &[u8],
        initial_iv_client_to_server: &[u8],
    ) {
        let cipher = Ctr128BE::<Aes128>::new(
            &<[u8; 16]>::try_from(encryption_key_client_to_server)
                .unwrap()
                .into(),
            &<[u8; 16]>::try_from(initial_iv_client_to_server)
                .unwrap()
                .into(),
        );
        self.cipher = Some(cipher);
    }

    pub async fn read_payload(&mut self) -> anyhow::Result<Vec<u8>> {
        println!("reading payload");
        // read the packet length and decrypt it
        let mut packet_length_bytes = [0u8; 4];
        self.read.read_exact(&mut packet_length_bytes).await?;
        if let Some(cipher) = &mut self.cipher {
            cipher.apply_keystream(&mut packet_length_bytes);
        }
        let packet_length = u32::from_be_bytes(packet_length_bytes) as usize;
        println!("packet length: {}", packet_length);

        // read the packet, one byte at a time so we don't allocate a huge buffer immediately
        let mut packet_bytes = Vec::new();
        for _ in 0..packet_length {
            let mut byte = [0u8; 1];
            self.read.read_exact(&mut byte).await?;
            packet_bytes.push(byte[0]);
        }
        if let Some(cipher) = &mut self.cipher {
            cipher.apply_keystream(&mut packet_bytes);
        }
        let mut packet_bytes = Cursor::new(packet_bytes);

        // now read the payload
        let padding_length = ReadBytesExt::read_u8(&mut packet_bytes)? as usize;
        let payload_length = packet_length - padding_length - 1;
        let mut payload = Vec::new();
        for _ in 0..payload_length {
            payload.push(ReadBytesExt::read_u8(&mut packet_bytes)?);
        }

        // read the padding
        let mut padding = vec![0; padding_length];
        Read::read_exact(&mut packet_bytes, &mut padding)?;

        if self.integrity_key.is_some() {
            // read 32 bytes for the mac-
            let mut mac = [0u8; 32];
            self.read.read_exact(&mut mac).await?;
        }

        Ok(payload)
    }

    pub async fn read_packet(&mut self) -> anyhow::Result<protocol::Message> {
        let payload = self.read_payload().await?;
        let message = read_message(Cursor::new(payload))?;

        Ok(message)
    }
}
