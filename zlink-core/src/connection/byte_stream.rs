//! The byte stream transport, shared by the runtime crates.

use core::{fmt, future::Future, marker::PhantomData};
use std::{io, os::fd::AsFd};

use super::socket;

/// The [`Socket`](socket::Socket) implementation over a byte stream `T` of the runtime `B`.
///
/// Only Unix Domain Sockets carry file descriptors and peer credentials, so this socket refuses
/// to send either.
///
/// A stream converts into it, so `Connection::new(stream.into())` makes a connection over the
/// stream.
pub struct Socket<T, B> {
    stream: T,
    // `fn() -> B`, so that the socket is `Send` and `Sync` whenever the stream is.
    runtime: PhantomData<fn() -> B>,
}

impl<T, B> Socket<T, B>
where
    B: ByteStream<T>,
{
    /// Create a socket over `stream`.
    pub fn new(stream: T) -> Self {
        Self {
            stream,
            runtime: PhantomData,
        }
    }

    /// Consume the socket into the underlying stream.
    pub fn into_inner(self) -> T {
        self.stream
    }
}

impl<T, B> From<T> for Socket<T, B>
where
    B: ByteStream<T>,
{
    fn from(stream: T) -> Self {
        Self::new(stream)
    }
}

impl<T, B> socket::Socket for Socket<T, B>
where
    B: ByteStream<T>,
{
    type ReadHalf = ReadHalf<T, B>;
    type WriteHalf = WriteHalf<T, B>;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        let (read, write) = B::split(self.stream);

        (ReadHalf(read), WriteHalf(write))
    }

    fn join(read: Self::ReadHalf, write: Self::WriteHalf) -> crate::Result<Self> {
        let stream = B::join(read.0, write.0).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the halves are of two different streams",
            )
        })?;

        Ok(Self::new(stream))
    }
}

// Not derived, so that `T` needn't be `Debug`.
impl<T, B> fmt::Debug for Socket<T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Socket").finish_non_exhaustive()
    }
}

/// The [`ReadHalf`](socket::ReadHalf) implementation over a byte stream.
pub struct ReadHalf<T, B: ByteStream<T>>(B::ReadHalf);

impl<T, B> socket::ReadHalf for ReadHalf<T, B>
where
    B: ByteStream<T>,
{
    async fn read(&mut self, buf: &mut [u8]) -> crate::Result<socket::ReadResult> {
        let n = B::read(&mut self.0, buf).await?;

        Ok(socket::ReadResult::new(n))
    }
}

impl<T, B: ByteStream<T>> fmt::Debug for ReadHalf<T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ReadHalf").finish_non_exhaustive()
    }
}

/// The [`WriteHalf`](socket::WriteHalf) implementation over a byte stream.
pub struct WriteHalf<T, B: ByteStream<T>>(B::WriteHalf);

impl<T, B> socket::WriteHalf for WriteHalf<T, B>
where
    B: ByteStream<T>,
{
    async fn write(
        &mut self,
        buf: &[u8],
        fds: &[impl AsFd],
        #[cfg(target_os = "linux")] credentials: Option<&super::PassedCredentials>,
    ) -> crate::Result<usize> {
        if !fds.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a byte stream can't transfer file descriptors",
            )
            .into());
        }
        #[cfg(target_os = "linux")]
        if credentials.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a byte stream can't pass credentials",
            )
            .into());
        }

        let n = B::write(&mut self.0, buf).await?;

        Ok(n)
    }

    async fn flush(&mut self) -> crate::Result<()> {
        B::flush(&mut self.0).await?;

        Ok(())
    }
}

impl<T, B: ByteStream<T>> fmt::Debug for WriteHalf<T, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("WriteHalf").finish_non_exhaustive()
    }
}

/// The byte stream operations of an async runtime, for its streams of type `T`.
///
/// The runtimes have their own read and write traits, so each runtime crate implements this for a
/// marker type of its own, for every `T` that implements the traits of that runtime. The marker
/// only selects the implementation: it holds no stream.
pub trait ByteStream<T> {
    /// The read half of a stream.
    type ReadHalf;
    /// The write half of a stream.
    type WriteHalf;

    /// Split `stream` into its read and write halves.
    fn split(stream: T) -> (Self::ReadHalf, Self::WriteHalf);

    /// Join the halves back into the stream they were split from.
    ///
    /// Returns `None` if the halves aren't of one stream.
    fn join(read: Self::ReadHalf, write: Self::WriteHalf) -> Option<T>;

    /// Read from the stream into `buf`.
    ///
    /// Returns the number of bytes read, 0 at the end of the stream. The returned future has the
    /// same requirements as that of [`socket::ReadHalf::read`].
    fn read(read: &mut Self::ReadHalf, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>>;

    /// Write as much of `buf` to the stream as it accepts at once.
    ///
    /// Returns the number of bytes written, which can be fewer than `buf` holds. The returned
    /// future has the same requirements as that of [`socket::WriteHalf::write`].
    fn write(write: &mut Self::WriteHalf, buf: &[u8]) -> impl Future<Output = io::Result<usize>>;

    /// Flush any bytes buffered by previous writes.
    ///
    /// The returned future has the same requirements as that of [`socket::WriteHalf::flush`].
    fn flush(write: &mut Self::WriteHalf) -> impl Future<Output = io::Result<()>>;
}
