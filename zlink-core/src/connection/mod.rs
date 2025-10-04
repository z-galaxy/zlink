//! Contains connection related API.

mod read_connection;
pub use read_connection::ReadConnection;
pub mod chain;
pub mod socket;
mod write_connection;
use crate::{
    reply::{self, Reply},
    Call, Result,
};
pub use chain::Chain;
use core::{fmt::Debug, sync::atomic::AtomicUsize};
pub use write_connection::WriteConnection;

use serde::{Deserialize, Serialize};
pub use socket::Socket;

/// A connection.
///
/// The low-level API to send and receive messages.
///
/// Each connection gets a unique identifier when created that can be queried using
/// [`Connection::id`]. This ID is shared betwen the read and write halves of the connection. It
/// can be used to associate the read and write halves of the same connection.
///
/// # Cancel safety
///
/// All async methods of this type are cancel safe unless explicitly stated otherwise in its
/// documentation.
#[derive(Debug)]
pub struct Connection<S: Socket> {
    read: ReadConnection<S::ReadHalf>,
    write: WriteConnection<S::WriteHalf>,
}

impl<S> Connection<S>
where
    S: Socket,
{
    /// Create a new connection.
    pub fn new(socket: S) -> Self {
        let (read, write) = socket.split();
        let id = NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        Self {
            read: ReadConnection::new(read, id),
            write: WriteConnection::new(write, id),
        }
    }

    /// The reference to the read half of the connection.
    pub fn read(&self) -> &ReadConnection<S::ReadHalf> {
        &self.read
    }

    /// The mutable reference to the read half of the connection.
    pub fn read_mut(&mut self) -> &mut ReadConnection<S::ReadHalf> {
        &mut self.read
    }

    /// The reference to the write half of the connection.
    pub fn write(&self) -> &WriteConnection<S::WriteHalf> {
        &self.write
    }

    /// The mutable reference to the write half of the connection.
    pub fn write_mut(&mut self) -> &mut WriteConnection<S::WriteHalf> {
        &mut self.write
    }

    /// Split the connection into read and write halves.
    pub fn split(self) -> (ReadConnection<S::ReadHalf>, WriteConnection<S::WriteHalf>) {
        (self.read, self.write)
    }

    /// Join the read and write halves into a connection (the opposite of [`Connection::split`]).
    pub fn join(read: ReadConnection<S::ReadHalf>, write: WriteConnection<S::WriteHalf>) -> Self {
        Self { read, write }
    }

    /// The unique identifier of the connection.
    pub fn id(&self) -> usize {
        assert_eq!(self.read.id(), self.write.id());
        self.read.id()
    }

    /// Sends a method call.
    ///
    /// Convenience wrapper around [`WriteConnection::send_call`].
    pub async fn send_call<Method>(&mut self, call: &Call<Method>) -> Result<()>
    where
        Method: Serialize + Debug,
    {
        self.write.send_call(call).await
    }

    /// Receives a method call reply.
    ///
    /// Convenience wrapper around [`ReadConnection::receive_reply`].
    pub async fn receive_reply<'r, ReplyParams, ReplyError>(
        &'r mut self,
    ) -> Result<reply::Result<ReplyParams, ReplyError>>
    where
        ReplyParams: Deserialize<'r> + Debug,
        ReplyError: Deserialize<'r> + Debug,
    {
        self.read.receive_reply().await
    }

    /// Call a method and receive a reply.
    ///
    /// This is a convenience method that combines [`Connection::send_call`] and
    /// [`Connection::receive_reply`].
    pub async fn call_method<'r, Method, ReplyParams, ReplyError>(
        &'r mut self,
        call: &Call<Method>,
    ) -> Result<reply::Result<ReplyParams, ReplyError>>
    where
        Method: Serialize + Debug,
        ReplyParams: Deserialize<'r> + Debug,
        ReplyError: Deserialize<'r> + Debug,
    {
        self.send_call(call).await?;
        self.receive_reply().await
    }

    /// Receive a method call over the socket.
    ///
    /// Convenience wrapper around [`ReadConnection::receive_call`].
    pub async fn receive_call<'m, Method>(&'m mut self) -> Result<Call<Method>>
    where
        Method: Deserialize<'m> + Debug,
    {
        self.read.receive_call().await
    }

    /// Send a reply over the socket.
    ///
    /// Convenience wrapper around [`WriteConnection::send_reply`].
    pub async fn send_reply<ReplyParams>(&mut self, reply: &Reply<ReplyParams>) -> Result<()>
    where
        ReplyParams: Serialize + Debug,
    {
        self.write.send_reply(reply).await
    }

    /// Send an error reply over the socket.
    ///
    /// Convenience wrapper around [`WriteConnection::send_error`].
    pub async fn send_error<ReplyError>(&mut self, error: &ReplyError) -> Result<()>
    where
        ReplyError: Serialize + Debug,
    {
        self.write.send_error(error).await
    }

    /// Enqueue a call to the server.
    ///
    /// Convenience wrapper around [`WriteConnection::enqueue_call`].
    pub fn enqueue_call<Method>(&mut self, method: &Call<Method>) -> Result<()>
    where
        Method: Serialize + Debug,
    {
        self.write.enqueue_call(method)
    }

    /// Flush the connection.
    ///
    /// Convenience wrapper around [`WriteConnection::flush`].
    pub async fn flush(&mut self) -> Result<()> {
        self.write.flush().await
    }

    /// Start a chain of method calls.
    ///
    /// This allows batching multiple calls together and sending them in a single write operation.
    ///
    /// # Examples
    ///
    /// ## Basic Usage with Sequential Access
    ///
    /// ```no_run
    /// use zlink_core::{Connection, Call, reply};
    /// use serde::{Serialize, Deserialize};
    /// use serde_prefix_all::prefix_all;
    /// use futures_util::{pin_mut, stream::StreamExt};
    ///
    /// # async fn example() -> zlink_core::Result<()> {
    /// # let mut conn: Connection<zlink_core::connection::socket::impl_for_doc::Socket> = todo!();
    ///
    /// #[prefix_all("org.example.")]
    /// #[derive(Debug, Serialize, Deserialize)]
    /// #[serde(tag = "method", content = "parameters")]
    /// enum Methods {
    ///     GetUser { id: u32 },
    ///     GetProject { id: u32 },
    /// }
    ///
    /// #[derive(Debug, Deserialize)]
    /// struct User { name: String }
    ///
    /// #[derive(Debug, Deserialize)]
    /// struct Project { title: String }
    ///
    /// #[derive(Debug, zlink_core::ReplyError)]
    /// #[zlink(
    ///     interface = "org.example",
    ///     // Not needed in the real code because you'll use `ReplyError` through `zlink` crate.
    ///     crate = "zlink_core",
    /// )]
    /// enum ApiError {
    ///     UserNotFound { code: i32 },
    ///     ProjectNotFound { code: i32 },
    /// }
    ///
    /// let get_user = Call::new(Methods::GetUser { id: 1 });
    /// let get_project = Call::new(Methods::GetProject { id: 2 });
    ///
    /// // Chain calls and send them in a batch
    /// let replies = conn
    ///     .chain_call::<Methods, User, ApiError>(&get_user)?
    ///     .append(&get_project)?
    ///     .send().await?;
    /// pin_mut!(replies);
    ///
    /// // Access replies sequentially - types are now fixed by the chain
    /// let user_reply = replies.next().await.unwrap()?;
    /// let project_reply = replies.next().await.unwrap()?;
    ///
    /// match user_reply {
    ///     Ok(user) => println!("User: {}", user.parameters().unwrap().name),
    ///     Err(error) => println!("User error: {:?}", error),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Arbitrary Number of Calls
    ///
    /// ```no_run
    /// # use zlink_core::{Connection, Call, reply};
    /// # use serde::{Serialize, Deserialize};
    /// # use futures_util::{pin_mut, stream::StreamExt};
    /// # use serde_prefix_all::prefix_all;
    /// # async fn example() -> zlink_core::Result<()> {
    /// # let mut conn: Connection<zlink_core::connection::socket::impl_for_doc::Socket> = todo!();
    /// # #[prefix_all("org.example.")]
    /// # #[derive(Debug, Serialize, Deserialize)]
    /// # #[serde(tag = "method", content = "parameters")]
    /// # enum Methods {
    /// #     GetUser { id: u32 },
    /// # }
    /// # #[derive(Debug, Deserialize)]
    /// # struct User { name: String }
    /// # #[derive(Debug, zlink_core::ReplyError)]
    /// #[zlink(
    ///     interface = "org.example",
    ///     // Not needed in the real code because you'll use `ReplyError` through `zlink` crate.
    ///     crate = "zlink_core",
    /// )]
    /// # enum ApiError {
    /// #     UserNotFound { code: i32 },
    /// #     ProjectNotFound { code: i32 },
    /// # }
    /// # let get_user = Call::new(Methods::GetUser { id: 1 });
    ///
    /// // Chain many calls (no upper limit)
    /// let mut chain = conn.chain_call::<Methods, User, ApiError>(&get_user)?;
    /// for i in 2..100 {
    ///     chain = chain.append(&Call::new(Methods::GetUser { id: i }))?;
    /// }
    ///
    /// let replies = chain.send().await?;
    /// pin_mut!(replies);
    ///
    /// // Process all replies sequentially - types are fixed by the chain
    /// while let Some(user_reply) = replies.next().await {
    ///     let user_reply = user_reply?;
    ///     // Handle each reply...
    ///     match user_reply {
    ///         Ok(user) => println!("User: {}", user.parameters().unwrap().name),
    ///         Err(error) => println!("Error: {:?}", error),
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Performance Benefits
    ///
    /// Instead of multiple write operations, the chain sends all calls in a single
    /// write operation, reducing context switching and therefore minimizing latency.
    pub fn chain_call<'c, Method, ReplyParams, ReplyError>(
        &'c mut self,
        call: &Call<Method>,
    ) -> Result<Chain<'c, S, ReplyParams, ReplyError>>
    where
        Method: Serialize + Debug,
        ReplyParams: Deserialize<'c> + Debug,
        ReplyError: Deserialize<'c> + Debug,
    {
        Chain::new(self, call)
    }
}

impl<S> From<S> for Connection<S>
where
    S: Socket,
{
    fn from(socket: S) -> Self {
        Self::new(socket)
    }
}

pub(crate) const BUFFER_SIZE: usize = 1024;
const MAX_BUFFER_SIZE: usize = 100 * 1024 * 1024; // Don't allow buffers over 100MB.

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
