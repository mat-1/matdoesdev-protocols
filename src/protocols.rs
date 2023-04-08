use crate::crawl::SiteData;

pub mod gemini;

pub trait Protocol {
    fn generate(data: &SiteData) -> Self;
    async fn serve(self);
}
