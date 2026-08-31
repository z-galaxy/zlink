use crate::{
    FromPattern, TryFromError,
    parser::{bytes_to_str, parses_fully},
};

use alloc::string::String;
use winnow::{
    ModalResult, Parser,
    combinator::{opt, repeat},
    error::{ErrMode, InputError, ParserError},
    token::one_of,
};

use core::fmt::{Display, Formatter};

use zcheapstr::CheapStr;

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
/// assert!(!is_valid_field_name("user_name_"));
/// assert!(!is_valid_field_name("user-name"));
/// ```
pub fn is_valid_field_name(name: &str) -> bool {
    parses_fully(parse_field_name, name)
}

/// Parse a field name: starts with letter, continues with alphanumeric, with single underscores
/// allowed as separators (never trailing or doubled).
pub fn parse_field_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    (
        one_of(|c: u8| c.is_ascii_alphabetic()),
        repeat::<_, _, (), _, _>(
            0..,
            (
                opt(one_of(|c: u8| c == b'_')),
                one_of(|c: u8| c.is_ascii_alphanumeric()),
            )
                .void(),
        ),
    )
        .take()
        .map(bytes_to_str)
        .parse_next(input)
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// A valid Varlink field name, e.g. `user_name`.
///
/// Struct fields, method parameters and enum variants all follow this rule.
pub struct FieldName<'name>(CheapStr<'name>);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Owned sibling of [`FieldName`].
pub struct OwnedFieldName(FieldName<'static>);

impl FromPattern for FieldName<'_> {
    fn from_pattern() -> &'static str {
        r"[A-Za-z](_?[A-Za-z0-9])*"
    }
}

impl FieldName<'_> {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Clone into an owned field name.
    pub fn to_owned(&self) -> OwnedFieldName {
        OwnedFieldName(FieldName(self.0.to_owned()))
    }
}

impl FromPattern for OwnedFieldName {
    fn from_pattern() -> &'static str {
        FieldName::from_pattern()
    }
}

impl OwnedFieldName {
    pub fn as_ref(&self) -> FieldName<'_> {
        FieldName(self.0.0.as_ref())
    }

    pub fn inner(&self) -> &FieldName<'static> {
        &self.0
    }
}

/// Try constructing a field name. Only works if name is actually valid.
/// # Examples
///
/// ```
/// use zlink_names::FieldName;
///
/// assert!(FieldName::try_from("user_name").is_ok());
/// assert!(FieldName::try_from("user-name").is_err());
/// ```
impl<'name> TryFrom<&'name str> for FieldName<'name> {
    type Error = TryFromError;

    fn try_from(name: &'name str) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_field_name(&mut input)?;
        // `parse_field_name` accepts a valid prefix; reject any leftover suffix.
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(FieldName(CheapStr::from(name)))
    }
}

impl<'s> TryFrom<&'s str> for OwnedFieldName {
    type Error = TryFromError;

    fn try_from(name: &'s str) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_field_name(&mut input)?;
        // `parse_field_name` accepts a valid prefix; reject any leftover suffix.
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(OwnedFieldName(FieldName(CheapStr::from(name).into_owned())))
    }
}

impl TryFrom<String> for OwnedFieldName {
    type Error = TryFromError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_field_name(&mut input).map_err(TryFromError::from)?;
        // `parse_field_name` accepts a valid prefix; reject any leftover suffix.
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(OwnedFieldName(FieldName(CheapStr::from(name))))
    }
}

impl Display for FieldName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::ops::Deref for FieldName<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::Deref for OwnedFieldName {
    type Target = FieldName<'static>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for OwnedFieldName {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<FieldName<'_>> for OwnedFieldName {
    fn eq(&self, other: &FieldName<'_>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<OwnedFieldName> for FieldName<'_> {
    fn eq(&self, other: &OwnedFieldName) -> bool {
        *self == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: [&str; 6] = ["a", "Z", "user_name", "user0", "a_0_b", "type"];
    const INVALID: [&str; 9] = [
        "",
        "_foo",
        "0foo",
        "user-name",
        "user name",
        "user!",
        "a_",
        "x__",
        "a__b",
    ];

    #[test]
    fn field_names() {
        for name in VALID {
            assert!(is_valid_field_name(name), "`{name}` must be valid");
        }
        for name in INVALID {
            assert!(!is_valid_field_name(name), "`{name}` must be invalid");
        }
    }
}
