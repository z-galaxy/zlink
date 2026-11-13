//! Provides transport over any byte stream.

zlink_core::impl_byte_stream! {
    io: futures_util::io,
    split: futures_util::io::AsyncReadExt::split,
    join: |read, write| read.reunite(write).ok(),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Call, Reply};
    use serde::{Deserialize, Serialize};
    use std::{
        pin::Pin,
        task::{Context, Poll, ready},
    };
    use zruntime::{SharedRuntime, net::unix::UnixStream};

    /// A call and its reply go over a stream that carries nothing but bytes.
    #[test]
    fn call_and_reply_over_a_byte_stream() {
        crate::block_on(async {
            let (client, server) = pair();
            let mut client = Connection::new(Stream::new(client));
            let mut server = Connection::new(Stream::new(server));

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
        });
    }

    /// The peer going away is the end of the stream, like on a socket. The stream converts into
    /// the socket here.
    #[test]
    fn peer_hangup_is_eof() {
        crate::block_on(async {
            let (client, server) = pair();
            let mut client = Connection::new(client.into());
            drop(server);

            let err = client.receive_reply::<Echoed, ()>().await.unwrap_err();
            assert!(matches!(err, crate::Error::UnexpectedEof), "{err:?}");
        });
    }

    /// The halves of a stream are joined back into it.
    #[test]
    fn halves_join_back_into_the_stream() {
        let (client, _server) = pair();
        let (read, write) = Stream::new(client).split();

        assert!(Stream::join(read, write).is_ok());
    }

    /// The halves of two different streams aren't joined.
    #[test]
    fn halves_of_two_streams_are_not_joined() {
        let (read, _) = Stream::new(pair().0).split();
        let (_, write) = Stream::new(pair().0).split();

        assert!(Stream::join(read, write).is_err());
    }

    /// A call reaches the peer over a stream that sends only when it's flushed.
    #[test]
    fn call_reaches_the_peer_of_a_buffering_stream() {
        crate::block_on(async {
            let (client, server) = pair();
            let mut client = Connection::new(Stream::new(Buffering {
                stream: client,
                held: Vec::new(),
            }));
            let mut server = Connection::new(Stream::new(server));

            let call = Call::new(Method::Echo {
                text: "hello".into(),
            });
            client.send_call(&call, vec![]).await.unwrap();

            let (received, _) = server.receive_call::<Method>().await.unwrap();
            assert_eq!(received.method(), call.method());
        });
    }

    /// File descriptors are refused, as a byte stream can't carry them.
    #[test]
    fn file_descriptors_are_refused() {
        crate::block_on(async {
            let (client, _server) = pair();
            let mut client = Connection::new(Stream::new(client));

            let call = Call::new(Method::Echo {
                text: "hello".into(),
            });
            let fd = std::fs::File::open("/dev/null").unwrap().into();
            let err = client.send_call(&call, vec![fd]).await.unwrap_err();
            assert!(
                matches!(&err, crate::Error::Io(e) if e.kind() == std::io::ErrorKind::Unsupported),
                "{err:?}"
            );
        });
    }

    /// A stream that sends what is written to it only when it's flushed, as a TLS stream does.
    struct Buffering {
        stream: UnixStream<zruntime::Shared>,
        held: Vec<u8>,
    }

    impl AsyncRead for Buffering {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
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

        fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_close(cx)
        }
    }

    fn pair() -> (UnixStream<zruntime::Shared>, UnixStream<zruntime::Shared>) {
        UnixStream::pair(&SharedRuntime::current().unwrap()).unwrap()
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
