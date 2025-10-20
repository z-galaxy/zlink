/// Error types that can occur during Varlink interface code generation.
#[derive(Debug)]
pub enum Error {
    /// An invalid argument was provided
    InvalidArgument,

    /// The code generation process failed.
    CodegenFailed,

    /// The generated code formatting failed.
    FormatFailed,

    /// An I/O error occurred during file operations.
    Io(std::io::Error),

    /// An error from the zlink-core library.
    Zlink(zlink::Error)
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<zlink::Error> for Error {
    fn from(e: zlink::Error) -> Self {
        Error::Zlink(e)
    }
}
