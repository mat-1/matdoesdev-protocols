use std::io::{Cursor, Read};

use aes::{
    cipher::{KeyIvInit, StreamCipher},
    Aes128,
};
use ctr::Ctr128BE;
use futures_util::StreamExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio_util::codec::{BytesCodec, FramedRead};

use crate::protocols::ssh::protocol::read_payload;

use super::protocol::{self, read_message};

pub struct ReadConnection {
    pub read: FramedRead<OwnedReadHalf, BytesCodec>,
    pub cipher: Option<Ctr128BE<Aes128>>,
    pub integrity_key: Option<Vec<u8>>,

    pub buffer: Vec<u8>,
}

impl ReadConnection {
    pub fn new(read: FramedRead<OwnedReadHalf, BytesCodec>) -> Self {
        Self {
            read,
            cipher: None,
            integrity_key: None,
            buffer: Vec::new(),
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
        let mut buffer = Cursor::new(self.buffer.clone());

        loop {
            if let Ok(payload) =
                read_payload(&mut buffer, &mut self.cipher, self.integrity_key.is_some())
            {
                println!("payload: {payload:?}");
                self.buffer = buffer.remaining_slice().to_owned();
                return Ok(payload);
            };

            let mut frame =
                self.read.next().await.transpose()?.ok_or_else(|| {
                    anyhow::anyhow!("connection closed before receiving KexEcdhInit")
                })?;
            buffer.get_mut().extend(frame);
        }
    }

    pub async fn read_packet(&mut self) -> anyhow::Result<protocol::Message> {
        let payload = self.read_payload().await?;
        let message = read_message(Cursor::new(payload))?;

        Ok(message)
    }
}
