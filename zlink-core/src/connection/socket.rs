//! The low-level Socket read and write traits.

use core::future::Future;

#[cfg(feature = "std")]
use std::os::fd::{AsFd, OwnedFd};

/// The socket trait.
///
/// This is the trait that needs to be implemented for a type to be used as a socket/transport.
pub trait Socket: core::fmt::Debug {
    /// The read half of the socket.
    type ReadHalf: ReadHalf;
    /// The write half of the socket.
    type WriteHalf: WriteHalf;

    /// Whether this socket can transfer file descriptors.
    ///
    /// This is `true` for Unix domain sockets and `false` for other socket types.
    const CAN_TRANSFER_FDS: bool = false;

    /// Split the socket into read and write halves.
    fn split(self) -> (Self::ReadHalf, Self::WriteHalf);

    /// Join `read` and `write`, the halves that [`Socket::split`] split a socket into, back into
    /// it (the opposite of [`Socket::split`]).
    ///
    /// It fails if the halves aren't of one socket.
    fn join(read: Self::ReadHalf, write: Self::WriteHalf) -> crate::Result<Self>
    where
        Self: Sized;
}

/// The read half of a socket.
pub trait ReadHalf: core::fmt::Debug {
    /// Read from a socket.
    ///
    /// On completion, the outcome of the read is returned as a [`ReadResult`].
    ///
    /// Notes for implementers:
    ///
    /// * The future returned by this method must be cancel safe.
    /// * While there is no explicit `Unpin` bound on the future returned by this method, it is
    ///   expected that it provides the same guarantees as `Unpin` would require. The reason `Unpin`
    ///   is not explicitly required is that it would force boxing (and therefore allocation) on the
    ///   implementation that use `async fn`, which is undesirable for embedded use cases. See [this
    ///   issue](https://github.com/rust-lang/rust/issues/82187) for details.
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = crate::Result<ReadResult>>;
}

/// The write half of a socket.
pub trait WriteHalf: core::fmt::Debug {
    /// Write as much of `buf` to the socket as it accepts at once.
    ///
    /// It returns the number of bytes written, which can be fewer than `buf` holds. The `fds`
    /// parameter contains file descriptors to send along with the data (std only).
    ///
    /// Notes for implementers:
    ///
    /// * The returned future has the same requirements as that of [`ReadHalf::read`]. To be cancel
    ///   safe, it must write nothing if it's dropped before it completes, so it writes in a single
    ///   operation, such as one `sendmsg` call.
    /// * It waits until the socket accepts some of `buf`, and then writes at least one byte of it,
    ///   unless `buf` is empty or the write fails.
    /// * The file descriptors must be sent along with the first byte of `buf`. When there are file
    ///   descriptors to pass, `buf` starts with the first byte of the message they belong to, in
    ///   line with the [file descriptor passing] convention. Implementations for Unix domain
    ///   sockets can use the hidden `zlink_core::unix_utils::sendmsg` helper, which does that.
    ///
    /// [file descriptor passing]: crate::connection#file-descriptor-passing
    fn write(
        &mut self,
        buf: &[u8],
        #[cfg(feature = "std")] fds: &[impl AsFd],
        #[cfg(all(feature = "std", target_os = "linux"))] credentials: Option<
            &crate::connection::PassedCredentials,
        >,
    ) -> impl Future<Output = crate::Result<usize>>;

    /// Write all of `buf` to the socket, calling [`WriteHalf::write`] as many times as needed, and
    /// then [`WriteHalf::flush`].
    ///
    /// The file descriptors are sent with the first call, along with the first byte of `buf`, and
    /// the credentials with every call.
    ///
    /// Unlike that of [`WriteHalf::write`], the returned future isn't cancel safe: if it's dropped
    /// before it completes, part of `buf` may have been written, with no way to tell how much.
    fn write_all(
        &mut self,
        buf: &[u8],
        #[cfg(feature = "std")] fds: &[impl AsFd],
        #[cfg(all(feature = "std", target_os = "linux"))] credentials: Option<
            &crate::connection::PassedCredentials,
        >,
    ) -> impl Future<Output = crate::Result<()>> {
        async move {
            let mut pos = 0;
            while pos < buf.len() {
                // The file descriptors go with the first byte alone.
                #[cfg(feature = "std")]
                let fds = if pos == 0 { fds } else { &fds[..0] };
                let written = self
                    .write(
                        &buf[pos..],
                        #[cfg(feature = "std")]
                        fds,
                        #[cfg(all(feature = "std", target_os = "linux"))]
                        credentials,
                    )
                    .await?;
                // A socket that accepts nothing would have us loop forever.
                if written == 0 {
                    return Err(crate::Error::SocketWrite);
                }
                pos += written;
            }

            self.flush().await
        }
    }

    /// Flush any bytes buffered by previous writes.
    ///
    /// The connection calls this after writing all that is queued. The default does nothing: only
    /// a socket that buffers, such as one over TLS or a WebSocket, needs to implement it. `write`
    /// can't flush by itself, as a flush dropped midway would lose the count of the bytes written.
    ///
    /// Notes for implementers:
    ///
    /// * The returned future has the same requirements as that of [`ReadHalf::read`].
    /// * If the future is dropped before it completes, the socket must keep the bytes it has not
    ///   sent yet, and its progress, so that the next flush resumes without loss or duplication.
    fn flush(&mut self) -> impl Future<Output = crate::Result<()>> {
        async { Ok(()) }
    }
}

/// A socket that is a Unix domain socket, which it can be given up as.
///
/// A connection over such a socket can be handed over to another process (with the `handover`
/// feature, on Linux). The runtime crates implement this for their Unix domain sockets.
#[cfg(feature = "std")]
pub trait IntoStd: Socket {
    /// The std Unix domain socket that this socket is.
    ///
    /// The socket is left open, as it is but for no longer being watched by a runtime, if it was,
    /// so that something else can go on with it, such as another process.
    fn into_std(self) -> crate::Result<std::os::unix::net::UnixStream>;
}

/// Trait for fetching peer credentials from a socket.
///
/// This trait provides the low-level capability to fetch credentials from a socket's underlying
/// file descriptor. It is typically implemented by socket read halves that support credentials.
#[cfg(feature = "std")]
pub trait FetchPeerCredentials {
    /// Fetch the peer credentials for this socket.
    ///
    /// This is the low-level method that socket implementations should override to provide peer
    /// credentials. Higher-level APIs should use [`super::Connection::peer_credentials`] instead.
    fn fetch_peer_credentials(&self) -> impl Future<Output = std::io::Result<super::Credentials>>;
}

/// Trait for Unix Domain Sockets.
///
/// Implementing this trait signals that the type is a Unix Domain Socket (UDS) where credentials
/// fetching through a file descriptor will work correctly. [`FetchPeerCredentials`] is implemented
/// for all types that implement this trait.
#[cfg(feature = "std")]
pub trait UnixSocket: AsFd {}

#[cfg(feature = "std")]
impl<T> FetchPeerCredentials for T
where
    T: UnixSocket,
{
    async fn fetch_peer_credentials(&self) -> std::io::Result<super::Credentials> {
        // Assume peer credentials fetching never blocks so it's fine to call this synchronous
        // method from an async context.
        crate::unix_utils::get_peer_credentials(self)
    }
}

/// Documentation-only socket implementations for doc tests.
///
/// These types exist only to make doc tests compile and should never be used in real code.
#[doc(hidden)]
pub mod impl_for_doc {

    /// A mock socket for documentation examples.
    #[derive(Debug)]
    pub struct Socket;

    impl super::Socket for Socket {
        type ReadHalf = ReadHalf;
        type WriteHalf = WriteHalf;

        fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
            (ReadHalf, WriteHalf)
        }

        fn join(_read: Self::ReadHalf, _write: Self::WriteHalf) -> crate::Result<Self> {
            Ok(Socket)
        }
    }

    /// A mock read half for documentation examples.
    #[derive(Debug)]
    pub struct ReadHalf;

    impl super::ReadHalf for ReadHalf {
        async fn read(&mut self, _buf: &mut [u8]) -> crate::Result<super::ReadResult> {
            unreachable!("This is only for doc tests")
        }
    }

    /// A mock write half for documentation examples.
    #[derive(Debug)]
    pub struct WriteHalf;

    impl super::WriteHalf for WriteHalf {
        async fn write(
            &mut self,
            _buf: &[u8],
            #[cfg(feature = "std")] _fds: &[impl super::AsFd],
            #[cfg(all(feature = "std", target_os = "linux"))] _credentials: Option<
                &crate::connection::PassedCredentials,
            >,
        ) -> crate::Result<usize> {
            unreachable!("This is only for doc tests")
        }
    }
}

/// Result type for [`ReadHalf::read`] operations.
///
/// Along with the number of bytes read, it holds the file descriptors (`std` only) and credentials
/// (`std` on Linux only) received with those bytes.
///
/// Implementations of [`ReadHalf`] must return the file descriptors received by the same `recvmsg`
/// call as the bytes, since the [file descriptor passing] convention associates file descriptors
/// with the message whose first byte came with them. Implementations for Unix domain sockets can
/// use the hidden `zlink_core::unix_utils::recvmsg` helper, which does that.
///
/// [file descriptor passing]: crate::connection#file-descriptor-passing
#[derive(Debug)]
pub struct ReadResult {
    /// The number of bytes read.
    bytes_read: usize,
    /// The file descriptors received, if any. This is only available with the `std` feature.
    #[cfg(feature = "std")]
    fds: alloc::vec::Vec<OwnedFd>,
    /// The credentials received, if any. This is only available with the `std` feature and `linux`
    /// target.
    #[cfg(all(feature = "std", target_os = "linux"))]
    credentials: Option<crate::connection::PassedCredentials>,
}

impl ReadResult {
    /// Creates a new `ReadResult` with the given number of bytes read.
    #[doc(hidden)]
    pub fn new(bytes_read: usize) -> Self {
        Self {
            bytes_read,
            #[cfg(feature = "std")]
            fds: vec![],
            #[cfg(all(feature = "std", target_os = "linux"))]
            credentials: None,
        }
    }

    /// The number of bytes read.
    pub fn bytes_read(&self) -> usize {
        self.bytes_read
    }

    /// The file descriptors received, if any. This is only available with the `std` feature.
    ///
    /// These belong to the message whose first byte was received along with them, as per the
    /// [file descriptor passing] convention.
    ///
    /// [file descriptor passing]: crate::connection#file-descriptor-passing
    #[cfg(feature = "std")]
    pub fn fds(&self) -> &[OwnedFd] {
        &self.fds
    }

    /// The credentials received, if any. This is only available with the `std` feature and `linux`
    /// target.
    #[cfg(all(feature = "std", target_os = "linux"))]
    pub fn credentials(&self) -> Option<&crate::connection::PassedCredentials> {
        self.credentials.as_ref()
    }

    /// Sets the file descriptors received, if any. This is only available with the `std` feature.
    #[cfg(feature = "std")]
    #[doc(hidden)]
    pub fn set_fds<F>(mut self, fds: F) -> Self
    where
        F: Into<alloc::vec::Vec<OwnedFd>>,
    {
        self.fds = fds.into();

        self
    }

    /// Sets the credentials received, if any. This is only available with the `std` feature and
    /// `linux` target.
    #[cfg(all(feature = "std", target_os = "linux"))]
    #[doc(hidden)]
    pub fn set_credentials(
        mut self,
        credentials: Option<crate::connection::PassedCredentials>,
    ) -> Self {
        self.credentials = credentials;

        self
    }

    /// Takes the file descriptors received, leaving an empty list in their place. This is only
    /// available with the `std` feature.
    #[cfg(feature = "std")]
    pub fn take_fds(&mut self) -> alloc::vec::Vec<OwnedFd> {
        core::mem::take(&mut self.fds)
    }

    /// Consumes this `ReadResult` and returns the file descriptors received, if any. This is only
    /// available with the `std` feature.
    #[cfg(feature = "std")]
    pub fn into_fds(self) -> alloc::vec::Vec<OwnedFd> {
        self.fds
    }

    /// Takes the credentials received, leaving `None` in their place. This is only available with
    /// the `std` feature and `linux` target.
    #[cfg(all(feature = "std", target_os = "linux"))]
    pub fn take_credentials(&mut self) -> Option<crate::connection::PassedCredentials> {
        self.credentials.take()
    }

    /// Consumes this `ReadResult` and returns the credentials received, if any. This is only
    /// available with the `std` feature and `linux` target.
    #[cfg(all(feature = "std", target_os = "linux"))]
    pub fn into_credentials(self) -> Option<crate::connection::PassedCredentials> {
        self.credentials
    }
}
