use std::{
    collections::HashMap,
    io::{Cursor, Read},
};

use aes::{
    cipher::{KeyIvInit, KeySizeUser, StreamCipher},
    Aes128,
};
use byteorder::ReadBytesExt;
use ctr::Ctr128BE;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
};

use super::{
    crypto,
    protocol::{self, read_message},
};

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
        // read the packet length and decrypt it
        let mut packet_length_bytes = [0u8; 4];
        self.read.read_exact(&mut packet_length_bytes).await?;
        if let Some(cipher) = &mut self.cipher {
            cipher.apply_keystream(&mut packet_length_bytes);
        }
        let packet_length = u32::from_be_bytes(packet_length_bytes) as usize;

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

pub struct EncryptedConnection {
    write: OwnedWriteHalf,

    cipher_server_to_client: Ctr128BE<Aes128>,
    integrity_key_server_to_client: Vec<u8>,
    sequence_number_server_to_client: u32,

    pub channels: HashMap<u32, Channel>,
}
pub struct Channel {
    pub recipient_window_size: u32,
    pub sender_window_size: u32,

    pub recipient_maximum_packet_size: u32,
    pub sender_maximum_packet_size: u32,
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
            channels: HashMap::new(),
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

    pub async fn write_data(&mut self, data: &[u8], recipient_channel: u32) -> anyhow::Result<()> {
        if let Some(channel) = self.channels.get_mut(&recipient_channel) {
            channel.recipient_window_size -= data.len() as u32;
        }

        let max_packet_size = self
            .channels
            .get(&recipient_channel)
            .map(|channel| channel.recipient_maximum_packet_size)
            .unwrap_or(32768);

        for chunk in data.chunks(max_packet_size as usize) {
            self.write_packet(protocol::Message::ChannelData {
                recipient_channel,
                data: chunk.to_vec(),
            })
            .await?;
        }

        Ok(())
    }
}
