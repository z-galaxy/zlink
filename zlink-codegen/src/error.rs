/// Error types that can occur during Varlink interface code generation.
#[derive(Debug)]
pub enum Error {
    /// An invalid argument was provided
    InvalidArgument,

    /// An I/O error occurred during file operations.
    Io(std::io::Error),

    /// An error from the zlink-core library.
    Zlink(zlink::Error),

    /// A generic anyhow error.
    Anyhow(anyhow::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArgument => write!(f, "Invalid argument provided"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Zlink(e) => write!(f, "Zlink error: {e}"),
            Self::Anyhow(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Zlink(e) => Some(e),
            Self::Anyhow(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<zlink::Error> for Error {
    fn from(e: zlink::Error) -> Self {
        Self::Zlink(e)
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self::Anyhow(e)
    }
}
