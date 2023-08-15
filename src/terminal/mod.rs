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

    scroll: usize,
}

#[derive(Default, Clone, Debug, Eq, PartialEq, Hash)]
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
                if let Some((location, _)) = page.links.get(index) {
                    self.location = location.clone();
                    self.ctx.scroll = 0;
                    self.ctx.link_index = None;
                    return self.page().rendered;
                }
            }
        }
        // down arrow key
        else if keys == [27, 91, 66] {
            self.ctx.scroll += 2;
            return self.page().rendered;
        }
        // scroll up
        else if keys == [27, 91, 65] {
            if self.ctx.scroll >= 2 {
                self.ctx.scroll -= 2;
            } else {
                self.ctx.scroll = 0;
            }
            return self.page().rendered;
        }
        // page up
        else if keys == [27, 91, 53, 126] {
            if self.ctx.scroll >= self.ctx.height {
                self.ctx.scroll -= self.ctx.height;
            } else {
                self.ctx.scroll = 0;
            }
            return self.page().rendered;
        }
        // page down
        else if keys == [27, 91, 54, 126] {
            self.ctx.scroll += self.ctx.height;
            return self.page().rendered;
        } else if let Some(keys) = keys.strip_prefix(&[27, 91, 60]) {
            // https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Extended-coordinates
            let Some((&last, keys)) = keys.split_last() else {
                return vec![];
            };
            let mut split = keys.split(|&k| k == 59);
            let Some(button_value) = split
                .next()
                .and_then(|c| String::from_utf8(c.to_vec()).ok())
            else {
                return vec![];
            };
            let Some(px) = split
                .next()
                .and_then(|c| String::from_utf8(c.to_vec()).ok())
                .and_then(|s| s.parse::<usize>().ok())
            else {
                return vec![];
            };
            let Some(py) = split
                .next()
                .and_then(|c| String::from_utf8(c.to_vec()).ok())
                .and_then(|s| s.parse::<usize>().ok())
            else {
                return vec![];
            };
            let is_pressed = last == b'M';

            match button_value.as_str() {
                "0" if is_pressed => {
                    // left mouse click
                    let page = self.page();
                    // find if we clicked on a link
                    let mouse_position = Position {
                        x: px as isize - 1,
                        y: py as isize - 1,
                    };
                    for (location, positions) in page.links {
                        if positions.contains(&mouse_position) {
                            self.location = location.clone();
                            self.ctx.scroll = 0;
                            self.ctx.link_index = None;
                            return self.page().rendered;
                        }
                    }
                }
                "65" => {
                    // scroll down
                    self.ctx.scroll += 2;
                    return self.page().rendered;
                }
                "64" => {
                    // scroll up
                    if self.ctx.scroll >= 2 {
                        self.ctx.scroll -= 2;
                    } else {
                        self.ctx.scroll = 0;
                    }
                    return self.page().rendered;
                }
                _ => {}
            }
        }

        vec![]
    }

    pub fn on_open(&self) -> Vec<u8> {
        let mut out = String::new();
        // hide the cursor
        out.push_str("\x1b[?25l");
        // don't line wrap
        out.push_str("\x1b[?7l");
        // mouse capturing
        out.push_str("\x1b[?1003h");
        // enable "extended coordinates"
        out.push_str("\x1b[?1006h");
        out.as_bytes().to_vec()
    }

    pub fn on_close(&self) -> Vec<u8> {
        let mut out = String::new();
        out.push_str("\x1b[?25h");
        out.push_str("\x1b[?7h");
        out.push_str("\x1b[?1003l");
        out.push_str("\x1b[?1006l");
        out.push_str("Bye!\r\n");
        out.as_bytes().to_vec()
    }

    fn page(&mut self) -> Page {
        match &self.location {
            Location::Index => index_page(&mut self.ctx),
            Location::Blog => blog_page(&mut self.ctx),
            Location::BlogPost { slug } => blog_post_page(&mut self.ctx, slug),
            Location::Projects => projects_page(&mut self.ctx),
        }
    }
}

struct Page {
    rendered: Vec<u8>,
    links: Vec<(Location, Vec<Position>)>,
}

impl Page {
    pub fn new(ctx: &mut Context, max_width: usize, elements: Vec<Element>) -> Self {
        let width = max_width.min(ctx.width);
        let left = (ctx.width - width) / 2;

        let tree = Element::Rectangle {
            elements: elements.clone(),
            rect: Rectangle {
                left: left as isize,
                top: -(ctx.scroll as isize),
                width,
                height: ctx.height,
            },
        };

        let mut out: String = String::new();
        let mut data = elements::Data {
            links: vec![],
            link_index: ctx.link_index,
        };
        out.push_str("\x1b[2J\x1b[H"); // Clear screen
        let mut position = Position {
            x: 0,
            y: -(ctx.scroll as isize),
        };
        let initial_position = position.clone();
        out.push_str(&tree.render(
            &mut position,
            // this one doesn't matter since it'll get overwritten by the Element::Rectangle
            &Rectangle {
                left: 0,
                top: 0,
                width: ctx.width,
                height: ctx.height,
            },
            // this is the window size
            &Rectangle {
                left: 0,
                top: 0,
                width: ctx.width,
                height: ctx.height,
            },
            &mut data,
        ));
        out.push_str("\x1b[H"); // Move cursor to top left

        let page_height = (position.y - initial_position.y) as usize;

        // clamp scroll
        let original_scroll = ctx.scroll;
        if ctx.scroll + ctx.height > page_height {
            ctx.scroll = isize::max(0, page_height as isize - ctx.height as isize) as usize;
            if ctx.scroll < original_scroll {
                // yes i know this is inefficient
                // i do not care
                return Self::new(ctx, max_width, elements);
            }
        }

        Page {
            rendered: out.as_bytes().to_vec(),
            links: data.links,
        }
    }
}

fn index_page(ctx: &mut Context) -> Page {
    Page::new(
        ctx,
        50,
        vec![
            vertically_centered(container(vec![
                // title
                text("\n"),
                bold(horizontally_centered(white(text("matdoesdev")))),
                text("\n\n"),

                // socials
                horizontally_centered(gray(container(vec![
                    text("GitHub: "),
                    external_link(text("mat-1"), "https://github.com/mat-1"),
                ]))),
                text("\n"),
                horizontally_centered(gray(container(vec![
                    text("Matrix: "),
                    external_link(text("@mat:matdoes.dev"), "https://matrix.to/#/@mat:matdoes.dev"),
                ]))),
                text("\n"),
                horizontally_centered(gray(container(vec![
                    text("Ko-fi (donate): "),
                    external_link(text("matdoesdev"), "https://ko-fi.com/matdoesdev"),
                ]))),

                text("\n\n"),

                // description
                text("I'm mat, I do full-stack software development.\n"),
                text("This portfolio contains my blog posts and links to some of the projects I've made.\n"),
                text("\n"),

                // links
                horizontally_centered(container(vec![
                    link(text("[Blog]"), Location::Blog),
                    text(" "),
                    link(text("[Projects]"), Location::Projects),
                ])),
                text("\n"),
            ])),
            text("\n\n\n\n"),
            italic(gray(horizontally_centered(text("(use tab to navigate links, enter to select)")))),
        ],
    )
}

fn blog_page(ctx: &mut Context) -> Page {
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
                text(&blog_post.title),
                text("\n"),
                gray(text(&blog_post.published.format("%m/%d/%Y").to_string())),
            ]),
            Location::BlogPost {
                slug: blog_post.slug.clone(),
            },
        ));
        elements.push(text("\n\n"));
    }

    Page::new(ctx, 80, elements)
}

fn blog_post_page(ctx: &mut Context, slug: &str) -> Page {
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
                        image_desc.push_str(path);
                    }
                }
                if alt.is_some() {
                    image_desc.push(')');
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

fn projects_page(ctx: &mut Context) -> Page {
    let mut elements = vec![
        text("\n"),
        link(gray(text("← Home")), Location::Index),
        text("\n\n"),
        bold(white(text("Projects"))),
        text("\n\n"),
    ];
    for project in &ctx.site_data.projects {
        let mut project_name = bold(text(&project.name));
        if let Some(href) = &project.href {
            project_name = external_link(project_name, href);
        }
        elements.push(project_name);
        if let Some(source) = &project.source {
            elements.push(text(" "));
            elements.push(gray(external_link(text("(Source)"), source)));
        }
        elements.push(text("\n"));
        if !project.languages.is_empty() {
            elements.push(gray(text(&format!(
                "Languages: {}",
                project
                    .languages
                    .iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))));
            elements.push(text("\n"));
        }
        elements.push(text(&project.description));

        elements.push(text("\n\n"));
    }

    Page::new(ctx, 80, elements)
}
