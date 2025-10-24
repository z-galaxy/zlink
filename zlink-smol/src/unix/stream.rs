use crate::{
    connection::socket::{self, Socket},
    Result,
};
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::unix::UnixStream,
};
use std::io;

/// The connection type that uses Unix Domain Sockets for transport.
pub type Connection = crate::Connection<Stream>;

/// Connect to Unix Domain Socket at the given path.
pub async fn connect<P>(path: P) -> Result<Connection>
where
    P: AsRef<std::path::Path>,
{
    UnixStream::connect(path)
        .await
        .map(Stream)
        .map(Connection::new)
        .map_err(Into::into)
}

/// The [`Socket`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct Stream(UnixStream);

impl Socket for Stream {
    type ReadHalf = ReadHalf;
    type WriteHalf = WriteHalf;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        let (read, write) = self.0.split();

        (ReadHalf(read), WriteHalf(write))
    }
}

impl From<UnixStream> for Stream {
    fn from(stream: UnixStream) -> Self {
        Self(stream)
    }
}

/// The [`ReadHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct ReadHalf(smol::io::ReadHalf<UnixStream>);

impl socket::ReadHalf for ReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.0.read(buf).await.map_err(Into::into)
    }
}

/// The [`WriteHalf`] implementation using Unix Domain Sockets.
#[derive(Debug)]
pub struct WriteHalf(smol::io::WriteHalf<UnixStream>);

impl socket::WriteHalf for WriteHalf {
    async fn write(&mut self, buf: &[u8]) -> Result<()> {
        let mut pos = 0;

        while pos < buf.len() {
            let n = self.0.write(&buf[pos..]).await?;
            pos += n;
        }

        Ok(())
    }
}
