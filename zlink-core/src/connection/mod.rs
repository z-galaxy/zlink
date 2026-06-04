//! Contains connection related API.
//!
//! The [`Connection`] type provides a low-level API for sending and receiving Varlink messages.
//! For most use cases, you'll want to use the higher-level [`proxy`] and [`service`] attribute
//! macros instead, which generate type-safe client and server code respectively.
//!
//! # Client Usage with `proxy` Macro
//!
//! The [`proxy`] macro generates methods on `Connection<S>` for calling remote service methods:
//!
//! ```
//! #[zlink_core::proxy(
//!     interface = "org.example.Calculator",
//!     // Not needed in the real code because you'll use `proxy` through `zlink` crate.
//!     crate = "zlink_core",
//! )]
//! trait CalculatorProxy {
//!     async fn add(&mut self, a: f64, b: f64) -> zlink_core::Result<Result<f64, CalcError>>;
//! }
//!
//! #[derive(Debug, zlink_core::ReplyError)]
//! #[zlink(
//!     interface = "org.example.Calculator",
//!     // Not needed in the real code because you'll use `ReplyError` through `zlink` crate.
//!     crate = "zlink_core",
//! )]
//! enum CalcError {}
//! ```
//!
//! # Server Usage with `service` Macro
//!
//! The [`service`] macro generates the [`Service`] trait implementation. See the [`service`] macro
//! documentation for details and examples.
//!
//! # Low-Level API
//!
//! For advanced use cases that require more control, the [`Connection`] type provides direct access
//! to message sending and receiving via methods like [`Connection::send_call`],
//! [`Connection::receive_reply`], and [`Connection::chain_call`] for pipelining.
//!
//! # File descriptor passing
//!
//! On transports that support it (Unix domain sockets, `std` only), zlink can pass file
//! descriptors alongside messages. FD passing is not part of the Varlink protocol itself; it is
//! an `AF_UNIX` transport feature (`SCM_RIGHTS` ancillary data). A transport advertises the
//! capability via [`Socket::CAN_TRANSFER_FDS`] (`true` for Unix domain sockets). It is unavailable
//! under `no_std`.
//!
//! Because the Varlink spec does not define FD passing, there is no formal standard for it.
//! systemd's `sd-varlink` has implemented it over `AF_UNIX` since 2023 and ships it across many
//! system services, making its design the de-facto convention for "FD passing over Varlink".
//! zlink deliberately follows that convention so the two interoperate (verified against a live
//! systemd service). Its rules:
//!
//! * **First-byte association.** FDs belong to the message on whose **first byte** they arrive.
//!   zlink attaches a message's FDs to the write carrying its first byte, and on receive hands each
//!   batch of received FDs to the next message in arrival order.
//! * **At most 253 FDs per message** — the Linux `SCM_RIGHTS` kernel limit, which `sd-varlink` also
//!   enforces (along with a cap of 16384 incoming FDs per connection).
//! * **`SO_PASSRIGHTS` opt-in (Linux >= 6.16).** Recent `sd-varlink` only *receives* FDs if it has
//!   opted in; a peer that has not may have FDs it is sent silently dropped by the kernel.
//! * **No in-band negotiation.** Varlink has no handshake for FD passing; both peers must opt in
//!   out of band (zlink by using an FD-capable [`Socket`]).
//!
//! [`proxy`]: macro@crate::proxy
//! [`service`]: macro@crate::service
//! [`Service`]: crate::service::Service
//! [`Socket`]: socket::Socket
//! [`Socket::CAN_TRANSFER_FDS`]: socket::Socket::CAN_TRANSFER_FDS

#[cfg(feature = "std")]
mod credentials;
mod read_connection;
#[cfg(feature = "std")]
pub use credentials::Credentials;
#[cfg(feature = "std")]
pub use credentials::PassedCredentials;
pub use read_connection::ReadConnection;
#[cfg(feature = "std")]
pub use rustix::{process::Gid, process::Pid, process::Uid};
pub mod chain;
pub mod socket;
#[cfg(test)]
mod tests;
mod write_connection;
use crate::{
    Call, Result,
    reply::{self, Reply},
};
#[cfg(feature = "std")]
use alloc::vec;
pub use chain::Chain;
use core::{fmt::Debug, sync::atomic::AtomicUsize};
#[cfg(feature = "std")]
use socket::FetchPeerCredentials;
pub use write_connection::WriteConnection;

use serde::{Deserialize, Serialize};
pub use socket::Socket;

// Type alias for receive methods - std returns FDs, no_std doesn't
#[cfg(feature = "std")]
type RecvResult<T> = (T, Vec<std::os::fd::OwnedFd>);
#[cfg(not(feature = "std"))]
type RecvResult<T> = T;

/// A connection.
///
/// The low-level API to send and receive messages.
///
/// Each connection gets a unique identifier when created that can be queried using
/// [`Connection::id`]. This ID is shared between the read and write halves of the connection. It
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
    #[cfg(feature = "std")]
    credentials: Option<std::sync::Arc<Credentials>>,
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
            #[cfg(feature = "std")]
            credentials: None,
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
    ///
    /// Note: This consumes any cached credentials. If you need the credentials after splitting,
    /// call [`Connection::peer_credentials`] before splitting.
    pub fn split(self) -> (ReadConnection<S::ReadHalf>, WriteConnection<S::WriteHalf>) {
        (self.read, self.write)
    }

    /// Join the read and write halves into a connection (the opposite of [`Connection::split`]).
    pub fn join(read: ReadConnection<S::ReadHalf>, write: WriteConnection<S::WriteHalf>) -> Self {
        Self {
            read,
            write,
            #[cfg(feature = "std")]
            credentials: None,
        }
    }

    /// Release sent FDs that the read half has confirmed receiving via `recvmsg`.
    /// See `WriteConnection::held_fds` for details on the macOS kernel issue.
    #[cfg(all(feature = "std", target_os = "macos"))]
    fn drain_held_fds(&mut self) {
        let to_drain = self.read.fd_recvs;
        for _ in 0..to_drain {
            self.write.held_fds.pop_front();
        }
        self.read.fd_recvs -= to_drain;
    }

    /// The unique identifier of the connection.
    pub fn id(&self) -> usize {
        assert_eq!(self.read.id(), self.write.id());
        self.read.id()
    }

    /// Sends a method call.
    ///
    /// Convenience wrapper around [`WriteConnection::send_call`].
    pub async fn send_call<Method>(
        &mut self,
        call: &Call<Method>,
        #[cfg(feature = "std")] fds: Vec<std::os::fd::OwnedFd>,
    ) -> Result<()>
    where
        Method: Serialize + Debug,
    {
        #[cfg(feature = "std")]
        {
            self.write.send_call(call, fds).await
        }
        #[cfg(not(feature = "std"))]
        {
            self.write.send_call(call).await
        }
    }

    /// Receives a method call reply.
    ///
    /// Convenience wrapper around [`ReadConnection::receive_reply`].
    pub async fn receive_reply<'r, ReplyParams, ReplyError>(
        &'r mut self,
    ) -> Result<RecvResult<reply::Result<ReplyParams, ReplyError>>>
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
        #[cfg(feature = "std")] fds: Vec<std::os::fd::OwnedFd>,
    ) -> Result<RecvResult<reply::Result<ReplyParams, ReplyError>>>
    where
        Method: Serialize + Debug,
        ReplyParams: Deserialize<'r> + Debug,
        ReplyError: Deserialize<'r> + Debug,
    {
        #[cfg(feature = "std")]
        self.send_call(call, fds).await?;
        #[cfg(not(feature = "std"))]
        self.send_call(call).await?;

        self.receive_reply().await
    }

    /// Receive a method call over the socket.
    ///
    /// Convenience wrapper around [`ReadConnection::receive_call`].
    pub async fn receive_call<'m, Method>(&'m mut self) -> Result<RecvResult<Call<Method>>>
    where
        Method: Deserialize<'m> + Debug,
    {
        self.read.receive_call().await
    }

    /// Send a reply over the socket.
    ///
    /// Convenience wrapper around [`WriteConnection::send_reply`].
    pub async fn send_reply<ReplyParams>(
        &mut self,
        reply: &Reply<ReplyParams>,
        #[cfg(feature = "std")] fds: Vec<std::os::fd::OwnedFd>,
    ) -> Result<()>
    where
        ReplyParams: Serialize + Debug,
    {
        #[cfg(all(feature = "std", target_os = "macos"))]
        self.drain_held_fds();
        #[cfg(feature = "std")]
        {
            self.write.send_reply(reply, fds).await
        }
        #[cfg(not(feature = "std"))]
        {
            self.write.send_reply(reply).await
        }
    }

    /// Send an error reply over the socket.
    ///
    /// Convenience wrapper around [`WriteConnection::send_error`].
    pub async fn send_error<ReplyError>(
        &mut self,
        error: &ReplyError,
        #[cfg(feature = "std")] fds: Vec<std::os::fd::OwnedFd>,
    ) -> Result<()>
    where
        ReplyError: Serialize + Debug,
    {
        #[cfg(all(feature = "std", target_os = "macos"))]
        self.drain_held_fds();
        #[cfg(feature = "std")]
        {
            self.write.send_error(error, fds).await
        }
        #[cfg(not(feature = "std"))]
        {
            self.write.send_error(error).await
        }
    }

    /// Enqueue a call to the server.
    ///
    /// Convenience wrapper around [`WriteConnection::enqueue_call`].
    pub fn enqueue_call<Method>(&mut self, method: &Call<Method>) -> Result<()>
    where
        Method: Serialize + Debug,
    {
        #[cfg(feature = "std")]
        {
            self.write.enqueue_call(method, vec![])
        }
        #[cfg(not(feature = "std"))]
        {
            self.write.enqueue_call(method)
        }
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
    /// # #[cfg(feature = "std")]
    /// let replies = conn
    ///     .chain_call::<Methods>(&get_user, vec![])?
    ///     .append(&get_project, vec![])?
    ///     .send::<User, ApiError>().await?;
    /// # #[cfg(not(feature = "std"))]
    /// # let replies = conn
    /// #     .chain_call::<Methods>(&get_user)?
    /// #     .append(&get_project)?
    /// #     .send::<User, ApiError>().await?;
    /// pin_mut!(replies);
    ///
    /// // Access replies sequentially.
    /// # #[cfg(feature = "std")]
    /// # {
    /// let (user_reply, _fds) = replies.next().await.unwrap()?;
    /// let (project_reply, _fds) = replies.next().await.unwrap()?;
    ///
    /// match user_reply {
    ///     Ok(user) => println!("User: {}", user.parameters().unwrap().name),
    ///     Err(error) => println!("User error: {:?}", error),
    /// }
    /// # }
    /// # #[cfg(not(feature = "std"))]
    /// # {
    /// # let user_reply = replies.next().await.unwrap()?;
    /// # let _project_reply = replies.next().await.unwrap()?;
    /// #
    /// # match user_reply {
    /// #     Ok(user) => println!("User: {}", user.parameters().unwrap().name),
    /// #     Err(error) => println!("User error: {:?}", error),
    /// # }
    /// # }
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
    /// # #[cfg(feature = "std")]
    /// let mut chain = conn.chain_call::<Methods>(&get_user, vec![])?;
    /// # #[cfg(not(feature = "std"))]
    /// # let mut chain = conn.chain_call::<Methods>(&get_user)?;
    /// # #[cfg(feature = "std")]
    /// for i in 2..100 {
    ///     chain = chain.append(&Call::new(Methods::GetUser { id: i }), vec![])?;
    /// }
    /// # #[cfg(not(feature = "std"))]
    /// # for i in 2..100 {
    /// #     chain = chain.append(&Call::new(Methods::GetUser { id: i }))?;
    /// # }
    ///
    /// let replies = chain.send::<User, ApiError>().await?;
    /// pin_mut!(replies);
    ///
    /// // Process all replies sequentially.
    /// # #[cfg(feature = "std")]
    /// while let Some(result) = replies.next().await {
    ///     let (user_reply, _fds) = result?;
    ///     // Handle each reply...
    ///     match user_reply {
    ///         Ok(user) => println!("User: {}", user.parameters().unwrap().name),
    ///         Err(error) => println!("Error: {:?}", error),
    ///     }
    /// }
    /// # #[cfg(not(feature = "std"))]
    /// # while let Some(result) = replies.next().await {
    /// #     let user_reply = result?;
    /// #     // Handle each reply...
    /// #     match user_reply {
    /// #         Ok(user) => println!("User: {}", user.parameters().unwrap().name),
    /// #         Err(error) => println!("Error: {:?}", error),
    /// #     }
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Performance Benefits
    ///
    /// Instead of multiple write operations, the chain sends all calls in a single
    /// write operation, reducing context switching and therefore minimizing latency.
    pub fn chain_call<'c, Method>(
        &'c mut self,
        call: &Call<Method>,
        #[cfg(feature = "std")] fds: alloc::vec::Vec<std::os::fd::OwnedFd>,
    ) -> Result<Chain<'c, S>>
    where
        Method: Serialize + Debug,
    {
        Chain::new(
            self,
            call,
            #[cfg(feature = "std")]
            fds,
        )
    }

    /// Create a chain from an iterator of method calls.
    ///
    /// This allows creating a chain from any iterator yielding method types or calls. Each item
    /// is automatically converted to a [`Call`] via [`Into<Call<Method>>`]. Unlike
    /// [`Connection::chain_call`], this method allows building chains from dynamically-sized
    /// collections.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyChain`] if the iterator is empty.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use zlink_core::Connection;
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
    /// }
    ///
    /// #[derive(Debug, Deserialize)]
    /// struct User { name: String }
    ///
    /// #[derive(Debug, zlink_core::ReplyError)]
    /// #[zlink(interface = "org.example", crate = "zlink_core")]
    /// enum ApiError {
    ///     UserNotFound { code: i32 },
    /// }
    ///
    /// let user_ids = [1, 2, 3, 4, 5];
    /// let replies = conn
    ///     .chain_from_iter::<Methods, _, _>(
    ///         user_ids.iter().map(|&id| Methods::GetUser { id })
    ///     )?
    ///     .send::<User, ApiError>()
    ///     .await?;
    /// pin_mut!(replies);
    ///
    /// # #[cfg(feature = "std")]
    /// while let Some(result) = replies.next().await {
    ///     let (user_reply, _fds) = result?;
    ///     // Handle each reply...
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Error::EmptyChain`]: crate::Error::EmptyChain
    pub fn chain_from_iter<'c, Method, MethodCall, MethodCalls>(
        &'c mut self,
        calls: MethodCalls,
    ) -> Result<Chain<'c, S>>
    where
        Method: Serialize + Debug,
        MethodCall: Into<Call<Method>>,
        MethodCalls: IntoIterator<Item = MethodCall>,
    {
        let mut iter = calls.into_iter();
        let first: Call<Method> = iter.next().ok_or(crate::Error::EmptyChain)?.into();

        #[cfg(feature = "std")]
        let mut chain = Chain::new(self, &first, alloc::vec::Vec::new())?;
        #[cfg(not(feature = "std"))]
        let mut chain = Chain::new(self, &first)?;

        for call in iter {
            let call: Call<Method> = call.into();
            #[cfg(feature = "std")]
            {
                chain = chain.append(&call, alloc::vec::Vec::new())?;
            }
            #[cfg(not(feature = "std"))]
            {
                chain = chain.append(&call)?;
            }
        }

        Ok(chain)
    }

    /// Create a chain from an iterator of method calls with file descriptors.
    ///
    /// Similar to [`Connection::chain_from_iter`], but allows passing file descriptors with each
    /// call. Each item in the iterator is a tuple of a method type (or [`Call`]) and its
    /// associated file descriptors.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyChain`] if the iterator is empty.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use zlink_core::Connection;
    /// use serde::{Serialize, Deserialize};
    /// use serde_prefix_all::prefix_all;
    /// use std::os::fd::OwnedFd;
    ///
    /// # async fn example() -> zlink_core::Result<()> {
    /// # let mut conn: Connection<zlink_core::connection::socket::impl_for_doc::Socket> = todo!();
    ///
    /// #[prefix_all("org.example.")]
    /// #[derive(Debug, Serialize, Deserialize)]
    /// #[serde(tag = "method", content = "parameters")]
    /// enum Methods {
    ///     SendFile { name: String },
    /// }
    ///
    /// #[derive(Debug, Deserialize)]
    /// struct FileResult { success: bool }
    ///
    /// #[derive(Debug, zlink_core::ReplyError)]
    /// #[zlink(interface = "org.example", crate = "zlink_core")]
    /// enum ApiError {
    ///     SendFailed { reason: String },
    /// }
    ///
    /// let calls_with_fds: Vec<(Methods, Vec<OwnedFd>)> = vec![
    ///     (Methods::SendFile { name: "file1.txt".into() }, vec![/* fd1 */]),
    ///     (Methods::SendFile { name: "file2.txt".into() }, vec![/* fd2 */]),
    /// ];
    ///
    /// let replies = conn
    ///     .chain_from_iter_with_fds::<Methods, _, _>(calls_with_fds)?
    ///     .send::<FileResult, ApiError>()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Error::EmptyChain`]: crate::Error::EmptyChain
    #[cfg(feature = "std")]
    pub fn chain_from_iter_with_fds<'c, Method, MethodCall, MethodCalls>(
        &'c mut self,
        calls: MethodCalls,
    ) -> Result<Chain<'c, S>>
    where
        Method: Serialize + Debug,
        MethodCall: Into<Call<Method>>,
        MethodCalls: IntoIterator<Item = (MethodCall, alloc::vec::Vec<std::os::fd::OwnedFd>)>,
    {
        let mut iter = calls.into_iter();
        let (first, first_fds) = iter.next().ok_or(crate::Error::EmptyChain)?;
        let first: Call<Method> = first.into();
        let mut chain = Chain::new(self, &first, first_fds)?;

        for (call, fds) in iter {
            let call: Call<Method> = call.into();
            chain = chain.append(&call, fds)?;
        }

        Ok(chain)
    }

    /// Get the peer credentials.
    ///
    /// This method caches the credentials on the first call.
    #[cfg(feature = "std")]
    pub async fn peer_credentials(&mut self) -> std::io::Result<&std::sync::Arc<Credentials>>
    where
        S::ReadHalf: socket::FetchPeerCredentials,
    {
        if self.credentials.is_none() {
            let creds = self.read.read_half().fetch_peer_credentials().await?;
            self.credentials = Some(std::sync::Arc::new(creds));
        }

        // Safety: `unwrap` won't panic because we ensure above that it's set correctly if the
        // method doesn't error out.
        Ok(self.credentials.as_ref().unwrap())
    }

    /// The credentials passed through over the socket with the latest message (if any).
    #[cfg(all(feature = "std", target_os = "linux"))]
    pub fn received_credentials(&self) -> Option<&std::sync::Arc<PassedCredentials>>
    where
        S::ReadHalf: socket::UnixSocket,
    {
        self.read.received_credentials()
    }

    /// Set the credentials to send with all subsequent writes.
    ///
    /// This is only available for `std` feature and `linux` target.
    #[cfg(all(feature = "std", target_os = "linux"))]
    pub fn set_credentials(&mut self, credentials: Option<PassedCredentials>) {
        self.write.set_credentials(credentials);
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

pub(crate) const BUFFER_SIZE: usize = 256;
const MAX_BUFFER_SIZE: usize = 100 * 1024 * 1024; // Don't allow buffers over 100MB.

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
