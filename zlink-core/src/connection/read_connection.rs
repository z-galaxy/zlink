//! Contains connection related API.

#[cfg(feature = "std")]
use core::ops::RangeInclusive;
use core::{fmt::Debug, str::from_utf8};

#[cfg(all(feature = "std", target_os = "linux"))]
use crate::connection::PassedCredentials;
use crate::{Result, varlink_service};

use super::{
    BUFFER_SIZE, Call, MAX_BUFFER_SIZE,
    reply::{self, Reply},
    rewind::Rewind,
    socket::ReadHalf,
};
#[cfg(feature = "std")]
use alloc::collections::VecDeque;
use alloc::{string::String, vec::Vec};
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
    // The data read from the socket but not received yet is `buffer[msg_pos..read_pos]`:
    // `buffer[msg_pos..msgs_end]` holds complete messages, each terminated by a NUL byte, and
    // `buffer[msgs_end..read_pos]` the start of a message that wasn't completely read yet.
    read_pos: usize,
    msg_pos: usize,
    msgs_end: usize,
    buffer: Vec<u8>,
    id: usize,
    // File descriptors received along with data in `buffer` that haven't been returned yet.
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
            msgs_end: 0,
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
    /// Errors of the `org.varlink.service` interface are returned as [`Error::VarlinkService`], and
    /// other errors that `ReplyError` can't be deserialized from, e.g. ones that the client doesn't
    /// know, as [`Error::UnknownErrorReply`].
    ///
    /// Returns the reply and any file descriptors received (std only).
    ///
    /// [`Error::VarlinkService`]: crate::Error::VarlinkService
    /// [`Error::UnknownErrorReply`]: crate::Error::UnknownErrorReply
    pub async fn receive_reply<'r, ReplyParams, ReplyError>(
        &'r mut self,
    ) -> Result<RecvResult<reply::Result<ReplyParams, ReplyError>>>
    where
        ReplyParams: Deserialize<'r> + Debug,
        ReplyError: Deserialize<'r> + Debug,
    {
        // A successful reply is deserialized right away, along with the `error` field that only
        // error replies have. Error replies, and replies that fail to deserialize, are then
        // deserialized again as the kind they are, so serde can tell what doesn't match in them.
        #[derive(Debug, Deserialize)]
        struct ReplyMsg<ReplyParams> {
            error: Option<String>,
            parameters: Option<ReplyParams>,
            continues: Option<bool>,
        }
        #[derive(Debug, Deserialize)]
        struct ErrorName {
            error: Option<String>,
        }
        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum KnownError<'m, ReplyError> {
            #[serde(borrow)]
            Varlink(varlink_service::Error<'m>),
            Error(ReplyError),
        }

        self.read_message(|msg: serde_json::Result<ReplyMsg<ReplyParams>>, json| {
            let error = match msg {
                Ok(msg) => match msg.error {
                    Some(error) => error,
                    None => return Ok(Ok(Reply::new(msg.parameters).set_continues(msg.continues))),
                },
                Err(e) => match serde_json::from_slice::<ErrorName>(json) {
                    Ok(ErrorName { error: Some(error) }) => error,
                    // Not an error reply, so `e` tells what's wrong with it.
                    _ => return Err(e.into()),
                },
            };

            match serde_json::from_slice(json) {
                // Varlink service interface error need to be returned as the top-level error.
                Ok(KnownError::Varlink(e)) => Err(crate::Error::VarlinkService(e.into())),
                Ok(KnownError::Error(e)) => Ok(Err(e)),
                // So do the errors that the client doesn't know.
                Err(_) => Err(crate::Error::UnknownErrorReply(error)),
            }
        })
        .await
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
        self.read_message(|call, _| Ok(call?)).await
    }

    // Receives the next message, reading from the socket first if no complete one is buffered.
    //
    // `f` is passed the result of deserializing the message as `M`, along with the bytes of the
    // message to deserialize it again as needed, e.g. as another type when it isn't an `M`.
    async fn read_message<'m, M, T>(
        &'m mut self,
        f: impl FnOnce(serde_json::Result<M>, &'m [u8]) -> Result<T>,
    ) -> Result<RecvResult<T>>
    where
        M: Deserialize<'m> + Debug,
    {
        if self.msg_pos == self.msgs_end {
            self.read_from_socket().await?;
        }

        let start = self.msg_pos;
        let mut stream =
            Deserializer::from_slice(&self.buffer[start..self.msgs_end]).into_iter::<M>();
        let msg = stream.next();
        // The deserializer tells where the JSON value ends, so the NUL byte terminating a
        // well-formed message is found right away.
        let json_end = start + stream.byte_offset();
        let null_index = self.null_index_from(json_end);
        #[cfg(feature = "std")]
        let fds = take_fds(&mut self.pending_fds, start..=null_index);
        self.msg_pos = null_index + 1;
        if self.msg_pos == self.read_pos {
            // Everything read was received, so the buffer can be reused from its start.
            self.read_pos = 0;
            self.msg_pos = 0;
            self.msgs_end = 0;
            // Nothing is left for the remaining file descriptors, if any, to belong to.
            #[cfg(feature = "std")]
            self.pending_fds.clear();
        }

        let msg = match msg {
            Some(Ok(_)) if !is_json_whitespace(&self.buffer[json_end..null_index]) => {
                return Err(crate::Error::Json(serde::de::Error::custom(
                    "trailing characters after the message",
                )));
            }
            Some(msg) => msg,
            None => return Err(crate::Error::UnexpectedEof),
        };
        let buffer = &self.buffer[start..null_index];
        // The message isn't necessarily valid UTF-8, even if it was deserialized: the
        // deserializer doesn't check the values that the message type ignores.
        trace!(
            "connection {}: received a message: {}",
            self.id,
            from_utf8(buffer).unwrap_or("<invalid UTF-8>")
        );

        let msg = f(msg, buffer)?;

        #[cfg(feature = "std")]
        {
            Ok((msg, fds))
        }
        #[cfg(not(feature = "std"))]
        Ok(msg)
    }

    // The index of the NUL byte terminating the message that `pos` is in.
    fn null_index_from(&self, pos: usize) -> usize {
        // The complete messages end with a NUL byte.
        self.buffer[pos..self.msgs_end]
            .iter()
            .position(|b| *b == b'\0')
            .map_or(self.msgs_end - 1, |i| pos + i)
    }

    // Reads from the socket until at least one complete message is buffered.
    async fn read_from_socket(&mut self) -> Result<()> {
        if self.msg_pos > 0 {
            // Move the start of the next message to the start of the buffer, making room for the
            // rest of it.
            self.buffer.copy_within(self.msg_pos..self.read_pos, 0);
            #[cfg(feature = "std")]
            self.move_pending_fds_to_start();
            self.read_pos -= self.msg_pos;
            self.msg_pos = 0;
            self.msgs_end = 0;
        }

        loop {
            if self.read_pos == self.buffer.len() {
                if self.read_pos >= MAX_BUFFER_SIZE {
                    return Err(crate::Error::BufferOverflow);
                }

                self.buffer.extend(core::iter::repeat_n(0, BUFFER_SIZE));
            }

            let read_start = self.read_pos;
            let result = self.socket.read(&mut self.buffer[read_start..]).await?;
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
                // Peers send the file descriptors of a message along with its first byte, in a
                // write that carries no other message, but the reads don't necessarily match the
                // writes. On Linux, the read that receives the file descriptors can also return
                // the data of earlier writes but it ends with the data of the write that carried
                // them. BSD-derived kernels instead begin the read with that write and can append
                // the data of later writes to it. Hence, the last byte read came with the file
                // descriptors on Linux, and the first one elsewhere.
                #[cfg(any(target_os = "linux", target_os = "android"))]
                let pos = self.read_pos - 1;
                #[cfg(not(any(target_os = "linux", target_os = "android")))]
                let pos = read_start;
                self.pending_fds.push_back(PendingFds {
                    pos,
                    fds: result.into_fds(),
                });
                // Track receipt so `Connection` can drain `WriteConnection::held_fds`.
                #[cfg(target_os = "macos")]
                {
                    self.fd_recvs += 1;
                }
            }

            // The last NUL byte read terminates the last message read completely, if any.
            if let Some(i) = self.buffer[read_start..self.read_pos]
                .iter()
                .rposition(|b| *b == b'\0')
            {
                self.msgs_end = read_start + i + 1;

                return Ok(());
            }
        }
    }

    // Moves the pending file descriptors along with the data they came with, as the data that was
    // received is dropped from the start of the buffer.
    #[cfg(feature = "std")]
    fn move_pending_fds_to_start(&mut self) {
        let msg_pos = self.msg_pos;
        self.pending_fds.retain_mut(|pending| {
            let Some(pos) = pending.pos.checked_sub(msg_pos) else {
                // Left over from a message that was received already.
                return false;
            };
            pending.pos = pos;

            true
        });
    }

    /// The underlying read half of the socket.
    pub fn read_half(&self) -> &Read {
        &self.socket
    }

    /// Consume the connection into the underlying read half of the socket.
    ///
    /// The connection reads from the socket in chunks, so it may have read data that the peer
    /// sent after the last message received through it. The returned read half returns that data
    /// first, exactly as it was read, along with the file descriptors that came with it (`std`
    /// only). It's where the data of another protocol starts when switching the socket over to
    /// one, e.g. after a method call that asked for an upgrade (see [`Call::upgrade`]).
    pub fn into_read_half(self) -> Rewind<Read> {
        #[cfg(feature = "std")]
        {
            let (socket, data, fds) = self.into_unreceived();

            Rewind::new(socket, data, fds)
        }
        #[cfg(not(feature = "std"))]
        {
            let (socket, data) = self.into_unreceived();

            Rewind::new(socket, data)
        }
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

impl<Read: ReadHalf> ReadConnection<Read> {
    /// A connection that receives `data` first, along with `fds`, the file descriptors that came
    /// with it, and `credentials`, those that came with the latest read, and then reads from
    /// `socket`.
    ///
    /// `data` is what another connection read from the socket but didn't receive, as
    /// [`ReadConnection::into_unreceived`] gives it, and `credentials` are those that
    /// [`ReadConnection::take_received_credentials`] took from it.
    #[cfg(all(feature = "handover", target_os = "linux"))]
    pub(super) fn with_unreceived(
        socket: Read,
        id: usize,
        mut data: Vec<u8>,
        fds: VecDeque<PendingFds>,
        credentials: Option<std::sync::Arc<PassedCredentials>>,
    ) -> Self {
        let read_pos = data.len();
        // The complete messages end with the last NUL byte.
        let msgs_end = data.iter().rposition(|b| *b == b'\0').map_or(0, |i| i + 1);
        data.resize(read_pos.max(BUFFER_SIZE), 0);
        let mut conn = Self::new(socket, id);
        conn.buffer = data;
        conn.read_pos = read_pos;
        conn.msgs_end = msgs_end;
        conn.pending_fds = fds;
        conn.received_credentials = credentials;

        conn
    }

    /// Take the credentials that came with the latest read, which
    /// [`ReadConnection::received_credentials`] gives for each message received until the next
    /// read, those of what was read but not received included.
    #[cfg(all(feature = "handover", target_os = "linux"))]
    pub(super) fn take_received_credentials(
        &mut self,
    ) -> Option<std::sync::Arc<PassedCredentials>> {
        self.received_credentials.take()
    }

    /// Consume the connection into the read half of the socket, what was read from it but not
    /// received, and the file descriptors that came with that (`std` only), positioned in it.
    #[cfg(feature = "std")]
    pub(super) fn into_unreceived(mut self) -> (Read, Vec<u8>, VecDeque<PendingFds>) {
        self.unreceived_to_start();

        (self.socket, self.buffer, self.pending_fds)
    }

    /// Consume the connection into the read half of the socket and what was read from it but not
    /// received.
    #[cfg(not(feature = "std"))]
    pub(super) fn into_unreceived(mut self) -> (Read, Vec<u8>) {
        self.unreceived_to_start();

        (self.socket, self.buffer)
    }

    // Moves what wasn't received to the start of the buffer, which holds nothing else then.
    fn unreceived_to_start(&mut self) {
        // Reuse the buffer for the data that wasn't received.
        self.buffer.truncate(self.read_pos);
        self.buffer.drain(..self.msg_pos);
        #[cfg(feature = "std")]
        self.move_pending_fds_to_start();
    }
}

/// File descriptors received along with data in the buffer.
#[cfg(feature = "std")]
#[derive(Debug)]
pub(super) struct PendingFds {
    /// Buffer position of a byte that came with the file descriptors.
    ///
    /// The file descriptors belong to the message this byte is part of. That's only known once the
    /// message is read, as what follows a message isn't necessarily another message, e.g. after a
    /// protocol upgrade.
    pub(super) pos: usize,
    /// The file descriptors.
    pub(super) fds: Vec<OwnedFd>,
}

/// Take the file descriptors received for the message spanning `msg` in the buffer, terminator
/// included.
#[cfg(feature = "std")]
fn take_fds(pending_fds: &mut VecDeque<PendingFds>, msg: RangeInclusive<usize>) -> Vec<OwnedFd> {
    let mut fds = Vec::new();
    while let Some(pending) = pending_fds.front_mut() {
        match pending.pos {
            pos if pos > *msg.end() => break,
            // Came with data before the message, which was received already.
            pos if pos < *msg.start() => (),
            _ if fds.is_empty() => fds = core::mem::take(&mut pending.fds),
            _ => fds.append(&mut pending.fds),
        }
        pending_fds.pop_front();
    }

    fds
}

// Whether `bytes` is only JSON whitespace.
fn is_json_whitespace(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}
