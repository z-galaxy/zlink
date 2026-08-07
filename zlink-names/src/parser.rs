use winnow::error::{ErrMode, InputError};

/// Whether `parser` accepts the whole of `name`, leaving nothing behind.
///
/// The parsers succeed on a valid prefix (e.g. `field_name` takes `foo` out of `foo-bar`), so a
/// name is only valid when parsing also consumes every byte.
pub(crate) fn parses_fully<'a, P, T>(mut parser: P, name: &'a str) -> bool
where
    P: FnMut(&mut &'a [u8]) -> Result<T, ErrMode<InputError<&'a [u8]>>>,
{
    let mut input = name.as_bytes();

    parser(&mut input).is_ok() && input.is_empty()
}

/// Convert bytes to str with input lifetime.
pub(crate) fn bytes_to_str(bytes: &[u8]) -> &str {
    // SAFETY: We only accept ASCII characters in our parsers
    core::str::from_utf8(bytes).unwrap()
}
