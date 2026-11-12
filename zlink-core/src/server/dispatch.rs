//! What the code that the `Service` derive macro generates needs to find the interface of a method
//! call before deserializing its parameters. Not part of the public API.

use alloc::{borrow::Cow, boxed::Box, string::String};
use core::{fmt, marker::PhantomData};

use serde::{
    Deserialize, Deserializer,
    de::{
        self, DeserializeSeed, IntoDeserializer, MapAccess, Visitor, value::MapAccessDeserializer,
    },
};
use serde_json::value::RawValue;

use crate::idl;

/// A method call whose method is known, and whose parameters are yet to be deserialized.
#[derive(Debug)]
pub struct Pending<'de, A> {
    method: Cow<'de, str>,
    rest: Rest<'de, A>,
}

/// Read the object of a method call from `map` up to where its method is known.
///
/// If the `method` field comes first, as it does in the calls that clients usually send, nothing
/// else is read. Otherwise, the entire object is read and its parameters are borrowed as raw JSON
/// until the method is known. This requires the input JSON to outlive the deserialized call, as it
/// does for calls received through a connection.
pub fn start<'de, A>(mut map: A) -> Result<Pending<'de, A>, A::Error>
where
    A: MapAccess<'de>,
{
    let mut method = None;
    let mut parameters = None;
    // `parameters` can be there and be `null`, which is the same as none to a method.
    let mut has_parameters = false;
    let mut first = true;
    while let Some(key) = map.next_key()? {
        match key {
            Key::Method if first => {
                let Str(method) = map.next_value()?;

                return Ok(Pending {
                    method,
                    rest: Rest::Map(map),
                });
            }
            Key::Method if method.is_some() => return Err(de::Error::duplicate_field("method")),
            Key::Method => {
                let Str(value) = map.next_value()?;
                method = Some(value);
            }
            Key::Parameters if has_parameters => {
                return Err(de::Error::duplicate_field("parameters"));
            }
            Key::Parameters => {
                parameters = map.next_value::<Option<&'de RawValue>>()?;
                has_parameters = true;
            }
            Key::Other => {
                map.next_value::<de::IgnoredAny>()?;
            }
        }
        first = false;
    }
    let Some(method) = method else {
        return Err(de::Error::missing_field("method"));
    };

    Ok(Pending {
        method,
        rest: Rest::Parameters(parameters),
    })
}

impl<'de, A> Pending<'de, A>
where
    A: MapAccess<'de>,
{
    /// The fully-qualified name of the called method.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Deserialize the call as `T`, which reads the `method` and `parameters` fields of an object.
    pub fn finish<T>(self) -> Result<T, A::Error>
    where
        T: Deserialize<'de>,
    {
        let method = Some(&*self.method);

        match self.rest {
            Rest::Map(inner) => T::deserialize(MapAccessDeserializer::new(Replay {
                method,
                value: None,
                inner,
            })),
            Rest::Parameters(parameters) => T::deserialize(MapAccessDeserializer::new(Read {
                method,
                value: None,
                parameters,
                error: PhantomData,
            })),
        }
    }

    /// Skip what's left of the call, whose method no interface has, and give the name of that
    /// method.
    pub fn skip(self) -> Result<Cow<'de, str>, A::Error> {
        if let Rest::Map(mut rest) = self.rest {
            while rest
                .next_entry::<de::IgnoredAny, de::IgnoredAny>()?
                .is_some()
            {}
        }

        Ok(self.method)
    }
}

/// Whether `method`, a fully-qualified method name, is a method of `interface`.
pub fn has_method(interface: &idl::Interface<'_>, method: &str) -> bool {
    let Some((name, member)) = method.rsplit_once('.') else {
        return false;
    };

    interface.name().as_str() == name && interface.methods().any(|m| m.name().as_str() == member)
}

/// Whether `a` and `b` are descriptions of interfaces of the same name.
///
/// A service routes each call by the name of its interface, so its interfaces must all differ in
/// name. This is a `const fn` for the service to check that at compile time.
pub const fn same_name(a: &idl::Interface<'_>, b: &idl::Interface<'_>) -> bool {
    let a = a.name().as_str().as_bytes();
    let b = b.name().as_str().as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }

    true
}

/// The state of the interfaces that need none.
pub fn no_state<'a>() -> &'a mut () {
    // `()` takes no space, so this neither allocates nor leaks any memory.
    Box::leak(Box::new(()))
}

/// What's left of a method call once its method is known.
#[derive(Debug)]
enum Rest<'de, A> {
    /// The fields after `method`, which came first.
    Map(A),
    /// Nothing: all the fields were read, and these are the parameters, if any.
    Parameters(Option<&'de RawValue>),
}

/// The name of a field of a method call. Reading it never allocates, whichever way the
/// deserializer hands the name over.
enum Key {
    Method,
    Parameters,
    Other,
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KeyVisitor;

        impl Visitor<'_> for KeyVisitor {
            type Value = Key;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("the name of a field")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
                Ok(match v {
                    "method" => Key::Method,
                    "parameters" => Key::Parameters,
                    _ => Key::Other,
                })
            }
        }

        deserializer.deserialize_identifier(KeyVisitor)
    }
}

/// A string that borrows from the input if it can.
struct Str<'de>(Cow<'de, str>);

impl<'de> Deserialize<'de> for Str<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrVisitor;

        impl<'de> Visitor<'de> for StrVisitor {
            type Value = Str<'de>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }

            fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
                Ok(Str(Cow::Borrowed(v)))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
                Ok(Str(Cow::Owned(String::from(v))))
            }
        }

        deserializer.deserialize_str(StrVisitor)
    }
}

/// A map that gives the `method` field of a call again, then the fields after it, from `inner`.
struct Replay<'a, A> {
    /// The method, until its key is given.
    method: Option<&'a str>,
    /// The method, between its key and its value.
    value: Option<&'a str>,
    inner: A,
}

impl<'de, A> MapAccess<'de> for Replay<'_, A>
where
    A: MapAccess<'de>,
{
    type Error = A::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, A::Error>
    where
        K: DeserializeSeed<'de>,
    {
        let Some(method) = self.method.take() else {
            return self.inner.next_key_seed(seed);
        };
        self.value = Some(method);

        seed.deserialize("method".into_deserializer()).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, A::Error>
    where
        V: DeserializeSeed<'de>,
    {
        match self.value.take() {
            Some(method) => seed.deserialize(method.into_deserializer()),
            None => self.inner.next_value_seed(seed),
        }
    }
}

/// A map that gives the `method` field of a call that was read in full, then its `parameters`.
struct Read<'a, 'de, E> {
    /// The method, until its key is given.
    method: Option<&'a str>,
    /// The method, between its key and its value.
    value: Option<&'a str>,
    parameters: Option<&'de RawValue>,
    error: PhantomData<E>,
}

impl<'de, E> MapAccess<'de> for Read<'_, 'de, E>
where
    E: de::Error,
{
    type Error = E;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, E>
    where
        K: DeserializeSeed<'de>,
    {
        let key = match self.method.take() {
            Some(method) => {
                self.value = Some(method);
                "method"
            }
            None if self.parameters.is_some() => "parameters",
            None => return Ok(None),
        };

        seed.deserialize(key.into_deserializer()).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, E>
    where
        V: DeserializeSeed<'de>,
    {
        if let Some(method) = self.value.take() {
            return seed.deserialize(method.into_deserializer());
        }
        let Some(parameters) = self.parameters.take() else {
            return Err(E::custom("value of a method call requested before its key"));
        };

        seed.deserialize(&mut serde_json::Deserializer::from_str(parameters.get()))
            .map_err(E::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::{Call, varlink_service};

    #[test]
    fn method_first() {
        let call = call(r#"{"method":"org.example.Get","more":true,"parameters":{"key":"a"}}"#);
        assert!(call.more());
        assert!(matches!(
            call.method(),
            Parsed::Get(Method::Get { key: "a" })
        ));
    }

    #[test]
    fn parameters_first() {
        let json = r#"{"parameters":{"key":"a"},"oneway":true,"method":"org.example.Get"}"#;
        let call = call(json);
        assert!(call.oneway());
        let Parsed::Get(Method::Get { key }) = call.method() else {
            panic!("{call:?}");
        };
        // The parameters borrow from the input.
        assert!(json.as_bytes().as_ptr_range().contains(&key.as_ptr()));

        // Fields that a call doesn't have are skipped.
        let call = self::call(r#"{"x":[1],"method":"org.example.Get","parameters":{"key":"a"}}"#);
        assert!(matches!(
            call.method(),
            Parsed::Get(Method::Get { key: "a" })
        ));
    }

    #[test]
    fn no_parameters() {
        for json in [
            r#"{"method":"org.example.Ping"}"#,
            r#"{"more":true,"method":"org.example.Ping"}"#,
            r#"{"parameters":null,"method":"org.example.Ping"}"#,
            r#"{"x":1,"method":"org.example.Ping"}"#,
        ] {
            assert!(
                matches!(call(json).method(), Parsed::Get(Method::Ping)),
                "{json}"
            );
        }
    }

    #[test]
    fn unknown_method() {
        for json in [
            r#"{"method":"org.example.Nope","parameters":{"key":1}}"#,
            r#"{"parameters":{"key":1},"method":"org.example.Nope"}"#,
            r#"{"parameters":null,"method":"org.example.Nope"}"#,
        ] {
            let call = call(json);
            assert!(
                matches!(call.method(), Parsed::Unknown(method) if method == "org.example.Nope"),
                "{json}"
            );
        }
    }

    #[test]
    fn invalid_calls() {
        for json in [
            // Parameters that the method doesn't take, in both orders.
            r#"{"method":"org.example.Get","parameters":{"key":1}}"#,
            r#"{"parameters":{"key":1},"method":"org.example.Get"}"#,
            r#"{"parameters":{"key":"a"}}"#,
            "{}",
            // A field given twice, in both orders.
            r#"{"method":"org.example.Get","parameters":{"key":"a"},"parameters":{"key":"b"}}"#,
            r#"{"parameters":{"key":"a"},"parameters":{"key":"b"},"method":"org.example.Get"}"#,
            r#"{"parameters":null,"parameters":{"key":"b"},"method":"org.example.Get"}"#,
            r#"{"method":"org.example.Get","method":"org.example.Ping","parameters":{"key":"a"}}"#,
            r#"{"x":1,"method":"org.example.Get","method":"org.example.Ping"}"#,
        ] {
            serde_json::from_str::<Call<Parsed<'_>>>(json).unwrap_err();
        }
    }

    #[test]
    fn methods_of_an_interface() {
        let interface = varlink_service::DESCRIPTION;
        assert!(has_method(interface, "org.varlink.service.GetInfo"));
        assert!(!has_method(interface, "org.varlink.service.Nope"));
        assert!(!has_method(interface, "org.varlink.GetInfo"));
        assert!(!has_method(interface, "GetInfo"));
    }

    #[test]
    fn names_of_interfaces() {
        const OTHER: &idl::Interface<'static> = &idl::Interface::new(
            crate::names::InterfaceName::from_static_str_unchecked("org.varlink.servicf"),
            &[],
            &[],
            &[],
            &[],
        );
        const SAME: bool = same_name(varlink_service::DESCRIPTION, varlink_service::DESCRIPTION);
        const DIFFERENT: bool = same_name(varlink_service::DESCRIPTION, OTHER);
        assert!(SAME);
        assert!(!DIFFERENT);
    }

    fn call(json: &str) -> Call<Parsed<'_>> {
        serde_json::from_str(json).unwrap()
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "method", content = "parameters")]
    enum Method<'a> {
        #[serde(rename = "org.example.Get")]
        Get { key: &'a str },
        #[serde(rename = "org.example.Ping")]
        Ping,
        // Takes the calls to the methods that the interface doesn't have, if they come without
        // parameters, as the type that the `interface` macro generates does.
        #[serde(other)]
        Unknown,
    }

    #[derive(Debug)]
    enum Parsed<'a> {
        Get(Method<'a>),
        Unknown(Cow<'a, str>),
    }

    impl<'de> Deserialize<'de> for Parsed<'de> {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct ParsedVisitor;

            impl<'de> Visitor<'de> for ParsedVisitor {
                type Value = Parsed<'de>;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a method call")
                }

                fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
                where
                    A: MapAccess<'de>,
                {
                    let call = start(map)?;
                    if matches!(call.method(), "org.example.Get" | "org.example.Ping") {
                        return call.finish().map(Parsed::Get);
                    }

                    call.skip().map(Parsed::Unknown)
                }
            }

            deserializer.deserialize_map(ParsedVisitor)
        }
    }
}
