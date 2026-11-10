#[cfg(target_os = "linux")]
use crate::unix_utils::{self, SocketRole};
use crate::{
    Result,
    connection::socket::{self, Socket},
};
use std::{
    fmt,
    os::{
        fd::{AsFd, BorrowedFd},
        unix::net::UnixStream as StdUnixStream,
    },
    path::Path,
};
use zlink_core::connection::socket::ReadResult;
use zruntime::{Local, Mode, Runtime, Shared, SharedRuntime, net::unix};

/// The connection type that uses Unix Domain Sockets for transport.
///
/// `M` is the flavour of the zruntime runtime the socket is on, as for [`Stream`].
pub type Connection<M = Shared> = crate::Connection<Stream<M>>;

/// Connect to Unix Domain Socket at the given path.
///
/// The socket is on the runtime [`SharedRuntime::current`] returns, so the connection can be used
/// from any thread and polled by any executor. [`connect_local`] connects on the local runtime the
/// calling thread is running instead.
///
/// On Linux the resulting socket is tagged with a `user.varlink` extended attribute set to
/// `client`. Tagging is best effort; kernels without support (older than Linux 7.1) are silently
/// tolerated.
pub async fn connect<P>(path: P) -> Result<Connection>
where
    P: AsRef<Path>,
{
    connect_on(&SharedRuntime::current()?, path.as_ref()).await
}

/// Connect to Unix Domain Socket at the given path, on the local runtime the calling thread is
/// running.
///
/// The connection stays on the calling thread, and makes progress while the thread runs that
/// runtime, as a task of it or inside its [`Runtime::block_on`]. Fails where the thread is running
/// no local runtime.
///
/// The socket is tagged as by [`connect`].
pub async fn connect_local<P>(path: P) -> Result<Connection<Local>>
where
    P: AsRef<Path>,
{
    connect_on(&super::running_local()?, path.as_ref()).await
}

async fn connect_on<M>(runtime: &Runtime<M>, path: &Path) -> Result<Connection<M>>
where
    M: Mode,
{
    let stream = unix::UnixStream::connect(runtime, path).await?;
    #[cfg(target_os = "linux")]
    unix_utils::tag_socket(&stream, SocketRole::Client);

    Ok(crate::Connection::new(Stream::try_from(stream)?))
}

/// The [`Socket`] implementation using Unix Domain Sockets.
///
/// `M` is the flavour of the zruntime runtime the socket is on: [`Shared`], the default, for a
/// socket that can be used from any thread, or [`Local`], for one that stays on the thread that
/// made it.
pub struct Stream<M = Shared>(unix::UnixStream<M>)
where
    M: Mode;

impl<M> fmt::Debug for Stream<M>
where
    M: Mode,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Stream").field(&self.0).finish()
    }
}

impl<M> Socket for Stream<M>
where
    M: Mode,
{
    type ReadHalf = ReadHalf<M>;
    type WriteHalf = WriteHalf<M>;

    const CAN_TRANSFER_FDS: bool = true;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        let (read, write) = self.0.into_split();

        (ReadHalf(read), WriteHalf(write))
    }

    fn join(read: Self::ReadHalf, write: Self::WriteHalf) -> Result<Self> {
        let stream = read.0.reunite(write.0).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the halves are of two different sockets",
            )
        })?;

        Ok(Self(stream))
    }
}

impl<M> TryFrom<unix::UnixStream<M>> for Stream<M>
where
    M: Mode,
{
    type Error = crate::Error;

    fn try_from(stream: unix::UnixStream<M>) -> Result<Self> {
        #[cfg(target_os = "linux")]
        zlink_core::unix_utils::enable_passcred(&stream)?;
        Ok(Self(stream))
    }
}

/// The socket is on the runtime [`SharedRuntime::current`] returns.
impl TryFrom<StdUnixStream> for Stream {
    type Error = crate::Error;

    fn try_from(stream: StdUnixStream) -> Result<Self> {
        unix::UnixStream::from_std(&SharedRuntime::current()?, stream)?.try_into()
    }
}

impl<M> AsFd for Stream<M>
where
    M: Mode,
{
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl<M> socket::UnixSocket for Stream<M> where M: Mode {}

/// On a shared runtime, this may block for a moment, until a thread running the runtime returns
/// from a wait that watches the socket.
impl<M> socket::IntoStd for Stream<M>
where
    M: Mode,
{
    fn into_std(self) -> Result<StdUnixStream> {
        Ok(self.0.into_std())
    }
}

/// The [`ReadHalf`] implementation using Unix Domain Sockets.
///
/// [`ReadHalf`]: socket::ReadHalf
pub struct ReadHalf<M = Shared>(unix::ReadHalf<M>)
where
    M: Mode;

impl<M> fmt::Debug for ReadHalf<M>
where
    M: Mode,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ReadHalf").field(&self.0).finish()
    }
}

impl<M> socket::ReadHalf for ReadHalf<M>
where
    M: Mode,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<ReadResult> {
        self.0
            .read_with(|stream| crate::unix_utils::recvmsg(stream, buf))
            .await
            .map_err(Into::into)
    }
}

impl<M> AsFd for ReadHalf<M>
where
    M: Mode,
{
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl<M> socket::UnixSocket for ReadHalf<M> where M: Mode {}

/// The [`WriteHalf`] implementation using Unix Domain Sockets.
///
/// [`WriteHalf`]: socket::WriteHalf
pub struct WriteHalf<M = Shared>(unix::WriteHalf<M>)
where
    M: Mode;

impl<M> fmt::Debug for WriteHalf<M>
where
    M: Mode,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("WriteHalf").field(&self.0).finish()
    }
}

impl<M> socket::WriteHalf for WriteHalf<M>
where
    M: Mode,
{
    async fn write(
        &mut self,
        buf: &[u8],
        fds: &[impl AsFd],
        #[cfg(target_os = "linux")] creds: Option<&crate::connection::PassedCredentials>,
    ) -> Result<()> {
        // Convert to BorrowedFd for rustix.
        let borrowed_fds: Vec<BorrowedFd<'_>> = fds.iter().map(|f| f.as_fd()).collect();

        let mut pos = 0;
        while pos < buf.len() {
            // Use FDs on first write, empty slice on subsequent writes.
            let fds_to_send = if pos == 0 { &borrowed_fds[..] } else { &[] };

            pos += self
                .0
                .write_with(|stream| {
                    crate::unix_utils::sendmsg(
                        stream,
                        &buf[pos..],
                        fds_to_send,
                        #[cfg(target_os = "linux")]
                        creds,
                    )
                })
                .await?;
        }

        Ok(())
    }
}

impl<M> AsFd for WriteHalf<M>
where
    M: Mode,
{
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl<M> socket::UnixSocket for WriteHalf<M> where M: Mode {}

#[cfg(test)]
mod tests {
    use super::*;
    use socket::IntoStd;

    fn assert_send<T: Send>(_: &T) {}

    /// The std socket that a stream is given up as is open both ways.
    #[test]
    fn into_std_leaves_the_socket_open() {
        let (ours, theirs) = StdUnixStream::pair().unwrap();
        let ours = Stream::try_from(ours).unwrap().into_std().unwrap();

        check_open(ours, theirs);
    }

    /// The socket that its halves are joined back into is open both ways, as a std socket.
    #[test]
    fn halves_join_back_into_an_open_socket() {
        let (ours, theirs) = StdUnixStream::pair().unwrap();
        let (read, write) = Stream::try_from(ours).unwrap().split();
        let ours = Stream::join(read, write).unwrap().into_std().unwrap();

        check_open(ours, theirs);
    }

    /// The halves of two different sockets aren't joined.
    #[test]
    fn halves_of_two_sockets_are_not_joined() {
        let (read, _) = Stream::try_from(StdUnixStream::pair().unwrap().0)
            .unwrap()
            .split();
        let (_, write) = Stream::try_from(StdUnixStream::pair().unwrap().0)
            .unwrap()
            .split();

        assert!(Stream::join(read, write).is_err());
    }

    /// Connections on a shared runtime, and connecting, must be usable from multi-threaded
    /// executors.
    #[test]
    fn connection_is_send() {
        let future = connect("/nonexistent.sock");
        assert_send(&future);
        assert!(zruntime::block_on(future).is_err());

        let (stream, _peer) = StdUnixStream::pair().unwrap();
        let connection = Connection::new(Stream::try_from(stream).unwrap());
        assert_send(&connection);
    }

    /// Nothing is there to connect on outside a local runtime: the free `block_on` runs a shared
    /// one.
    #[test]
    fn connect_local_needs_a_local_runtime() {
        assert!(zruntime::block_on(connect_local("/nonexistent.sock")).is_err());
    }

    /// Check that `ours` and `theirs`, the two ends of a connected pair, are both open both ways.
    fn check_open(ours: StdUnixStream, theirs: StdUnixStream) {
        use std::io::{Read, Write};

        ours.set_nonblocking(false).unwrap();
        for (mut from, mut to) in [(&ours, &theirs), (&theirs, &ours)] {
            from.write_all(b"ping").unwrap();
            let mut buf = [0; 4];
            to.read_exact(&mut buf).unwrap();
            assert_eq!(&buf, b"ping");
        }
    }
}
