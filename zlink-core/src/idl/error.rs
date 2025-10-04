//! Error definitions for Varlink IDL.

use core::fmt;

use alloc::vec::Vec;

use super::{Comment, Field, List};

/// An error definition in Varlink IDL.
#[derive(Debug, Clone, Eq)]
pub struct Error<'a> {
    /// The name of the error.
    name: &'a str,
    /// The fields of the error.
    fields: List<'a, Field<'a>>,
    /// Comments associated with this error.
    comments: List<'a, Comment<'a>>,
}

impl<'a> Error<'a> {
    /// Creates a new error with the given name, borrowed fields, and comments.
    pub const fn new(
        name: &'a str,
        fields: &'a [&'a Field<'a>],
        comments: &'a [&'a Comment<'a>],
    ) -> Self {
        Self {
            name,
            fields: List::Borrowed(fields),
            comments: List::Borrowed(comments),
        }
    }

    /// Creates a new error with the given name, owned fields, and comments.
    /// Same as `new` but takes `fields` by value.
    pub fn new_owned(name: &'a str, fields: Vec<Field<'a>>, comments: Vec<Comment<'a>>) -> Self {
        Self {
            name,
            fields: List::from(fields),
            comments: List::from(comments),
        }
    }

    /// Returns the name of the error.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns an iterator over the fields of the error.
    pub fn fields(&self) -> impl Iterator<Item = &Field<'a>> {
        self.fields.iter()
    }

    /// Returns true if the error has no fields.
    pub fn has_no_fields(&self) -> bool {
        self.fields.is_empty()
    }

    /// Returns the comments associated with this error.
    pub fn comments(&self) -> impl Iterator<Item = &Comment<'a>> {
        self.comments.iter()
    }
}

impl<'a> fmt::Display for Error<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Comments first
        for comment in self.comments.iter() {
            writeln!(f, "{comment}")?;
        }
        write!(f, "error {} (", self.name)?;
        let mut first = true;
        for field in self.fields.iter() {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            write!(f, "{field}")?;
        }
        write!(f, ")")
    }
}

impl<'a> PartialEq for Error<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.fields == other.fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idl::Type;

    #[test]
    fn error_creation() {
        let message_field = Field::new("message", &Type::String, &[]);
        let code_field = Field::new("code", &Type::Int, &[]);
        let fields = [&message_field, &code_field];

        let error = Error::new("InvalidInput", &fields, &[]);
        assert_eq!(error.name(), "InvalidInput");
        assert_eq!(error.fields().count(), 2);
        assert!(!error.has_no_fields());

        // Check the fields individually - order and values.
        let fields_vec: mayheap::Vec<_, 8> = error.fields().collect();
        assert_eq!(fields_vec[0].name(), "message");
        assert_eq!(fields_vec[0].ty(), &Type::String);
        assert_eq!(fields_vec[1].name(), "code");
        assert_eq!(fields_vec[1].ty(), &Type::Int);
    }

    #[test]
    fn error_no_fields() {
        let error = Error::new("UnknownError", &[], &[]);
        assert_eq!(error.name(), "UnknownError");
        assert!(error.has_no_fields());
    }

    #[test]
    fn display_with_comments() {
        use crate::idl::Comment;
        use core::fmt::Write;

        let comment1 = Comment::new("Authentication failed");
        let comment2 = Comment::new("Invalid credentials provided");
        let comments = [&comment1, &comment2];

        let message_field = Field::new("message", &Type::String, &[]);
        let code_field = Field::new("code", &Type::Int, &[]);
        let fields = [&message_field, &code_field];

        let error = Error::new("AuthError", &fields, &comments);
        let mut displayed = mayheap::String::<128>::new();
        write!(&mut displayed, "{}", error).unwrap();
        assert_eq!(
            displayed,
            "# Authentication failed\n# Invalid credentials provided\nerror AuthError (message: string, code: int)"
        );
    }
}
