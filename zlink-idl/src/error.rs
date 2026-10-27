//! Error definitions for Varlink IDL.

use core::fmt;

use alloc::vec::Vec;

use zlink_names::TypeName;

use super::{Comment, Field, List};

/// An error definition in Varlink IDL.
#[derive(Debug, Clone, Eq)]
pub struct Error<'a> {
    /// The name of the error.
    name: TypeName<'a>,
    /// The fields of the error.
    fields: List<'a, Field<'a>>,
    /// Comments associated with this error.
    comments: List<'a, Comment<'a>>,
}

impl<'a> Error<'a> {
    /// Creates a new error with the given name, borrowed fields, and comments.
    ///
    /// As this is a `const fn`, it takes a name that is already validated. In `const` contexts,
    /// make one with [`TypeName::from_static_str_unchecked`]. For anything else that converts
    /// into a [`TypeName`], such as a string slice, use [`Error::new_owned`].
    pub const fn new(
        name: TypeName<'a>,
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
    ///
    /// The name can be anything that converts into a [`TypeName`], such as a string slice.
    ///
    /// # Errors
    ///
    /// If `name` doesn't convert into a [`TypeName`], e.g. because it is a string that is not a
    /// valid type name.
    pub fn new_owned<N>(
        name: N,
        fields: Vec<Field<'a>>,
        comments: Vec<Comment<'a>>,
    ) -> Result<Self, N::Error>
    where
        N: TryInto<TypeName<'a>>,
    {
        Ok(Self {
            name: name.try_into()?,
            fields: List::from(fields),
            comments: List::from(comments),
        })
    }

    /// Returns the name of the error.
    pub fn name(&self) -> &TypeName<'a> {
        &self.name
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
    use crate::Type;

    #[test]
    fn error_creation() {
        let message_field = Field::new("message".try_into().unwrap(), &Type::String, &[]);
        let code_field = Field::new("code".try_into().unwrap(), &Type::Int, &[]);
        let fields = [&message_field, &code_field];

        let error = Error::new("InvalidInput".try_into().unwrap(), &fields, &[]);
        assert_eq!(error.name(), "InvalidInput");
        assert_eq!(error.fields().count(), 2);
        assert!(!error.has_no_fields());

        // Check the fields individually - order and values.
        let fields_vec: Vec<_> = error.fields().collect();
        assert_eq!(fields_vec[0].name(), "message");
        assert_eq!(fields_vec[0].ty(), &Type::String);
        assert_eq!(fields_vec[1].name(), "code");
        assert_eq!(fields_vec[1].ty(), &Type::Int);
    }

    #[test]
    fn error_no_fields() {
        let error = Error::new("UnknownError".try_into().unwrap(), &[], &[]);
        assert_eq!(error.name(), "UnknownError");
        assert!(error.has_no_fields());
    }

    #[test]
    fn new_owned_rejects_an_invalid_name() {
        let error = Error::new_owned("UnknownError", Vec::new(), Vec::new()).unwrap();
        assert_eq!(error.name(), "UnknownError");

        assert_eq!(
            Error::new_owned("Unknown-Error", Vec::new(), Vec::new()).unwrap_err(),
            zlink_names::Error::InvalidTypeName
        );
    }

    #[test]
    fn display_with_comments() {
        use crate::Comment;
        use core::fmt::Write;

        let comment1 = Comment::new("Authentication failed");
        let comment2 = Comment::new("Invalid credentials provided");
        let comments = [&comment1, &comment2];

        let message_field = Field::new("message".try_into().unwrap(), &Type::String, &[]);
        let code_field = Field::new("code".try_into().unwrap(), &Type::Int, &[]);
        let fields = [&message_field, &code_field];

        let error = Error::new("AuthError".try_into().unwrap(), &fields, &comments);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", error).unwrap();
        assert_eq!(
            displayed,
            "# Authentication failed\n# Invalid credentials provided\nerror AuthError (message: string, code: int)"
        );
    }
}
