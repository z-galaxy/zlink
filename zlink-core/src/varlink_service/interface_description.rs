use core::fmt::Debug;

use crate::idl::Interface;
#[cfg(feature = "introspection")]
use crate::introspect::Type;
#[cfg(feature = "idl-parse")]
use alloc::string::String;
#[cfg(feature = "idl-parse")]
use serde::Deserialize;
use serde::Serialize;

/// The interface description.
///
/// Use [`InterfaceDescription::parse`] to get the [`Interface`].
///
/// Under the hood, the interface description is either a parsed [`Interface`] or a raw string.
#[derive(Debug, Serialize, Clone)]
#[cfg_attr(feature = "introspection", derive(Type))]
#[cfg_attr(feature = "introspection", zlink(crate = "crate"))]
pub struct InterfaceDescription<'a> {
    description: Description<'a>,
}

impl<'a> InterfaceDescription<'a> {
    /// Parse the interface description as an [`Interface`].
    ///
    /// If the description is already parsed, it is returned as is. If the description is a raw
    /// string, it is parsed as an [`Interface`] and returned.
    pub fn parse(&self) -> crate::Result<Interface<'_>> {
        match &self.description {
            Description::Parsed(interface) => Ok(interface.clone()),
            #[cfg(feature = "idl-parse")]
            Description::Raw(description) => description.as_str().try_into(),
        }
    }

    /// The raw interface description, if `self` is based on a raw description.
    pub fn as_raw(&self) -> Option<&str> {
        match &self.description {
            Description::Parsed(_) => None,
            #[cfg(feature = "idl-parse")]
            Description::Raw(description) => Some(description.as_str()),
        }
    }
}

impl<'a> From<&Interface<'a>> for InterfaceDescription<'a> {
    fn from(interface: &Interface<'a>) -> Self {
        Self {
            description: Description::Parsed(interface.clone()),
        }
    }
}

#[cfg(feature = "idl-parse")]
impl<'de> Deserialize<'de> for InterfaceDescription<'static> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use core::fmt;

        use serde::de::{Error, MapAccess, Visitor};

        struct IDVisitor;

        impl<'de> Visitor<'de> for IDVisitor {
            type Value = InterfaceDescription<'static>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a valid interface description")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let (field_name, description): (&str, String) = map
                    .next_entry()?
                    .ok_or_else(|| A::Error::invalid_length(0, &self))?;
                if field_name != "description" {
                    return Err(A::Error::unknown_field(field_name, &["description"]));
                }

                Ok(InterfaceDescription {
                    description: Description::Raw(description),
                })
            }
        }

        deserializer.deserialize_map(IDVisitor)
    }
}

#[derive(Debug, Clone)]
enum Description<'a> {
    Parsed(Interface<'a>),
    #[cfg(feature = "idl-parse")]
    Raw(String),
}

#[cfg(feature = "introspection")]
impl Type for Description<'_> {
    const TYPE: &'static crate::idl::Type<'static> = &crate::idl::Type::String;
}

impl Serialize for Description<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Description::Parsed(interface) => serializer.collect_str(interface),
            #[cfg(feature = "idl-parse")]
            Description::Raw(description) => serializer.serialize_str(description),
        }
    }
}

#[cfg(feature = "idl-parse")]
impl<'de> Deserialize<'de> for Description<'static> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let description = String::deserialize(deserializer)?;
        Ok(Description::Raw(description))
    }
}
