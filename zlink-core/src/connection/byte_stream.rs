//! The byte stream transport, shared by the runtime crates.

/// Define the byte stream transport in the invoking module.
///
/// Each runtime crate gets its own copy, as the runtimes have their own `AsyncRead` and
/// `AsyncWrite` traits. `join` returns `None` for halves that aren't of one stream.
#[doc(hidden)]
#[macro_export]
macro_rules! impl_byte_stream {
    (io: $($io:ident)::+, split: $split:expr, join: $join:expr $(,)?) => {
        use $($io)::+::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
        use $crate::connection::socket::{self, Socket};

        /// The connection type that uses a byte stream for transport.
        pub type Connection<T> = $crate::Connection<Stream<T>>;

        /// The [`Socket`] implementation over any byte stream.
        ///
        /// Varlink needs no more than a reliable, ordered byte stream, so a connection can run
        /// over a TCP or TLS stream, or a WebSocket exposed through [`AsyncRead`] and
        /// [`AsyncWrite`]. Only Unix Domain Sockets carry file descriptors and peer credentials;
        /// for those, use [`unix::Stream`](crate::unix::Stream).
        ///
        /// A stream converts into it, so `Connection::new(stream.into())` makes a connection over
        /// the stream.
        pub struct Stream<T>(T);

        impl<T> Stream<T>
        where
            T: AsyncRead + AsyncWrite + Unpin,
        {
            /// Create a socket over `stream`.
            pub fn new(stream: T) -> Self {
                Self(stream)
            }

            /// Consume the socket into the underlying stream.
            pub fn into_inner(self) -> T {
                self.0
            }
        }

        impl<T> From<T> for Stream<T>
        where
            T: AsyncRead + AsyncWrite + Unpin,
        {
            fn from(stream: T) -> Self {
                Self(stream)
            }
        }

        impl<T> Socket for Stream<T>
        where
            T: AsyncRead + AsyncWrite + Unpin,
        {
            type ReadHalf = ReadHalf<T>;
            type WriteHalf = WriteHalf<T>;

            fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
                let (read, write) = ($split)(self.0);

                (ReadHalf(read), WriteHalf(write))
            }

            fn join(read: Self::ReadHalf, write: Self::WriteHalf) -> $crate::Result<Self> {
                let join: fn($($io)::+::ReadHalf<T>, $($io)::+::WriteHalf<T>) -> Option<T> = $join;
                let stream = join(read.0, write.0).ok_or_else(|| {
                    ::std::io::Error::new(
                        ::std::io::ErrorKind::InvalidInput,
                        "the halves are of two different streams",
                    )
                })?;

                Ok(Self(stream))
            }
        }

        // Not derived, so that `T` needn't be `Debug`.
        impl<T> ::core::fmt::Debug for Stream<T> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_tuple("Stream").finish_non_exhaustive()
            }
        }

        /// The [`ReadHalf`](socket::ReadHalf) implementation over a byte stream.
        pub struct ReadHalf<T>($($io)::+::ReadHalf<T>);

        impl<T> socket::ReadHalf for ReadHalf<T>
        where
            T: AsyncRead + Unpin,
        {
            // `AsyncReadExt::read` is cancel safe, which the trait requires.
            async fn read(&mut self, buf: &mut [u8]) -> $crate::Result<socket::ReadResult> {
                let n = self.0.read(buf).await?;

                Ok(socket::ReadResult::new(n))
            }
        }

        impl<T> ::core::fmt::Debug for ReadHalf<T> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_tuple("ReadHalf").finish_non_exhaustive()
            }
        }

        /// The [`WriteHalf`](socket::WriteHalf) implementation over a byte stream.
        pub struct WriteHalf<T>($($io)::+::WriteHalf<T>);

        impl<T> socket::WriteHalf for WriteHalf<T>
        where
            T: AsyncWrite + Unpin,
        {
            async fn write(
                &mut self,
                buf: &[u8],
                fds: &[impl ::std::os::fd::AsFd],
                #[cfg(target_os = "linux")] credentials: Option<
                    &$crate::connection::PassedCredentials,
                >,
            ) -> $crate::Result<usize> {
                if !fds.is_empty() {
                    return Err(::std::io::Error::new(
                        ::std::io::ErrorKind::Unsupported,
                        "a byte stream can't transfer file descriptors",
                    )
                    .into());
                }
                #[cfg(target_os = "linux")]
                if credentials.is_some() {
                    return Err(::std::io::Error::new(
                        ::std::io::ErrorKind::Unsupported,
                        "a byte stream can't pass credentials",
                    )
                    .into());
                }

                // `AsyncWriteExt::write` is cancel safe, which the trait requires.
                let n = self.0.write(buf).await?;

                Ok(n)
            }

            // The stream may buffer, as TLS streams and WebSocket adapters do.
            async fn flush(&mut self) -> $crate::Result<()> {
                self.0.flush().await?;

                Ok(())
            }
        }

        impl<T> ::core::fmt::Debug for WriteHalf<T> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_tuple("WriteHalf").finish_non_exhaustive()
            }
        }
    };
}
