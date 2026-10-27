//! Deserialization of the name types.

use alloc::string::String;
use core::fmt;

use serde::de::{self, Deserializer, Unexpected, Visitor};
use zcheapstr::CheapStr;

/// Deserialize a name from a string, borrowing it from the input where the deserializer allows.
///
/// `is_valid` checks the string, and `expecting` says what was expected, for the error message.
pub(crate) fn deserialize<'de, D>(
    deserializer: D,
    is_valid: fn(&str) -> bool,
    expecting: &'static str,
) -> Result<CheapStr<'de>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_str(NameVisitor {
        is_valid,
        expecting,
    })
}

struct NameVisitor {
    is_valid: fn(&str) -> bool,
    expecting: &'static str,
}

impl<'de> Visitor<'de> for NameVisitor {
    type Value = CheapStr<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.expecting)
    }

    // The input can be lent out as it is: no copy.
    fn visit_borrowed_str<E>(self, name: &'de str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.check(name)?;

        Ok(CheapStr::from(name))
    }

    // The input is only lent for the call, e.g. because it had to be unescaped: the name needs
    // its own copy.
    fn visit_str<E>(self, name: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.check(name)?;

        Ok(CheapStr::from(String::from(name)))
    }

    fn visit_string<E>(self, name: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.check(&name)?;

        Ok(CheapStr::from(name))
    }
}

impl NameVisitor {
    fn check<E>(&self, name: &str) -> Result<(), E>
    where
        E: de::Error,
    {
        if (self.is_valid)(name) {
            return Ok(());
        }

        Err(E::invalid_value(Unexpected::Str(name), self))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        FieldName, InterfaceName, OwnedFieldName, OwnedInterfaceName, OwnedTypeName, TypeName,
    };
    use serde::{Deserialize, de::DeserializeOwned};

    #[test]
    fn round_trip() {
        let field = FieldName::try_from("user_name").unwrap();
        let json = serde_json::to_string(&field).unwrap();
        assert_eq!(json, r#""user_name""#);
        assert_eq!(serde_json::from_str::<FieldName<'_>>(&json).unwrap(), field);
        assert_eq!(
            serde_json::to_string(&field.to_owned()).unwrap(),
            json,
            "owned names serialize like borrowing ones"
        );
        assert_eq!(
            serde_json::from_str::<OwnedFieldName>(&json).unwrap(),
            field
        );

        let ty = TypeName::try_from("FileNotFound").unwrap();
        let json = serde_json::to_string(&ty).unwrap();
        assert_eq!(json, r#""FileNotFound""#);
        assert_eq!(serde_json::from_str::<TypeName<'_>>(&json).unwrap(), ty);
        assert_eq!(serde_json::from_str::<OwnedTypeName>(&json).unwrap(), ty);

        let interface = InterfaceName::try_from("org.example.Foo").unwrap();
        let json = serde_json::to_string(&interface).unwrap();
        assert_eq!(json, r#""org.example.Foo""#);
        assert_eq!(
            serde_json::from_str::<InterfaceName<'_>>(&json).unwrap(),
            interface
        );
        assert_eq!(
            serde_json::from_str::<OwnedInterfaceName>(&json).unwrap(),
            interface
        );
    }

    #[test]
    fn borrows_from_the_input() {
        let json = String::from(r#""user_name""#);
        let name: FieldName<'_> = serde_json::from_str(&json).unwrap();

        assert_eq!(name, "user_name");
        // The name is the input itself, without the quotes.
        assert!(core::ptr::eq(name.as_str().as_ptr(), json[1..].as_ptr()));
    }

    #[test]
    fn input_that_cannot_be_lent_is_copied() {
        // The escape sequence means the string has to be unescaped, so it can't be lent as it is.
        let name: FieldName<'_> = serde_json::from_str("\"user\\u005fname\"").unwrap();
        assert_eq!(name, "user_name");
    }

    #[test]
    fn invalid_names() {
        let err = serde_json::from_str::<FieldName<'_>>(r#""user-name""#).unwrap_err();
        assert!(
            err.to_string().contains("expected a Varlink field name"),
            "unexpected error: {err}"
        );

        let err = serde_json::from_str::<TypeName<'_>>(r#""fileNotFound""#).unwrap_err();
        assert!(
            err.to_string().contains("expected a Varlink type name"),
            "unexpected error: {err}"
        );

        let err = serde_json::from_str::<InterfaceName<'_>>(r#""Foo""#).unwrap_err();
        assert!(
            err.to_string()
                .contains("expected a Varlink interface name"),
            "unexpected error: {err}"
        );

        // Validation covers the input that has to be copied, and the owned names, too.
        serde_json::from_str::<FieldName<'_>>("\"user\\u002dname\"").unwrap_err();
        let err = serde_json::from_str::<OwnedFieldName>(r#""user-name""#).unwrap_err();
        assert!(err.to_string().contains("expected a Varlink field name"));

        // And what isn't a string is no name either.
        let err = serde_json::from_str::<FieldName<'_>>("42").unwrap_err();
        assert!(err.to_string().contains("expected a Varlink field name"));
    }

    #[test]
    fn owned_names_do_not_borrow() {
        fn from_json<T: DeserializeOwned>(json: &str) -> T {
            serde_json::from_str(json).unwrap()
        }

        let name: OwnedInterfaceName = from_json(r#""org.example.Foo""#);
        assert_eq!(name, "org.example.Foo");

        // A reader can't lend anything, so only owned names can be read from one.
        let name: OwnedInterfaceName =
            serde_json::from_reader(r#""org.example.Foo""#.as_bytes()).unwrap();
        assert_eq!(name, "org.example.Foo");
    }

    #[test]
    fn borrowing_struct_field() {
        #[derive(Deserialize)]
        struct Service<'a> {
            #[serde(borrow)]
            name: InterfaceName<'a>,
        }

        let json = String::from(r#"{"name":"org.example.Foo"}"#);
        let service: Service<'_> = serde_json::from_str(&json).unwrap();

        assert_eq!(service.name, "org.example.Foo");
        let start = json.find("org").unwrap();
        assert!(core::ptr::eq(
            service.name.as_str().as_ptr(),
            json[start..].as_ptr()
        ));
    }
}
