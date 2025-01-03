mod cert;

use std::{
    collections::HashMap,
    io::{self},
    path::Path,
    sync::Arc,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::server::TlsStream;
use url::Url;

use crate::{
    crawl::{ImageSource, PostPart, SiteData},
    HOSTNAME,
};

use super::Protocol;

const BIND_HOST: &str = "[::]";
const BIND_PORT: u16 = 1965;

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
=> https://ko-fi.com/matdoesdev Ko-fi (donate)
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
            blog_gmi.push_str(&format!("=> /{slug} {date} - {title}\n"));
            // generate the content
            let mut content = String::new();

            content.push_str(&format!("# {title}\n"));
            content.push_str(&format!("{date}\n\n"));

            let mut queued_links: Vec<Link> = Vec::new();
            let mut last_tag_was_line_break = false;
            for (i, part) in post.content.iter().enumerate() {
                match part {
                    PostPart::Text(text) => content.push_str(text),
                    PostPart::CodeBlock(text) => {
                        content.push_str(&format!("```\n{text}\n```\n"));
                    }
                    PostPart::InlineCode(text) => {
                        content.push_str(&format!("`{text}`"));
                    }
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
                            href: match href {
                                h if h.starts_with("https://gemini.circumlunar.space/") => {
                                    // replace the https:// with gemini://
                                    h.replacen("https://", "gemini://", 1)
                                }
                                h if h.starts_with("https://gmi.skyjake.fi/") => {
                                    // replace the https://gmi. with gemini://
                                    h.replacen("https://gmi.", "gemini://", 1)
                                }
                                h => h.to_owned(),
                            },
                        });
                        // add the link text unless the part before and after are line breaks
                        let before_is_line_break =
                            i == 0 || matches!(post.content[i - 1], PostPart::LineBreak);
                        let after_is_line_break = i == post.content.len() - 1
                            || matches!(post.content[i + 1], PostPart::LineBreak);
                        if before_is_line_break && after_is_line_break {
                            // remove the last line break too
                            content.pop();
                        } else {
                            content.push_str(text);
                        }
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
                    PostPart::Italic(text) => {
                        content.push_str(&format!("*{text}*"));
                    }
                    PostPart::Bold(text) => {
                        content.push_str(&format!("**{text}**"));
                    }
                    PostPart::Quote(text) => {
                        for line in text.lines() {
                            content.push_str(&format!("> {line}\n"));
                        }
                    }
                }
                last_tag_was_line_break = false;
            }
            // flush the queued links
            for Link { href, text } in queued_links.drain(..) {
                content.push_str(&format!("=> {href} {text}\n"));
            }

            content.push_str("=> /blog ⬅ Back\n");

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
                    let pretty_href = pretty_href.strip_suffix('/').unwrap_or(pretty_href);
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
            } else if !project.languages.is_empty() {
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
        let listener = match TcpListener::bind(format!("{BIND_HOST}:{BIND_PORT}")).await {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("failed to bind to port {BIND_PORT}: {e}");
                return;
            }
        };

        loop {
            let (stream, remote_addr) = listener.accept().await.unwrap();
            println!("started tcp connection for gemini: {remote_addr:?}");
            let acceptor = acceptor.clone();

            let gemini = Arc::clone(&gemini);
            let fut = async move {
                let mut stream = acceptor.accept(stream).await?;
                println!("wrapped stream in tls");

                let response = respond(gemini, &mut stream)
                    .await
                    .unwrap_or(b"59 Internal error\r\n".to_vec());

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
    }
}

async fn respond(
    gemini: Arc<Gemini>,
    stream: &mut TlsStream<TcpStream>,
) -> std::io::Result<Vec<u8>> {
    let mut request = [0; 1026];
    let mut len = 0;
    loop {
        let mut buffer = [0; 1027];
        let Ok(n) = stream.read(&mut buffer).await else {
            return Ok(b"59 Couldn't receive request\r\n".to_vec());
        };
        println!("read {n} bytes: {}", String::from_utf8_lossy(&buffer[..n]));
        if n == 0 {
            break;
        }
        if n + len > request.len() {
            return Ok(b"59 Request is too large\r\n".to_vec());
        }
        // add the new data to the request
        request[len..len + n].copy_from_slice(&buffer[..n]);
        len += n;
        if buffer.contains(&b'\r') {
            break;
        }
    }
    // ignore everything after the first \r
    let request = request[..len].split(|v| v == &b'\r').next().unwrap();
    let Ok(request) = std::str::from_utf8(request) else {
        return Ok(b"59 Request is not UTF-8\r\n".to_vec());
    };

    println!("Gemini request: {request}");

    let Ok(url) = Url::parse(request) else {
        return Ok(b"59 Request is not a valid URL\r\n".to_vec());
    };

    if url.scheme() != "gemini" {
        return Ok(b"53 Request is not a Gemini URL\r\n".to_vec());
    };
    if url.host_str() != Some(HOSTNAME) {
        return Ok(b"53 Host doesn't match\r\n".to_vec());
    };
    if url.port().unwrap_or(BIND_PORT) != BIND_PORT {
        return Ok(b"53 Port doesn't match\r\n".to_vec());
    };

    Ok(match url.path() {
        "/" | "" => format!("20 text/gemini\r\n{INDEX_GMI}\n")
            .as_bytes()
            .to_vec(),
        "/blog" => format!("20 text/gemini\r\n{}\n", gemini.blog_gmi)
            .as_bytes()
            .to_vec(),
        "/projects" => format!("20 text/gemini\r\n{}\n", gemini.projects_gmi)
            .as_bytes()
            .to_vec(),
        path => {
            let slug = match path.strip_prefix('/') {
                Some(slug) => slug,
                None => path,
            };
            // if it has another slash, that means it's media
            if slug.contains('/') {
                // get the path relative to the media directory
                let path = slug;
                let path = Path::new("media").join(path);
                // this feels completely safe and not dangerous at all
                if !path
                    .components()
                    .all(|x| matches!(x, std::path::Component::Normal(..)))
                {
                    return Ok(b"inyaa~ >_<\tfake\t(NULL)\t0\r\n".to_vec());
                }
                let mime = mime_guess::from_path(&path).first_or_octet_stream();
                let mime = mime.to_string();
                println!("path: {path:?}, mime: {mime}");
                let Ok(mut file) = tokio::fs::File::open(path).await else {
                    return Ok(b"iNot found\tfake\t(NULL)\t0\r\n".to_vec());
                };
                let mut content = Vec::new();
                let _ = file.read_to_end(&mut content).await;
                format!("20 {}\r\n", mime)
                    .as_bytes()
                    .iter()
                    .copied()
                    .chain(content)
                    .collect()
            } else {
                match gemini.posts_gmi.get(slug) {
                    Some(post) => format!("20 text/gemini\r\n{}\r\n", post)
                        .as_bytes()
                        .to_vec(),
                    None => b"51 Not found\r\n".to_vec(),
                }
            }
        }
    })
}
