use crate::{
    connection::socket::{self, Socket},
    Result,
};
use async_io::Async;
use std::{os::unix::net::UnixStream as StdUnixStream, sync::Arc};

/// The connection type that uses Unix Domain Sockets for transport.
pub type Connection = crate::Connection<Stream>;

/// Connect to Unix Domain Socket at the given path.
pub async fn connect<P>(path: P) -> Result<Connection>
where
    P: AsRef<std::path::Path>,
{
    Async::<StdUnixStream>::connect(path)
        .await
        .map(Stream)
        .map(Connection::new)
        .map_err(Into::into)
}

/// The [`Socket`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct Stream(Async<StdUnixStream>);

impl Socket for Stream {
    type ReadHalf = ReadHalf;
    type WriteHalf = WriteHalf;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        let stream = Arc::new(self.0);

        (ReadHalf(Arc::clone(&stream)), WriteHalf(stream))
    }
}

impl From<Async<StdUnixStream>> for Stream {
    fn from(stream: Async<StdUnixStream>) -> Self {
        Self(stream)
    }
}

/// The [`ReadHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct ReadHalf(Arc<Async<StdUnixStream>>);

impl socket::ReadHalf for ReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        use futures_lite::io::AsyncReadExt;

        AsyncReadExt::read(&mut &*self.0, buf)
            .await
            .map_err(Into::into)
    }
}

/// The [`WriteHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct WriteHalf(Arc<Async<StdUnixStream>>);

impl socket::WriteHalf for WriteHalf {
    async fn write(&mut self, buf: &[u8]) -> Result<()> {
        use futures_lite::io::AsyncWriteExt;

        let mut pos = 0;

        while pos < buf.len() {
            let n = AsyncWriteExt::write(&mut &*self.0, &buf[pos..]).await?;
            pos += n;
        }

        Ok(())
    }
}
