use crate::crawl::SiteData;

pub mod finger;
pub mod gemini;
pub mod gopher;
pub mod http;
pub mod qotd;
pub mod ssh;
pub mod telnet;

pub trait Protocol {
    fn generate(data: &SiteData) -> Self;
    async fn serve(self);
}
