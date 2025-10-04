//! Object type definition for Varlink IDL.

use core::fmt;

use alloc::vec::Vec;

use super::{Field, List};

/// An object type definition in Varlink IDL (struct-like with named fields).
#[derive(Debug, Clone, Eq)]
pub struct CustomObject<'a> {
    /// The name of the object type.
    name: &'a str,
    /// The fields of the object type.
    fields: List<'a, Field<'a>>,
    /// The comments associated with this object type.
    comments: List<'a, super::Comment<'a>>,
}

impl<'a> CustomObject<'a> {
    /// Creates a new object type with the given name, borrowed fields, and comments.
    pub const fn new(
        name: &'a str,
        fields: &'a [&'a Field<'a>],
        comments: &'a [&'a super::Comment<'a>],
    ) -> Self {
        Self {
            name,
            fields: List::Borrowed(fields),
            comments: List::Borrowed(comments),
        }
    }

    /// Creates a new object type with the given name, owned fields, and comments.
    pub fn new_owned(
        name: &'a str,
        fields: Vec<Field<'a>>,
        comments: Vec<super::Comment<'a>>,
    ) -> Self {
        Self {
            name,
            fields: List::from(fields),
            comments: List::from(comments),
        }
    }

    /// Returns the name of the object type.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns an iterator over the fields of the object type.
    pub fn fields(&self) -> impl Iterator<Item = &Field<'a>> {
        self.fields.iter()
    }

    /// Returns an iterator over the comments associated with this object type.
    pub fn comments(&self) -> impl Iterator<Item = &super::Comment<'a>> {
        self.comments.iter()
    }
}

impl<'a> fmt::Display for CustomObject<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Comments first
        for comment in self.comments.iter() {
            writeln!(f, "{comment}")?;
        }
        write!(f, "type {} (", self.name)?;
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

impl<'a> PartialEq for CustomObject<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.fields == other.fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idl::{Comment, Field, Type};
    use core::fmt::Write;

    #[test]
    fn display_with_comments() {
        let comment1 = Comment::new("User data structure");
        let comment2 = Comment::new("Contains basic user information");
        let comments = [&comment1, &comment2];

        let name_field = Field::new("name", &Type::String, &[]);
        let age_field = Field::new("age", &Type::Int, &[]);
        let fields = [&name_field, &age_field];

        let custom_object = CustomObject::new("User", &fields, &comments);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", custom_object).unwrap();
        assert_eq!(
            displayed,
            "# User data structure\n# Contains basic user information\ntype User (name: string, age: int)"
        );
    }
}
