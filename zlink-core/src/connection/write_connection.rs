//! Contains connection related API.

use core::fmt::Debug;

#[cfg(feature = "std")]
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use serde::Serialize;

use super::{BUFFER_SIZE, Call, Reply, socket::WriteHalf};

#[cfg(feature = "std")]
use std::os::fd::OwnedFd;

/// A connection.
///
/// The low-level API to send messages.
///
/// # Cancel safety
///
/// All async methods of this type are cancel safe unless explicitly stated otherwise in its
/// documentation.
#[derive(Debug)]
pub struct WriteConnection<Write: WriteHalf> {
    pub(super) socket: Write,
    pub(super) buffer: Vec<u8>,
    pub(super) pos: usize,
    id: usize,
    #[cfg(feature = "std")]
    pending_fds: VecDeque<MessageFds>,
    // On macOS, SCM_RIGHTS does not properly hold a reference to the underlying file description
    // when the sender closes the FD in the same process before the receiver calls `recvmsg`. The
    // receiver ends up with a stale FD. We work around this by keeping sent FDs alive until the
    // read half confirms it has called `recvmsg` for them via `drain_held_fds`.
    #[cfg(all(feature = "std", target_os = "macos"))]
    pub(super) held_fds: VecDeque<Vec<OwnedFd>>,
}

impl<Write: WriteHalf> WriteConnection<Write> {
    /// Create a new connection.
    pub(super) fn new(socket: Write, id: usize) -> Self {
        Self {
            socket,
            id,
            buffer: alloc::vec![0; BUFFER_SIZE],
            pos: 0,
            #[cfg(feature = "std")]
            pending_fds: VecDeque::new(),
            #[cfg(all(feature = "std", target_os = "macos"))]
            held_fds: VecDeque::new(),
        }
    }

    /// The unique identifier of the connection.
    #[inline]
    pub fn id(&self) -> usize {
        self.id
    }

    /// Sends a method call.
    ///
    /// The generic `Method` is the type of the method name and its input parameters. This should be
    /// a type that can serialize itself to a complete method call message, i-e an object containing
    /// `method` and `parameter` fields. This can be easily achieved using the `serde::Serialize`
    /// derive:
    ///
    /// ```rust
    /// use serde::{Serialize, Deserialize};
    /// use serde_prefix_all::prefix_all;
    ///
    /// #[prefix_all("org.example.ftl.")]
    /// #[derive(Debug, Serialize, Deserialize)]
    /// #[serde(tag = "method", content = "parameters")]
    /// enum MyMethods<'m> {
    ///    // The name needs to be the fully-qualified name of the error.
    ///    Alpha { param1: u32, param2: &'m str},
    ///    Bravo,
    ///    Charlie { param1: &'m str },
    /// }
    /// ```
    ///
    /// The `fds` parameter contains file descriptors to send along with the call.
    pub async fn send_call<Method>(
        &mut self,
        call: &Call<Method>,
        #[cfg(feature = "std")] fds: Vec<OwnedFd>,
    ) -> crate::Result<()>
    where
        Method: Serialize + Debug,
    {
        trace!("connection {}: sending call: {:?}", self.id, call);
        #[cfg(feature = "std")]
        {
            self.write(call, fds).await
        }
        #[cfg(not(feature = "std"))]
        {
            self.write(call).await
        }
    }

    /// Send a reply over the socket.
    ///
    /// The generic parameter `Params` is the type of the successful reply. This should be a type
    /// that can serialize itself as the `parameters` field of the reply.
    ///
    /// The `fds` parameter contains file descriptors to send along with the reply.
    pub async fn send_reply<Params>(
        &mut self,
        reply: &Reply<Params>,
        #[cfg(feature = "std")] fds: Vec<OwnedFd>,
    ) -> crate::Result<()>
    where
        Params: Serialize + Debug,
    {
        trace!("connection {}: sending reply: {:?}", self.id, reply);
        #[cfg(feature = "std")]
        {
            self.write(reply, fds).await
        }
        #[cfg(not(feature = "std"))]
        {
            self.write(reply).await
        }
    }

    /// Send an error reply over the socket.
    ///
    /// The generic parameter `ReplyError` is the type of the error reply. This should be a type
    /// that can serialize itself to the whole reply object, containing `error` and `parameter`
    /// fields. This can be easily achieved using the `serde::Serialize` derive (See the code
    /// snippet in [`super::ReadConnection::receive_reply`] documentation for an example).
    ///
    /// The `fds` parameter contains file descriptors to send along with the error.
    pub async fn send_error<ReplyError>(
        &mut self,
        error: &ReplyError,
        #[cfg(feature = "std")] fds: Vec<OwnedFd>,
    ) -> crate::Result<()>
    where
        ReplyError: Serialize + Debug,
    {
        trace!("connection {}: sending error: {:?}", self.id, error);
        #[cfg(feature = "std")]
        {
            self.write(error, fds).await
        }
        #[cfg(not(feature = "std"))]
        {
            self.write(error).await
        }
    }

    /// Enqueue a call to be sent over the socket.
    ///
    /// Similar to [`WriteConnection::send_call`], except that the call is not sent immediately but
    /// enqueued for later sending. This is useful when you want to send multiple calls in a
    /// batch.
    ///
    /// The `fds` parameter contains file descriptors to send along with the call.
    pub fn enqueue_call<Method>(
        &mut self,
        call: &Call<Method>,
        #[cfg(feature = "std")] fds: Vec<OwnedFd>,
    ) -> crate::Result<()>
    where
        Method: Serialize + Debug,
    {
        trace!("connection {}: enqueuing call: {:?}", self.id, call);
        #[cfg(feature = "std")]
        {
            self.enqueue(call, fds)
        }
        #[cfg(not(feature = "std"))]
        {
            self.enqueue(call)
        }
    }

    /// Send out the enqueued calls.
    pub async fn flush(&mut self) -> crate::Result<()> {
        if self.pos == 0 {
            return Ok(());
        }

        #[allow(unused_mut)]
        let mut sent_pos = 0;

        #[cfg(feature = "std")]
        {
            // While there are FDs, send one message at a time.
            while !self.pending_fds.is_empty() {
                // Get the first FD entry.
                let pending = self.pending_fds.front().unwrap();
                let fd_offset = pending.offset;
                let msg_len = pending.len;

                // If there are bytes before the FD message, send them first without FDs.
                if sent_pos < fd_offset {
                    trace!(
                        "connection {}: flushing {} bytes before FD message",
                        self.id,
                        fd_offset - sent_pos
                    );
                    self.socket
                        .write(&self.buffer[sent_pos..fd_offset], &[] as &[OwnedFd])
                        .await?;
                }

                // Send this message with its FDs.
                let msg_end = fd_offset + msg_len;
                let MessageFds {
                    fds,
                    offset: _,
                    len: _,
                } = self.pending_fds.pop_front().unwrap();
                trace!(
                    "connection {}: flushing {} bytes with {} FDs",
                    self.id,
                    msg_len,
                    fds.len()
                );
                self.socket
                    .write(&self.buffer[fd_offset..msg_end], &fds)
                    .await?;
                sent_pos = msg_end;

                // On macOS, keep sent FDs alive until the read half confirms receipt via
                // `recvmsg`. See the comment on the `held_fds` field for details.
                #[cfg(target_os = "macos")]
                self.held_fds.push_back(fds);
            }
        }

        // No more FDs, send all remaining bytes at once.
        if sent_pos < self.pos {
            trace!(
                "connection {}: flushing {} bytes",
                self.id,
                self.pos - sent_pos
            );
            #[cfg(feature = "std")]
            {
                self.socket
                    .write(&self.buffer[sent_pos..self.pos], &[] as &[OwnedFd])
                    .await?;
            }
            #[cfg(not(feature = "std"))]
            {
                self.socket.write(&self.buffer[sent_pos..self.pos]).await?;
            }
        }

        self.pos = 0;
        Ok(())
    }

    /// The underlying write half of the socket.
    pub fn write_half(&self) -> &Write {
        &self.socket
    }

    pub(super) async fn write<T>(
        &mut self,
        value: &T,
        #[cfg(feature = "std")] fds: Vec<OwnedFd>,
    ) -> crate::Result<()>
    where
        T: Serialize + ?Sized + Debug,
    {
        #[cfg(feature = "std")]
        {
            self.enqueue(value, fds)?;
        }
        #[cfg(not(feature = "std"))]
        {
            self.enqueue(value)?;
        }
        self.flush().await
    }

    pub(super) fn enqueue<T>(
        &mut self,
        value: &T,
        #[cfg(feature = "std")] fds: Vec<OwnedFd>,
    ) -> crate::Result<()>
    where
        T: Serialize + ?Sized + Debug,
    {
        #[cfg(feature = "std")]
        let start_pos = self.pos;

        let len = loop {
            match crate::json_ser::to_slice(value, &mut self.buffer[self.pos..]) {
                Ok(len) => break len,
                Err(crate::json_ser::Error::BufferTooSmall) => {
                    // Buffer too small, grow it and retry
                    self.grow_buffer()?;
                }
                Err(crate::json_ser::Error::KeyMustBeAString) => {
                    // Actual serialization error
                    // Convert to serde_json::Error for public API
                    return Err(crate::Error::Json(serde::ser::Error::custom(
                        "key must be a string",
                    )));
                }
            }
        };

        // Add null terminator after this message.
        if self.pos + len == self.buffer.len() {
            self.grow_buffer()?;
        }
        self.buffer[self.pos + len] = b'\0';
        self.pos += len + 1;

        // Store FDs with message offset and length.
        #[cfg(feature = "std")]
        if !fds.is_empty() {
            self.pending_fds.push_back(MessageFds {
                offset: start_pos,
                len: len + 1, // Include null terminator.
                fds,
            });
        }

        Ok(())
    }

    /// Consumes the write connection and returns the raw write half of the socket.
    pub(super) fn into_inner(self) -> Write {
        self.socket
    }

    fn grow_buffer(&mut self) -> crate::Result<()> {
        if self.buffer.len() >= super::MAX_BUFFER_SIZE {
            return Err(crate::Error::BufferOverflow);
        }

        self.buffer.extend_from_slice(&[0; BUFFER_SIZE]);

        Ok(())
    }
}

/// Information about file descriptors pending to be sent with a message.
#[cfg(feature = "std")]
#[derive(Debug)]
struct MessageFds {
    /// File descriptors to send with this message.
    fds: Vec<OwnedFd>,
    /// Buffer offset where the message starts.
    offset: usize,
    /// Length of the message including the null terminator.
    len: usize,
}
