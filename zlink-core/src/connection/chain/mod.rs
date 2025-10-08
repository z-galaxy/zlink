//! Chain method calls.

mod reply_stream;
#[doc(hidden)]
pub use reply_stream::ReplyStream;

use crate::{connection::Socket, reply, Call, Connection, Result};
use core::fmt::Debug;
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};

/// A chain of method calls that will be sent together.
///
/// Each call must have the same method, reply, and error types for homogeneity. Use
/// [`Connection::chain_call`] to create a new chain, extend it with [`Chain::append`] and send the
/// the entire chain using [`Chain::send`].
///
/// With `std` feature enabled, this supports unlimited calls. Otherwise it is limited by how many
/// calls can fit in our fixed-sized buffer.
///
/// Oneway calls (where `Call::oneway() == Some(true)`) do not expect replies and are handled
/// automatically by the chain.
#[derive(Debug)]
pub struct Chain<'c, S: Socket, ReplyParams, ReplyError> {
    pub(super) connection: &'c mut Connection<S>,
    pub(super) call_count: usize,
    pub(super) reply_count: usize,
    _phantom: core::marker::PhantomData<(ReplyParams, ReplyError)>,
}

impl<'c, S, ReplyParams, ReplyError> Chain<'c, S, ReplyParams, ReplyError>
where
    S: Socket,
    ReplyParams: Deserialize<'c> + Debug,
    ReplyError: Deserialize<'c> + Debug,
{
    /// Create a new chain with the first call.
    pub(super) fn new<Method>(
        connection: &'c mut Connection<S>,
        call: &Call<Method>,
    ) -> Result<Self>
    where
        Method: Serialize + Debug,
    {
        connection.write.enqueue_call(call)?;
        let reply_count = if call.oneway() { 0 } else { 1 };
        Ok(Chain {
            connection,
            call_count: 1,
            reply_count,
            _phantom: core::marker::PhantomData,
        })
    }

    /// Append another method call to the chain.
    ///
    /// The call will be enqueued but not sent until [`Chain::send`] is called. Note that one way
    /// calls (where `Call::oneway() == Some(true)`) do not receive replies.
    ///
    /// Calls with `more == Some(true)` will stream multiple replies until a reply with
    /// `continues != Some(true)` is received.
    pub fn append<Method>(mut self, call: &Call<Method>) -> Result<Self>
    where
        Method: Serialize + Debug,
    {
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
    pub async fn send(
        self,
    ) -> Result<impl Stream<Item = Result<reply::Result<ReplyParams, ReplyError>>> + 'c>
    where
        ReplyParams: 'c,
        ReplyError: 'c,
    {
        // Flush all enqueued calls.
        self.connection.write.flush().await?;

        Ok(ReplyStream::new(
            self.connection.read_mut(),
            |conn| conn.receive_reply::<ReplyParams, ReplyError>(),
            self.reply_count,
        ))
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
        let socket = MockSocket::new(&responses);
        let mut conn = Connection::new(socket);

        let call1 = Call::new(GetUser { id: 1 });
        let call2 = Call::new(GetUser { id: 2 });

        let replies = conn
            .chain_call::<GetUser, User, ApiError>(&call1)?
            .append(&call2)?
            .send()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        let user1 = replies.next().await.unwrap()?.unwrap();
        assert_eq!(user1.parameters().unwrap().id, 1);

        let user2 = replies.next().await.unwrap()?.unwrap();
        assert_eq!(user2.parameters().unwrap().id, 2);

        // No more replies should be available.
        let no_reply = replies.next().await;
        assert!(no_reply.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn oneway_calls_no_reply() -> crate::Result<()> {
        // Only the first call expects a reply; the second is oneway.
        let responses = [r#"{"parameters":{"id":1}}"#];
        let socket = MockSocket::new(&responses);
        let mut conn = Connection::new(socket);

        let get_user = Call::new(GetUser { id: 1 });
        let oneway_call = Call::new(GetUser { id: 2 }).set_oneway(true);

        let replies = conn
            .chain_call::<GetUser, User, ApiError>(&get_user)?
            .append(&oneway_call)?
            .send()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        let user = replies.next().await.unwrap()?.unwrap();
        assert_eq!(user.parameters().unwrap().id, 1);

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
        let socket = MockSocket::new(&responses);
        let mut conn = Connection::new(socket);

        let more_call = Call::new(GetUser { id: 1 }).set_more(true);
        let regular_call = Call::new(GetUser { id: 2 });

        let replies = conn
            .chain_call::<GetUser, User, ApiError>(&more_call)?
            .append(&regular_call)?
            .send()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

        // First call - streaming replies
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
        let socket = MockSocket::new(&responses);
        let mut conn = Connection::new(socket);

        let call1 = Call::new(GetUser { id: 1 });
        let call2 = Call::new(GetUser { id: 2 });
        let call3 = Call::new(GetUser { id: 3 });

        let replies = conn
            .chain_call::<GetUser, User, ApiError>(&call1)?
            .append(&call2)?
            .append(&call3)?
            .send()
            .await?;

        // Use Stream's collect method to gather all results
        pin_mut!(replies);
        let results: Vec<_> = replies.collect().await;
        assert_eq!(results.len(), 3);

        // Verify all results are successful
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
            UserError(ApiError),
            PostError(PostError),
            DeleteError(DeleteError),
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
        let socket = MockSocket::new(&responses);
        let mut conn = Connection::new(socket);

        let get_user_call = Call::new(HeterogeneousMethods::GetUser { id: 1 });
        let get_post_call = Call::new(HeterogeneousMethods::GetPost { post_id: 123 });
        let delete_user_call = Call::new(HeterogeneousMethods::DeleteUser { user_id: 456 });

        let replies = conn
            .chain_call::<HeterogeneousMethods, HeterogeneousResponses, HeterogeneousErrors>(
                &get_user_call,
            )?
            .append(&get_post_call)?
            .append(&delete_user_call)?
            .send()
            .await?;

        use futures_util::stream::StreamExt;
        pin_mut!(replies);

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
        if let HeterogeneousResponses::DeleteResult(result) = delete_response.parameters().unwrap()
        {
            assert!(result.success);
        } else {
            panic!("Expected DeleteResult response");
        }

        // No more replies should be available.
        let no_reply = replies.next().await;
        assert!(no_reply.is_none());
        Ok(())
    }
}
