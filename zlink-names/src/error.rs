use core::fmt;

/// The error of converting a string that isn't a valid Varlink name into one.
///
/// It says which kind of name the string was meant to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The string is not a valid Varlink field name.
    InvalidFieldName,
    /// The string is not a valid Varlink type name.
    InvalidTypeName,
    /// The string is not a valid Varlink interface name.
    InvalidInterfaceName,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::InvalidFieldName => "Invalid Varlink field name",
            Self::InvalidTypeName => "Invalid Varlink type name",
            Self::InvalidInterfaceName => "Invalid Varlink interface name",
        };

        f.write_str(msg)
    }
}

impl core::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn display() {
        assert_eq!(
            Error::InvalidFieldName.to_string(),
            "Invalid Varlink field name"
        );
        assert_eq!(
            Error::InvalidTypeName.to_string(),
            "Invalid Varlink type name"
        );
        assert_eq!(
            Error::InvalidInterfaceName.to_string(),
            "Invalid Varlink interface name"
        );
    }
}
