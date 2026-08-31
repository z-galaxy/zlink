use core::{
    error::Error,
    fmt::{Display, Write},
};

use alloc::string::String;
use winnow::error::{ErrMode, InputError};

#[derive(Debug)]
pub struct TryFromError {
    buf: String,
}

impl Error for TryFromError {}

impl Display for TryFromError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.buf)
    }
}

impl<'a> From<ErrMode<InputError<&'a [u8]>>> for TryFromError {
    fn from(value: ErrMode<InputError<&'a [u8]>>) -> Self {
        let mut buf = String::new();
        // `write!` never fails for a `String` sink.
        let _ = write!(buf, "{value}");
        Self { buf }
    }
}
