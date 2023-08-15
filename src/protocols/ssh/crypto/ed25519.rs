use std::{io::Read, path::Path};

use ed25519_dalek::{ed25519::signature::Signature, Keypair, Signer};

use crate::protocols::ssh::protocol;

const KEYPAIR_PATH: &str = "data/ssh/keypair.bin";

pub fn load_keypair() -> Keypair {
    let keypair_path = Path::new(KEYPAIR_PATH);

    if !keypair_path.exists() {
        let new_keypair = generate_new_keypair();

        let keypair_bytes = new_keypair.to_bytes();

        // make the directory if it doesn't exist
        std::fs::create_dir_all(keypair_path.parent().unwrap()).unwrap();
        std::fs::write(keypair_path, keypair_bytes).unwrap();
    }

    let mut keypair_file = std::fs::File::open(keypair_path).unwrap();

    let mut keypair_bytes = Vec::new();
    keypair_file.read_to_end(&mut keypair_bytes).unwrap();

    ed25519_dalek::Keypair::from_bytes(&keypair_bytes).unwrap()
}

fn generate_new_keypair() -> Keypair {
    // ed25519_dalek uses an old version of rand
    #[allow(deprecated)]
    let keypair = ed25519_dalek::Keypair::generate(&mut rand_os::OsRng {});
    assert_eq!(
        keypair.public.as_bytes(),
        ed25519_dalek::PublicKey::from(&keypair.secret).as_bytes()
    );

    keypair
}

#[derive(Debug)]
pub struct Exchange {
    /// client's identification string (CR and LF excluded)
    pub client_id: Vec<u8>,
    /// server's identification string (CR and LF excluded)
    pub server_id: Vec<u8>,
    /// payload of the client's SSH_MSG_KEXINIT
    pub client_kex_init: Vec<u8>,
    /// payload of the server's SSH_MSG_KEXINIT
    pub server_kex_init: Vec<u8>,
    /// client's ephemeral public key octet string
    pub client_ephemeral: Vec<u8>,
    /// client's ephemeral public key octet string
    pub server_ephemeral: Vec<u8>,
}

pub fn compute_exchange_hash(
    key: &[u8],
    shared_secret: Option<&[u8]>,
    exchange: &Exchange,
) -> anyhow::Result<Vec<u8>> {
    // Computing the exchange hash, see page 7 of RFC 5656.

    let mut buffer = Vec::new();

    protocol::write_bytes(&mut buffer, &exchange.client_id)?;
    protocol::write_bytes(&mut buffer, &exchange.server_id)?;
    protocol::write_bytes(&mut buffer, &exchange.client_kex_init)?;
    protocol::write_bytes(&mut buffer, &exchange.server_kex_init)?;

    protocol::write_bytes(&mut buffer, key)?;
    protocol::write_bytes(&mut buffer, &exchange.client_ephemeral)?;
    protocol::write_bytes(&mut buffer, &exchange.server_ephemeral)?;

    if let Some(shared) = shared_secret {
        protocol::write_mpint(&mut buffer, shared)?;
    }

    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&buffer);

    let mut res = Vec::new();
    res.extend(hasher.finalize().as_slice());
    Ok(res)
}

pub fn add_signature(keypair: &ed25519_dalek::Keypair, to_sign: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let signature = keypair.sign(to_sign);
    protocol::write_string(&mut buffer, "ssh-ed25519")?;
    protocol::write_bytes(&mut buffer, signature.as_bytes())?;

    Ok(buffer)
}
