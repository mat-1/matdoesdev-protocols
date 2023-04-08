mod cert;

use std::{
    collections::HashMap,
    io::{self},
    path::Path,
    sync::Arc,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::{
    crawl::{ImageSource, PostPart, SiteData},
    HOSTNAME,
};

use super::Protocol;

const INDEX_GMI: &str = r#"```matdoesdev
                       888        888                                 888                   
                       888        888                                 888                   
                       888        888                                 888                   
88888b.d88b.   8888b.  888888 .d88888  .d88b.   .d88b.  .d8888b   .d88888  .d88b.  888  888 
888 "888 "88b     "88b 888   d88" 888 d88""88b d8P  Y8b 88K      d88" 888 d8P  Y8b 888  888 
888  888  888 .d888888 888   888  888 888  888 88888888 "Y8888b. 888  888 88888888 Y88  88P 
888  888  888 888  888 Y88b. Y88b 888 Y88..88P Y8b.          X88 Y88b 888 Y8b.      Y8bd8P  
888  888  888 "Y888888  "Y888 "Y88888  "Y88P"   "Y8888   88888P'  "Y88888  "Y8888    Y88P
```

I'm mat, I do full-stack software development.
This portfolio contains my blog posts and links to some of the projects I've made.

=> blog 📝 Blog
=> projects 💻 Projects

=> https://github.com/mat-1 GitHub
=> https://matrix.to/#/@mat:matdoes.dev Matrix
"#;

#[derive(Clone)]
pub struct Gemini {
    pub blog_gmi: String,
    pub posts_gmi: HashMap<String, String>,
    pub projects_gmi: String,
}

pub struct Link {
    pub text: String,
    pub href: String,
}

impl Protocol for Gemini {
    fn generate(data: &SiteData) -> Self {
        let mut blog_gmi = String::new();
        blog_gmi.push_str("# Blog\n\n");

        let mut posts = HashMap::new();
        for post in &data.blog {
            let slug = &post.slug;
            let date = post.published.format("%Y-%m-%d").to_string();
            let title = &post.title;
            // add it to the index
            blog_gmi.push_str(&format!("=> /blog/{slug} {date} - {title}\n"));
            // generate the content
            let mut content = String::new();

            content.push_str(&format!("# {title}\n"));

            let mut queued_links: Vec<Link> = Vec::new();
            let mut last_tag_was_line_break = false;
            for part in &post.content {
                match part {
                    PostPart::Text(text) => content.push_str(text),
                    PostPart::Image { src, alt } => {
                        let href = match src {
                            ImageSource::Local(path) => {
                                // get the path relative to the media directory
                                path.to_string_lossy()
                                    .into_owned()
                                    .strip_prefix(
                                        &Path::new("media").to_string_lossy().into_owned(),
                                    )
                                    .unwrap()
                                    .to_string()
                            }
                            ImageSource::Remote(url) => url.to_owned(),
                        };
                        match alt {
                            Some(alt) => content.push_str(&format!("=> {href} {alt}\n")),
                            None => content.push_str(&format!("=> {href}\n")),
                        }
                    }
                    PostPart::Link { text, href } => {
                        queued_links.push(Link {
                            text: text.to_owned(),
                            href: href.to_owned(),
                        });
                        content.push_str(text);
                    }
                    PostPart::LineBreak => {
                        if !last_tag_was_line_break {
                            content.push('\n');
                        }
                        if !queued_links.is_empty() {
                            // flush the queued links
                            for Link { href, text } in queued_links.drain(..) {
                                content.push_str(&format!("=> {href} {text}\n"));
                            }
                        }
                        content.push('\n');
                        last_tag_was_line_break = true;
                        continue;
                    }
                    PostPart::Heading { level, text } => match level {
                        1 => content.push_str(&format!("# {text}\n")),
                        2 => content.push_str(&format!("## {text}\n")),
                        3 => content.push_str(&format!("### {text}\n")),
                        _ => {}
                    },
                }
                last_tag_was_line_break = false;
            }
            // flush the queued links
            for Link { href, text } in queued_links.drain(..) {
                content.push_str(&format!("=> {href} {text}\n"));
            }

            content.push_str(&format!("=> /blog ⬅ Back\n"));

            // add the content to the posts map
            posts.insert(slug.to_string(), content);
        }

        // projects
        let mut projects_gmi = String::new();
        projects_gmi.push_str("# Projects\n\n");
        for project in &data.projects {
            let name = &project.name;
            let description = &project.description;
            projects_gmi.push_str(&format!("## {name}\n"));
            projects_gmi.push_str(&format!("{description}\n"));

            // only include the link if it's different from the source
            if project.href != project.source {
                if let Some(href) = &project.href {
                    let pretty_href = href
                        .strip_prefix("https://")
                        .unwrap_or(href.strip_prefix("http://").unwrap_or(href));
                    let pretty_href = pretty_href.strip_suffix("/").unwrap_or(pretty_href);
                    projects_gmi.push_str(&format!("=> {href} {pretty_href}\n"))
                }
            }

            if let Some(source) = &project.source {
                if project.languages.is_empty() {
                    projects_gmi.push_str(&format!("=> {source} Source code\n"))
                } else {
                    projects_gmi.push_str(&format!(
                        "=> {source} Source code ({})\n",
                        project
                            .languages
                            .iter()
                            .map(|l| l.to_string())
                            .collect::<Vec<String>>()
                            .join(", ")
                    ))
                }
            } else {
                if !project.languages.is_empty() {
                    projects_gmi.push_str(&format!(
                        "Languages: {}\n",
                        project
                            .languages
                            .iter()
                            .map(|l| l.to_string())
                            .collect::<Vec<String>>()
                            .join(", ")
                    ))
                }
            }
        }

        Gemini {
            blog_gmi,
            posts_gmi: posts,
            projects_gmi,
        }
    }

    async fn serve(self) {
        // start a tcp server

        let gemini = Arc::new(self);

        let acceptor = cert::acceptor();
        let listener = TcpListener::bind("0.0.0.0:1965").await.unwrap();

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();

            let gemini = Arc::clone(&gemini);
            let fut = async move {
                let mut stream = acceptor.accept(stream).await?;

                let mut buffer = [0; 1024];
                let n = stream.read(&mut buffer).await?;
                let request = String::from_utf8_lossy(&buffer[..n]).to_string();

                println!("Gemini request: {request}");

                // strip "gemini://{hostname}" from the request
                let path = request
                    .strip_prefix(&format!("gemini://{}", HOSTNAME))
                    .unwrap_or(&request)
                    .split("\r\n")
                    .next()
                    .unwrap();

                let response: Vec<u8> = match path {
                    "/" => format!("20 text/gemini\r\n{INDEX_GMI}\r\n")
                        .as_bytes()
                        .to_vec(),
                    "/blog" => format!("20 text/gemini\r\n{}\r\n", gemini.blog_gmi)
                        .as_bytes()
                        .to_vec(),
                    "/projects" => format!("20 text/gemini\r\n{}\r\n", gemini.projects_gmi)
                        .as_bytes()
                        .to_vec(),
                    path if path.starts_with("/blog/") => {
                        let slug = path.strip_prefix("/blog/").unwrap();
                        // if it has another slash, that means it's media
                        if slug.contains('/') {
                            // get the path relative to the media directory
                            let path = slug;
                            let path = Path::new("media/blog").join(path);
                            let mime = mime_guess::from_path(&path).first_or_octet_stream();
                            let mime = mime.to_string();
                            println!("path: {path:?}, mime: {mime}");
                            let mut file = tokio::fs::File::open(path).await.unwrap();
                            let mut content = Vec::new();
                            file.read_to_end(&mut content).await.unwrap();
                            format!("20 {}\r\n", mime)
                                .as_bytes()
                                .to_vec()
                                .into_iter()
                                .chain(content)
                                .collect()
                        } else {
                            let post = gemini.posts_gmi.get(slug).unwrap();
                            format!("20 text/gemini\r\n{}\r\n", post)
                                .as_bytes()
                                .to_vec()
                        }
                    }
                    _ => "51 Not found\r\n".as_bytes().to_vec(),
                };
                stream.write_all(&response).await.unwrap();
                let _ = stream.shutdown().await;

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
