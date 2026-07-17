//! Comment definitions for Varlink interfaces.

use core::fmt;

/// A comment in a Varlink interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment<'a> {
    /// The comment text content (without the leading #).
    content: &'a str,
}

impl<'a> Comment<'a> {
    /// Creates a new comment with the given text content (without # prefix).
    pub const fn new(content: &'a str) -> Self {
        Self { content }
    }

    /// Returns the comment text content.
    pub fn content(&self) -> &'a str {
        self.content
    }

    /// Returns the comment text content.
    pub fn text(&self) -> &'a str {
        self.content
    }
}

impl<'a> fmt::Display for Comment<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "# {}", self.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_creation() {
        let comment = Comment::new("This is a comment");
        assert_eq!(comment.content(), "This is a comment");
        assert_eq!(comment.text(), "This is a comment");
    }

    #[test]
    fn comment_without_hash() {
        let comment = Comment::new("This is not a proper comment");
        assert_eq!(comment.content(), "This is not a proper comment");
        assert_eq!(comment.text(), "This is not a proper comment");
    }

    #[test]
    fn comment_display() {
        let comment = Comment::new("A enum field allowing to gracefully get metadata");
        use core::fmt::Write;
        let mut buf = String::new();
        write!(buf, "{}", comment).unwrap();
        assert_eq!(
            buf.as_str(),
            "# A enum field allowing to gracefully get metadata"
        );
    }
}
