use super::{Connection, Socket};
use crate::{Call, Result, reply};
use core::fmt::Debug;
use serde::Serialize;

/// Raw connection halves and any leftover buffered bytes after upgrading.
#[derive(Debug)]
pub struct ConnectionParts<S: Socket> {
    /// The raw read half of the connection socket.
    pub read_half: S::ReadHalf,
    /// The raw write half of the connection socket.
    pub write_half: S::WriteHalf,
    /// The leftover bytes already read from the socket but not yet consumed by the Varlink parser.
    ///
    /// The caller MUST process these bytes before calling `read` on `read_half`, as they are
    /// already taken from the socket's kernel buffer.
    pub read_buffer: alloc::vec::Vec<u8>,
    /// Any received file descriptors that have not yet been consumed (std only).
    #[cfg(feature = "std")]
    pub received_fds: alloc::collections::VecDeque<alloc::vec::Vec<std::os::fd::OwnedFd>>,
}

/// A reply to an upgrade call, containing the deserialized reply and the extracted connection
/// parts.
#[derive(Debug)]
pub struct UpgradeReply<S: Socket, ReplyParams, ReplyError> {
    /// The deserialized Varlink reply.
    pub reply: reply::Result<ReplyParams, ReplyError>,
    /// The raw connection halves and any leftover buffered bytes.
    pub parts: ConnectionParts<S>,
}

impl<S> Connection<S>
where
    S: Socket,
{
    /// Call an upgrade method and return the single Varlink reply along with the raw connection
    /// parts.
    ///
    /// This consumes the connection, and after this call, the Varlink/JSON framing API is gone.
    /// The caller can continue communication using the raw socket halves inside `parts`.
    ///
    /// This method is NOT cancel-safe because it consumes the connection.
    pub async fn call_upgrade<Method, ReplyParams, ReplyError>(
        mut self,
        call: &Call<Method>,
        #[cfg(feature = "std")] fds: alloc::vec::Vec<std::os::fd::OwnedFd>,
    ) -> Result<UpgradeReply<S, ReplyParams, ReplyError>>
    where
        Method: Serialize + Debug,
        ReplyParams: serde::de::DeserializeOwned + Debug,
        ReplyError: serde::de::DeserializeOwned + Debug,
    {
        debug_assert!(call.upgrade());

        #[cfg(feature = "std")]
        self.send_call(call, fds).await?;
        #[cfg(not(feature = "std"))]
        self.send_call(call).await?;

        let recv_result = self.receive_reply::<ReplyParams, ReplyError>().await?;

        #[cfg(feature = "std")]
        let (reply, reply_fds) = recv_result;
        #[cfg(not(feature = "std"))]
        let reply = recv_result;

        #[allow(unused_mut)]
        let mut parts = self.into_parts();

        #[cfg(feature = "std")]
        if !reply_fds.is_empty() {
            parts.received_fds.push_front(reply_fds);
        }

        Ok(UpgradeReply { reply, parts })
    }
}
