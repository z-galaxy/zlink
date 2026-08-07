use crate::parser::{bytes_to_str, parses_fully};

use winnow::{
    ModalResult, Parser,
    combinator::repeat,
    error::InputError,
    token::{literal, one_of, take_while},
};

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
///   first segment      = \[A-Za-z\]\[A-Za-z0-9-\]*
///   subsequent segment = "." \[A-Za-z0-9\]\[A-Za-z0-9-\]*
///   name               = first_segment subsequent_segment+
pub fn parse_interface_name<'a>(
    input: &mut &'a [u8],
) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    (
        // First segment.
        (
            one_of(|c: u8| c.is_ascii_alphabetic()),
            take_while(0.., |c: u8| c.is_ascii_alphanumeric() || c == b'-'),
        ),
        // One or more dotted segments (so the name has at least one `.`).
        repeat::<_, _, (), _, _>(
            1..,
            (
                literal("."),
                one_of(|c: u8| c.is_ascii_alphanumeric()),
                take_while(0.., |c: u8| c.is_ascii_alphanumeric() || c == b'-'),
            )
                .void(),
        ),
    )
        .take()
        .map(bytes_to_str)
        .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;
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
