#[cfg(target_os = "linux")]
use crate::unix_utils::{self, SocketRole};
use crate::{
    Result,
    connection::socket::{self, Socket},
};
use std::os::{
    fd::{AsFd, BorrowedFd},
    unix::net::UnixStream as StdUnixStream,
};
use tokio::net::{UnixStream, unix};
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
    let stream = UnixStream::connect(path).await?;
    #[cfg(target_os = "linux")]
    unix_utils::tag_socket(&stream, SocketRole::Client);
    let stream = Stream::try_from(stream)?;

    Ok(Connection::new(stream))
}

/// The [`Socket`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct Stream(UnixStream);

impl Socket for Stream {
    type ReadHalf = ReadHalf;
    type WriteHalf = WriteHalf;

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

impl TryFrom<UnixStream> for Stream {
    type Error = crate::Error;

    fn try_from(stream: UnixStream) -> Result<Self> {
        #[cfg(target_os = "linux")]
        zlink_core::unix_utils::enable_passcred(&stream)?;
        Ok(Self(stream))
    }
}

impl TryFrom<StdUnixStream> for Stream {
    type Error = crate::Error;

    fn try_from(stream: StdUnixStream) -> Result<Self> {
        stream.set_nonblocking(true)?;
        UnixStream::from_std(stream)
            .map_err(Into::into)
            .and_then(TryInto::try_into)
    }
}

impl socket::UnixSocket for Stream {}

impl socket::IntoStd for Stream {
    fn into_std(self) -> Result<StdUnixStream> {
        self.0.into_std().map_err(Into::into)
    }
}

impl AsFd for Stream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

/// The [`ReadHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct ReadHalf(unix::OwnedReadHalf);

impl socket::ReadHalf for ReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> Result<ReadResult> {
        use std::{future::poll_fn, task::Poll};

        poll_fn(|cx| {
            loop {
                let stream: &UnixStream = self.0.as_ref();
                match stream.try_io(tokio::io::Interest::READABLE, || {
                    crate::unix_utils::recvmsg(stream, buf)
                }) {
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        match stream.poll_read_ready(cx) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(res) => res?,
                        }
                    }
                    v => return Poll::Ready(v.map_err(Into::into)),
                }
            }
        })
        .await
    }
}

impl AsFd for ReadHalf {
    fn as_fd(&self) -> BorrowedFd<'_> {
        let stream: &UnixStream = self.0.as_ref();
        stream.as_fd()
    }
}

impl socket::UnixSocket for ReadHalf {}

/// The [`WriteHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct WriteHalf(unix::OwnedWriteHalf);

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
                    let stream: &UnixStream = self.0.as_ref();
                    match stream.try_io(tokio::io::Interest::WRITABLE, || {
                        crate::unix_utils::sendmsg(
                            stream,
                            &buf[pos..],
                            fds_to_send,
                            #[cfg(target_os = "linux")]
                            creds,
                        )
                    }) {
                        Ok(bytes_sent) => return Poll::Ready(Ok::<_, crate::Error>(bytes_sent)),
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            match stream.poll_write_ready(cx) {
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
        let stream: &UnixStream = self.0.as_ref();
        stream.as_fd()
    }
}

impl socket::UnixSocket for WriteHalf {}

#[cfg(test)]
mod tests {
    use super::*;
    use socket::IntoStd;
    use std::{
        io::Write,
        os::{
            fd::{FromRawFd, IntoRawFd},
            unix::net::UnixStream as StdUnixStream,
        },
    };

    /// The std socket that a stream is given up as is open both ways.
    #[tokio::test]
    async fn into_std_leaves_the_socket_open() {
        let (ours, theirs) = StdUnixStream::pair().unwrap();
        let ours = Stream::try_from(ours).unwrap().into_std().unwrap();

        check_open(ours, theirs);
    }

    /// The socket that its halves are joined back into is open both ways, as a std socket, where
    /// dropping the write half would have shut it down.
    #[tokio::test]
    async fn halves_join_back_into_an_open_socket() {
        let (ours, theirs) = StdUnixStream::pair().unwrap();
        let (read, write) = Stream::try_from(ours).unwrap().split();
        let ours = Stream::join(read, write).unwrap().into_std().unwrap();

        check_open(ours, theirs);
    }

    /// The halves of two different sockets aren't joined.
    #[tokio::test]
    async fn halves_of_two_sockets_are_not_joined() {
        let (read, _) = Stream::try_from(StdUnixStream::pair().unwrap().0)
            .unwrap()
            .split();
        let (_, write) = Stream::try_from(StdUnixStream::pair().unwrap().0)
            .unwrap()
            .split();

        assert!(Stream::join(read, write).is_err());
    }

    /// Verify that FD passing works when sender and receiver use **separate** connections (as in
    /// cross-process communication). On macOS, same-connection FD passing requires a workaround
    /// (see `WriteConnection::held_fds`), but separate connections should work without it because
    /// FD number reuse between `sendmsg` and `recvmsg` cannot happen across different FD tables.
    ///
    /// This test intentionally uses split halves (`WriteConnection::send_reply`) rather than
    /// `Connection::send_reply`, so the `drain_held_fds` workaround is never invoked. This
    /// proves the workaround is not needed for cross-connection FD passing.
    #[tokio::test]
    async fn fd_passing_across_separate_connections() {
        let (std_a, std_b) = StdUnixStream::pair().unwrap();
        std_a.set_nonblocking(true).unwrap();
        std_b.set_nonblocking(true).unwrap();

        let conn_a = Connection::new(UnixStream::from_std(std_a).unwrap().try_into().unwrap());
        let conn_b = Connection::new(UnixStream::from_std(std_b).unwrap().try_into().unwrap());

        let (_, mut write_a) = conn_a.split();
        let (mut read_b, _) = conn_b.split();

        // Send 3 FDs one at a time, closing the sender's copy after each sendmsg.
        for name in ["alpha", "beta", "gamma"] {
            let (r, mut w) = StdUnixStream::pair().unwrap();
            w.write_all(name.as_bytes()).unwrap();
            drop(w);

            let reply = crate::Reply::new(Some(name.to_string())).set_continues(Some(false));
            write_a.send_reply(&reply, vec![r.into()]).await.unwrap();
        }

        // Receive and verify each FD has the correct data.
        for name in ["alpha", "beta", "gamma"] {
            let (reply, fds) = read_b.receive_reply::<String, ()>().await.unwrap();
            let params = reply.unwrap().into_parameters().unwrap();
            assert_eq!(params, name);
            assert_eq!(fds.len(), 1);

            let recv_fd = fds.into_iter().next().unwrap();
            let mut stream = unsafe { StdUnixStream::from_raw_fd(recv_fd.into_raw_fd()) };
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut stream, &mut buf).unwrap();
            assert_eq!(buf, name, "FD data mismatch for {name:?}");
        }
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
