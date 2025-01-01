//! HTTP server for stuff like changing the QOTD. The actual matdoes.dev HTTP
//! server is built statically and served by Caddy.

use std::{
    collections::HashMap,
    io::{self},
    sync::Arc,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::{qotd::Qotd, Protocol};
use crate::{crawl::SiteData, protocols::qotd::QOTD_MESSAGE_PATH};

const BIND_HOST: &str = "[::]";
const BIND_PORT: u16 = 6758;

const QOTD_SECRET_PATH: &str = "data/qotd/secret.txt";

#[derive(Clone)]
pub struct Http {
    pub qotd: Qotd,
}

impl Protocol for Http {
    fn generate(_: &SiteData) -> Self {
        Http {
            qotd: Qotd {
                message: Default::default(),
            },
        }
    }

    async fn serve(self) {
        let http = Arc::new(self);

        let listener = match TcpListener::bind(format!("{BIND_HOST}:{BIND_PORT}")).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("failed to bind to port {BIND_PORT}: {e}");
                return;
            }
        };

        loop {
            let (mut stream, remote_addr) = listener.accept().await.unwrap();
            println!("started tcp connection for http: {remote_addr:?}");

            let http = Arc::clone(&http);
            let fut = async move {
                let response = respond(http, &mut stream)
                    .await
                    .unwrap_or(b"iNot found\tfake\t(NULL)\t0\r\n".to_vec());

                stream.write_all(&response).await?;
                stream.shutdown().await?;

                Ok(()) as io::Result<()>
            };

            tokio::spawn(async move {
                if let Err(err) = fut.await {
                    eprintln!("{:?}", err);
                }
            });
        }
    }
}

async fn respond(http: Arc<Http>, stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut request = String::new();
    loop {
        let c = stream.read_u8().await?;
        request.push(c as char);
        if request.len() > 65536 {
            // too long, no thanks
            return Ok(b"".to_vec());
        }
        // until it ends in \r\n\r\n
        if request.ends_with("\r\n\r\n") {
            break;
        }
    }

    // parse headers
    let mut headers = HashMap::new();
    let mut lines = request.lines();
    let request_line = lines.next().unwrap();
    for line in lines {
        let mut parts = line.splitn(2, ": ");
        let key = parts.next().unwrap_or_default().to_lowercase();
        if key.is_empty() {
            continue;
        }
        let value = parts.next().unwrap_or_default();
        headers.insert(key, value);
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    // ignore the version and hope it's fine. it should be http/1.1 anyways. :3
    let _version = parts.next().unwrap_or_default();

    println!("---");
    println!("request_line: {request_line:?}");
    println!("headers: {headers:?}");
    println!("HTTP request: {request:?}");
    println!("---");

    // parse query params
    let mut query_params = HashMap::new();
    let (path, query_string) = path.split_once('?').unwrap_or((path, ""));
    for pair in query_string.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        query_params.insert(key, value);
    }

    let content_length = headers
        .get("content-length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or_default()
        // 1mb of content. hopefully this is fine.
        .min(1024 * 1024);
    // read body
    let mut body = Vec::new();
    for _ in 0..content_length {
        body.push(stream.read_u8().await?);
    }

    let mut response = Vec::<u8>::new();

    match (path, method) {
        ("/qotd", "GET") => {
            response.extend(b"HTTP/1.1 200 OK\r\n");
            response.extend(b"Content-Type: text/plain\r\n");
            response.extend(b"\r\n");
            response.extend(http.qotd.message.read().as_slice());
        }
        ("/qotd", "POST") => {
            // validate the secret
            let expected_secret = tokio::fs::read_to_string(QOTD_SECRET_PATH)
                .await
                .unwrap_or_default();
            if !expected_secret.is_empty()
                && query_params.get("secret") == Some(&expected_secret.trim())
            {
                let qotd_content_str = String::from_utf8_lossy(&body);
                println!("changing qotd to \"{qotd_content_str}\"");
                let mut full_qotd = Vec::<u8>::new();
                full_qotd.extend(b"Quote of the day:\n");
                full_qotd.extend(&body);
                // add another \n if it's not there
                if full_qotd.last() != Some(&b'\n') {
                    full_qotd.push(b'\n');
                }

                // write to file
                tokio::fs::write(QOTD_MESSAGE_PATH, &full_qotd).await?;
                *http.qotd.message.write() = full_qotd;
                response.extend(b"HTTP/1.1 200 OK\r\n");
                response.extend(b"Content-Type: text/plain\r\n");
                response.extend(b"\r\n");
                response.extend(b"OK\n");
            } else {
                response.extend(b"HTTP/1.1 403 Forbidden\r\n");
                response.extend(b"Content-Type: text/plain\r\n");
                response.extend(b"\r\n");
                response.extend(b"Forbidden\n");
                return Ok(response);
            }
        }
        _ => {
            response.extend(b"HTTP/1.1 404 Not Found\r\n");
            response.extend(b"Content-Type: text/plain\r\n");
            response.extend(b"\r\n");
            response.extend(b"Not Found\n");
        }
    }

    Ok(response)
}
