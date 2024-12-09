use std::{
    collections::VecDeque,
    fs, io,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::RwLock;
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, UdpSocket},
    time::sleep,
};

use super::Protocol;
use crate::crawl::SiteData;

const BIND_HOST: &str = "[::]";
const BIND_PORT: u16 = {
    #[cfg(debug_assertions)]
    {
        1717
    }
    #[cfg(not(debug_assertions))]
    17
};

#[derive(Clone)]
pub struct Qotd {
    pub message: Arc<RwLock<String>>,
}

pub const QOTD_MESSAGE_PATH: &str = "data/qotd/message.txt";

impl Protocol for Qotd {
    fn generate(_: &SiteData) -> Self {
        // read message from file
        let message = fs::read_to_string(QOTD_MESSAGE_PATH).unwrap_or_default();

        Qotd {
            message: Arc::new(RwLock::new(message)),
        }
    }

    async fn serve(self) {
        // qotd runs on tcp and udp

        let qotd = Arc::new(self);

        let tcp_listener = match TcpListener::bind(format!("{BIND_HOST}:{BIND_PORT}")).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("failed to bind to port {BIND_PORT}: {e}");
                return;
            }
        };

        {
            let qotd = Arc::clone(&qotd);
            tokio::spawn(async move {
                loop {
                    let (mut stream, remote_addr) = tcp_listener.accept().await.unwrap();
                    println!("started tcp connection for qotd: {remote_addr:?}");

                    let qotd = Arc::clone(&qotd);
                    let fut = async move {
                        let response = qotd.message.read().as_bytes().to_vec();
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
            });
        }

        let mut udp_request_timestamps = VecDeque::<Instant>::new();

        let udp_listener = match UdpSocket::bind(format!("{BIND_HOST}:{BIND_PORT}")).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("failed to bind to udp port {BIND_PORT}: {e}");
                return;
            }
        };
        let mut buf = [0u8; 0];
        let mut ratelimited_until = None;
        loop {
            if let Ok((_, remote_addr)) = udp_listener.recv_from(&mut buf).await {
                if let Some(ratelimited_until_time) = ratelimited_until {
                    if Instant::now() < ratelimited_until_time {
                        continue;
                    }
                    ratelimited_until = None;
                }

                println!("received udp request for qotd: {remote_addr:?}");

                // if there's more than 120 requests in the past 60 seconds, wait until the
                // oldest request is older than 60 seconds.
                // this is to prevent us from becoming a ddos amplification vector.
                // sorry haylin.
                if udp_request_timestamps.len() > 120 {
                    let oldest = udp_request_timestamps.pop_front().unwrap();
                    let window = Duration::from_secs(60);
                    let elapsed = oldest.elapsed();
                    if elapsed < window {
                        println!("ratelimting qotd udp request from {remote_addr:?}");
                        ratelimited_until = Some(oldest + window);
                        continue;
                    }
                }
                udp_request_timestamps.push_back(Instant::now());

                let response = qotd.message.read().as_bytes().to_vec();
                let _ = udp_listener.send_to(&response, remote_addr).await;
            }
        }
    }
}
