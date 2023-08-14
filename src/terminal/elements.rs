use super::Location;

pub enum Element {
    // core
    Text(String),
    Centered(Box<Element>),
    Rectangle {
        elements: Vec<Element>,
        rect: Rectangle,
    },
    Container(Vec<Element>),

    // links
    Link {
        inner: Box<Element>,
        location: Location,
    },
    ExternalLink {
        inner: Box<Element>,
        url: String,
    },

    // formatting
    Formatted {
        inner: Box<Element>,
        format: String,
    },
}

#[derive(Debug)]
pub struct Rectangle {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Default, Debug, Clone)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Clone)]
pub struct Data {
    pub links: Vec<Location>,
    pub link_index: Option<usize>,
}

const RESET: &str = "\x1b[m";
fn move_cursor(pos: &Position) -> String {
    // 1-indexed
    format!("\x1b[{};{}H", pos.y + 1, pos.x + 1)
}

fn flush_word(pos: &mut Position, word: &mut String, parent_rect: &Rectangle, result: &mut String) {
    if pos.x + word.chars().count() > parent_rect.left + parent_rect.width {
        pos.x = parent_rect.left;
        pos.y += 1;
    }
    result.push_str(&move_cursor(pos));
    result.push_str(&word);
    pos.x += word.chars().count();
    word.clear();
}

impl Element {
    pub fn render(&self, pos: &mut Position, parent_rect: &Rectangle, data: &mut Data) -> String {
        let mut result = String::new();
        match self {
            Element::Text(text) => {
                let mut word = String::new();
                for c in text.chars() {
                    if c == ' ' {
                        flush_word(pos, &mut word, parent_rect, &mut result);
                        result.push_str(&" ");
                        pos.x += 1;
                    } else if c == '\t' {
                        flush_word(pos, &mut word, parent_rect, &mut result);
                        result.push_str(&"    ");
                        pos.x += 4;
                    } else if c == '\n' {
                        flush_word(pos, &mut word, parent_rect, &mut result);
                        pos.x = parent_rect.left;
                        pos.y += 1;
                    } else {
                        word.push(c);
                    }
                }
                flush_word(pos, &mut word, parent_rect, &mut result);
            }
            Element::Centered(inner) => {
                // render once to get length
                let initial_pos = pos.clone();
                inner.render(pos, &parent_rect, &mut data.clone());

                let length = if initial_pos.y == pos.y {
                    pos.x - initial_pos.x
                } else {
                    // if it wrapped to a new line, use the parent width
                    parent_rect.width
                };

                let left = parent_rect.left + (parent_rect.width - length) / 2;
                let rect = Rectangle {
                    left,
                    top: parent_rect.top,
                    width: length,
                    height: parent_rect.height,
                };
                pos.x = rect.left;
                result.push_str(&inner.render(pos, &rect, data));
            }
            Element::Rectangle { elements, rect } => {
                for element in elements {
                    let element_rendered = element.render(pos, rect, data);
                    result.push_str(&element_rendered);
                }
            }
            Element::Container(elements) => {
                for element in elements {
                    let element_rendered = element.render(pos, parent_rect, data);
                    result.push_str(&element_rendered);
                }
            }

            Element::Link { inner, location } => {
                data.links.push(location.clone());
                let selected = data.link_index == Some(data.links.len() - 1);
                if selected {
                    result.push_str("\x1b[1m");
                }
                result.push_str(&inner.render(pos, parent_rect, data));
                if selected {
                    result.push_str(RESET);
                }
            }
            Element::ExternalLink { inner, url } => {
                result.push_str("\x1b[4m"); // underline
                result.push_str(&format!("\x1b]8;;{url}\x1b\\"));
                result.push_str(&inner.render(pos, parent_rect, data));
                result.push_str("\x1b]8;;\x1b\\");
                result.push_str(RESET);
            }

            Element::Formatted { inner, format } => {
                result.push_str("\x1b[");
                result.push_str(format);
                result.push_str("m");
                result.push_str(&inner.render(pos, parent_rect, data));
                result.push_str(RESET);
            }
        }
        result
    }
}

pub mod prelude {
    pub use super::{
        bold, centered, colorless_link, container, external_link, gray, italic, link, rectangle,
        reset, text, white, Element, Position, Rectangle,
    };
}

pub fn text(text: &str) -> Element {
    Element::Text(text.to_string())
}
pub fn centered(inner: Element) -> Element {
    Element::Centered(Box::new(inner))
}
pub fn rectangle(elements: Vec<Element>, rect: Rectangle) -> Element {
    Element::Rectangle { elements, rect }
}
pub fn container(elements: Vec<Element>) -> Element {
    Element::Container(elements)
}
pub fn link(inner: Element, location: Location) -> Element {
    Element::Formatted {
        inner: Box::new(Element::Link {
            inner: Box::new(inner),
            location,
        }),
        format: "38;2;13;199;249".to_string(),
    }
}
pub fn colorless_link(inner: Element, location: Location) -> Element {
    Element::Link {
        inner: Box::new(inner),
        location,
    }
}
pub fn external_link(inner: Element, url: &str) -> Element {
    Element::ExternalLink {
        inner: Box::new(inner),
        url: url.to_string(),
    }
}

pub fn bold(inner: Element) -> Element {
    Element::Formatted {
        inner: Box::new(inner),
        format: "1".to_string(),
    }
}
pub fn italic(inner: Element) -> Element {
    Element::Formatted {
        inner: Box::new(inner),
        format: "3".to_string(),
    }
}
pub fn gray(inner: Element) -> Element {
    Element::Formatted {
        inner: Box::new(inner),
        format: "90".to_string(),
    }
}
pub fn white(inner: Element) -> Element {
    Element::Formatted {
        inner: Box::new(inner),
        format: "97".to_string(),
    }
}
pub fn reset(inner: Element) -> Element {
    Element::Formatted {
        inner: Box::new(inner),
        format: "".to_string(),
    }
}
