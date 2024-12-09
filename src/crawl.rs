//! Obtain the project list and blog posts

use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

use async_recursion::async_recursion;
use chrono::{DateTime, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tl::{HTMLTag, Node, NodeHandle};
use tokio::fs;

const CRAWL_SCHEME: &str = "https";
const CRAWL_HOSTNAME: &str = "matdoes.dev";

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SiteData {
    pub projects: Vec<Project>,
    pub blog: Vec<Post>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
    pub name: String,
    pub href: Option<String>,
    pub source: Option<String>,
    #[serde(default)]
    pub languages: Vec<LanguageName>,
    pub description: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum LanguageName {
    Python,
    Svelte,
    Rust,
    TypeScript,
    JavaScript,
}
impl Display for LanguageName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LanguageName::Python => write!(f, "Python"),
            LanguageName::Svelte => write!(f, "Svelte"),
            LanguageName::Rust => write!(f, "Rust"),
            LanguageName::TypeScript => write!(f, "TypeScript"),
            LanguageName::JavaScript => write!(f, "JavaScript"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Post {
    pub title: String,
    pub slug: String,
    pub published: DateTime<Utc>,
    pub content: Vec<PostPart>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum PostPart {
    Text(String),
    InlineCode(String),
    CodeBlock(String),
    Italic(String),
    Bold(String),
    Image {
        src: ImageSource,
        alt: Option<String>,
    },
    Link {
        text: String,
        href: String,
    },
    LineBreak,
    Heading {
        level: usize,
        text: String,
    },
    Quote(String),
}
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum ImageSource {
    Local(PathBuf),
    Remote(String),
}

pub async fn crawl() -> Result<SiteData, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let projects = crawl_projects(&client).await?;
    let blog = crawl_blog(&client).await?;
    Ok(SiteData { projects, blog })
}

async fn crawl_projects(
    client: &reqwest::Client,
) -> Result<Vec<Project>, Box<dyn std::error::Error>> {
    println!("Crawling projects...");
    let url = format!("{CRAWL_SCHEME}://{CRAWL_HOSTNAME}/projects.json");
    let response = client.get(url).send().await?;
    let projects: Vec<Project> = response.json().await?;
    println!("Crawled {} projects", projects.len());
    Ok(projects)
}

async fn get_image(client: &reqwest::Client, image_url: &Url) -> PathBuf {
    // download the image
    let response = client.get(image_url.clone()).send().await.unwrap();
    let bytes = response.bytes().await.unwrap();
    let directory = Path::new("media").join(image_url.path().trim_start_matches('/'));

    println!("Saving image to {:#?}", directory);

    let parent_directory = directory.parent().unwrap();

    // make the media directory if it doesn't exist
    fs::create_dir_all(parent_directory).await.unwrap();
    fs::write(directory.clone(), bytes).await.unwrap();

    directory
}

async fn crawl_blog(client: &reqwest::Client) -> Result<Vec<Post>, Box<dyn std::error::Error>> {
    println!("Crawling blog...");
    let url = format!("{CRAWL_SCHEME}://{CRAWL_HOSTNAME}/blog.json");
    let response = client.get(url).send().await?;
    let posts_json: serde_json::Value = response.json().await?;

    let mut posts: Vec<Post> = Vec::new();

    // clear the media directory
    let _ = fs::remove_dir_all("media").await;

    for post_json in posts_json.as_array().unwrap() {
        let slug = post_json["slug"].as_str().unwrap();
        println!("Crawling {slug}...");
        let url = format!("{CRAWL_SCHEME}://{CRAWL_HOSTNAME}/{slug}.json");
        let response = client.get(&url).send().await?;
        let post_json: serde_json::Value = response.json().await?;

        fn html_escape(text: String) -> String {
            html_escape::decode_html_entities(&text).to_string()
        }

        fn html_tag_to_string(parser: &tl::Parser, element: &HTMLTag) -> String {
            html_escape(
                element
                    .children()
                    .all(parser)
                    .iter()
                    .map(|node| match node {
                        Node::Raw(text) => text.as_utf8_str().to_string(),
                        Node::Tag(element) => element
                            .children()
                            .all(parser)
                            .iter()
                            .map(|node| match node {
                                Node::Raw(text) => text.as_utf8_str().to_string(),
                                _ => "".to_string(),
                            })
                            .collect::<Vec<String>>()
                            .join(""),
                        _ => "".to_string(),
                    })
                    .collect::<Vec<String>>()
                    .join(""),
            )
        }

        #[async_recursion(?Send)]
        async fn parse_node(
            client: &reqwest::Client,
            parser: &tl::Parser,
            node: &NodeHandle,
            content: &mut Vec<PostPart>,
            slug: &str,
        ) {
            match node.get(parser).unwrap() {
                Node::Raw(text) => {
                    let text = html_escape(text.as_utf8_str().trim_end_matches('\n').to_string());
                    if !text.is_empty() {
                        content.push(PostPart::Text(text));
                    }
                }
                Node::Tag(element) => {
                    let element_name = element.name().as_utf8_str().to_string();

                    if matches!(element_name.as_str(), "p" | "pre" | "h1" | "h2" | "h3")
                        && !content.is_empty()
                    {
                        // sometimes there's random raw spaces in the html that aren't meant to be
                        // displayed
                        if content.last().unwrap() == &PostPart::Text(" ".to_owned()) {
                            content.pop();
                        }
                    }

                    match element_name.as_str() {
                        "img" => {
                            let src = element
                                .attributes()
                                .get("src")
                                .unwrap()
                                .as_ref()
                                .expect("all images must have a src")
                                .as_utf8_str()
                                .to_string();

                            // combine the base url with the src
                            let image_url =
                                Url::parse(&format!("{CRAWL_SCHEME}://{CRAWL_HOSTNAME}/{slug}"))
                                    .unwrap()
                                    .join(&src)
                                    .unwrap();

                            if image_url.host_str().unwrap() != CRAWL_HOSTNAME {
                                content.push(PostPart::Image {
                                    src: ImageSource::Remote(src.to_string()),
                                    alt: element
                                        .attributes()
                                        .get("alt")
                                        .unwrap()
                                        .map(|alt| alt.as_utf8_str().to_string()),
                                });
                                return;
                            }

                            let file_path = get_image(client, &image_url).await;

                            content.push(PostPart::Image {
                                src: ImageSource::Local(file_path.to_path_buf()),
                                alt: element
                                    .attributes()
                                    .get("alt")
                                    .unwrap()
                                    .map(|alt| alt.as_utf8_str().to_string()),
                            });
                        }
                        "a" => {
                            let href = element
                                .attributes()
                                .get("href")
                                .unwrap()
                                .as_ref()
                                .expect("all links must have a href")
                                .as_utf8_str()
                                .to_string();

                            content.push(PostPart::Link {
                                href: href.to_string(),
                                text: html_tag_to_string(parser, element),
                            });
                        }
                        "br" => {
                            content.push(PostPart::LineBreak);
                        }
                        "p" | "button" => {
                            if !content.is_empty() {
                                // sometimes there's random raw spaces in the html that aren't meant
                                // to be displayed
                                if content.last().unwrap() == &PostPart::Text(" ".to_owned()) {
                                    content.pop();
                                }
                            }
                            for child in element.children().top().iter() {
                                parse_node(client, parser, child, content, slug).await;
                            }
                            content.push(PostPart::LineBreak);
                        }
                        "code" => {
                            content.push(PostPart::InlineCode(html_tag_to_string(parser, element)));
                        }
                        "pre" => {
                            content.push(PostPart::CodeBlock(html_tag_to_string(parser, element)));
                        }
                        "blockquote" => {
                            content.push(PostPart::Quote(html_tag_to_string(parser, element)));
                        }
                        "em" | "i" => {
                            content.push(PostPart::Italic(html_tag_to_string(parser, element)));
                        }
                        "strong" | "b" => {
                            content.push(PostPart::Bold(html_tag_to_string(parser, element)));
                        }
                        "h1" => {
                            content.push(PostPart::Heading {
                                level: 1,
                                text: html_tag_to_string(parser, element),
                            });
                        }
                        "h2" => {
                            content.push(PostPart::Heading {
                                level: 2,
                                text: html_tag_to_string(parser, element),
                            });
                        }
                        "h3" => {
                            content.push(PostPart::Heading {
                                level: 3,
                                text: html_tag_to_string(parser, element),
                            });
                        }
                        "li" => {
                            content.push(PostPart::Text(" • ".to_owned()));
                            for child in element.children().top().iter() {
                                parse_node(client, parser, child, content, slug).await;
                            }
                            content.push(PostPart::LineBreak);
                        }
                        _ => {
                            for child in element.children().top().iter() {
                                parse_node(client, parser, child, content, slug).await;
                            }
                        }
                    }
                }
                Node::Comment(_) => {}
            }
        }

        let dom = tl::parse(
            post_json["html"].as_str().unwrap(),
            tl::ParserOptions::default(),
        )
        .unwrap();
        let parser = dom.parser();
        let mut content = Vec::new();
        for child in dom.children() {
            parse_node(client, parser, child, &mut content, slug).await;
        }

        let post = Post {
            title: post_json["title"].as_str().unwrap().to_string(),
            slug: slug.to_string(),
            // 2022-09-28T02:17:25.000Z
            published: DateTime::parse_from_rfc3339(post_json["published"].as_str().unwrap())?
                .into(),
            content,
        };
        posts.push(post);
    }

    Ok(posts)
}
