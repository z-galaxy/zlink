//! Errors returned when parsing Varlink IDL.

use alloc::string::String;
use core::fmt;

/// An error that occurred while parsing Varlink IDL.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The input was empty.
    Empty,
    /// Input remained after the interface was parsed.
    TrailingInput(String),
    /// The input could not be parsed.
    Parse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Empty => write!(f, "Input is empty"),
            Error::TrailingInput(input) => write!(f, "Unexpected remaining input: {input:?}"),
            Error::Parse(e) => write!(f, "Parse error: {e}"),
        }
    }
}

impl core::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn display() {
        let cases = [
            (Error::Empty, "Input is empty"),
            (
                Error::TrailingInput("extra junk".to_string()),
                "Unexpected remaining input: \"extra junk\"",
            ),
            (
                Error::Parse("bad token".to_string()),
                "Parse error: bad token",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }
}
