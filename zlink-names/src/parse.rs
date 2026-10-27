//! The Varlink name grammar, as winnow parsers.
//!
//! zlink-idl reads the names in interface definitions with these, so it applies exactly the rules
//! this crate validates names with. Not a supported API: these can change in any release.

use winnow::{
    ModalResult, Parser,
    combinator::{opt, repeat},
    error::{ErrMode, InputError},
    token::{literal, one_of, take_while},
};
use zcheapstr::CheapStr;

use crate::{FieldName, InterfaceName, TypeName};

/// Parse a field name: starts with a letter, continues with alphanumerics, each optionally
/// preceded by a single underscore (no leading, trailing or doubled underscores).
pub fn field_name<'a>(input: &mut &'a [u8]) -> ModalResult<FieldName<'a>, InputError<&'a [u8]>> {
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
        .map(|name| FieldName(CheapStr::from(bytes_to_str(name))))
        .parse_next(input)
}

/// Parse a type name: starts with uppercase letter, continues with alphanumeric.
pub fn type_name<'a>(input: &mut &'a [u8]) -> ModalResult<TypeName<'a>, InputError<&'a [u8]>> {
    (
        one_of(|c: u8| c.is_ascii_uppercase()),
        take_while(0.., |c: u8| c.is_ascii_alphanumeric()),
    )
        .take()
        .map(|name| TypeName(CheapStr::from(bytes_to_str(name))))
        .parse_next(input)
}

/// Parse an interface name: reverse domain notation like `org.example.test`.
///
/// Grammar:
///   first_character    = [A-Za-z]
///   first segment      = [-]*[A-Za-z0-9]
///   subsequent segment = "." [A-Za-z0-9]([-]*[A-Za-z0-9])*
///   name               = first_character first_segment* subsequent_segment+
pub fn interface_name<'a>(
    input: &mut &'a [u8],
) -> ModalResult<InterfaceName<'a>, InputError<&'a [u8]>> {
    (
        // First character.
        one_of(|c: u8| c.is_ascii_alphabetic()),
        // Zero or more first segments.
        repeat::<_, _, (), _, _>(0.., segment_tail),
        // One or more dotted segments (so the name has at least one `.`).
        repeat::<_, _, (), _, _>(
            1..,
            (
                literal("."),
                one_of(|c: u8| c.is_ascii_alphanumeric()),
                repeat::<_, _, (), _, _>(0.., segment_tail),
            )
                .void(),
        ),
    )
        .take()
        .map(|name| InterfaceName(CheapStr::from(bytes_to_str(name))))
        .parse_next(input)
}

/// Whether `parser` accepts the whole of `name`, leaving nothing behind.
///
/// The parsers succeed on a valid prefix (e.g. `field_name` takes `foo` out of `foo-bar`), so a
/// name is only valid when parsing also consumes every byte. Validating a name this way runs the
/// very parsers zlink-idl reads names with, so there is no second copy of the grammar to drift from
/// them.
pub(crate) fn parses_fully<'a, P, T>(mut parser: P, name: &'a str) -> bool
where
    P: FnMut(&mut &'a [u8]) -> Result<T, ErrMode<InputError<&'a [u8]>>>,
{
    let mut input = name.as_bytes();

    parser(&mut input).is_ok() && input.is_empty()
}

/// Parse a `[-]*[A-Za-z0-9]` segment tail: zero or more hyphens followed by one
/// alphanumeric character, so hyphens can never trail without a following alnum.
fn segment_tail<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    (
        take_while(0.., |c: u8| c == b'-'),
        one_of(|c: u8| c.is_ascii_alphanumeric()),
    )
        .void()
        .parse_next(input)
}

/// Convert bytes to str with input lifetime.
fn bytes_to_str(bytes: &[u8]) -> &str {
    // SAFETY: We only accept ASCII characters in our parsers
    core::str::from_utf8(bytes).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interface_name() {
        let input = b"org.example.test";
        let mut input_mut = input.as_slice();
        let result = interface_name(&mut input_mut).unwrap();
        assert_eq!(result, "org.example.test");
        assert!(input_mut.is_empty());

        let input = b"com.example.foo.bar";
        let mut input_mut = input.as_slice();
        let result = interface_name(&mut input_mut).unwrap();
        assert_eq!(result, "com.example.foo.bar");
        assert!(input_mut.is_empty());

        // Invalid: no dot
        let mut input_mut = b"example".as_slice();
        assert!(interface_name(&mut input_mut).is_err());

        // Invalid: starts with number
        let mut input_mut = b"1example.test".as_slice();
        assert!(interface_name(&mut input_mut).is_err());

        // Valid: hyphen inside a segment.
        let input = b"org.exam-ple.test";
        let mut input_mut = input.as_slice();
        let result = interface_name(&mut input_mut).unwrap();
        assert_eq!(result, "org.exam-ple.test");
        assert!(input_mut.is_empty());

        // Valid: a dotted segment may start with a digit.
        let input = b"org.0example";
        let mut input_mut = input.as_slice();
        let result = interface_name(&mut input_mut).unwrap();
        assert_eq!(result, "org.0example");
        assert!(input_mut.is_empty());

        // Invalid: first segment ends with a trailing hyphen (not followed by an alnum).
        let mut input_mut = b"org-.example".as_slice();
        assert!(!full_interface_name_is_valid(&mut input_mut));

        // Invalid: dotted segment ends with a trailing hyphen (not followed by an alnum).
        let mut input_mut = b"org.example-".as_slice();
        assert!(!full_interface_name_is_valid(&mut input_mut));

        // Invalid: dotted segment starts with a hyphen (must start with an alnum).
        let mut input_mut = b"org.-example".as_slice();
        assert!(!full_interface_name_is_valid(&mut input_mut));
    }

    /// Parse `input` as an interface name and return whether it is valid *and* fully consumed.
    fn full_interface_name_is_valid(input: &mut &[u8]) -> bool {
        interface_name(input).is_ok() && input.is_empty()
    }
}
