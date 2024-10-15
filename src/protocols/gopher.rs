use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    io::{self},
    path::Path,
    sync::Arc,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
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
        7070
    }
    #[cfg(not(debug_assertions))]
    70
};

const INDEX_HEADER: &str = r#"                       888        888                                 888                   
                       888        888                                 888                   
                       888        888                                 888                   
88888b.d88b.   8888b.  888888 .d88888  .d88b.   .d88b.  .d8888b   .d88888  .d88b.  888  888 
888 "888 "88b     "88b 888   d88" 888 d88""88b d8P  Y8b 88K      d88" 888 d8P  Y8b 888  888 
888  888  888 .d888888 888   888  888 888  888 88888888 "Y8888b. 888  888 88888888 Y88  88P 
888  888  888 888  888 Y88b. Y88b 888 Y88..88P Y8b.          X88 Y88b 888 Y8b.      Y8bd8P  
888  888  888 "Y888888  "Y888 "Y88888  "Y88P"   "Y8888   88888P'  "Y88888  "Y8888    Y88P

I'm mat, I do full-stack software development.
This portfolio contains my blog posts and links to some of the projects I've made.
"#;

// => blog 📝 Blog
// => projects 💻 Projects

// => https://github.com/mat-1 GitHub
// => https://matrix.to/#/@mat:matdoes.dev Matrix
// => https://ko-fi.com/matdoesdev Ko-fi (donate)

#[derive(Clone)]
pub struct Gopher {
    pub index_content: String,
    pub blog_content: String,
    pub posts_content: HashMap<String, String>,
    pub projects_content: String,
}

pub struct Link {
    pub text: String,
    pub href: String,
}

#[derive(Default, Clone)]
pub struct GopherBuffer {
    pub buffer: String,
    pub out: String,
}

impl GopherBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&mut self, content: &str) {
        self.buffer.push_str(content);
    }

    pub fn line(&mut self, content: &str) {
        self.flush();
        if content.contains('\n') {
            for line in content.lines() {
                self.out.push_str(&format!("i{line}\tfake\tnull\t0\r\n"));
            }
        } else {
            self.out.push_str(&format!("i{content}\tfake\tnull\t0\r\n"));
        }
    }

    pub fn flush(&mut self) {
        let buffer = std::mem::take(&mut self.buffer);
        for line in buffer.lines() {
            // spaces at the beginning make lagrange format it as a codeblock
            let line = line.trim();
            self.out.push_str(&format!("i{line}\tfake\tnull\t0\r\n"));
        }
    }

    pub fn link(&mut self, href: &str, text: &str) {
        self.flush();
        for line in text.lines() {
            self.out
                .push_str(&format!("1{line}\t{href}\t{HOSTNAME}\t{BIND_PORT}\r\n"));
        }
    }

    pub fn image(&mut self, href: &str, alt: &str) {
        self.flush();
        self.out
            .push_str(&format!("I{alt}\t{href}\t{HOSTNAME}\t{BIND_PORT}\r\n"));
    }

    pub fn external_link(&mut self, href: &str, text: &str) {
        self.flush();
        for line in text.lines() {
            self.out
                .push_str(&format!("h{line}\tURL:{href}\t\t443\r\n"));
        }
    }
}

impl Display for GopherBuffer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut flushed = self.clone();
        flushed.flush();
        write!(f, "{}\r\n.", flushed.out)
    }
}

impl Protocol for Gopher {
    fn generate(data: &SiteData) -> Self {
        let mut index_content = GopherBuffer::new();
        index_content.line(INDEX_HEADER);

        index_content.line("");
        index_content.link("/blog", "Blog");
        index_content.link("/projects", "Projects");
        index_content.line("");
        index_content.external_link("https://github.com/mat-1", "GitHub");
        index_content.external_link("https://matrix.to/#/@mat:matdoes.dev", "Matrix");
        index_content.external_link("https://ko-fi.com/matdoesdev", "Ko-fi (donate)");

        let mut blog_content = GopherBuffer::new();
        blog_content.line("# Blog");
        blog_content.line("");

        let mut posts_content = HashMap::new();
        for post in &data.blog {
            let slug = &post.slug;
            let date = post.published.format("%Y-%m-%d").to_string();
            let title = &post.title;
            // add it to the index
            blog_content.link(&format!("/{slug}"), &format!("{date} - {title}"));
            // generate the content
            let mut out = GopherBuffer::new();

            out.line(&format!("# {title}"));
            out.line(&date);
            out.line("");

            let mut queued_links: Vec<Link> = Vec::new();
            for (i, part) in post.content.iter().enumerate() {
                match part {
                    PostPart::Text(content) => out.text(content),
                    PostPart::CodeBlock(content) => {
                        out.line(&format!("```\n{content}\n```\n"));
                    }
                    PostPart::InlineCode(text) => {
                        out.text(&format!("`{text}`"));
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
                                out.image(&local_path, &alt.to_owned().unwrap_or_default());
                            }
                            ImageSource::Remote(url) => {
                                out.external_link(url, &alt.to_owned().unwrap_or_default());
                            }
                        };
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
                            // out.pop();
                        } else {
                            out.text(text);
                        }
                    }
                    PostPart::LineBreak => {
                        out.flush();
                        if !queued_links.is_empty() {
                            // flush the queued links
                            for Link { href, text } in queued_links.drain(..) {
                                out.link(&href, &text);
                            }
                        }
                        continue;
                    }
                    PostPart::Heading { level, text } => match level {
                        1 => out.line(&format!("# {text}\n")),
                        2 => out.line(&format!("## {text}\n")),
                        3 => out.line(&format!("### {text}\n")),
                        _ => {}
                    },
                    PostPart::Italic(text) => {
                        out.line(&format!("*{text}*"));
                    }
                    PostPart::Bold(text) => {
                        out.line(&format!("**{text}**"));
                    }
                    PostPart::Quote(text) => {
                        for line in text.lines() {
                            out.line(&format!("> {line}\n"));
                        }
                    }
                }
            }
            // flush the queued links
            for Link { href, text } in queued_links.drain(..) {
                out.link(&href, &text);
            }

            // add the content to the posts map
            posts_content.insert(slug.to_string(), out.to_string());
        }

        // projects
        let mut projects_content = GopherBuffer::new();
        projects_content.line("Projects");
        for project in &data.projects {
            let name = &project.name;
            let description = &project.description;
            projects_content.line(&format!("## {name}\n"));
            projects_content.line(&format!("{description}\n"));

            // only include the link if it's different from the source
            if project.href != project.source {
                if let Some(href) = &project.href {
                    if href.starts_with('/') {
                        projects_content.link(href, href);
                    } else {
                        let pretty_href = href
                            .strip_prefix("https://")
                            .unwrap_or(href.strip_prefix("http://").unwrap_or(href));
                        let pretty_href = pretty_href.strip_suffix('/').unwrap_or(pretty_href);
                        projects_content.external_link(href, pretty_href);
                    }
                }
            }

            if let Some(source) = &project.source {
                if project.languages.is_empty() {
                    projects_content.external_link(source, "Source code");
                } else {
                    projects_content.external_link(
                        source,
                        &format!(
                            "Source code ({})\n",
                            project
                                .languages
                                .iter()
                                .map(|l| l.to_string())
                                .collect::<Vec<String>>()
                                .join(", ")
                        ),
                    );
                }
            } else if !project.languages.is_empty() {
                projects_content.line(&format!(
                    "Languages: {}",
                    project
                        .languages
                        .iter()
                        .map(|l| l.to_string())
                        .collect::<Vec<String>>()
                        .join(", ")
                ))
            }
        }

        Gopher {
            index_content: index_content.to_string(),
            blog_content: blog_content.to_string(),
            posts_content,
            projects_content: projects_content.to_string(),
        }
    }

    async fn serve(self) {
        // start a tcp server

        let gopher = Arc::new(self);

        let listener = TcpListener::bind(format!("{BIND_HOST}:{BIND_PORT}"))
            .await
            .unwrap();

        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            println!("started tcp connection");

            let gopher = Arc::clone(&gopher);
            let fut = async move {
                let response = respond(gopher, &mut stream)
                    .await
                    .unwrap_or(b"iNot found\tfake\t(NULL)\t0\r\n".to_vec());

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

async fn respond(gopher: Arc<Gopher>, stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut retreival_string = String::new();
    loop {
        let c = stream.read_u8().await?;
        if matches!(c, b'\n' | b'\t') {
            break;
        }
        retreival_string.push(c as char);
    }
    let retreival_string = retreival_string.trim_end_matches('\r').to_owned();

    println!("Gopher request: {retreival_string:?}");

    let content = match retreival_string.as_str() {
        "/" | "" => gopher.index_content.as_bytes().to_vec(),
        "/blog" => gopher.blog_content.as_bytes().to_vec(),
        "/projects" => gopher.projects_content.as_bytes().to_vec(),
        path => {
            let slug = match path.strip_prefix('/') {
                Some(slug) => slug,
                None => path,
            };
            // if it has another slash, that means it's media
            if slug.contains('/') {
                // get the path relative to the media directory
                let path = slug;
                // this feels completely safe and not dangerous at all

                let path = Path::new("media").join(path);
                if path
                    .components()
                    .into_iter()
                    .any(|x| matches!(x, std::path::Component::Normal(..)))
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
                content.extend_from_slice(b"\r\n");
                content
            } else {
                match gopher.posts_content.get(slug) {
                    Some(post) => post.as_bytes().to_vec(),
                    None => b"iNot found\tfake\t(NULL)\t0\r\n".to_vec(),
                }
            }
        }
    };

    Ok(content)
}
