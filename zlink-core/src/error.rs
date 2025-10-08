use core::str::Utf8Error;

/// The Error type for the zlink crate.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An error occurred while reading from the socket.
    SocketRead,
    /// An error occurred while writing to the socket.
    SocketWrite,
    /// Buffer overflow.
    BufferOverflow,
    /// Invalid UTF-8 data.
    InvalidUtf8(Utf8Error),
    /// Error serializing or deserializing to/from JSON.
    Json(serde_json::Error),
    /// An I/O error.
    #[cfg(feature = "std")]
    Io(std::io::Error),
    /// Unexpected end of file/stream.
    UnexpectedEof,
    /// An error occurred while parsing IDL.
    #[cfg(feature = "idl-parse")]
    IdlParse(alloc::string::String),
    /// Missing required parameters.
    MissingParameters,
    /// A general service error.
    VarlinkService(crate::varlink_service::Error),
}

/// The Result type for the zlink crate.
pub type Result<T> = core::result::Result<T, Error>;

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Error::Json(e) => Some(e),
            #[cfg(feature = "std")]
            Error::Io(e) => Some(e),
            Error::InvalidUtf8(e) => Some(e),
            #[cfg(feature = "idl-parse")]
            Error::IdlParse(_) => None,
            Error::VarlinkService(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::SocketRead => write!(f, "An error occurred while reading from the socket"),
            Error::SocketWrite => write!(f, "An error occurred while writing to the socket"),
            Error::BufferOverflow => write!(f, "Buffer overflow"),
            Error::InvalidUtf8(e) => write!(f, "Invalid UTF-8 data: {e}"),
            Error::Json(e) => write!(f, "Error serializing or deserializing to/from JSON: {e}"),
            #[cfg(feature = "std")]
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::UnexpectedEof => write!(f, "Unexpected end of file/stream"),
            #[cfg(feature = "idl-parse")]
            Error::IdlParse(e) => write!(f, "IDL parse error: {e}"),
            Error::MissingParameters => write!(f, "Missing required parameters"),
            Error::VarlinkService(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for Error {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        match self {
            Error::SocketRead => {
                defmt::write!(fmt, "An error occurred while reading from the socket")
            }
            Error::SocketWrite => {
                defmt::write!(fmt, "An error occurred while writing to the socket")
            }
            Error::BufferOverflow => defmt::write!(fmt, "Buffer overflow"),
            Error::InvalidUtf8(_) => defmt::write!(fmt, "Invalid UTF-8 data"),
            Error::Json(_) => {
                defmt::write!(fmt, "Error serializing or deserializing to/from JSON")
            }
            #[cfg(feature = "std")]
            Error::Io(_) => defmt::write!(fmt, "I/O error"),
            Error::UnexpectedEof => defmt::write!(fmt, "I/O error"),
            #[cfg(feature = "idl-parse")]
            Error::IdlParse(_) => defmt::write!(fmt, "IDL parse error"),
            Error::MissingParameters => defmt::write!(fmt, "Missing required parameters"),
            Error::VarlinkService(_) => defmt::write!(fmt, "Varlink service error"),
        }
    }
}
