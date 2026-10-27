//! Varlink type names.

use zcheapstr::CheapStr;

use crate::parse;

/// A Varlink type name, e.g. `FileNotFound`.
///
/// Custom types, methods and errors are named this way: an uppercase ASCII letter followed by
/// ASCII letters and digits.
///
/// # Examples
///
/// ```
/// use zlink_names::TypeName;
///
/// let name = TypeName::try_from("FileNotFound")?;
/// assert_eq!(name, "FileNotFound");
/// assert!(TypeName::try_from("fileNotFound").is_err());
/// # Ok::<(), zlink_names::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeName<'a>(pub(crate) CheapStr<'a>);

/// An owned [`TypeName`], independent of the data it was created from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnedTypeName(TypeName<'static>);

impl_name!(
    TypeName,
    OwnedTypeName,
    is_valid_type_name,
    crate::Error::InvalidTypeName,
    "a Varlink type name"
);

/// Whether `name` is a valid Varlink type name.
///
/// Method and error names follow this rule too.
///
/// # Examples
///
/// ```
/// use zlink_names::is_valid_type_name;
///
/// assert!(is_valid_type_name("FileNotFound"));
/// assert!(!is_valid_type_name("fileNotFound"));
/// ```
pub fn is_valid_type_name(name: &str) -> bool {
    parse::parses_fully(parse::type_name, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, is_valid_field_name};

    #[test]
    fn type_names() {
        let valid = ["A", "Foo", "FileNotFound", "Foo0", "X"];
        for name in valid {
            assert!(is_valid_type_name(name), "`{name}` must be valid");
        }

        let invalid = [
            "", "foo", "_Foo", "0Foo", "Foo_bar", "Foo-bar", "Foo bar", "Foo.Bar", "Foo!", "Über",
        ];
        for name in invalid {
            assert!(!is_valid_type_name(name), "`{name}` must be invalid");
        }
    }

    /// Type names are the stricter rule: every one of them is also a valid field name.
    #[test]
    fn type_names_are_field_names() {
        for name in ["A", "Foo", "FileNotFound", "Foo0"] {
            assert!(is_valid_field_name(name), "`{name}` must be valid");
        }
    }

    #[test]
    fn try_from() {
        let name = TypeName::try_from("FileNotFound").unwrap();
        assert_eq!(name.as_str(), "FileNotFound");
        let name = TypeName::try_from(String::from("FileNotFound")).unwrap();
        assert_eq!(name.as_str(), "FileNotFound");

        assert_eq!(
            TypeName::try_from("fileNotFound"),
            Err(Error::InvalidTypeName)
        );
        assert_eq!(
            TypeName::try_from(String::from("File_not_found")),
            Err(Error::InvalidTypeName)
        );
    }

    #[test]
    fn owned_try_from() {
        let name = OwnedTypeName::try_from("FileNotFound").unwrap();
        assert_eq!(name.as_str(), "FileNotFound");
        let name = OwnedTypeName::try_from(String::from("FileNotFound")).unwrap();
        assert_eq!(name.as_str(), "FileNotFound");

        assert_eq!(
            OwnedTypeName::try_from("fileNotFound"),
            Err(Error::InvalidTypeName)
        );
        assert_eq!(
            OwnedTypeName::try_from(String::new()),
            Err(Error::InvalidTypeName)
        );
    }
}
