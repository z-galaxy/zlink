#[cfg(target_os = "linux")]
use crate::unix_utils::{self, SocketRole};
use crate::{
    Result,
    connection::socket::{self, Socket},
};
use async_io::Async;
use std::{
    os::{
        fd::{AsFd, BorrowedFd},
        unix::net::UnixStream as StdUnixStream,
    },
    sync::Arc,
};
use zlink_core::connection::socket::ReadResult;

/// The connection type that uses Unix Domain Sockets for transport.
pub type Connection = crate::Connection<Stream>;

/// Connect to Unix Domain Socket at the given path.
///
/// On Linux the resulting socket is tagged with a `user.varlink` extended attribute set to
/// `client`. Tagging is best effort; kernels without support (older than Linux 7.1) are silently
/// tolerated.
pub async fn connect<P>(path: P) -> Result<Connection>
where
    P: AsRef<std::path::Path>,
{
    let stream = Async::<StdUnixStream>::connect(path).await?;
    #[cfg(target_os = "linux")]
    unix_utils::tag_socket(&stream, SocketRole::Client);
    let stream = Stream::try_from(stream)?;

    Ok(Connection::new(stream))
}

/// The [`Socket`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct Stream(Async<StdUnixStream>);

impl Socket for Stream {
    type ReadHalf = ReadHalf;
    type WriteHalf = WriteHalf;

    const CAN_TRANSFER_FDS: bool = true;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        let stream = Arc::new(self.0);

        (ReadHalf(Arc::clone(&stream)), WriteHalf(stream))
    }
}

impl TryFrom<Async<StdUnixStream>> for Stream {
    type Error = crate::Error;

    fn try_from(stream: Async<StdUnixStream>) -> Result<Self> {
        #[cfg(target_os = "linux")]
        zlink_core::unix_utils::enable_passcred(&stream)?;
        Ok(Self(stream))
    }
}

impl TryFrom<StdUnixStream> for Stream {
    type Error = crate::Error;

    fn try_from(stream: StdUnixStream) -> Result<Self> {
        stream.set_nonblocking(true)?;
        Async::new(stream)?.try_into()
    }
}

impl AsFd for Stream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl socket::UnixSocket for Stream {}

impl socket::IntoStd for Stream {
    fn into_std(self) -> Result<StdUnixStream> {
        self.0.into_inner().map_err(Into::into)
    }
}

/// The [`ReadHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct ReadHalf(Arc<Async<StdUnixStream>>);

impl socket::ReadHalf for ReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> Result<ReadResult> {
        use std::{future::poll_fn, task::Poll};

        poll_fn(|cx| {
            loop {
                match crate::unix_utils::recvmsg(self.0.as_ref(), buf) {
                    Ok(result) => return Poll::Ready(Ok(result)),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        match self.0.poll_readable(cx) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(res) => res?,
                        }
                    }
                    Err(e) => return Poll::Ready(Err(e.into())),
                }
            }
        })
        .await
    }
}

impl AsFd for ReadHalf {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_ref().as_fd()
    }
}

impl socket::UnixSocket for ReadHalf {}

/// The [`WriteHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct WriteHalf(Arc<Async<StdUnixStream>>);

impl socket::WriteHalf for WriteHalf {
    async fn write(
        &mut self,
        buf: &[u8],
        fds: &[impl AsFd],
        #[cfg(target_os = "linux")] creds: Option<&crate::connection::PassedCredentials>,
    ) -> Result<()> {
        use std::{future::poll_fn, task::Poll};

        // Convert to BorrowedFd for rustix.
        let borrowed_fds: Vec<BorrowedFd<'_>> = fds.iter().map(|f| f.as_fd()).collect();

        let mut pos = 0;
        while pos < buf.len() {
            // Use FDs on first write, empty slice on subsequent writes.
            let fds_to_send = if pos == 0 { &borrowed_fds[..] } else { &[] };

            let n: usize = poll_fn(|cx| {
                loop {
                    match crate::unix_utils::sendmsg(
                        self.0.as_ref(),
                        &buf[pos..],
                        fds_to_send,
                        #[cfg(target_os = "linux")]
                        creds,
                    ) {
                        Ok(bytes_sent) => return Poll::Ready(Ok::<_, crate::Error>(bytes_sent)),
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            match self.0.poll_writable(cx) {
                                Poll::Pending => return Poll::Pending,
                                Poll::Ready(res) => res?,
                            }
                        }
                        Err(e) => return Poll::Ready(Err(e.into())),
                    }
                }
            })
            .await?;

            pos += n;
        }

        Ok(())
    }
}

impl AsFd for WriteHalf {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_ref().as_fd()
    }
}

impl socket::UnixSocket for WriteHalf {}

#[cfg(test)]
mod tests {
    use super::*;
    use socket::IntoStd;

    /// The std socket that a stream is given up as is open both ways.
    #[test]
    fn into_std_leaves_the_socket_open() {
        let (ours, theirs) = StdUnixStream::pair().unwrap();
        let ours = Stream::try_from(ours).unwrap().into_std().unwrap();

        check_open(ours, theirs);
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
