#![allow(incomplete_features)]
#![feature(cursor_split)]

use tokio::fs;

use crate::protocols::Protocol;

mod crawl;
mod protocols;
pub mod terminal;

const HOSTNAME: &str = "matdoes.dev";

#[tokio::main]
async fn main() {
    println!("Hello, world!");

    // read from the cache if it exists
    // mainly meant for debugging
    let use_cache = cfg!(debug_assertions);

    let data = if use_cache {
        if let Ok(cache) = fs::read_to_string("cache.json").await {
            serde_json::from_str(&cache).unwrap()
        } else {
            panic!("no cache");
        }
    } else {
        let crawl_result = crawl::crawl().await.unwrap();
        // write the results to a cache
        fs::write("cache.json", serde_json::to_string(&crawl_result).unwrap())
            .await
            .unwrap();
        crawl_result
    };

    println!("now serving");

    let gemini = protocols::gemini::Gemini::generate(&data);
    let ssh = protocols::ssh::Ssh::generate(&data);
    let telnet = protocols::telnet::Telnet::generate(&data);
    let gopher = protocols::gopher::Gopher::generate(&data);
    let finger = protocols::finger::Finger::generate(&data);
    let qotd = protocols::qotd::Qotd::generate(&data);
    let mut http = protocols::http::Http::generate(&data);

    http.qotd = qotd.clone();

    tokio::join!(
        gemini.serve(),
        ssh.serve(),
        telnet.serve(),
        gopher.serve(),
        finger.serve(),
        qotd.serve(),
        http.serve()
    );

    // println!("{:?}", crawl_result);
}
