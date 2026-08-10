use crate::{
    FromPattern, TryFromError,
    parser::{bytes_to_str, parses_fully},
};

use alloc::string::String;
use core::fmt::{Display, Formatter};
use winnow::{
    ModalResult, Parser,
    error::{ErrMode, InputError, ParserError},
    token::{one_of, take_while},
};
use zcheapstr::CheapStr;

/// Whether `name` is a valid Varlink type name.
///
/// Method and error names follow this rule too.
///
/// # Examples
///
/// ```
/// use zlink_names::is_valid_type_name;
///
/// assert!(is_valid_type_name("FileNotFound"));
/// assert!(!is_valid_type_name("fileNotFound"));
/// ```
pub fn is_valid_type_name(name: &str) -> bool {
    parses_fully(parse_type_name, name)
}

/// Parse a type name: starts with uppercase letter, continues with alphanumeric.
pub fn parse_type_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    (
        one_of(|c: u8| c.is_ascii_uppercase()),
        take_while(0.., |c: u8| c.is_ascii_alphanumeric()),
    )
        .take()
        .map(bytes_to_str)
        .parse_next(input)
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// A valid Varlink type name, e.g. `FileNotFound`.
///
/// Method and error names follow this rule too.
pub struct TypeName<'name>(CheapStr<'name>);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// Owned sibling of [`TypeName`].
pub struct OwnedTypeName(TypeName<'static>);

impl FromPattern for TypeName<'_> {
    fn from_pattern() -> &'static str {
        r"[A-Z][A-Za-z0-9]*"
    }
}

impl TypeName<'_> {
    /// Returns the type name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Clone into an owned type name.
    pub fn to_owned(&self) -> OwnedTypeName {
        OwnedTypeName(TypeName(self.0.to_owned()))
    }
}

impl FromPattern for OwnedTypeName {
    fn from_pattern() -> &'static str {
        TypeName::from_pattern()
    }
}

impl OwnedTypeName {
    pub fn as_ref(&self) -> TypeName<'_> {
        TypeName(self.0.0.as_ref())
    }

    pub fn inner(&self) -> &TypeName<'static> {
        &self.0
    }
}

/// Try constructing a type name. Only works if name is actually valid.
/// # Examples
///
/// ```
/// use zlink_names::TypeName;
///
/// assert!(TypeName::try_from("FileNotFound").is_ok());
/// assert!(TypeName::try_from("fileNotFound").is_err());
/// ```
impl<'name> TryFrom<&'name str> for TypeName<'name> {
    type Error = TryFromError;

    fn try_from(name: &'name str) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_type_name(&mut input)?;
        // `parse_field_name` accepts a valid prefix; reject any leftover suffix.
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(TypeName(CheapStr::from(name)))
    }
}

impl<'s> TryFrom<&'s str> for OwnedTypeName {
    type Error = TryFromError;

    fn try_from(name: &'s str) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_type_name(&mut input)?;
        // `parse_type_name` accepts a valid prefix; reject any leftover suffix.
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(OwnedTypeName(TypeName(CheapStr::from(name).into_owned())))
    }
}

impl TryFrom<String> for OwnedTypeName {
    type Error = TryFromError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let mut input = name.as_bytes();
        parse_type_name(&mut input).map_err(TryFromError::from)?;
        // `parse_type_name` accepts a valid prefix; reject any leftover
        if !input.is_empty() {
            return Err(ErrMode::Backtrack(ParserError::from_input(&input)).into());
        }
        Ok(OwnedTypeName(TypeName(CheapStr::from(name))))
    }
}

impl Display for TypeName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::ops::Deref for TypeName<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::Deref for OwnedTypeName {
    type Target = TypeName<'static>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for OwnedTypeName {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<TypeName<'_>> for OwnedTypeName {
    fn eq(&self, other: &TypeName<'_>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<OwnedTypeName> for TypeName<'_> {
    fn eq(&self, other: &OwnedTypeName) -> bool {
        *self == other.0
    }
}
