pub mod elements;

use elements::prelude::*;

/// A session for the terminal-based protocols (currently just ssh)
#[derive(Default)]
pub struct TerminalSession {
    location: Location,
    ctx: Context,
}

#[derive(Default)]
pub struct Context {
    width: usize,
    height: usize,
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Vec<u8> {
        self.ctx.width = width as usize;
        self.ctx.height = height as usize;
        self.render()
    }

    pub fn on_keystroke(&mut self, keys: &[u8]) -> Vec<u8> {
        let page = self.page();
        // if keys.len() == 1 {
        //     if let Some(char) = char::from_u32(keys[0] as u32) {
        //         match char {
        //             '0'..='9' => {
        //                 let index = keys[0] - b'0';
        //                 println!("index: {index}");
        //                 let Some(location) = page.links.get(index as usize).cloned() else {
        //                     return vec![];
        //                 };
        //                 self.location = location;
        //                 return self.render();
        //             }
        //             _ => {}
        //         }
        //     }
        // }

        vec![]
    }

    fn page(&self) -> Page {
        match &self.location {
            Location::Index => index_page(&self.ctx),
            Location::Blog => blog_page(&self.ctx),
            _ => todo!(),
        }
    }

    fn render(&self) -> Vec<u8> {
        self.page().render(&self.ctx)
    }
}

struct Page {
    tree: Element,
}

impl Page {
    pub fn new(ctx: &Context, max_width: usize, elements: Vec<Element>) -> Self {
        let width = max_width.min(ctx.width);
        let left = (ctx.width - width) / 2;

        Page {
            tree: Element::Rectangle {
                elements,
                rect: Rectangle {
                    left,
                    top: 0,
                    width,
                    height: ctx.height,
                },
            },
        }
    }

    pub fn render(&self, ctx: &Context) -> Vec<u8> {
        let mut out: String = String::new();

        out.push_str(&"\x1b[2J\x1b[H"); // Clear screen
        out.push_str(&self.tree.render(
            &mut Position::default(),
            &Rectangle {
                left: 0,
                top: 0,
                width: ctx.width,
                height: ctx.height,
            },
        ));
        out.push_str(&format!("\x1b[H")); // Move cursor to top left

        out.as_bytes().to_vec()
    }
}

fn index_page(ctx: &Context) -> Page {
    let page = Page::new(
        ctx,
        50,
        vec![
            // title
            text("\n"),
            bold(centered(text("matdoesdev"))),
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
        ],
    );

    // page.text("\n\n ");
    // page.link("Blog", Location::Blog);
    // page.text("\n ");
    // page.link("Projects", Location::Projects);

    // page.text("\n\n");

    // page.text(&format!(
    //     "GitHub: {}\n",
    //     external_link("github.com/mat-1", "https://github.com/mat-1")
    // ));
    // page.text(&format!(
    //     "Matrix: {}\n",
    //     external_link("@mat:matdoes.dev", "https://matrix.to/#/@mat:matdoes.dev")
    // ));
    // page.text(&format!(
    //     "Ko-fi (donate): {}\n",
    //     external_link("ko-fi.com/matdoesdev", "https://ko-fi.com/matdoesdev")
    // ));

    // page.text("\n");
    // page.text(&format!(
    //     "\x1b[90m(use numbers or tab+enter to click links){RESET}\n"
    // ));

    page
}

fn blog_page(ctx: &Context) -> Page {
    let mut page = Page::new(ctx, 50, vec![]);

    // page.link("← Home", Location::Index);
    // page.text("\n\n");
    // page.text(&bold("Blog"));
    // page.text("\n\n\n");

    page
}
