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
}

/// The read half of a socket.
pub trait ReadHalf: core::fmt::Debug {
    /// Read from a socket.
    ///
    /// On completion, the number of bytes read and any file descriptors received are returned
    /// (see [`ReadResult`]).
    ///
    /// Any file descriptors returned here are taken to belong to the message whose first byte
    /// arrives in this read. The [`connection`](crate::connection#file-descriptor-passing) module
    /// docs describe the full FD-to-message association contract; in short, implementers must
    /// return the FDs received by a given `recvmsg` together with the bytes received by that same
    /// `recvmsg`, so the positional association upheld by [`ReadConnection`](super::ReadConnection)
    /// is correct.
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
    /// Write to the socket.
    ///
    /// The `fds` parameter contains file descriptors to send along with the data (std only). The
    /// implementation must attach the given `fds` to the `sendmsg` that carries the first byte of
    /// `buf` (e.g. via `SCM_RIGHTS`), so that the receiver can associate them with the message
    /// starting at that byte. [`WriteConnection`](super::WriteConnection) only ever calls this with
    /// `fds` for a `buf` that begins at a message boundary; see the
    /// [`connection`](crate::connection#file-descriptor-passing) module docs for the full contract.
    ///
    /// The returned future has the same requirements as that of [`ReadHalf::read`].
    fn write(
        &mut self,
        buf: &[u8],
        #[cfg(feature = "std")] fds: &[impl AsFd],
        #[cfg(all(feature = "std", target_os = "linux"))] credentials: Option<
            &crate::connection::PassedCredentials,
        >,
    ) -> impl Future<Output = crate::Result<()>>;
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
        ) -> crate::Result<()> {
            unreachable!("This is only for doc tests")
        }
    }
}

/// Result type for [`ReadHalf::read`] operations.
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
