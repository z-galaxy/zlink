//! Deserializing a call's `parameters` when there are none to pass.
//!
//! A client may leave `parameters` out, or send it as an empty object, when it has none to pass:
//! for a method without parameters, or one whose parameters are all optional. serde's adjacently
//! tagged enums take only one of the two: a unit variant refuses an object, and any other variant
//! refuses a missing `parameters`. These helpers take both, and `null`.

use core::fmt;

use serde::{
    Deserialize, Deserializer,
    de::{self, IgnoredAny, MapAccess, Visitor, value::MapDeserializer},
};

/// The parameters of a method that has some: from an object, or from no object (a missing or
/// `null` `parameters`) as if it were empty, so that only a parameter that is not optional is
/// missing.
#[derive(Debug)]
pub struct Parameters<T>(pub T);

impl<'de, T> Deserialize<'de> for Parameters<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Option::<T>::deserialize(deserializer)? {
            Some(params) => Ok(Self(params)),
            None => T::deserialize(MapDeserializer::<_, D::Error>::new(core::iter::empty::<(
                (),
                (),
            )>()))
            .map(Self),
        }
    }
}

/// For `#[serde(deserialize_with)]` on the unit variant of a method without parameters: takes a
/// missing or `null` `parameters`, or an object, whose members are ignored as they are for a
/// method with parameters.
pub fn none<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_option(NoneVisitor)
}

struct NoneVisitor;

impl<'de> Visitor<'de> for NoneVisitor {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no parameters, or an object")
    }

    fn visit_none<E>(self) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }

    fn visit_map<A>(self, mut map: A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(())
    }
}
