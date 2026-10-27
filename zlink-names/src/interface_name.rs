//! Varlink interface names.

use zcheapstr::CheapStr;

use crate::parse;

/// A Varlink interface name, e.g. `org.example.Foo`.
///
/// Interface names are in reverse-domain notation: at least two dot-separated segments of ASCII
/// letters, digits and hyphens. The first segment starts with a letter, and no segment starts or
/// ends with a hyphen.
///
/// # Examples
///
/// ```
/// use zlink_names::InterfaceName;
///
/// let name = InterfaceName::try_from("org.example.Foo")?;
/// assert_eq!(name, "org.example.Foo");
/// assert!(InterfaceName::try_from("Foo").is_err());
/// # Ok::<(), zlink_names::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceName<'a>(pub(crate) CheapStr<'a>);

/// An owned [`InterfaceName`], independent of the data it was created from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnedInterfaceName(InterfaceName<'static>);

impl_name!(
    InterfaceName,
    OwnedInterfaceName,
    is_valid_interface_name,
    crate::Error::InvalidInterfaceName,
    "a Varlink interface name"
);

/// Whether `name` is a valid Varlink interface name.
///
/// Interface names are reverse-domain notation: dot-separated segments, at least one dot.
///
/// # Examples
///
/// ```
/// use zlink_names::is_valid_interface_name;
///
/// assert!(is_valid_interface_name("org.example.Foo"));
/// assert!(!is_valid_interface_name("Foo"));
/// ```
pub fn is_valid_interface_name(name: &str) -> bool {
    parse::parses_fully(parse::interface_name, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

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

    #[test]
    fn try_from() {
        let name = InterfaceName::try_from("org.example.Foo").unwrap();
        assert_eq!(name.as_str(), "org.example.Foo");
        let name = InterfaceName::try_from(String::from("org.example.Foo")).unwrap();
        assert_eq!(name.as_str(), "org.example.Foo");

        assert_eq!(
            InterfaceName::try_from("Foo"),
            Err(Error::InvalidInterfaceName)
        );
        assert_eq!(
            InterfaceName::try_from(String::from("org.example-")),
            Err(Error::InvalidInterfaceName)
        );
    }

    #[test]
    fn owned_try_from() {
        let name = OwnedInterfaceName::try_from("org.example.Foo").unwrap();
        assert_eq!(name.as_str(), "org.example.Foo");
        let name = OwnedInterfaceName::try_from(String::from("org.example.Foo")).unwrap();
        assert_eq!(name.as_str(), "org.example.Foo");

        assert_eq!(
            OwnedInterfaceName::try_from("Foo"),
            Err(Error::InvalidInterfaceName)
        );
        assert_eq!(
            OwnedInterfaceName::try_from(String::new()),
            Err(Error::InvalidInterfaceName)
        );
    }
}
