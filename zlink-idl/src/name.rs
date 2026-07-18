//! Validating names against the Varlink grammar.
//!
//! These run the actual IDL parsers, so a name is accepted here exactly when the parser would
//! accept it in an interface definition -- there is no second copy of the grammar to drift from it.

use winnow::error::{ErrMode, InputError};

use crate::parse;

/// Whether `name` is a valid Varlink field name.
///
/// Struct fields, method parameters and enum variants all follow this rule.
///
/// # Examples
///
/// ```
/// use zlink_idl::is_valid_field_name;
///
/// assert!(is_valid_field_name("user_name"));
/// assert!(!is_valid_field_name("user-name"));
/// ```
pub fn is_valid_field_name(name: &str) -> bool {
    parses_fully(parse::field_name, name)
}

/// Whether `name` is a valid Varlink type name.
///
/// Method and error names follow this rule too.
///
/// # Examples
///
/// ```
/// use zlink_idl::is_valid_type_name;
///
/// assert!(is_valid_type_name("FileNotFound"));
/// assert!(!is_valid_type_name("fileNotFound"));
/// ```
pub fn is_valid_type_name(name: &str) -> bool {
    parses_fully(parse::type_name, name)
}

/// Whether `name` is a valid Varlink interface name.
///
/// Interface names are reverse-domain notation: dot-separated segments, at least one dot.
///
/// # Examples
///
/// ```
/// use zlink_idl::is_valid_interface_name;
///
/// assert!(is_valid_interface_name("org.example.Foo"));
/// assert!(!is_valid_interface_name("Foo"));
/// ```
pub fn is_valid_interface_name(name: &str) -> bool {
    parses_fully(parse::interface_name, name)
}

/// Whether `parser` accepts the whole of `name`, leaving nothing behind.
///
/// The parsers succeed on a valid prefix (e.g. `field_name` takes `foo` out of `foo-bar`), so a
/// name is only valid when parsing also consumes every byte.
fn parses_fully<'a, P, T>(mut parser: P, name: &'a str) -> bool
where
    P: FnMut(&mut &'a [u8]) -> Result<T, ErrMode<InputError<&'a [u8]>>>,
{
    let mut input = name.as_bytes();

    parser(&mut input).is_ok() && input.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_names() {
        let valid = ["a", "Z", "user_name", "user0", "a_0_b", "type", "x__"];
        for name in valid {
            assert!(is_valid_field_name(name), "`{name}` must be valid");
        }

        let invalid = [
            "",
            "_foo",
            "0foo",
            "user-name",
            "user name",
            "user.name",
            "user!",
            "üser",
            "user\u{e9}",
        ];
        for name in invalid {
            assert!(!is_valid_field_name(name), "`{name}` must be invalid");
        }
    }

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
    fn interface_names() {
        let valid = ["org.example.Foo", "a.b", "org.varlink.service", "a1.b-2.c3"];
        for name in valid {
            assert!(is_valid_interface_name(name), "`{name}` must be valid");
        }

        let invalid = [
            "",
            "Foo",          // No dot.
            "org.",         // Trailing dot, empty segment.
            ".org",         // Leading dot.
            "org..example", // Empty segment.
            "org.example.", // Trailing dot.
            "1org.example", // First segment must start with a letter.
            "org.example.Foo bar",
            "org.example.Foö",
        ];
        for name in invalid {
            assert!(!is_valid_interface_name(name), "`{name}` must be invalid");
        }
    }
}
