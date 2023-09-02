use std::{collections::HashMap, path::Path, sync::Arc};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{tcp::OwnedReadHalf, TcpListener},
};

use crate::{
    crawl::{ImageSource, PostPart, SiteData},
    HOSTNAME,
};

use super::Protocol;

const BIND_HOST: &str = "[::]";
const BIND_PORT: u16 = {
    #[cfg(debug_assertions)]
    {
        7979
    }
    #[cfg(not(debug_assertions))]
    79
};

#[derive(Clone)]
pub struct Finger {
    pub index_content: String,
    pub blog_content: String,
    pub projects_content: String,
    pub posts_content: HashMap<String, String>,
}

impl Protocol for Finger {
    fn generate(data: &SiteData) -> Self {
        let mut blog_content = String::new();
        blog_content.push_str("# Blog\n\n");
        for post in &data.blog {
            let date = post.published.format("%Y-%m-%d").to_string();
            blog_content.push_str(&format!(
                "{date} - {title}\n{slug}@{HOSTNAME}\n\n",
                title = post.title,
                slug = post.slug,
            ));
        }

        let mut posts_content = HashMap::new();
        for post in &data.blog {
            let slug = &post.slug;
            let date = post.published.format("%Y-%m-%d").to_string();
            let title = &post.title;
            // generate the content
            let mut out = String::new();

            out.push_str(&format!("# {title}\n{date}\n\n"));

            for part in post.content.iter() {
                match part {
                    PostPart::Text(content) => out.push_str(content),
                    PostPart::CodeBlock(content) => {
                        out.push_str(&format!("\n```\n{content}\n```\n"));
                    }
                    PostPart::InlineCode(text) => {
                        out.push_str(&format!("`{text}`"));
                    }
                    PostPart::Image { src, alt } => {
                        match src {
                            ImageSource::Local(path) => {
                                // get the path relative to the media directory
                                let local_path = path
                                    .to_string_lossy()
                                    .into_owned()
                                    .strip_prefix(
                                        &Path::new("media").to_string_lossy().into_owned(),
                                    )
                                    .unwrap()
                                    .to_string();
                                if let Some(alt) = alt {
                                    out.push_str(&format!("![{alt}]({local_path})"));
                                } else {
                                    out.push_str(&format!("![]({local_path})"));
                                }
                            }
                            ImageSource::Remote(url) => {
                                // out.external_link(url, &alt.to_owned().unwrap_or_default());
                                if let Some(alt) = alt {
                                    out.push_str(&format!("![{alt}]({url})"));
                                } else {
                                    out.push_str(&format!("![]({url})"));
                                }
                            }
                        };
                    }
                    PostPart::Link { text, href } => {
                        if let Some(href) = href.strip_prefix('/') {
                            out.push_str(&format!("[{text}]({href}@{HOSTNAME})"));
                        } else {
                            out.push_str(&format!("[{text}]({href})"));
                        }
                    }
                    PostPart::LineBreak => {
                        out.push('\n');
                        continue;
                    }
                    PostPart::Heading { level, text } => match level {
                        1 => out.push_str(&format!("\n# {text}\n")),
                        2 => out.push_str(&format!("\n## {text}\n")),
                        3 => out.push_str(&format!("\n### {text}\n")),
                        _ => out.push_str(&format!("\n{text}\n")),
                    },
                    PostPart::Italic(text) => {
                        out.push_str(&format!("*{text}*"));
                    }
                    PostPart::Bold(text) => {
                        out.push_str(&format!("**{text}**"));
                    }
                    PostPart::Quote(text) => {
                        for line in text.lines() {
                            out.push_str(&format!("\n> {line}\n"));
                        }
                    }
                }
            }
            // add the content to the posts map
            posts_content.insert(slug.to_string(), out.to_string());
        }

        let mut projects_content = String::new();
        projects_content.push_str("# Projects\n\n");
        for project in &data.projects {
            let name = &project.name;
            let description = &project.description;
            projects_content.push_str(&format!("## {name}\n{description}\n"));

            // only include the link if it's different from the source
            if project.href != project.source {
                if let Some(href) = &project.href {
                    if let Some(href) = href.strip_prefix('/') {
                        projects_content.push_str(&format!("{href}@{HOSTNAME}\n"));
                    } else {
                        projects_content.push_str(&format!("{href}\n"));
                    }
                }
            }

            if let Some(source) = &project.source {
                if project.languages.is_empty() {
                    projects_content.push_str(&format!("Source code: {source}\n"));
                } else {
                    projects_content.push_str(&format!(
                        "Source code ({}): {source}\n",
                        project
                            .languages
                            .iter()
                            .map(|l| l.to_string())
                            .collect::<Vec<String>>()
                            .join(", ")
                    ));
                }
            } else if !project.languages.is_empty() {
                projects_content.push_str(&format!(
                    "Languages: {}\n",
                    project
                        .languages
                        .iter()
                        .map(|l| l.to_string())
                        .collect::<Vec<String>>()
                        .join(", ")
                ))
            }

            projects_content.push('\n');
        }

        Finger {
            index_content: format!(
                r#"{INDEX_HEADER}
Blog: blog@{HOSTNAME}
Projects: projects@{HOSTNAME}

GitHub: https://github.com/mat-1
Matrix: https://matrix.to/#/@mat:matdoes.dev
Ko-fi (donate): https://ko-fi.com/matdoesdev"#
            ),
            blog_content,
            posts_content,
            projects_content,
        }
    }

    async fn serve(self) {
        let listener = TcpListener::bind(format!("{BIND_HOST}:{BIND_PORT}"))
            .await
            .unwrap();
        let finger = Arc::new(self);

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            println!("started tcp connection");

            let (read, mut write) = stream.into_split();

            let finger = Arc::clone(&finger);
            tokio::spawn(async move {
                match respond(finger, read).await {
                    Ok(response) => {
                        write
                            .write_all(
                                format!(
                                    "{}\r\n",
                                    response.replace("\r\n", "\n").replace('\n', "\r\n").trim()
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    Err(e) => {
                        println!("error: {}", e);
                    }
                }
            });
        }
    }
}

const INDEX_HEADER: &str = r#"                                   matdoesdev

I'm mat, I do full-stack software development.
This portfolio contains my blog posts and links to some of the projects I've made.
"#;

async fn respond(finger: Arc<Finger>, mut read: OwnedReadHalf) -> anyhow::Result<String> {
    // read until \r\n

    let mut request = String::new();
    loop {
        let mut buf = [0u8; 1];
        read.read_exact(&mut buf).await?;
        request.push(buf[0] as char);
        if request.ends_with("\r\n") {
            request.pop();
            request.pop();
            break;
        }
    }
    let request = request.trim();
    println!("Finger request: {request}");

    match request {
        "" => Ok(finger.index_content.clone()),
        "blog" => Ok(finger.blog_content.clone()),
        "projects" => Ok(finger.projects_content.clone()),
        _ => {
            if let Some(post) = finger.posts_content.get(request) {
                return Ok(post.clone());
            }
            Ok("Not found".to_string())
        }
    }
}
