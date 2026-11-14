//! Contains connection related API.

use core::fmt::Debug;

#[cfg(feature = "std")]
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use serde::Serialize;

use super::{BUFFER_SIZE, Call, Reply, socket::WriteHalf};
use crate::log::debug2format;

#[cfg(feature = "std")]
use std::os::fd::OwnedFd;

/// A connection.
///
/// The low-level API to send messages.
///
/// # Cancel safety
///
/// All async methods of this type are cancel safe unless explicitly stated otherwise in its
/// documentation. If the future of a method that writes is dropped before it completes, the
/// unwritten data stays queued, and the next flush or send writes it first.
#[derive(Debug)]
pub struct WriteConnection<Write: WriteHalf> {
    pub(super) socket: Write,
    pub(super) buffer: Vec<u8>,
    pub(super) pos: usize,
    /// How much of the queued data, `buffer[..pos]`, was written.
    written: usize,
    id: usize,
    #[cfg(feature = "std")]
    pending_fds: VecDeque<MessageFds>,
    #[cfg(all(feature = "std", target_os = "linux"))]
    credentials: Option<crate::connection::PassedCredentials>,
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
            written: 0,
            #[cfg(feature = "std")]
            pending_fds: VecDeque::new(),
            #[cfg(all(feature = "std", target_os = "linux"))]
            credentials: None,
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
        trace!(
            "connection {}: sending call: {:?}",
            self.id,
            debug2format(call)
        );
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
        trace!(
            "connection {}: sending reply: {:?}",
            self.id,
            debug2format(reply)
        );
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
        trace!(
            "connection {}: sending error: {:?}",
            self.id,
            debug2format(error)
        );
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
        trace!(
            "connection {}: enqueuing call: {:?}",
            self.id,
            debug2format(call)
        );
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
    ///
    /// If the returned future is dropped before it completes, the unwritten data stays queued, and
    /// the next flush or send writes it first. The same goes for the bytes the socket has buffered
    /// (see [`WriteHalf::flush`]): the next flush or send sends them out.
    pub async fn flush(&mut self) -> crate::Result<()> {
        while self.written < self.pos {
            self.write_next().await?;
        }
        self.pos = 0;
        self.written = 0;

        // Even with nothing queued: an earlier flush may have been dropped right here.
        self.socket.flush().await
    }

    /// The underlying write half of the socket.
    pub fn write_half(&self) -> &Write {
        &self.socket
    }

    /// Consume the connection into the underlying write half of the socket.
    ///
    /// Messages that were enqueued but not flushed yet are dropped, so [flush](Self::flush) them
    /// first if needed. This is useful to switch the socket over to another protocol, e.g. after a
    /// method call that asked for an upgrade (see [`Call::upgrade`]).
    pub fn into_write_half(self) -> Write {
        self.socket
    }

    /// Set the credentials to send with all subsequent writes.
    ///
    /// This is only available for `std` feature and `linux` target.
    #[cfg(all(feature = "std", target_os = "linux"))]
    pub fn set_credentials(&mut self, credentials: Option<crate::connection::PassedCredentials>) {
        self.credentials = credentials;
    }

    /// A connection that writes `data` first, along with `fds`, the file descriptors of its
    /// messages, and then what is sent through it, over `socket`.
    ///
    /// `data` is what was enqueued for another connection over the socket but not written, as
    /// [`WriteConnection::into_unwritten`] gives it.
    #[cfg(all(feature = "handover", target_os = "linux"))]
    pub(super) fn with_unwritten(
        socket: Write,
        id: usize,
        mut data: Vec<u8>,
        fds: VecDeque<MessageFds>,
    ) -> Self {
        let pos = data.len();
        data.resize(pos.max(BUFFER_SIZE), 0);
        let mut conn = Self::new(socket, id);
        conn.buffer = data;
        conn.pos = pos;
        conn.pending_fds = fds;

        conn
    }

    /// Consume the connection into the write half of the socket, what was enqueued but not
    /// written, and the file descriptors of its messages, positioned in it.
    ///
    /// If a flush was cancelled after a partial write, only the unwritten part is included.
    #[cfg(all(feature = "handover", target_os = "linux"))]
    pub(super) fn into_unwritten(mut self) -> (Write, Vec<u8>, VecDeque<MessageFds>) {
        self.buffer.truncate(self.pos);
        self.buffer.drain(..self.written);
        // The FDs of a partly written message went with its first bytes, and its entry with them,
        // so the remaining entries all start at or after `written`.
        for msg in &mut self.pending_fds {
            msg.offset -= self.written;
        }

        (self.socket, self.buffer, self.pending_fds)
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

    /// Write part of the unwritten data: up to the next message with file descriptors, or that
    /// message, with its file descriptors, if it's next.
    async fn write_next(&mut self) -> crate::Result<()> {
        let start = self.written;
        #[cfg(feature = "std")]
        let (end, fds, of_msg) = match self.pending_fds.front() {
            // The file descriptors go with the first byte of their message.
            Some(msg) if msg.offset == start => (start + msg.len, &msg.fds[..], true),
            Some(msg) => (msg.offset, &[][..], false),
            None => (self.pos, &[][..], false),
        };
        #[cfg(feature = "std")]
        trace!(
            "connection {}: flushing {} bytes with {} FDs",
            self.id,
            end - start,
            fds.len()
        );
        #[cfg(not(feature = "std"))]
        let end = self.pos;
        #[cfg(not(feature = "std"))]
        trace!("connection {}: flushing {} bytes", self.id, end - start);

        let written = self
            .socket
            .write(
                &self.buffer[start..end],
                #[cfg(feature = "std")]
                fds,
                #[cfg(all(feature = "std", target_os = "linux"))]
                self.credentials.as_ref(),
            )
            .await?;
        // A socket that accepts nothing would have us loop forever.
        if written == 0 {
            return Err(crate::Error::SocketWrite);
        }
        self.written += written;

        // The message's FDs went with its first bytes, so drop its entry, even an empty one, which
        // a hand-over can bring (see `WriteConnection::with_unwritten`).
        #[cfg(feature = "std")]
        if of_msg {
            // On macOS, keep sent FDs alive until the read half confirms receipt via `recvmsg`.
            // See the comment on the `held_fds` field for details.
            #[cfg(target_os = "macos")]
            self.held_fds
                .extend(self.pending_fds.pop_front().map(|msg| msg.fds));
            #[cfg(not(target_os = "macos"))]
            self.pending_fds.pop_front();
        }

        Ok(())
    }

    // Double the buffer, so a message that does not fit is serialized again only O(log n) times
    // rather than once per `BUFFER_SIZE` bytes, which would make large messages quadratic.
    fn grow_buffer(&mut self) -> crate::Result<()> {
        let len = self.buffer.len();
        if len >= super::MAX_BUFFER_SIZE {
            return Err(crate::Error::BufferOverflow);
        }

        let new_len = (len * 2).clamp(BUFFER_SIZE, super::MAX_BUFFER_SIZE);
        self.buffer.resize(new_len, 0);

        Ok(())
    }
}

/// Information about file descriptors pending to be sent with a message.
#[cfg(feature = "std")]
#[derive(Debug)]
pub(super) struct MessageFds {
    /// File descriptors to send with this message.
    pub(super) fds: Vec<OwnedFd>,
    /// Buffer offset where the message starts.
    pub(super) offset: usize,
    /// Length of the message including the null terminator.
    pub(super) len: usize,
}
