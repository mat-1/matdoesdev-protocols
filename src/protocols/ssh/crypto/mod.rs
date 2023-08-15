use super::protocol;

pub mod ed25519;

/// https://datatracker.ietf.org/doc/html/rfc4253#section-7.2
pub struct EncryptionKeys {
    // aka nonce
    pub initial_iv_client_to_server: Vec<u8>,
    pub initial_iv_server_to_client: Vec<u8>,

    pub encryption_key_client_to_server: Vec<u8>,
    pub encryption_key_server_to_client: Vec<u8>,

    // aka MAC key
    pub integrity_key_client_to_server: Vec<u8>,
    pub integrity_key_server_to_client: Vec<u8>,
}

pub fn compute_keys(
    shared_secret: &[u8],
    exchange_hash: &[u8],
    session_id: &[u8],

    cipher_key_size: usize,
    cipher_iv_size: usize,
    mac_key_size: usize,
) -> anyhow::Result<EncryptionKeys> {
    println!("mac_key_size: {mac_key_size}");

    Ok(EncryptionKeys {
        initial_iv_client_to_server: compute_key(
            shared_secret,
            exchange_hash,
            'A',
            session_id,
            cipher_iv_size,
        )?,
        initial_iv_server_to_client: compute_key(
            shared_secret,
            exchange_hash,
            'B',
            session_id,
            cipher_iv_size,
        )?,
        encryption_key_client_to_server: compute_key(
            shared_secret,
            exchange_hash,
            'C',
            session_id,
            cipher_key_size,
        )?,
        encryption_key_server_to_client: compute_key(
            shared_secret,
            exchange_hash,
            'D',
            session_id,
            cipher_key_size,
        )?,
        integrity_key_client_to_server: compute_key(
            shared_secret,
            exchange_hash,
            'E',
            session_id,
            mac_key_size,
        )?,
        integrity_key_server_to_client: compute_key(
            shared_secret,
            exchange_hash,
            'F',
            session_id,
            mac_key_size,
        )?,
    })
}

/// https://datatracker.ietf.org/doc/html/rfc4253#section-7.2
pub fn compute_key(
    shared_secret: &[u8],
    exchange_hash: &[u8],
    character: char,
    session_id: &[u8],
    key_length: usize,
) -> anyhow::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut key = Vec::new();

    protocol::write_mpint(&mut buffer, shared_secret)?;
    buffer.extend(exchange_hash);
    buffer.push(character as u8);
    buffer.extend(session_id);

    key.extend(sha256(&buffer));

    // extend the key if it's not long enough
    while key.len() < key_length {
        let mut buffer = Vec::new();
        protocol::write_mpint(&mut buffer, shared_secret)?;
        buffer.extend(exchange_hash);
        buffer.extend(&key);
        key.extend(&sha256(&buffer));
    }

    key.resize(key_length, 0);

    Ok(key)
}

pub fn sha256(buffer: &[u8]) -> Vec<u8> {
    // usually the hasher would depend on the key exchange algorithm, but we only support curve25519-sha256
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(buffer);
    hasher.finalize().as_slice().to_vec()
}

pub fn generate_cookie() -> [u8; 16] {
    rand::random()
}
