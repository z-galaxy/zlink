use crate::parser::{bytes_to_str, parses_fully};

use winnow::{
    ModalResult, Parser,
    error::InputError,
    token::{one_of, take_while},
};

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
