// From https://github.com/mainrs/terminal-link-rs

use std::fmt;

/// A clickable link component.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Link<'a> {
    pub url: &'a str,
}

impl<'a> Link<'a> {
    /// Create a new link with a target url.
    pub fn new(url: &'a str) -> Self {
        Self { url }
    }
}

impl fmt::Display for Link<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if check_hyperlink_support() {
            write!(
                f,
                "\u{1b}]8;;{}\u{1b}\\{}\u{1b}]8;;\u{1b}\\",
                self.url, self.url
            )
        } else {
            write!(f, "{}", self.url)
        }
    }
}

/// Check if the terminal has support for hyperlinks.
fn check_hyperlink_support() -> bool {
    if let Ok(term) = std::env::var("TERM") {
        term.contains("xterm") || term.contains("rxvt") || term.contains("kitty")
    } else {
        false
    }
}
