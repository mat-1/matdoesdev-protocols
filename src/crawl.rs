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

#[derive(Serialize, Deserialize, Debug)]
pub struct SiteData {
    pub projects: Vec<Project>,
    pub blog: Vec<Post>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Project {
    pub name: String,
    pub href: Option<String>,
    pub source: Option<String>,
    #[serde(default)]
    pub languages: Vec<LanguageName>,
    pub description: String,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Post {
    pub title: String,
    pub slug: String,
    pub published: DateTime<Utc>,
    pub content: Vec<PostPart>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum PostPart {
    Text(String),
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
}
#[derive(Serialize, Deserialize, Debug)]
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
    let url = "https://matdoes.dev/projects.json";
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
    let url = "https://matdoes.dev/blog.json";
    let response = client.get(url).send().await?;
    let posts_json: serde_json::Value = response.json().await?;

    let mut posts: Vec<Post> = Vec::new();

    // clear the media directory
    let _ = fs::remove_dir_all("media").await;

    for post_json in posts_json.as_array().unwrap() {
        let slug = post_json["slug"].as_str().unwrap();
        println!("Crawling {slug}...");
        let url = format!("https://matdoes.dev/blog/{slug}.json");
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
                Node::Tag(element) => match element.name().as_utf8_str().to_string().as_str() {
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
                        let image_url = Url::parse("https://matdoes.dev/blog/{slug}")
                            .unwrap()
                            .join(&src)
                            .unwrap();

                        if image_url.host_str().unwrap() != "matdoes.dev" {
                            content.push(PostPart::Image {
                                src: ImageSource::Remote(src.to_string()),
                                alt: element
                                    .attributes()
                                    .get("alt")
                                    .unwrap()
                                    .clone()
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
                                .clone()
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
                    "p" => {
                        for child in element.children().top().iter() {
                            parse_node(client, parser, child, content, slug).await;
                        }
                        content.push(PostPart::LineBreak);
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
                    _ => {
                        for child in element.children().top().iter() {
                            parse_node(client, parser, child, content, slug).await;
                        }
                        content.push(PostPart::LineBreak);
                    }
                },
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
