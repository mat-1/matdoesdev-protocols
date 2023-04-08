use std::{io::Read, path::Path, sync::Arc};

use rcgen::{Certificate, CertificateParams, DnType};
use tokio_rustls::{
    rustls::{self, ServerConfig},
    TlsAcceptor,
};

use crate::HOSTNAME;

fn generate_new_cert() -> Certificate {
    let mut cert_params = CertificateParams::new(vec![HOSTNAME.to_string()]);
    cert_params
        .distinguished_name
        .push(DnType::CommonName, HOSTNAME);

    Certificate::from_params(cert_params).unwrap()
}

const KEY_PATH: &str = "gemini/certs";
const PUBLIC_KEY_FILENAME: &str = "public.der";
const PRIVATE_KEY_FILENAME: &str = "private.der";

fn load_certs() -> (rustls::Certificate, rustls::PrivateKey) {
    // try to load the key files first, then generate them if they don't exist

    let key_path = Path::new(KEY_PATH);

    let public_key_path = key_path.join(PUBLIC_KEY_FILENAME);
    let private_key_path = key_path.join(PRIVATE_KEY_FILENAME);

    if !public_key_path.exists() || !private_key_path.exists() {
        let new_cert = generate_new_cert();

        let public_key = new_cert.serialize_der().unwrap();
        let private_key = new_cert.serialize_private_key_der();

        // make the directory if it doesn't exist
        std::fs::create_dir_all(key_path).unwrap();
        std::fs::write(&public_key_path, public_key).unwrap();
        std::fs::write(&private_key_path, private_key).unwrap();
    }

    let mut public_key_file = std::fs::File::open(public_key_path).unwrap();
    let mut private_key_file = std::fs::File::open(private_key_path).unwrap();

    let mut public_key = Vec::new();
    let mut private_key = Vec::new();
    public_key_file.read_to_end(&mut public_key).unwrap();
    private_key_file.read_to_end(&mut private_key).unwrap();

    let cert = rustls::Certificate(public_key);
    let private_key = rustls::PrivateKey(private_key);
    (cert, private_key)
}

pub fn acceptor() -> TlsAcceptor {
    let (certs, keys) = load_certs();

    let tls_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(vec![certs], keys)
        .unwrap();
    let tls_config = Arc::new(tls_config);
    TlsAcceptor::from(Arc::clone(&tls_config))
}
