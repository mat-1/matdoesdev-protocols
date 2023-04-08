#![allow(incomplete_features)]
#![feature(async_fn_in_trait)]

use tokio::fs;

use crate::protocols::Protocol;

mod crawl;
mod protocols;

const HOSTNAME: &str = "matdoes.dev";

#[tokio::main]
async fn main() {
    println!("Hello, world!");

    // read from the cache if it exists

    let use_cache = false;

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

    gemini.serve().await;

    // println!("{:?}", crawl_result);
}
