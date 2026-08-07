use crate::parser::{bytes_to_str, parses_fully};

use winnow::{
    ModalResult, Parser,
    error::InputError,
    token::{one_of, take_while},
};

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
    parses_fully(parse_field_name, name)
}

/// Parse a field name: starts with letter, continues with alphanumeric and underscores.
pub fn parse_field_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    (
        one_of(|c: u8| c.is_ascii_alphabetic()),
        take_while(0.., |c: u8| c.is_ascii_alphanumeric() || c == b'_'),
    )
        .take()
        .map(bytes_to_str)
        .parse_next(input)
}
