pub mod elements;

use elements::prelude::*;

use crate::crawl::{ImageSource, PostPart, SiteData};

/// A session for the terminal-based protocols (currently just ssh)
pub struct TerminalSession {
    location: Location,
    ctx: Context,
}

#[derive(Default)]
pub struct Context {
    width: usize,
    height: usize,

    site_data: SiteData,

    link_index: Option<usize>,
}

#[derive(Default, Clone, Debug)]
pub enum Location {
    #[default]
    Index,
    Blog,
    Projects,
    BlogPost {
        slug: String,
    },
}

impl TerminalSession {
    pub fn new(site_data: SiteData) -> Self {
        Self {
            location: Location::default(),
            ctx: Context {
                site_data,
                ..Default::default()
            },
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Vec<u8> {
        self.ctx.width = width as usize;
        self.ctx.height = height as usize;
        self.page().rendered
    }

    pub fn on_keystroke(&mut self, keys: &[u8]) -> Vec<u8> {
        let page = self.page();

        // tab
        if keys == [9] {
            if let Some(index) = self.ctx.link_index {
                self.ctx.link_index = Some((index + 1) % page.links.len());
            } else {
                self.ctx.link_index = Some(0);
            }
            return self.page().rendered;
        }
        // shift+tab
        else if keys == [27, 91, 90] {
            if let Some(index) = self.ctx.link_index {
                self.ctx.link_index = Some((index + page.links.len() - 1) % page.links.len());
            } else {
                self.ctx.link_index = Some(0);
            }
            return self.page().rendered;
        }
        // enter
        else if keys == [13] {
            if let Some(index) = self.ctx.link_index {
                if let Some(location) = page.links.get(index) {
                    self.location = location.clone();
                    self.ctx.link_index = None;
                    return self.page().rendered;
                }
            }
        }

        vec![]
    }

    fn page(&self) -> Page {
        match &self.location {
            Location::Index => index_page(&self.ctx),
            Location::Blog => blog_page(&self.ctx),
            Location::BlogPost { slug } => blog_post_page(&self.ctx, slug),
            Location::Projects => todo!(),
        }
    }
}

struct Page {
    rendered: Vec<u8>,
    links: Vec<Location>,
}

impl Page {
    pub fn new(ctx: &Context, max_width: usize, elements: Vec<Element>) -> Self {
        let width = max_width.min(ctx.width);
        let left = (ctx.width - width) / 2;

        let tree = Element::Rectangle {
            elements,
            rect: Rectangle {
                left,
                top: 0,
                width,
                height: ctx.height,
            },
        };

        let mut out: String = String::new();
        let mut data = elements::Data {
            links: vec![],
            link_index: ctx.link_index,
        };
        out.push_str(&"\x1b[2J\x1b[H"); // Clear screen
        out.push_str(&tree.render(
            &mut Position::default(),
            &Rectangle {
                left: 0,
                top: 0,
                width: ctx.width,
                height: ctx.height,
            },
            &mut data,
        ));
        out.push_str(&format!("\x1b[H")); // Move cursor to top left
        Page {
            rendered: out.as_bytes().to_vec(),
            links: data.links,
        }
    }
}

fn index_page(ctx: &Context) -> Page {
    Page::new(
        ctx,
        50,
        vec![
            // title
            text("\n"),
            bold(centered(white(text("matdoesdev")))),
            text("\n\n"),

            // socials
            centered(gray(container(vec![
                text("GitHub: "),
                external_link(text("mat-1"), "https://github.com/mat-1"),
            ]))),
            text("\n"),
            centered(gray(container(vec![
                text("Matrix: "),
                external_link(text("@mat:matdoes.dev"), "https://matrix.to/#/@mat:matdoes.dev"),
            ]))),
            text("\n"),
            centered(gray(container(vec![
                text("Ko-fi (donate): "),
                external_link(text("matdoesdev"), "https://ko-fi.com/matdoesdev"),
            ]))),

            text("\n\n"),

            // description
            text("I'm mat, I do full-stack software development.\n"),
            text("This portfolio contains my blog posts and links to some of the projects I've made.\n"),
            text("\n"),

            // links
            centered(container(vec![
                link(text("[Blog]"), Location::Blog),
                text(" "),
                link(text("[Projects]"), Location::Projects),
            ])),
            text("\n\n\n\n\n\n"),
            italic(gray(centered(text("(use tab to navigate links, enter to select)")))),
        ],
    )
}

fn blog_page(ctx: &Context) -> Page {
    let mut elements = vec![
        text("\n"),
        link(gray(text("← Home")), Location::Index),
        text("\n\n"),
        bold(white(text("Blog"))),
        text("\n\n\n"),
    ];
    for blog_post in &ctx.site_data.blog {
        elements.push(colorless_link(
            container(vec![
                text(&format!("{}", blog_post.title)),
                text("\n"),
                gray(text(&blog_post.published.format("%m/%d/%Y").to_string())),
            ]),
            Location::BlogPost {
                slug: blog_post.slug.clone(),
            },
        ));
        elements.push(text("\n\n"));
    }

    Page::new(ctx, 50, elements)
}

fn blog_post_page(ctx: &Context, slug: &str) -> Page {
    let Some(blog_post) = ctx.site_data.blog.iter().find(|p| p.slug == slug) else {
        // uhhhh idk go to index page ig
        return index_page(ctx);
    };

    let mut elements = vec![
        text("\n"),
        link(gray(text("← Back")), Location::Blog),
        text("\n\n"),
        bold(white(text(&blog_post.title))),
        text("\n"),
        gray(text(&blog_post.published.format("%m/%d/%Y").to_string())),
        text("\n\n"),
    ];

    let mut last_tag_was_line_break = false;
    for part in &blog_post.content {
        match part {
            PostPart::Text(t) => {
                elements.push(text(t));
            }
            PostPart::InlineCode(t) => {
                elements.push(italic(text(&format!("`{t}`"))));
            }
            PostPart::CodeBlock(t) => {
                elements.push(italic(text(&format!("```\n{t}\n```\n"))));
            }
            PostPart::Italic(t) => {
                elements.push(italic(text(t)));
            }
            PostPart::Bold(content) => {
                elements.push(bold(text(content)));
            }
            PostPart::Image { src, alt } => {
                let mut image_desc = String::new();
                image_desc.push_str("Image: ");
                if let Some(alt) = alt {
                    image_desc.push_str(alt);
                    image_desc.push_str(" (");
                }
                match src {
                    ImageSource::Local(path) => {
                        image_desc.push_str(&path.to_string_lossy());
                    }
                    ImageSource::Remote(path) => {
                        image_desc.push_str(&path);
                    }
                }
                if alt.is_some() {
                    image_desc.push_str(")");
                }
                elements.push(italic(gray(text(&format!("\n{image_desc}\n")))));
            }
            PostPart::Link { text: t, href } => {
                elements.push(external_link(text(t), href));
            }
            PostPart::LineBreak => {
                elements.push(text("\n\n"));
                last_tag_was_line_break = true;
                continue;
            }
            PostPart::Heading { level: _, text: t } => {
                if !last_tag_was_line_break {
                    elements.push(text("\n"));
                }
                elements.push(bold(white(text(&format!("{t}\n")))));
            }
            PostPart::Quote(t) => {
                elements.push(italic(text(&format!("> {t}\n"))));
            }
        }
        last_tag_was_line_break = false;
    }

    Page::new(ctx, 80, elements)
}
