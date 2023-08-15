use super::Location;

#[derive(Clone)]
pub enum Element {
    // core
    Text(String),
    HorizontallyCentered(Box<Element>),
    VerticallyCentered(Box<Element>),
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

#[derive(Debug, Clone)]
pub struct Rectangle {
    pub left: isize,
    pub top: isize,
    pub width: usize,
    pub height: usize,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Position {
    pub x: isize,
    pub y: isize,
}

#[derive(Debug, Clone)]
pub struct Data {
    pub links: Vec<(Location, Vec<Position>)>,
    pub link_index: Option<usize>,
}

const RESET: &str = "\x1b[m";
fn move_cursor(pos: &Position) -> String {
    // 1-indexed
    format!("\x1b[{};{}H", pos.y + 1, pos.x + 1)
}

/// Write the word while doing line wrapping. Returns whether the word was inside of the window.
fn flush_word(
    pos: &mut Position,
    word: &mut String,
    parent_rect: &Rectangle,
    window: &Rectangle,
    result: &mut String,
) -> bool {
    let word_length = word.chars().count();
    if pos.x + word_length as isize > parent_rect.left + parent_rect.width as isize {
        pos.x = parent_rect.left;
        pos.y += 1;
    }

    let in_window = pos.y >= 0 && pos.y < window.height as isize;
    if in_window {
        result.push_str(&move_cursor(pos));
        result.push_str(&word);
    }
    pos.x += word_length as isize;
    word.clear();

    in_window
}

impl Element {
    pub fn render(
        &self,
        pos: &mut Position,
        parent_rect: &Rectangle,
        window: &Rectangle,
        data: &mut Data,
    ) -> String {
        let mut result = String::new();
        match self {
            Element::Text(text) => {
                let mut word = String::new();
                for c in text.chars() {
                    if c == ' ' {
                        if flush_word(pos, &mut word, parent_rect, window, &mut result) {
                            result.push_str(&" ");
                        }
                        pos.x += 1;
                    } else if c == '\t' {
                        if flush_word(pos, &mut word, parent_rect, window, &mut result) {
                            result.push_str(&"    ");
                        }
                        pos.x += 4;
                    } else if c == '\n' {
                        flush_word(pos, &mut word, parent_rect, window, &mut result);
                        pos.x = parent_rect.left;
                        pos.y += 1;
                    } else {
                        word.push(c);
                    }
                }
                flush_word(pos, &mut word, parent_rect, window, &mut result);
            }
            Element::HorizontallyCentered(inner) => {
                // render once to get length
                let initial_pos = pos.clone();
                inner.render(pos, &parent_rect, window, &mut data.clone());

                let width = if initial_pos.y == pos.y {
                    (pos.x - initial_pos.x) as usize
                } else {
                    // if it wrapped to a new line, use the parent width
                    parent_rect.width
                };

                let left = parent_rect.left + ((parent_rect.width - width) as isize) / 2;
                let rect = Rectangle {
                    left,
                    top: parent_rect.top,
                    width,
                    height: parent_rect.height,
                };
                pos.x = rect.left;
                result.push_str(&inner.render(pos, &rect, window, data));
            }
            Element::VerticallyCentered(inner) => {
                // render once to get height
                let initial_pos = pos.clone();
                inner.render(pos, &parent_rect, window, &mut data.clone());

                let height = usize::min((pos.y - initial_pos.y) as usize, parent_rect.height);

                let top = parent_rect.top + ((parent_rect.height - height) as isize) / 2;
                let rect = Rectangle {
                    left: parent_rect.left,
                    top,
                    width: parent_rect.width,
                    height,
                };
                pos.y = rect.top;
                result.push_str(&inner.render(pos, &rect, window, data));
            }
            Element::Rectangle { elements, rect } => {
                for element in elements {
                    let element_rendered = element.render(pos, rect, window, data);
                    result.push_str(&element_rendered);
                }
            }
            Element::Container(elements) => {
                for element in elements {
                    let element_rendered = element.render(pos, parent_rect, window, data);
                    result.push_str(&element_rendered);
                }
            }

            Element::Link { inner, location } => {
                let start_pos = pos.clone();
                let selected = data.link_index == Some(data.links.len());
                if selected {
                    result.push_str("\x1b[1m");
                }
                result.push_str(&inner.render(pos, parent_rect, window, data));
                if selected {
                    result.push_str(RESET);
                }

                // i was too lazy to make wrapping work
                let mut positions = Vec::new();
                for x in start_pos.x..=pos.x {
                    for y in start_pos.y..=pos.y {
                        positions.push(Position { x, y });
                    }
                }
                data.links.push((location.clone(), positions));
            }
            Element::ExternalLink { inner, url } => {
                result.push_str("\x1b[4m"); // underline
                result.push_str(&format!("\x1b]8;;{url}\x1b\\"));
                result.push_str(&inner.render(pos, parent_rect, window, data));
                result.push_str("\x1b]8;;\x1b\\");
                result.push_str(RESET);
            }

            Element::Formatted { inner, format } => {
                result.push_str("\x1b[");
                result.push_str(format);
                result.push_str("m");
                result.push_str(&inner.render(pos, parent_rect, window, data));
                result.push_str(RESET);
            }
        }
        result
    }
}

pub mod prelude {
    pub use super::{
        bold, colorless_link, container, external_link, gray, horizontally_centered, italic, link,
        rectangle, reset, text, vertically_centered, white, Element, Position, Rectangle,
    };
}

pub fn text(text: &str) -> Element {
    Element::Text(text.to_string())
}
pub fn horizontally_centered(inner: Element) -> Element {
    Element::HorizontallyCentered(Box::new(inner))
}
pub fn vertically_centered(inner: Element) -> Element {
    Element::VerticallyCentered(Box::new(inner))
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
