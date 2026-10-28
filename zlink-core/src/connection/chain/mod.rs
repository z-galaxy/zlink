//! Chain method calls.

mod reply_stream;
pub use reply_stream::ReplyStream;

use crate::{Call, Connection, Result, connection::Socket};
use core::fmt::Debug;
use serde::{Serialize, de::DeserializeOwned};

/// A chain of method calls that will be sent together.
///
/// Use [`Connection::chain_call`] to create a new chain, extend it with [`Chain::append`] and send
/// the entire chain using [`Chain::send`] or [`Chain::send_ignore_replies`].
///
/// With `std` feature enabled, this supports unlimited calls. Otherwise it is limited by how many
/// calls can fit in our fixed-sized buffer.
///
/// Oneway calls (where `Call::oneway() == Some(true)`) do not expect replies and are handled
/// automatically by the chain.
#[derive(Debug)]
pub struct Chain<'c, S: Socket> {
    pub(super) connection: &'c mut Connection<S>,
    pub(super) call_count: usize,
    pub(super) reply_count: usize,
}

impl<'c, S> Chain<'c, S>
where
    S: Socket,
{
    /// Create a new chain with the first call.
    pub(super) fn new<Method>(
        connection: &'c mut Connection<S>,
        call: &Call<Method>,
        #[cfg(feature = "std")] fds: alloc::vec::Vec<std::os::fd::OwnedFd>,
    ) -> Result<Self>
    where
        Method: Serialize + Debug,
    {
        #[cfg(feature = "std")]
        connection.write.enqueue_call(call, fds)?;
        #[cfg(not(feature = "std"))]
        connection.write.enqueue_call(call)?;

        let reply_count = if call.oneway() { 0 } else { 1 };
        Ok(Chain {
            connection,
            call_count: 1,
            reply_count,
        })
    }

    /// Append another method call to the chain.
    ///
    /// The call will be enqueued but not sent until [`Chain::send`] is called. Note that one way
    /// calls (where `Call::oneway() == Some(true)`) do not receive replies.
    ///
    /// Calls with `more == Some(true)` will stream multiple replies until a reply with
    /// `continues != Some(true)` is received.
    ///
    /// In std mode, the `fds` parameter contains file descriptors to send along with the call.
    pub fn append<Method>(
        mut self,
        call: &Call<Method>,
        #[cfg(feature = "std")] fds: alloc::vec::Vec<std::os::fd::OwnedFd>,
    ) -> Result<Self>
    where
        Method: Serialize + Debug,
    {
        #[cfg(feature = "std")]
        self.connection.write.enqueue_call(call, fds)?;
        #[cfg(not(feature = "std"))]
        self.connection.write.enqueue_call(call)?;

        if !call.oneway() {
            self.reply_count += 1;
        };
        self.call_count += 1;
        Ok(self)
    }

    /// Send all enqueued calls and return a replies stream.
    ///
    /// This will flush all enqueued calls in a single write operation and then return a stream
    /// that allows reading the replies.
    ///
    /// In std mode, each reply includes any file descriptors received.
    pub async fn send<ReplyParams, ReplyError>(
        self,
    ) -> Result<ReplyStream<'c, ReplyParams, ReplyError>>
    where
        ReplyParams: DeserializeOwned + Debug,
        ReplyError: DeserializeOwned + Debug,
    {
        // Flush all enqueued calls.
        self.connection.write.flush().await?;

        Ok(ReplyStream::new(
            self.connection.read_mut(),
            self.reply_count,
        ))
    }

    /// Send all enqueued calls and ignore the replies.
    ///
    /// This will flush all enqueued calls in a single write operation and then read and discard
    /// all replies. Use this when you don't need to process the responses.
    ///
    /// Note: Any file descriptors received with replies will also be discarded.
    pub async fn send_ignore_replies(self) -> Result<()> {
        use futures_util::StreamExt;
        use serde::de::IgnoredAny;

        let replies = self.send::<IgnoredAny, IgnoredAny>().await?;
        futures_util::pin_mut!(replies);

        while let Some(result) = replies.next().await {
            let _ = result?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Call;
    use futures_util::pin_mut;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    struct GetUser {
        id: u32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct User {
        id: u32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct ApiError {
        code: i32,
    }

    // Use consolidated mock socket from test_utils.
    use crate::test_utils::mock_socket::MockSocket;

    #[tokio::test]
    async fn homogeneous_calls() -> crate::Result<()> {
        let responses = [r#"{"parameters":{"id":1}}"#, r#"{"parameters":{"id":2}}"#];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let call1 = Call::new(GetUser { id: 1 });
        let call2 = Call::new(GetUser { id: 2 });

        #[cfg(feature = "std")]
        let replies = conn
            .chain_call::<GetUser>(&call1, vec![])?
            .append(&call2, vec![])?
            .send::<User, ApiError>()
            .await?;
        #[cfg(not(feature = "std"))]
        let replies = conn
            .chain_call::<GetUser>(&call1)?
            .append(&call2)?
            .send::<User, ApiError>()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        #[cfg(feature = "std")]
        {
            let (user1, _fds) = replies.next().await.unwrap()?;
            let user1 = user1.unwrap();
            assert_eq!(user1.parameters().unwrap().id, 1);

            let (user2, _fds) = replies.next().await.unwrap()?;
            let user2 = user2.unwrap();
            assert_eq!(user2.parameters().unwrap().id, 2);
        }
        #[cfg(not(feature = "std"))]
        {
            let user1 = replies.next().await.unwrap()?.unwrap();
            assert_eq!(user1.parameters().unwrap().id, 1);

            let user2 = replies.next().await.unwrap()?.unwrap();
            assert_eq!(user2.parameters().unwrap().id, 2);
        }

        // No more replies should be available.
        let no_reply = replies.next().await;
        assert!(no_reply.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn replies_after_varlink_service_error() -> crate::Result<()> {
        let responses = [
            r#"{"error":"org.varlink.service.MethodNotFound","parameters":{"method":"GetUser"}}"#,
            r#"{"parameters":{"id":2}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let call1 = Call::new(GetUser { id: 1 });
        let call2 = Call::new(GetUser { id: 2 });

        #[cfg(feature = "std")]
        let replies = conn
            .chain_call::<GetUser>(&call1, vec![])?
            .append(&call2, vec![])?
            .send::<User, ApiError>()
            .await?;
        #[cfg(not(feature = "std"))]
        let replies = conn
            .chain_call::<GetUser>(&call1)?
            .append(&call2)?
            .send::<User, ApiError>()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        // The error reply to the first call doesn't end the replies to the others.
        let error = replies.next().await.unwrap().unwrap_err();
        let crate::Error::VarlinkService(error) = error else {
            panic!("unexpected error: {error:?}");
        };
        assert!(matches!(
            error.inner(),
            crate::varlink_service::Error::MethodNotFound { .. }
        ));

        #[cfg(feature = "std")]
        let (user, _fds) = replies.next().await.unwrap()?;
        #[cfg(not(feature = "std"))]
        let user = replies.next().await.unwrap()?;
        assert_eq!(user.unwrap().parameters().unwrap().id, 2);

        assert!(replies.next().await.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn replies_after_unknown_error() -> crate::Result<()> {
        let responses = [
            r#"{"error":"org.example.Unknown"}"#,
            r#"{"parameters":{"id":2}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let call1 = Call::new(GetUser { id: 1 });
        let call2 = Call::new(GetUser { id: 2 });

        #[cfg(feature = "std")]
        let replies = conn
            .chain_call::<GetUser>(&call1, vec![])?
            .append(&call2, vec![])?
            .send::<User, ApiError>()
            .await?;
        #[cfg(not(feature = "std"))]
        let replies = conn
            .chain_call::<GetUser>(&call1)?
            .append(&call2)?
            .send::<User, ApiError>()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        // The unknown error reply to the first call doesn't end the replies to the others.
        let error = replies.next().await.unwrap().unwrap_err();
        let crate::Error::UnknownErrorReply(name) = error else {
            panic!("unexpected error: {error:?}");
        };
        assert_eq!(name, "org.example.Unknown");

        #[cfg(feature = "std")]
        let (user, _fds) = replies.next().await.unwrap()?;
        #[cfg(not(feature = "std"))]
        let user = replies.next().await.unwrap()?;
        assert_eq!(user.unwrap().parameters().unwrap().id, 2);

        assert!(replies.next().await.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn oneway_calls_no_reply() -> crate::Result<()> {
        // Only the first call expects a reply; the second is oneway.
        let responses = [r#"{"parameters":{"id":1}}"#];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let get_user = Call::new(GetUser { id: 1 });
        let oneway_call = Call::new(GetUser { id: 2 }).set_oneway(true);

        #[cfg(feature = "std")]
        let replies = conn
            .chain_call::<GetUser>(&get_user, vec![])?
            .append(&oneway_call, vec![])?
            .send::<User, ApiError>()
            .await?;
        #[cfg(not(feature = "std"))]
        let replies = conn
            .chain_call::<GetUser>(&get_user)?
            .append(&oneway_call)?
            .send::<User, ApiError>()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        #[cfg(feature = "std")]
        {
            let (user, _fds) = replies.next().await.unwrap()?;
            let user = user.unwrap();
            assert_eq!(user.parameters().unwrap().id, 1);
        }
        #[cfg(not(feature = "std"))]
        {
            let user = replies.next().await.unwrap()?.unwrap();
            assert_eq!(user.parameters().unwrap().id, 1);
        }

        // No more replies should be available.
        let no_reply = replies.next().await;
        assert!(no_reply.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn more_calls_with_streaming() -> crate::Result<()> {
        let responses = [
            r#"{"parameters":{"id":1},"continues":true}"#,
            r#"{"parameters":{"id":2},"continues":true}"#,
            r#"{"parameters":{"id":3},"continues":false}"#,
            r#"{"parameters":{"id":4}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let more_call = Call::new(GetUser { id: 1 }).set_more(true);
        let regular_call = Call::new(GetUser { id: 2 });

        #[cfg(feature = "std")]
        let replies = conn
            .chain_call::<GetUser>(&more_call, vec![])?
            .append(&regular_call, vec![])?
            .send::<User, ApiError>()
            .await?;
        #[cfg(not(feature = "std"))]
        let replies = conn
            .chain_call::<GetUser>(&more_call)?
            .append(&regular_call)?
            .send::<User, ApiError>()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        // First call - streaming replies
        #[cfg(feature = "std")]
        {
            let (user1, _fds) = replies.next().await.unwrap()?;
            let user1 = user1.unwrap();
            assert_eq!(user1.parameters().unwrap().id, 1);
            assert_eq!(user1.continues(), Some(true));

            let (user2, _fds) = replies.next().await.unwrap()?;
            let user2 = user2.unwrap();
            assert_eq!(user2.parameters().unwrap().id, 2);
            assert_eq!(user2.continues(), Some(true));

            let (user3, _fds) = replies.next().await.unwrap()?;
            let user3 = user3.unwrap();
            assert_eq!(user3.parameters().unwrap().id, 3);
            assert_eq!(user3.continues(), Some(false));

            // Second call - single reply
            let (user4, _fds) = replies.next().await.unwrap()?;
            let user4 = user4.unwrap();
            assert_eq!(user4.parameters().unwrap().id, 4);
            assert_eq!(user4.continues(), None);
        }
        #[cfg(not(feature = "std"))]
        {
            let user1 = replies.next().await.unwrap()?.unwrap();
            assert_eq!(user1.parameters().unwrap().id, 1);
            assert_eq!(user1.continues(), Some(true));

            let user2 = replies.next().await.unwrap()?.unwrap();
            assert_eq!(user2.parameters().unwrap().id, 2);
            assert_eq!(user2.continues(), Some(true));

            let user3 = replies.next().await.unwrap()?.unwrap();
            assert_eq!(user3.parameters().unwrap().id, 3);
            assert_eq!(user3.continues(), Some(false));

            // Second call - single reply
            let user4 = replies.next().await.unwrap()?.unwrap();
            assert_eq!(user4.parameters().unwrap().id, 4);
            assert_eq!(user4.continues(), None);
        }

        // No more replies should be available.
        let no_reply = replies.next().await;
        assert!(no_reply.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn stream_interface_works() -> crate::Result<()> {
        use futures_util::stream::StreamExt;

        let responses = [
            r#"{"parameters":{"id":1}}"#,
            r#"{"parameters":{"id":2}}"#,
            r#"{"parameters":{"id":3}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let call1 = Call::new(GetUser { id: 1 });
        let call2 = Call::new(GetUser { id: 2 });
        let call3 = Call::new(GetUser { id: 3 });

        #[cfg(feature = "std")]
        let replies = conn
            .chain_call::<GetUser>(&call1, vec![])?
            .append(&call2, vec![])?
            .append(&call3, vec![])?
            .send::<User, ApiError>()
            .await?;
        #[cfg(not(feature = "std"))]
        let replies = conn
            .chain_call::<GetUser>(&call1)?
            .append(&call2)?
            .append(&call3)?
            .send::<User, ApiError>()
            .await?;

        // Use Stream's collect method to gather all results
        pin_mut!(replies);
        let results: Vec<_> = replies.collect().await;
        assert_eq!(results.len(), 3);

        // Verify all results are successful
        #[cfg(feature = "std")]
        for (i, result) in results.into_iter().enumerate() {
            let (reply, _fds) = result?;
            let user = reply.unwrap();
            assert_eq!(user.parameters().unwrap().id, (i + 1) as u32);
        }
        #[cfg(not(feature = "std"))]
        for (i, result) in results.into_iter().enumerate() {
            let user = result?.unwrap();
            assert_eq!(user.parameters().unwrap().id, (i + 1) as u32);
        }

        Ok(())
    }

    #[tokio::test]
    async fn heterogeneous_calls() -> crate::Result<()> {
        // Types for heterogeneous calls test
        #[derive(Debug, Serialize, Deserialize)]
        #[serde(tag = "method")]
        enum HeterogeneousMethods {
            GetUser { id: u32 },
            GetPost { post_id: u32 },
            DeleteUser { user_id: u32 },
        }

        #[derive(Debug, Serialize, Deserialize)]
        #[serde(untagged)]
        enum HeterogeneousResponses {
            Post(Post),
            User(User),
            DeleteResult(DeleteResult),
        }

        #[derive(Debug, Serialize, Deserialize)]
        struct DeleteResult {
            success: bool,
        }

        #[derive(Debug, Serialize, Deserialize)]
        struct Post {
            id: u32,
            title: String,
        }

        #[derive(Debug, Serialize, Deserialize)]
        #[serde(untagged)]
        enum HeterogeneousErrors {
            User(ApiError),
            Post(PostError),
            Delete(DeleteError),
        }

        #[derive(Debug, Serialize, Deserialize)]
        struct DeleteError {
            reason: String,
        }

        #[derive(Debug, Serialize, Deserialize)]
        struct PostError {
            message: String,
        }

        let responses = [
            r#"{"parameters":{"id":1}}"#,
            r#"{"parameters":{"id":123,"title":"Test Post"}}"#,
            r#"{"parameters":{"success":true}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let get_user_call = Call::new(HeterogeneousMethods::GetUser { id: 1 });
        let get_post_call = Call::new(HeterogeneousMethods::GetPost { post_id: 123 });
        let delete_user_call = Call::new(HeterogeneousMethods::DeleteUser { user_id: 456 });

        #[cfg(feature = "std")]
        let replies = conn
            .chain_call::<HeterogeneousMethods>(&get_user_call, vec![])?
            .append(&get_post_call, vec![])?
            .append(&delete_user_call, vec![])?
            .send::<HeterogeneousResponses, HeterogeneousErrors>()
            .await?;
        #[cfg(not(feature = "std"))]
        let replies = conn
            .chain_call::<HeterogeneousMethods>(&get_user_call)?
            .append(&get_post_call)?
            .append(&delete_user_call)?
            .send::<HeterogeneousResponses, HeterogeneousErrors>()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        #[cfg(feature = "std")]
        {
            // First response: User
            let (user_response, _fds) = replies.next().await.unwrap()?;
            let user_response = user_response.unwrap();
            if let HeterogeneousResponses::User(user) = user_response.parameters().unwrap() {
                assert_eq!(user.id, 1);
            } else {
                panic!("Expected User response");
            }

            // Second response: Post
            let (post_response, _fds) = replies.next().await.unwrap()?;
            let post_response = post_response.unwrap();
            if let HeterogeneousResponses::Post(post) = post_response.parameters().unwrap() {
                assert_eq!(post.id, 123);
                assert_eq!(post.title, "Test Post");
            } else {
                panic!("Expected Post response");
            }

            // Third response: DeleteResult
            let (delete_response, _fds) = replies.next().await.unwrap()?;
            let delete_response = delete_response.unwrap();
            if let HeterogeneousResponses::DeleteResult(result) =
                delete_response.parameters().unwrap()
            {
                assert!(result.success);
            } else {
                panic!("Expected DeleteResult response");
            }
        }
        #[cfg(not(feature = "std"))]
        {
            // First response: User
            let user_response = replies.next().await.unwrap()?.unwrap();
            if let HeterogeneousResponses::User(user) = user_response.parameters().unwrap() {
                assert_eq!(user.id, 1);
            } else {
                panic!("Expected User response");
            }

            // Second response: Post
            let post_response = replies.next().await.unwrap()?.unwrap();
            if let HeterogeneousResponses::Post(post) = post_response.parameters().unwrap() {
                assert_eq!(post.id, 123);
                assert_eq!(post.title, "Test Post");
            } else {
                panic!("Expected Post response");
            }

            // Third response: DeleteResult
            let delete_response = replies.next().await.unwrap()?.unwrap();
            if let HeterogeneousResponses::DeleteResult(result) =
                delete_response.parameters().unwrap()
            {
                assert!(result.success);
            } else {
                panic!("Expected DeleteResult response");
            }
        }

        // No more replies should be available.
        let no_reply = replies.next().await;
        assert!(no_reply.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn chain_from_iter() -> crate::Result<()> {
        use futures_util::stream::StreamExt;

        let responses = [
            r#"{"parameters":{"id":1}}"#,
            r#"{"parameters":{"id":2}}"#,
            r#"{"parameters":{"id":3}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let replies = conn
            .chain_from_iter::<GetUser, _, _>((1..=3).map(|id| GetUser { id }))?
            .send::<User, ApiError>()
            .await?;

        pin_mut!(replies);
        let results: Vec<_> = replies.collect().await;
        assert_eq!(results.len(), 3);

        #[cfg(feature = "std")]
        for (i, result) in results.into_iter().enumerate() {
            let (reply, _fds) = result?;
            let user = reply.unwrap();
            assert_eq!(user.parameters().unwrap().id, (i + 1) as u32);
        }
        #[cfg(not(feature = "std"))]
        for (i, result) in results.into_iter().enumerate() {
            let user = result?.unwrap();
            assert_eq!(user.parameters().unwrap().id, (i + 1) as u32);
        }

        Ok(())
    }

    #[tokio::test]
    async fn chain_from_iter_with_calls() -> crate::Result<()> {
        use futures_util::stream::StreamExt;

        let responses = [r#"{"parameters":{"id":1}}"#, r#"{"parameters":{"id":2}}"#];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let calls = vec![Call::new(GetUser { id: 1 }), Call::new(GetUser { id: 2 })];

        let replies = conn
            .chain_from_iter::<GetUser, _, _>(calls)?
            .send::<User, ApiError>()
            .await?;

        pin_mut!(replies);
        let results: Vec<_> = replies.collect().await;
        assert_eq!(results.len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn chain_from_empty_iter_fails() -> crate::Result<()> {
        let socket = MockSocket::with_responses(&[]);
        let mut conn = Connection::new(socket);

        let methods: Vec<GetUser> = vec![];

        let result = conn.chain_from_iter::<GetUser, _, _>(methods);

        assert!(matches!(result, Err(crate::Error::EmptyChain)));
        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "std")]
    async fn chain_from_iter_with_fds() -> crate::Result<()> {
        use crate::{
            connection::socket::{ReadHalf, WriteHalf},
            test_utils::mock_socket::MockWriteHalf,
        };
        use futures_util::stream::StreamExt;
        use rustix::{fd::AsFd, io::write};
        use std::os::unix::net::UnixStream;

        // Create FDs to send with calls.
        let (send1_r, send1_w) = UnixStream::pair().unwrap();
        let (send2_r, send2_w) = UnixStream::pair().unwrap();
        write(send1_w.as_fd(), b"send1").unwrap();
        write(send2_w.as_fd(), b"send2").unwrap();

        let responses = [r#"{"parameters":{"id":1}}"#, r#"{"parameters":{"id":2}}"#];
        let socket = MockSocket::new(&responses, vec![]);
        let (read_half, write_half) = socket.split();

        // Socket wrapper that provides access to the write half after use.
        #[derive(Debug)]
        struct TrackingSocket<R, W> {
            read: R,
            write: W,
        }

        impl<R: ReadHalf, W: WriteHalf> crate::connection::Socket for TrackingSocket<R, W> {
            type ReadHalf = R;
            type WriteHalf = W;

            fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
                (self.read, self.write)
            }
        }

        #[derive(Debug)]
        struct TrackingWriteHalf {
            mock: MockWriteHalf,
        }

        impl WriteHalf for TrackingWriteHalf {
            async fn write(
                &mut self,
                buf: &[u8],
                fds: &[impl AsFd],
                #[cfg(target_os = "linux")] credentials: Option<
                    &crate::connection::PassedCredentials,
                >,
            ) -> crate::Result<()> {
                self.mock
                    .write(
                        buf,
                        fds,
                        #[cfg(target_os = "linux")]
                        credentials,
                    )
                    .await
            }
        }

        let tracking_write = TrackingWriteHalf { mock: write_half };
        let mut conn = Connection::new(TrackingSocket {
            read: read_half,
            write: tracking_write,
        });

        let calls_with_fds: Vec<(GetUser, Vec<std::os::fd::OwnedFd>)> = vec![
            (GetUser { id: 1 }, vec![send1_r.into()]),
            (GetUser { id: 2 }, vec![send2_r.into()]),
        ];

        let replies = conn
            .chain_from_iter_with_fds::<GetUser, _, _>(calls_with_fds)?
            .send::<User, ApiError>()
            .await?;

        // Collect replies to release borrow on conn.
        let reply_results: Vec<_> = {
            pin_mut!(replies);
            replies.collect().await
        };

        // Verify write-side FD association: WriteConnection sends each message with FDs separately.
        let fds_written = conn.write_mut().socket.mock.fds_written();
        assert_eq!(fds_written.len(), 2, "Should have written FDs twice");
        assert_eq!(fds_written[0].len(), 1, "First call should send 1 FD");
        assert_eq!(fds_written[1].len(), 1, "Second call should send 1 FD");

        // Verify the FDs contain the expected data.
        let mut buf = [0u8; 5];
        rustix::io::read(fds_written[0][0].as_fd(), &mut buf).unwrap();
        assert_eq!(&buf, b"send1");
        rustix::io::read(fds_written[1][0].as_fd(), &mut buf).unwrap();
        assert_eq!(&buf, b"send2");

        // Verify replies.
        assert_eq!(reply_results.len(), 2);
        let (reply1, _) = reply_results[0].as_ref().unwrap();
        assert_eq!(reply1.as_ref().unwrap().parameters().unwrap().id, 1);
        let (reply2, _) = reply_results[1].as_ref().unwrap();
        assert_eq!(reply2.as_ref().unwrap().parameters().unwrap().id, 2);

        Ok(())
    }

    #[tokio::test]
    async fn ignore_replies() -> crate::Result<()> {
        let responses = [r#"{"parameters":{"id":1}}"#, r#"{"parameters":{"id":2}}"#];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        let call1 = Call::new(GetUser { id: 1 });
        let call2 = Call::new(GetUser { id: 2 });

        #[cfg(feature = "std")]
        conn.chain_call::<GetUser>(&call1, vec![])?
            .append(&call2, vec![])?
            .send_ignore_replies()
            .await?;
        #[cfg(not(feature = "std"))]
        conn.chain_call::<GetUser>(&call1)?
            .append(&call2)?
            .send_ignore_replies()
            .await?;

        Ok(())
    }
}
