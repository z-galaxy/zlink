//! Provides transport over any byte stream.

use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use zlink_core::connection::byte_stream::{self, ByteStream};

/// The connection type that uses a byte stream for transport.
pub type Connection<T> = crate::Connection<Socket<T>>;

/// The [`Socket`](crate::connection::socket::Socket) implementation over any byte stream.
///
/// Varlink needs no more than a reliable, ordered byte stream, so a connection can run over a TCP
/// or TLS stream, or a WebSocket exposed through [`AsyncRead`] and [`AsyncWrite`]. Only Unix
/// Domain Sockets carry file descriptors and peer credentials; for those, use
/// [`unix::Stream`](crate::unix::Stream).
///
/// A stream converts into it, so `Connection::new(stream.into())` makes a connection over the
/// stream.
pub type Socket<T> = byte_stream::Socket<T, TokioByteStream>;

/// The [`ReadHalf`](crate::connection::socket::ReadHalf) implementation over a byte stream.
pub type ReadHalf<T> = byte_stream::ReadHalf<T, TokioByteStream>;

/// The [`WriteHalf`](crate::connection::socket::WriteHalf) implementation over a byte stream.
pub type WriteHalf<T> = byte_stream::WriteHalf<T, TokioByteStream>;

/// Selects the byte stream operations of Tokio for the types above.
#[doc(hidden)]
#[derive(Debug)]
pub struct TokioByteStream;

impl<T> ByteStream<T> for TokioByteStream
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type ReadHalf = tokio::io::ReadHalf<T>;
    type WriteHalf = tokio::io::WriteHalf<T>;

    fn split(stream: T) -> (Self::ReadHalf, Self::WriteHalf) {
        tokio::io::split(stream)
    }

    fn join(read: Self::ReadHalf, write: Self::WriteHalf) -> Option<T> {
        // `unsplit` panics on the halves of two streams.
        read.is_pair_of(&write).then(|| read.unsplit(write))
    }

    // `AsyncReadExt::read` is cancel safe, which the trait requires.
    async fn read(read: &mut Self::ReadHalf, buf: &mut [u8]) -> std::io::Result<usize> {
        read.read(buf).await
    }

    // `AsyncWriteExt::write` is cancel safe, which the trait requires.
    async fn write(write: &mut Self::WriteHalf, buf: &[u8]) -> std::io::Result<usize> {
        write.write(buf).await
    }

    async fn flush(write: &mut Self::WriteHalf) -> std::io::Result<()> {
        write.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Call, Reply, connection::socket::Socket as _};
    use serde::{Deserialize, Serialize};
    use std::{
        pin::Pin,
        task::{Context, Poll, ready},
    };

    /// A call and its reply go over a stream that carries nothing but bytes.
    #[tokio::test]
    async fn call_and_reply_over_a_duplex_stream() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client = Connection::new(Socket::new(client));
        let mut server = Connection::new(Socket::new(server));

        let call = Call::new(Method::Echo {
            text: "hello".into(),
        });
        client.send_call(&call, vec![]).await.unwrap();

        let (received, fds) = server.receive_call::<Method>().await.unwrap();
        assert!(fds.is_empty());
        assert_eq!(received.method(), call.method());
        let reply = Reply::new(Some(Echoed {
            text: "hello".into(),
        }));
        server.send_reply(&reply, vec![]).await.unwrap();

        let (reply, fds) = client.receive_reply::<Echoed, ()>().await.unwrap();
        assert!(fds.is_empty());
        assert_eq!(
            reply.unwrap().into_parameters(),
            Some(Echoed {
                text: "hello".into()
            })
        );
    }

    /// The peer going away is the end of the stream, like on a socket. The stream converts into
    /// the socket here.
    #[tokio::test]
    async fn peer_hangup_is_eof() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client = Connection::new(client.into());
        drop(server);

        let err = client.receive_reply::<Echoed, ()>().await.unwrap_err();
        assert!(matches!(err, crate::Error::UnexpectedEof), "{err:?}");
    }

    /// The halves of a stream are joined back into it.
    #[tokio::test]
    async fn halves_join_back_into_the_stream() {
        let (client, _server) = tokio::io::duplex(1024);
        let (read, write) = Socket::new(client).split();

        assert!(Socket::join(read, write).is_ok());
    }

    /// The halves of two different streams aren't joined.
    #[tokio::test]
    async fn halves_of_two_streams_are_not_joined() {
        let (read, _) = Socket::new(tokio::io::duplex(1024).0).split();
        let (_, write) = Socket::new(tokio::io::duplex(1024).0).split();

        assert!(Socket::join(read, write).is_err());
    }

    /// The halves give the halves of the stream back, to go on without Varlink on them.
    #[tokio::test]
    async fn halves_give_the_halves_of_the_stream_back() {
        let (client, mut server) = tokio::io::duplex(1024);
        let (read, write) = Socket::new(client).split();
        let (mut read, mut write) = (read.into_inner(), write.into_inner());

        write.write_all(b"ping").await.unwrap();
        let mut buf = [0; 4];
        server.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        server.write_all(b"pong").await.unwrap();
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    /// A call reaches the peer over a stream that sends only when it's flushed.
    #[tokio::test]
    async fn call_reaches_the_peer_of_a_buffering_stream() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client = Connection::new(Socket::new(Buffering {
            stream: client,
            held: Vec::new(),
        }));
        let mut server = Connection::new(Socket::new(server));

        let call = Call::new(Method::Echo {
            text: "hello".into(),
        });
        client.send_call(&call, vec![]).await.unwrap();

        let (received, _) = server.receive_call::<Method>().await.unwrap();
        assert_eq!(received.method(), call.method());
    }

    /// File descriptors are refused, as a byte stream can't carry them.
    #[tokio::test]
    async fn file_descriptors_are_refused() {
        let (client, _server) = tokio::io::duplex(1024);
        let mut client = Connection::new(Socket::new(client));

        let call = Call::new(Method::Echo {
            text: "hello".into(),
        });
        let fd = std::fs::File::open("/dev/null").unwrap().into();
        let err = client.send_call(&call, vec![fd]).await.unwrap_err();
        assert!(
            matches!(&err, crate::Error::Io(e) if e.kind() == std::io::ErrorKind::Unsupported),
            "{err:?}"
        );
    }

    /// A stream that sends what is written to it only when it's flushed, as a TLS stream does.
    struct Buffering {
        stream: tokio::io::DuplexStream,
        held: Vec<u8>,
    }

    impl AsyncRead for Buffering {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for Buffering {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.held.extend_from_slice(buf);

            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            let this = &mut *self;
            while !this.held.is_empty() {
                let n = ready!(Pin::new(&mut this.stream).poll_write(cx, &this.held))?;
                this.held.drain(..n);
            }

            Pin::new(&mut this.stream).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_shutdown(cx)
        }
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "method", content = "parameters")]
    enum Method {
        #[serde(rename = "org.example.Echo")]
        Echo { text: String },
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Echoed {
        text: String,
    }
}
