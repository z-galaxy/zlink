//! Object type definition for Varlink IDL.

use core::fmt;

use alloc::vec::Vec;

use zlink_names::TypeName;

use super::{Field, List};

/// An object type definition in Varlink IDL (struct-like with named fields).
#[derive(Debug, Clone, Eq)]
pub struct CustomObject<'a> {
    /// The name of the object type.
    name: TypeName<'a>,
    /// The fields of the object type.
    fields: List<'a, Field<'a>>,
    /// The comments associated with this object type.
    comments: List<'a, super::Comment<'a>>,
}

impl<'a> CustomObject<'a> {
    /// Creates a new object type with the given name, borrowed fields, and comments.
    ///
    /// As this is a `const fn`, it takes a name that is already validated. In `const` contexts,
    /// make one with [`TypeName::from_static_str_unchecked`]. For anything else that converts
    /// into a [`TypeName`], such as a string slice, use [`CustomObject::new_owned`].
    pub const fn new(
        name: TypeName<'a>,
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
        comments: Vec<super::Comment<'a>>,
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

    /// Returns the name of the object type.
    pub fn name(&self) -> &TypeName<'a> {
        &self.name
    }

    /// Returns an iterator over the fields of the object type.
    pub fn fields(&self) -> impl Iterator<Item = &Field<'a>> {
        self.fields.iter()
    }

    /// The fields as a slice.
    pub const fn fields_as_slice(&self) -> Option<&[&Field<'a>]> {
        self.fields.as_borrowed()
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
    use crate::{Comment, Field, Type};
    use core::fmt::Write;

    #[test]
    fn display_with_comments() {
        let comment1 = Comment::new("User data structure");
        let comment2 = Comment::new("Contains basic user information");
        let comments = [&comment1, &comment2];

        let name_field = Field::new("name".try_into().unwrap(), &Type::String, &[]);
        let age_field = Field::new("age".try_into().unwrap(), &Type::Int, &[]);
        let fields = [&name_field, &age_field];

        let custom_object = CustomObject::new("User".try_into().unwrap(), &fields, &comments);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", custom_object).unwrap();
        assert_eq!(
            displayed,
            "# User data structure\n# Contains basic user information\ntype User (name: string, age: int)"
        );
    }

    #[test]
    fn new_owned_rejects_an_invalid_name() {
        let object = CustomObject::new_owned("User", Vec::new(), Vec::new()).unwrap();
        assert_eq!(object.name(), "User");

        assert_eq!(
            CustomObject::new_owned("user", Vec::new(), Vec::new()).unwrap_err(),
            zlink_names::Error::InvalidTypeName
        );
    }
}
