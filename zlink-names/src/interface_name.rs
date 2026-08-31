use crate::{
    FromPattern, TryFromError,
    parser::{bytes_to_str, parses_fully},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use winnow::{
    ModalResult, Parser,
    combinator::repeat,
    error::{ErrMode, InputError, ParserError},
    token::{literal, one_of, take_while},
};

use alloc::string::String;
use core::fmt::{Display, Formatter};

use zcheapstr::CheapStr;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// A valid Varlink interface name, e.g. `org.example.Foo`.
///
/// Interface names are reverse-domain notation: dot-separated segments, at least one dot.
pub struct InterfaceName<'name>(CheapStr<'name>);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Owned sibling of [`InterfaceName`].
pub struct OwnedInterfaceName(InterfaceName<'static>);

impl AsRef<str> for InterfaceName<'_> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<&str> for InterfaceName<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl Serialize for InterfaceName<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de: 'name, 'name> Deserialize<'de> for InterfaceName<'name> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = <&str>::deserialize(deserializer)?;
        Self::try_from(name).map_err(|_| de::Error::custom("invalid Varlink interface name"))
    }
}

impl Serialize for OwnedInterfaceName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OwnedInterfaceName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Self::try_from(name).map_err(|_| de::Error::custom("invalid Varlink interface name"))
    }
}

impl FromPattern for InterfaceName<'_> {
    fn from_pattern() -> &'static str {
        r"[A-Za-z]([-]*[A-Za-z0-9])*(\.[A-Za-z0-9]([-]*[A-Za-z0-9])*)+"
    }
}

impl InterfaceName<'_> {
    /// Returns the interface name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Internal function only. Do not call this directly in your own code!
    pub const fn from_static_str_unchecked(name: &'static str) -> Self {
        Self(CheapStr::from_static(name))
    }

    /// Convert this interface name into an owned version with `'static` lifetime.
    pub fn into_owned(self) -> InterfaceName<'static> {
        InterfaceName(self.0.into_owned())
    }

    /// Clone into an owned interface name.
    pub fn to_owned(&self) -> OwnedInterfaceName {
        OwnedInterfaceName(InterfaceName(self.0.to_owned()))
    }
}

impl FromPattern for OwnedInterfaceName {
    fn from_pattern() -> &'static str {
        InterfaceName::from_pattern()
    }
}

impl OwnedInterfaceName {
    pub fn as_ref(&self) -> InterfaceName<'_> {
        InterfaceName(self.0.0.as_ref())
    }

    pub fn inner(&self) -> &InterfaceName<'static> {
        &self.0
    }
}

/// Try constructing a Interface name. Only works if name is actually valid.
/// # Examples
///
/// ```
/// use zlink_names::InterfaceName;
///
/// assert!(InterfaceName::try_from("org.example.Foo").is_ok());
/// assert!(InterfaceName::try_from("Foo").is_err());
/// ```
impl<'name> TryFrom<&'name str> for InterfaceName<'name> {
    type Error = TryFromError;

    fn try_from(name: &'name str) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_interface_name(&mut input)?;
        // `parse_interface_name` accepts a valid prefix; reject any leftover suffix.
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(Self(CheapStr::from(name)))
    }
}

impl<'s> TryFrom<&'s str> for OwnedInterfaceName {
    type Error = TryFromError;

    fn try_from(name: &'s str) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_interface_name(&mut input)?;
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(Self(InterfaceName(CheapStr::from(name).into_owned())))
    }
}

impl TryFrom<String> for OwnedInterfaceName {
    type Error = TryFromError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_interface_name(&mut input).map_err(TryFromError::from)?;
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(Self(InterfaceName(CheapStr::from(name))))
    }
}

impl Display for InterfaceName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::ops::Deref for InterfaceName<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::Deref for OwnedInterfaceName {
    type Target = InterfaceName<'static>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for OwnedInterfaceName {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<InterfaceName<'_>> for OwnedInterfaceName {
    fn eq(&self, other: &InterfaceName<'_>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<OwnedInterfaceName> for InterfaceName<'_> {
    fn eq(&self, other: &OwnedInterfaceName) -> bool {
        *self == other.0
    }
}

impl From<OwnedInterfaceName> for InterfaceName<'static> {
    fn from(name: OwnedInterfaceName) -> Self {
        name.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: [&str; 4] = ["org.example.Foo", "a.b", "org.varlink.service", "a1.b-2.c3"];
    const INVALID: [&str; 11] = [
        "",
        "Foo",          // No dot.
        "org.",         // Trailing dot, empty segment.
        ".org",         // Leading dot.
        "org..example", // Empty segment.
        "org.example.", // Trailing dot.
        "1org.example", // First segment must start with a letter.
        "org.example.Foo bar",
        "org.example.Foö",
        "org-.example", // First segment ends in a hyphen.
        "org.example-", // Subsequent segment ends in a hyphen.
    ];

    #[test]
    fn interface_names() {
        for name in VALID {
            assert!(is_valid_interface_name(name), "`{name}` must be valid");
        }
        for name in INVALID {
            assert!(!is_valid_interface_name(name), "`{name}` must be invalid");
        }
    }

    #[test]
    fn test_parse_interface_name() {
        let input = b"org.example.test";
        let mut input_mut = input.as_slice();
        let result = parse_interface_name(&mut input_mut).unwrap();
        assert_eq!(result, "org.example.test");
        assert!(input_mut.is_empty());

        let input = b"com.example.foo.bar";
        let mut input_mut = input.as_slice();
        let result = parse_interface_name(&mut input_mut).unwrap();
        assert_eq!(result, "com.example.foo.bar");
        assert!(input_mut.is_empty());

        // Invalid: no dot
        let mut input_mut = b"example".as_slice();
        assert!(parse_interface_name(&mut input_mut).is_err());

        // Invalid: starts with number
        let mut input_mut = b"1example.test".as_slice();
        assert!(parse_interface_name(&mut input_mut).is_err());
    }
}

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
    parses_fully(parse_interface_name, name)
}

/// Parse an interface name: reverse domain notation like `org.example.test`.
///
/// Grammar:
///   first segment      = \[A-Za-z\](\[-\]*\[A-Za-z0-9\])*
///   subsequent segment = "." \[A-Za-z0-9\](\[-\]*\[A-Za-z0-9\])*
///   name               = first_segment subsequent_segment+
pub fn parse_interface_name<'a>(
    input: &mut &'a [u8],
) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    (
        // First segment.
        (one_of(|c: u8| c.is_ascii_alphabetic()), segment_rest),
        // One or more dotted segments (so the name has at least one `.`).
        repeat::<_, _, (), _, _>(
            1..,
            (
                literal("."),
                one_of(|c: u8| c.is_ascii_alphanumeric()),
                segment_rest,
            )
                .void(),
        ),
    )
        .take()
        .map(bytes_to_str)
        .parse_next(input)
}

/// Rest of a segment after its leading character: zero or more runs of hyphens each
/// immediately followed by an alphanumeric, so a segment can never end in `-`.
fn segment_rest<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    repeat::<_, _, (), _, _>(
        0..,
        (
            take_while(0.., |c: u8| c == b'-'),
            one_of(|c: u8| c.is_ascii_alphanumeric()),
        )
            .void(),
    )
    .parse_next(input)
}
