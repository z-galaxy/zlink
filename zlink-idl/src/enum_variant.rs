//! Enum variant definition for Varlink IDL.

use core::fmt;

use super::{Comment, List};

/// A single variant in an enum type definition.
#[derive(Debug, Clone, Eq)]
pub struct EnumVariant<'a> {
    /// The name of the variant.
    name: &'a str,
    /// The comments associated with this variant.
    comments: List<'a, Comment<'a>>,
}

impl<'a> EnumVariant<'a> {
    /// Creates a new enum variant with the given name and borrowed comments.
    pub const fn new(name: &'a str, comments: &'a [&'a Comment<'a>]) -> Self {
        Self {
            name,
            comments: List::Borrowed(comments),
        }
    }

    /// Creates a new enum variant with the given name and owned comments.
    pub fn new_owned(name: &'a str, comments: alloc::vec::Vec<Comment<'a>>) -> Self {
        Self {
            name,
            comments: List::from(comments),
        }
    }

    /// Returns the name of the variant.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns an iterator over the comments associated with this variant.
    pub fn comments(&self) -> impl Iterator<Item = &Comment<'a>> {
        self.comments.iter()
    }

    /// Returns true if this variant has any comments.
    pub fn has_comments(&self) -> bool {
        !self.comments.is_empty()
    }
}

impl<'a> fmt::Display for EnumVariant<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Comments first, each on its own line
        for comment in self.comments.iter() {
            writeln!(f, "{comment}")?;
        }
        // Then the variant name
        write!(f, "{}", self.name)
    }
}

impl<'a> PartialEq for EnumVariant<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.comments == other.comments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    #[test]
    fn simple_variant() {
        let variant = EnumVariant::new("active", &[]);
        assert_eq!(variant.name(), "active");
        assert!(!variant.has_comments());

        let mut displayed = String::new();
        write!(&mut displayed, "{}", variant).unwrap();
        assert_eq!(displayed, "active");
    }

    #[test]
    fn variant_with_comments() {
        let comment = Comment::new("The active state");
        let comments = [&comment];
        let variant = EnumVariant::new("active", &comments);

        assert_eq!(variant.name(), "active");
        assert!(variant.has_comments());

        let mut displayed = String::new();
        write!(&mut displayed, "{}", variant).unwrap();
        assert_eq!(displayed, "# The active state\nactive");
    }

    #[test]
    fn variant_with_multiple_comments() {
        let comment1 = Comment::new("First comment");
        let comment2 = Comment::new("Second comment");
        let comments = [&comment1, &comment2];
        let variant = EnumVariant::new("complex", &comments);

        assert_eq!(variant.name(), "complex");
        assert!(variant.has_comments());
        assert_eq!(variant.comments().count(), 2);

        let mut displayed = String::new();
        write!(&mut displayed, "{}", variant).unwrap();
        assert_eq!(displayed, "# First comment\n# Second comment\ncomplex");
    }

    #[test]
    fn owned_variant_with_comments() {
        let comments = alloc::vec![
            Comment::new("Owned comment 1"),
            Comment::new("Owned comment 2"),
        ];
        let variant = EnumVariant::new_owned("owned", comments);

        assert_eq!(variant.name(), "owned");
        assert!(variant.has_comments());

        let mut displayed = String::new();
        write!(&mut displayed, "{}", variant).unwrap();
        assert_eq!(displayed, "# Owned comment 1\n# Owned comment 2\nowned");
    }
}
