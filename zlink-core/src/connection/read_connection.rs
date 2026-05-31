//! Contains connection related API.

use core::{fmt::Debug, str::from_utf8_unchecked};

use crate::{Result, varlink_service};

use super::{
    BUFFER_SIZE, Call, MAX_BUFFER_SIZE,
    reply::{self, Reply},
    socket::ReadHalf,
};
#[cfg(feature = "std")]
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use serde::Deserialize;
use serde_json::Deserializer;

#[cfg(feature = "std")]
use std::os::fd::OwnedFd;

// Type alias for receive methods - std returns FDs, no_std doesn't
#[cfg(feature = "std")]
type RecvResult<T> = (T, Vec<OwnedFd>);
#[cfg(not(feature = "std"))]
type RecvResult<T> = T;

/// A connection that can only be used for reading.
///
/// # Cancel safety
///
/// All async methods of this type are cancel safe unless explicitly stated otherwise in its
/// documentation.
#[derive(Debug)]
pub struct ReadConnection<Read: ReadHalf> {
    socket: Read,
    read_pos: usize,
    msg_pos: usize,
    /// Byte index one past the terminating `\0` of the most recently returned message.
    ///
    /// Together with `last_read_end`, this is the boundary an `upgrade` handoff uses to recover
    /// leftover raw bytes via [`Self::into_parts`], *after* the normal end-of-burst reset has
    /// already zeroed `msg_pos`/`read_pos`. It is derived purely from positions, so raw leftovers
    /// that begin with `\0` are preserved verbatim.
    last_msg_end: usize,
    /// Value of `read_pos` (end of real received data, excluding the synthesized sentinel) at the
    /// time the most recent message was returned, i.e. before any end-of-burst reset.
    last_read_end: usize,
    buffer: Vec<u8>,
    id: usize,
    #[cfg(feature = "std")]
    pending_fds: VecDeque<Vec<OwnedFd>>,
    // Number of `recvmsg` calls that returned FDs. Used by `Connection` to drain
    // `WriteConnection::held_fds` on macOS. See that field's comment for details.
    #[cfg(all(feature = "std", target_os = "macos"))]
    pub(super) fd_recvs: usize,
}

impl<Read: ReadHalf> ReadConnection<Read> {
    /// Create a new connection.
    pub(super) fn new(socket: Read, id: usize) -> Self {
        Self {
            socket,
            read_pos: 0,
            msg_pos: 0,
            last_msg_end: 0,
            last_read_end: 0,
            id,
            buffer: alloc::vec![0; BUFFER_SIZE],
            #[cfg(feature = "std")]
            pending_fds: VecDeque::new(),
            #[cfg(all(feature = "std", target_os = "macos"))]
            fd_recvs: 0,
        }
    }

    /// The unique identifier of the connection.
    #[inline]
    pub fn id(&self) -> usize {
        self.id
    }

    /// Receives a method call reply.
    ///
    /// The generic parameters needs some explanation:
    ///
    /// * `ReplyParams` is the type of the successful reply. This should be a type that can
    ///   deserialize itself from the `parameters` field of the reply.
    /// * `ReplyError` is the type of the error reply. This should be a type that can deserialize
    ///   itself from the whole reply object itself and must fail when there is no `error` field in
    ///   the object. This can be easily achieved using the `zlink::ReplyError` derive:
    ///
    /// ```rust
    /// use zlink_core::ReplyError;
    ///
    /// #[derive(Debug, ReplyError)]
    /// #[zlink(
    ///     interface = "org.example.ftl",
    ///     // Not needed in the real code because you'll use `ReplyError` through `zlink` crate.
    ///     crate = "zlink_core",
    /// )]
    /// enum MyError {
    ///     Alpha { param1: u32, param2: String },
    ///     Bravo,
    ///     Charlie { param1: String },
    /// }
    /// ```
    ///
    /// Returns the reply and any file descriptors received (std only).
    pub async fn receive_reply<'r, ReplyParams, ReplyError>(
        &'r mut self,
    ) -> Result<RecvResult<reply::Result<ReplyParams, ReplyError>>>
    where
        ReplyParams: Deserialize<'r> + Debug,
        ReplyError: Deserialize<'r> + Debug,
    {
        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum ReplyMsg<'m, ReplyParams, ReplyError> {
            #[serde(borrow)]
            Varlink(varlink_service::Error<'m>),
            Error(ReplyError),
            Reply(Reply<ReplyParams>),
        }

        let recv_result = self
            .read_message::<ReplyMsg<'_, ReplyParams, ReplyError>>()
            .await?;

        #[cfg(feature = "std")]
        let (msg, fds) = recv_result;
        #[cfg(not(feature = "std"))]
        let msg = recv_result;

        let result = match msg {
            // Varlink service interface error need to be returned as the top-level error.
            ReplyMsg::Varlink(e) => Err(crate::Error::VarlinkService(e.into())),
            ReplyMsg::Error(e) => Ok(Err(e)),
            ReplyMsg::Reply(reply) => Ok(Ok(reply)),
        };

        #[cfg(feature = "std")]
        return result.map(|r| (r, fds));
        #[cfg(not(feature = "std"))]
        return result;
    }

    /// Receive a method call over the socket.
    ///
    /// The generic `Method` is the type of the method name and its input parameters. This should be
    /// a type that can deserialize itself from a complete method call message, i-e an object
    /// containing `method` and `parameter` fields. This can be easily achieved using the
    /// `serde::Deserialize` derive (See the code snippet in [`super::WriteConnection::send_call`]
    /// documentation for an example).
    ///
    /// Returns the call and any file descriptors received (std only).
    pub async fn receive_call<'m, Method>(&'m mut self) -> Result<RecvResult<Call<Method>>>
    where
        Method: Deserialize<'m> + Debug,
    {
        self.read_message::<Call<Method>>().await
    }

    // Reads at least one full message from the socket and return a single message bytes.
    async fn read_message<'m, M>(&'m mut self) -> Result<RecvResult<M>>
    where
        M: Deserialize<'m> + Debug,
    {
        self.read_from_socket().await?;

        let mut stream = Deserializer::from_slice(&self.buffer[self.msg_pos..]).into_iter::<M>();
        let msg = stream.next();
        let null_index = self.msg_pos + stream.byte_offset();
        let buffer = &self.buffer[self.msg_pos..null_index];

        // Record where this message ends and how much real data the burst held, *before* the
        // end-of-burst reset below may zero `msg_pos`/`read_pos`. An `upgrade` handoff
        // (`into_parts`) uses these to recover any leftover raw bytes verbatim — including
        // leftovers that begin with `\0` — since the slice is taken by position, not content.
        self.last_msg_end = null_index + 1;
        self.last_read_end = self.read_pos;

        // Varlink framing boundary logic: a `\0` immediately after this message's terminator marks
        // the end of the burst (either consecutive framing nulls from the peer, or the sentinel
        // `read_from_socket` writes at `buffer[read_pos]`). On normal traffic this resets so the
        // next read starts fresh; the recorded fields above let the upgrade path still recover any
        // leftovers afterward.
        if self.buffer[null_index + 1] == b'\0' {
            // This means we're reading the last message and can now reset the indices.
            self.read_pos = 0;
            self.msg_pos = 0;
        } else {
            self.msg_pos = null_index + 1;
        }

        match msg {
            Some(Ok(msg)) => {
                // SAFETY: Since the parsing from JSON already succeeded, we can be sure that the
                // buffer contains a valid UTF-8 string.
                trace!("connection {}: received a message: {}", self.id, unsafe {
                    from_utf8_unchecked(buffer)
                });

                #[cfg(feature = "std")]
                {
                    let fds = self.pending_fds.pop_front().unwrap_or_default();
                    Ok((msg, fds))
                }
                #[cfg(not(feature = "std"))]
                Ok(msg)
            }
            Some(Err(e)) => Err(e.into()),
            None => Err(crate::Error::UnexpectedEof),
        }
    }

    // Reads at least one full message from the socket.
    async fn read_from_socket(&mut self) -> Result<()> {
        if self.msg_pos > 0 {
            // This means we already have at least one message in the buffer so no need to read.
            return Ok(());
        }

        loop {
            #[cfg(feature = "std")]
            let (bytes_read, fds) = self.socket.read(&mut self.buffer[self.read_pos..]).await?;
            #[cfg(not(feature = "std"))]
            let bytes_read = self.socket.read(&mut self.buffer[self.read_pos..]).await?;

            if bytes_read == 0 {
                return Err(crate::Error::UnexpectedEof);
            }
            self.read_pos += bytes_read;
            #[cfg(feature = "std")]
            if !fds.is_empty() {
                self.pending_fds.push_back(fds);
                // Track receipt so `Connection` can drain `WriteConnection::held_fds`.
                #[cfg(target_os = "macos")]
                {
                    self.fd_recvs += 1;
                }
            }

            if self.read_pos == self.buffer.len() {
                if self.read_pos >= MAX_BUFFER_SIZE {
                    return Err(crate::Error::BufferOverflow);
                }

                self.buffer.extend(core::iter::repeat_n(0, BUFFER_SIZE));
            }

            // This marks end of all messages. After this loop is finished, we'll have 2 consecutive
            // null bytes at the end. This is then used by the callers to determine that they've
            // read all messages and can now reset the `read_pos`.
            self.buffer[self.read_pos] = b'\0';

            if self.buffer[self.read_pos - 1] == b'\0' {
                // One or more full messages were read.
                break;
            }
        }

        Ok(())
    }

    /// The underlying read half of the socket.
    pub fn read_half(&self) -> &Read {
        &self.socket
    }

    /// Consumes the read connection and extracts the raw read socket half, leftover buffered bytes,
    /// and received FDs.
    pub(super) fn into_parts(self) -> ReadParts<Read> {
        // Use the boundary recorded by the last `read_message` rather than the live
        // `msg_pos`/`read_pos`, which the normal end-of-burst reset may have already zeroed. This
        // yields exactly the bytes the peer sent after the final Varlink `\0`, verbatim (leading
        // `\0`s of a raw frame intact). If no message was ever read, both are `0` and the buffer
        // is empty.
        debug_assert!(self.last_msg_end <= self.last_read_end);
        debug_assert!(self.last_read_end <= self.buffer.len());
        let read_buffer = self.buffer[self.last_msg_end..self.last_read_end].to_vec();
        ReadParts {
            socket: self.socket,
            read_buffer,
            #[cfg(feature = "std")]
            pending_fds: self.pending_fds,
        }
    }
}

pub(super) struct ReadParts<Read> {
    pub(super) socket: Read,
    pub(super) read_buffer: Vec<u8>,
    #[cfg(feature = "std")]
    pub(super) pending_fds: VecDeque<Vec<OwnedFd>>,
}
