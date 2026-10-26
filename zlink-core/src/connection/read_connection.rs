//! Contains connection related API.

#[cfg(feature = "std")]
use core::cmp::Ordering;
use core::{fmt::Debug, str::from_utf8};

#[cfg(all(feature = "std", target_os = "linux"))]
use crate::connection::PassedCredentials;
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
    buffer: Vec<u8>,
    id: usize,
    // File descriptors received for messages in `buffer` that haven't been returned yet.
    #[cfg(feature = "std")]
    pending_fds: VecDeque<PendingFds>,
    // Number of `recvmsg` calls that returned FDs. Used by `Connection` to drain
    // `WriteConnection::held_fds` on macOS. See that field's comment for details.
    #[cfg(all(feature = "std", target_os = "macos"))]
    pub(super) fd_recvs: usize,
    #[cfg(all(feature = "std", target_os = "linux"))]
    received_credentials: Option<std::sync::Arc<PassedCredentials>>,
}

impl<Read: ReadHalf> ReadConnection<Read> {
    /// Create a new connection.
    pub(super) fn new(socket: Read, id: usize) -> Self {
        Self {
            socket,
            read_pos: 0,
            msg_pos: 0,
            id,
            buffer: alloc::vec![0; BUFFER_SIZE],
            #[cfg(feature = "std")]
            pending_fds: VecDeque::new(),
            #[cfg(all(feature = "std", target_os = "macos"))]
            fd_recvs: 0,
            #[cfg(all(feature = "std", target_os = "linux"))]
            received_credentials: None,
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

        #[cfg(feature = "std")]
        let fds = self.take_fds();

        let mut stream = Deserializer::from_slice(&self.buffer[self.msg_pos..]).into_iter::<M>();
        let msg = stream.next();
        let null_index = self.msg_pos + stream.byte_offset();
        let buffer = &self.buffer[self.msg_pos..null_index];
        if self.buffer[null_index + 1] == b'\0' {
            // This means we're reading the last message and can now reset the indices.
            self.read_pos = 0;
            self.msg_pos = 0;
            // Nothing is left for the remaining file descriptors, if any, to belong to.
            #[cfg(feature = "std")]
            self.pending_fds.clear();
        } else {
            self.msg_pos = null_index + 1;
        }

        match msg {
            Some(Ok(msg)) => {
                // The message isn't necessarily valid UTF-8 even though it was deserialized: the
                // deserializer doesn't check the values that the message type ignores.
                trace!(
                    "connection {}: received a message: {}",
                    self.id,
                    from_utf8(buffer).unwrap_or("<invalid UTF-8>")
                );

                #[cfg(feature = "std")]
                {
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
            let read_start = self.read_pos;
            let result = self.socket.read(&mut self.buffer[self.read_pos..]).await?;
            #[cfg(all(feature = "std", target_os = "linux"))]
            let mut result = result;

            if result.bytes_read() == 0 {
                return Err(crate::Error::UnexpectedEof);
            }
            self.read_pos += result.bytes_read();
            #[cfg(all(feature = "std", target_os = "linux"))]
            if let Some(creds) = result.take_credentials() {
                self.received_credentials = Some(std::sync::Arc::new(creds));
            }
            #[cfg(feature = "std")]
            if !result.fds().is_empty() {
                let msg_pos = self.fds_msg_pos(read_start);
                self.pending_fds.push_back(PendingFds {
                    msg_pos,
                    fds: result.into_fds(),
                });
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

    /// The position of the message, in `buffer`, that the file descriptors received by the latest
    /// read belong to.
    ///
    /// Peers send the file descriptors of a message along with its first byte, in a write that
    /// carries no other message, but the reads don't necessarily match the writes. On Linux, the
    /// read that receives the file descriptors can also return the data of earlier writes but it
    /// ends with the data of the write that carried them. BSD-derived kernels instead begin the
    /// read with that write and can append the data of later writes to it. Hence, the file
    /// descriptors belong to the last message starting in the read on Linux, and to the first one
    /// elsewhere.
    #[cfg(feature = "std")]
    fn fds_msg_pos(&self, read_start: usize) -> usize {
        // Skip the `\0` terminators since the message must have content.
        let read = &self.buffer[read_start..self.read_pos];
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let content_pos = read.iter().rposition(|b| *b != b'\0');
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let content_pos = read.iter().position(|b| *b != b'\0');
        let content_pos = read_start + content_pos.unwrap_or_default();

        // The message starts right after the terminator of the one before it.
        self.buffer[..content_pos]
            .iter()
            .rposition(|b| *b == b'\0')
            .map_or(0, |pos| pos + 1)
    }

    /// Take the file descriptors received for the message at `msg_pos`.
    #[cfg(feature = "std")]
    fn take_fds(&mut self) -> Vec<OwnedFd> {
        let mut fds = Vec::new();
        while let Some(pending) = self.pending_fds.front_mut() {
            match pending.msg_pos.cmp(&self.msg_pos) {
                // Left over from a message that couldn't be read.
                Ordering::Less => (),
                Ordering::Equal if fds.is_empty() => fds = core::mem::take(&mut pending.fds),
                Ordering::Equal => fds.append(&mut pending.fds),
                Ordering::Greater => break,
            }
            self.pending_fds.pop_front();
        }

        fds
    }

    /// The underlying read half of the socket.
    pub fn read_half(&self) -> &Read {
        &self.socket
    }

    /// The credentials passed through over the socket with the latest message (if any).
    #[cfg(all(feature = "std", target_os = "linux"))]
    pub fn received_credentials(&self) -> Option<&std::sync::Arc<PassedCredentials>>
    where
        Read: super::socket::UnixSocket,
    {
        self.received_credentials.as_ref()
    }
}

/// File descriptors received for a message in the buffer.
#[cfg(feature = "std")]
#[derive(Debug)]
struct PendingFds {
    /// Buffer position where the message starts.
    msg_pos: usize,
    /// The file descriptors.
    fds: Vec<OwnedFd>,
}
