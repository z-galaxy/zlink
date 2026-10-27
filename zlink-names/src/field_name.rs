//! Varlink field names.

use zcheapstr::CheapStr;

use crate::parse;

/// A Varlink field name, e.g. `user_name`.
///
/// Struct fields, method parameters and enum variants are named this way: an ASCII letter,
/// followed by ASCII letters and digits, each of which may be preceded by a single underscore.
///
/// # Examples
///
/// ```
/// use zlink_names::FieldName;
///
/// let name = FieldName::try_from("user_name")?;
/// assert_eq!(name, "user_name");
/// assert!(FieldName::try_from("user-name").is_err());
/// # Ok::<(), zlink_names::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldName<'a>(pub(crate) CheapStr<'a>);

/// An owned [`FieldName`], independent of the data it was created from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnedFieldName(FieldName<'static>);

impl_name!(
    FieldName,
    OwnedFieldName,
    is_valid_field_name,
    crate::Error::InvalidFieldName,
    "a Varlink field name"
);

/// Whether `name` is a valid Varlink field name.
///
/// Struct fields, method parameters and enum variants all follow this rule.
///
/// # Examples
///
/// ```
/// use zlink_names::is_valid_field_name;
///
/// assert!(is_valid_field_name("user_name"));
/// assert!(!is_valid_field_name("user-name"));
/// ```
pub fn is_valid_field_name(name: &str) -> bool {
    parse::parses_fully(parse::field_name, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn field_names() {
        let valid = ["a", "Z", "user_name", "user0", "a_0_b", "type", "foo123"];
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
            "x__",
            "foo__bar",
        ];
        for name in invalid {
            assert!(!is_valid_field_name(name), "`{name}` must be invalid");
        }
    }

    #[test]
    fn try_from() {
        let name = FieldName::try_from("user_name").unwrap();
        assert_eq!(name.as_str(), "user_name");
        let name = FieldName::try_from(String::from("user_name")).unwrap();
        assert_eq!(name.as_str(), "user_name");

        assert_eq!(
            FieldName::try_from("user-name"),
            Err(Error::InvalidFieldName)
        );
        assert_eq!(
            FieldName::try_from(String::from("_user")),
            Err(Error::InvalidFieldName)
        );
    }

    #[test]
    fn owned_try_from() {
        let name = OwnedFieldName::try_from("user_name").unwrap();
        assert_eq!(name.as_str(), "user_name");
        let name = OwnedFieldName::try_from(String::from("user_name")).unwrap();
        assert_eq!(name.as_str(), "user_name");

        assert_eq!(
            OwnedFieldName::try_from("user-name"),
            Err(Error::InvalidFieldName)
        );
        assert_eq!(
            OwnedFieldName::try_from(String::new()),
            Err(Error::InvalidFieldName)
        );
    }

    // The tests below cover the API that all the name families share, through this one.

    #[test]
    fn owned_copies_are_independent() {
        let source = String::from("user_name");
        let name = FieldName::try_from(source.as_str()).unwrap();
        let owned: OwnedFieldName = name.to_owned();
        let owned_too: OwnedFieldName = name.clone().into_owned();
        let owned_from = OwnedFieldName::from(name);
        // The owned names don't borrow from `source`.
        drop(source);

        assert_eq!(owned, "user_name");
        assert_eq!(owned_too, owned);
        assert_eq!(owned_from, owned);
    }

    #[test]
    fn static_names_are_not_copied() {
        const NAME: FieldName<'static> = FieldName::from_static_str_unchecked("user_name");

        let name = NAME;
        assert_eq!(name, "user_name");
        assert_eq!(
            name.to_owned().as_str().as_ptr(),
            name.as_str().as_ptr(),
            "an owned copy of a static name shares its data"
        );
    }

    #[test]
    fn as_str_in_const() {
        const NAME: &FieldName<'static> = &FieldName::from_static_str_unchecked("user_name");
        const NAME_STR: &str = NAME.as_str();

        assert_eq!(NAME_STR, "user_name");
    }

    #[test]
    fn borrowed_views_share_data() {
        let owned = OwnedFieldName::try_from(String::from("user_name")).unwrap();
        let ptr = owned.as_str().as_ptr();

        let view = owned.as_ref();
        assert_eq!(view.as_str().as_ptr(), ptr);
        assert_eq!(FieldName::from(&owned).as_str().as_ptr(), ptr);
        assert_eq!(FieldName::from(&view).as_str().as_ptr(), ptr);
        assert_eq!(owned.inner().as_str().as_ptr(), ptr);

        let inner = owned.into_inner();
        assert_eq!(inner.as_str().as_ptr(), ptr);
        assert_eq!(FieldName::from(OwnedFieldName::from(inner)), "user_name");
    }

    #[test]
    fn comparing() {
        let name = FieldName::try_from("user_name").unwrap();
        assert!(name == *"user_name");
        assert!(name == "user_name");
        assert!(*"user_name" == name);
        assert!("user_name" == name);
        assert!(name != "other");
        assert!("other" != name);

        let owned = name.to_owned();
        assert!(owned == *"user_name");
        assert!(owned == "user_name");
        assert!(*"user_name" == owned);
        assert!("user_name" == owned);
        assert!(owned != "other");

        // Borrowed and owned names compare with each other, and so do names that hold their
        // data differently.
        assert!(name == owned);
        assert!(owned == name);
        assert_eq!(name, FieldName::from_static_str_unchecked("user_name"));
        assert!(FieldName::from_static_str_unchecked("a") < FieldName::try_from("b").unwrap());
    }

    #[test]
    fn string_like_access() {
        let name = FieldName::try_from("user_name").unwrap();
        assert_eq!(name.to_string(), "user_name");
        assert_eq!(format!("{name:>11}"), "  user_name");
        assert_eq!(OwnedFieldName::from(name.clone()).to_string(), "user_name");

        // `Deref` to `str`.
        assert_eq!(name.len(), 9);
        assert!(name.starts_with("user"));
        // `AsRef<str>`.
        let as_ref: &str = name.as_ref();
        assert_eq!(as_ref, "user_name");
        // The owned name coerces to `&str` as well.
        let owned = name.to_owned();
        let as_str: &str = &owned;
        assert_eq!(as_str, "user_name");
    }
}
