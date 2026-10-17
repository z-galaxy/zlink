use core::{fmt, marker::PhantomData};

use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, MapAccess, Visitor},
    forward_to_deserialize_any,
};

/// Method call parameters that may be omitted altogether.
///
/// The `parameters` field of a method call is optional in the Varlink protocol and callers (e.g.
/// `varlinkctl`) leave it out when they have nothing to pass. This wraps a type that deserializes
/// from the `parameters` object and makes it deserialize from an empty object when the field is
/// missing. It is meant to be used as the content of an adjacently tagged enum variant.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct OptionalParams<T>(pub T);

impl<'de, T> Deserialize<'de> for OptionalParams<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ParamsVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for ParamsVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = OptionalParams<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an optional parameters object")
            }

            // Serde's adjacently tagged enums call these when the content is missing (or `null`).
            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                T::deserialize(EmptyParams(PhantomData)).map(OptionalParams)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_none()
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                T::deserialize(deserializer).map(OptionalParams)
            }
        }

        deserializer.deserialize_option(ParamsVisitor(PhantomData))
    }
}

/// Deserializer for omitted parameters: an empty object.
struct EmptyParams<E>(PhantomData<E>);

impl<'de, E> Deserializer<'de> for EmptyParams<E>
where
    E: de::Error,
{
    type Error = E;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(EmptyMap(PhantomData))
    }

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value, E>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

struct EmptyMap<E>(PhantomData<E>);

impl<'de, E> MapAccess<'de> for EmptyMap<E>
where
    E: de::Error,
{
    type Error = E;

    fn next_key_seed<K>(&mut self, _seed: K) -> Result<Option<K::Value>, E>
    where
        K: DeserializeSeed<'de>,
    {
        Ok(None)
    }

    fn next_value_seed<V>(&mut self, _seed: V) -> Result<V::Value, E>
    where
        V: DeserializeSeed<'de>,
    {
        Err(de::Error::custom("no value in an empty map"))
    }
}
