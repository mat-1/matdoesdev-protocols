use chrono::{DateTime, Utc};
use html_parser::{Dom, Node};

pub use crate::crawl::Project;
use crate::crawl::{CrawlResult, CrawledPost};

#[derive(Debug)]
pub struct SiteData {
    pub blog: Vec<Post>,
    pub projects: Vec<Project>,
}

#[derive(Debug)]
pub struct Post {
    pub title: String,
    pub slug: String,
    pub published: DateTime<Utc>,
    pub content: Vec<PostPart>,
}

#[derive(Debug)]
pub enum PostPart {
    Text(String),
    Image(String),
    Link { text: String, href: String },
    LineBreak,
}

impl From<CrawledPost> for Post {
    fn from(crawled_post: CrawledPost) -> Self {
        let title = crawled_post.title;
        let slug = crawled_post.slug;
        let published = crawled_post.published;
        let mut content = vec![];


        fn parse_node(node: &Node, content: &mut Vec<PostPart>) {
            match node {
                Node::Text(text) => {
                    content.push(PostPart::Text(text.to_string()));
                }
                Node::Element(element) => match element.name.as_str() {
                    "img" => {
                        let src = element
                            .attributes
                            .get("src")
                            .unwrap()
                            .as_ref()
                            .expect("all images must have a src");
                        content.push(PostPart::Image(src.to_string()));
                    }
                    "a" => {
                        let href = element
                            .attributes
                            .get("href")
                            .unwrap()
                            .as_ref()
                            .expect("all links must have a href");
                        fn node_to_string(node: &Node) -> String {
                            match node {
                                Node::Text(text) => text.to_string(),
                                Node::Element(element) => {
                                    let mut string = String::new();
                                    for child in &element.children {
                                        string.push_str(&node_to_string(child));
                                    }
                                    string
                                }
                                Node::Comment(_) => String::new(),
                            }
                        }
                        content.push(PostPart::Link {
                            href: href.to_string(),
                            text: element
                                .children
                                .iter()
                                .map(|child| node_to_string(child))
                                .collect(),
                        });
                    }
                    "br" => {
                        content.push(PostPart::LineBreak);
                    }
                    "p" => {
                        for child in &element.children {
                            parse_node(child, content);
                        }
                        content.push(PostPart::LineBreak);
                    }
                    _ => {
                        for child in &element.children {
                            parse_node(child, content);
                        }
                    }
                },
                Node::Comment(_) => {}
            }
        }

        let dom = Dom::parse(&crawled_post.html).unwrap();
        for child in &dom.children {
            parse_node(child, &mut content);
        }

        Post {
            title,
            slug,
            published,
            content,
        }
    }
}

impl From<CrawlResult> for SiteData {
    fn from(crawl_result: CrawlResult) -> Self {
        let blog = crawl_result
            .blog
            .into_iter()
            .map(|post| Post::from(post))
            .collect();
        let projects = crawl_result.projects;
        SiteData { blog, projects }
    }
}
