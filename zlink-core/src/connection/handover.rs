//! The hand-over of a connection to another process: [`Handover`].
//!
//! The record of a hand-over, which [`zoter::Save`] makes and [`zoter::Restore`] reads, is a JSON
//! object of zlink's own, such as:
//!
//! ```json
//! {
//!     "version": 1,
//!     "unreceived": "eyJtZXRob2QiOiJhIn0AeyJtZXRob2Q=",
//!     "unreceivedFds": [{ "pos": 3, "count": 1 }],
//!     "receivedCredentials": { "pid": 4242, "uid": 1000, "gid": 100 },
//!     "unwritten": "eyJwYXJhbWV0ZXJzIjp7fX0A",
//!     "unwrittenFds": [{ "offset": 0, "len": 18, "count": 1 }]
//! }
//! ```
//!
//! * `version`: the version of the record's format, `VERSION`. zlink refuses a record of a later
//!   version of the format, and ignores the fields that it doesn't know, so that adding fields
//!   needs no new version of the format.
//! * `unreceived`: what was read off the socket and not received, in base64, as it needn't be text:
//!   it can end in the middle of a message, or be anything that a client sent.
//! * `unreceivedFds`: the lists of file descriptors that came with what was read, in the order of
//!   the bytes that they came with, each the position of its byte in what was read and its count of
//!   file descriptors.
//! * `receivedCredentials`: the credentials that came with the latest read, or `null` where none
//!   came: the IDs of the process, of its user and of its group.
//! * `unwritten`: what was enqueued and not written, in base64.
//! * `unwrittenFds`: the lists of file descriptors that go with what was enqueued, in the order of
//!   their messages, each the position of its message in what was enqueued, the length of the
//!   message and its count of file descriptors.
//!
//! The file descriptors of the record are the socket's, then those of the lists of what was read,
//! and then those of the lists of what was enqueued, each in their order.

use alloc::{borrow::Cow, collections::VecDeque, vec::Vec};
use core::{iter, sync::atomic::Ordering};
use std::{
    io,
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::net::UnixStream,
    },
    sync::Arc,
};

use rustix::net::{AddressFamily, SocketType, sockopt};
use serde::{Deserialize, Serialize};

use super::{
    Connection, Gid, NEXT_ID, PassedCredentials, Pid, ReadConnection, Uid, WriteConnection,
    read_connection::PendingFds,
    socket::{IntoStd, Socket},
    write_connection::MessageFds,
};

impl<S> Connection<S>
where
    S: Socket,
{
    /// Hand the connection over, for another process to go on with it from where it is, such as
    /// the next instance of a service that restarts.
    ///
    /// The [`Handover`] holds the socket of the connection, its halves joined back into it and
    /// given up whole (see [`Socket::join`] and [`IntoStd`]), what was read off it but not
    /// received, such as the method calls that a server read but didn't handle, and what was
    /// enqueued but not written, along with the file descriptors of both, and the credentials that
    /// came with the latest read, which are those of what was read but not received. The other
    /// process goes on with it through [`Connection::take_over`]. The credentials of the peer, and
    /// those to send, are left behind, for the other process to fetch or to set anew.
    ///
    /// It fails as [`Socket::join`] and [`IntoStd::into_std`] do.
    pub fn hand_over(self) -> crate::Result<Handover>
    where
        S: IntoStd,
    {
        let (mut read, write) = self.split();
        let received_credentials = read.take_received_credentials();
        let (read, unreceived, unreceived_fds) = read.into_unreceived();
        let (write, unwritten, unwritten_fds) = write.into_unwritten();
        let socket = S::join(read, write)?.into_std()?;

        Ok(Handover {
            socket,
            unreceived,
            unreceived_fds,
            received_credentials,
            unwritten,
            unwritten_fds,
        })
    }

    /// The connection that `handover` handed over, which goes on from where it was: what was read
    /// but not received is received first, and what was enqueued but not written goes ahead of
    /// what is sent next, or flushed.
    ///
    /// The socket becomes an `S` through its `TryFrom` impl, as those of the runtime crates make
    /// one on the runtime of the calling thread: tokio's panics outside of a runtime.
    pub fn take_over(handover: Handover) -> crate::Result<Self>
    where
        S: TryFrom<UnixStream, Error = crate::Error>,
    {
        let Handover {
            socket,
            unreceived,
            unreceived_fds,
            received_credentials,
            unwritten,
            unwritten_fds,
        } = handover;
        let (read, write) = S::try_from(socket)?.split();
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let read = ReadConnection::with_unreceived(
            read,
            id,
            unreceived,
            unreceived_fds,
            received_credentials,
        );

        Ok(Self {
            read,
            write: WriteConnection::with_unwritten(write, id, unwritten, unwritten_fds),
            credentials: None,
        })
    }
}

/// A connection handed over to another process (see [`Connection::hand_over`]), such as the next
/// instance of a service that restarts, which goes on with it from where it was (see
/// [`Connection::take_over`]).
///
/// It goes to the other process as a record of bytes and file descriptors, through zoter's
/// [`Save`](zoter::Save) and [`Restore`](zoter::Restore), as an item of a [`zoter::Handover`]. A
/// server gives its connections back between method calls as it stops, for that (see
/// `Server::run_until`). The record is a JSON object of zlink's own, which later versions of zlink
/// read too, and which zlink refuses where its format is of a later version.
#[derive(Debug)]
pub struct Handover {
    /// The socket of the connection.
    socket: UnixStream,
    /// What was read off the socket but not received.
    unreceived: Vec<u8>,
    /// The file descriptors that came with `unreceived`, positioned in it.
    unreceived_fds: VecDeque<PendingFds>,
    /// The credentials that came with the latest read, those of `unreceived`.
    received_credentials: Option<Arc<PassedCredentials>>,
    /// What was enqueued to be written to the socket but not written.
    unwritten: Vec<u8>,
    /// The file descriptors of the messages of `unwritten`, positioned in it.
    unwritten_fds: VecDeque<MessageFds>,
}

impl Handover {
    /// The record of the hand-over.
    fn record(&self) -> Record<'_> {
        let unreceived_fds = self.unreceived_fds.iter().map(|pending| UnreceivedFds {
            pos: pending.pos,
            count: pending.fds.len(),
        });
        let unwritten_fds = self.unwritten_fds.iter().map(|msg| UnwrittenFds {
            offset: msg.offset,
            len: msg.len,
            count: msg.fds.len(),
        });
        let received_credentials = self
            .received_credentials
            .as_deref()
            .map(|creds| Credentials {
                pid: creds.process_id().as_raw_pid(),
                uid: creds.unix_user_id().as_raw(),
                gid: creds.unix_primary_group_id().as_raw(),
            });

        Record {
            version: VERSION,
            unreceived: Cow::Borrowed(&self.unreceived),
            unreceived_fds: unreceived_fds.collect(),
            received_credentials,
            unwritten: Cow::Borrowed(&self.unwritten),
            unwritten_fds: unwritten_fds.collect(),
        }
    }
}

/// The record of the hand-over, and its file descriptors.
impl zoter::Save for Handover {
    fn saved_len(&self) -> Option<usize> {
        let mut len = Len(0);
        // Writing the record fails only where its writer does, which counting never does.
        serde_json::to_writer(&mut len, &self.record()).ok()?;

        Some(len.0)
    }

    /// Write the record of the hand-over into `dest`, which is [`Save::saved_len`] bytes long.
    ///
    /// It fails with [`io::ErrorKind::InvalidInput`] for a `dest` of another length.
    ///
    /// [`Save::saved_len`]: zoter::Save::saved_len
    fn save(&self, dest: &mut [u8]) -> io::Result<()> {
        let mut rest = &mut *dest;
        // Writing the record fails only where `rest` runs out.
        let written = serde_json::to_writer(&mut rest, &self.record());
        if written.is_err() || !rest.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the memory is not as long as the record",
            ));
        }

        Ok(())
    }

    /// The file descriptors of the hand-over: that of the socket, then those that came with what
    /// was read, and then those of what was enqueued, in their order.
    fn fds(&self) -> Vec<BorrowedFd<'_>> {
        let unreceived = self.unreceived_fds.iter().flat_map(|pending| &pending.fds);
        let unwritten = self.unwritten_fds.iter().flat_map(|msg| &msg.fds);

        iter::once(self.socket.as_fd())
            .chain(unreceived.chain(unwritten).map(AsFd::as_fd))
            .collect()
    }
}

/// The hand-over that [`zoter::Save`] saved, out of its record and its file descriptors.
impl zoter::Restore for Handover {
    type Error = crate::Error;

    /// Restore the hand-over that `record` and `fds` are of.
    ///
    /// It fails with [`crate::Error::Io`], of [`io::ErrorKind::InvalidData`] for a record that
    /// isn't of a hand-over, for file descriptors that don't match it, or whose first one isn't a
    /// Unix domain stream socket, and of [`io::ErrorKind::Unsupported`] for a record that a later
    /// version of zlink made, which this one can't read.
    fn restore(record: &[u8], fds: Vec<OwnedFd>) -> crate::Result<Self> {
        parse(record, fds).map_err(Into::into)
    }
}

/// The hand-over that `record` and `fds` are of.
fn parse(record: &[u8], fds: Vec<OwnedFd>) -> io::Result<Handover> {
    // The version comes first, as a record of a later one needn't read as one of this version.
    let Version { version } = serde_json::from_slice(record).map_err(|_| malformed())?;
    if version > VERSION {
        return Err(unsupported());
    }
    let Record {
        version,
        unreceived,
        unreceived_fds: unreceived_lists,
        received_credentials,
        unwritten,
        unwritten_fds: unwritten_lists,
    } = serde_json::from_slice(record).map_err(|_| malformed())?;
    // No version of the format came before the first.
    if version < VERSION {
        return Err(malformed());
    }
    let received_credentials = received_credentials.map(Credentials::passed).transpose()?;

    let mut fds = fds.into_iter();
    let socket = fds.next().ok_or_else(malformed)?;
    let unix_stream = sockopt::socket_type(&socket).is_ok_and(|kind| kind == SocketType::STREAM)
        && sockopt::socket_domain(&socket).is_ok_and(|domain| domain == AddressFamily::UNIX);
    if !unix_stream {
        return Err(malformed());
    }
    let mut take_fds = |count: usize| -> io::Result<Vec<OwnedFd>> {
        let taken: Vec<_> = fds.by_ref().take(count).collect();
        if taken.len() != count {
            return Err(malformed());
        }

        Ok(taken)
    };

    let mut unreceived_fds = VecDeque::with_capacity(unreceived_lists.len());
    for UnreceivedFds { pos, count } in unreceived_lists {
        // Each list came with a byte of its own, so they come in the order of their bytes.
        let after_last = unreceived_fds
            .back()
            .is_none_or(|last: &PendingFds| pos > last.pos);
        if pos >= unreceived.len() || !after_last {
            return Err(malformed());
        }
        unreceived_fds.push_back(PendingFds {
            pos,
            fds: take_fds(count)?,
        });
    }
    let mut unwritten_fds = VecDeque::with_capacity(unwritten_lists.len());
    for UnwrittenFds { offset, len, count } in unwritten_lists {
        // Each list goes with a message of its own, one after the other.
        let end = unwritten_fds
            .back()
            .map_or(0, |last: &MessageFds| last.offset + last.len);
        let fits = offset
            .checked_add(len)
            .is_some_and(|msg_end| msg_end <= unwritten.len());
        if offset < end || len == 0 || !fits {
            return Err(malformed());
        }
        unwritten_fds.push_back(MessageFds {
            fds: take_fds(count)?,
            offset,
            len,
        });
    }
    if fds.next().is_some() {
        return Err(malformed());
    }

    Ok(Handover {
        socket: UnixStream::from(socket),
        unreceived: unreceived.into_owned(),
        unreceived_fds,
        received_credentials,
        unwritten: unwritten.into_owned(),
        unwritten_fds,
    })
}

/// The record of a hand-over, which the module's documentation tells of.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record<'h> {
    version: u32,
    #[serde(with = "in_base64")]
    unreceived: Cow<'h, [u8]>,
    unreceived_fds: Vec<UnreceivedFds>,
    received_credentials: Option<Credentials>,
    #[serde(with = "in_base64")]
    unwritten: Cow<'h, [u8]>,
    unwritten_fds: Vec<UnwrittenFds>,
}

/// The version of the format of a record, which is read before the rest of the record.
#[derive(Deserialize)]
struct Version {
    version: u32,
}

/// A list of file descriptors that came with what was read, in a record.
#[derive(Serialize, Deserialize)]
struct UnreceivedFds {
    /// The position of the byte that the list came with, in what was read.
    pos: usize,
    /// The count of the file descriptors of the list.
    count: usize,
}

/// A list of file descriptors that go with a message that was enqueued, in a record.
#[derive(Serialize, Deserialize)]
struct UnwrittenFds {
    /// The position of the message, in what was enqueued.
    offset: usize,
    /// The length of the message.
    len: usize,
    /// The count of the file descriptors of the list.
    count: usize,
}

/// The credentials that came with the latest read, in a record.
#[derive(Serialize, Deserialize)]
struct Credentials {
    /// The ID of the process.
    pid: i32,
    /// The ID of its user.
    uid: u32,
    /// The ID of its group.
    gid: u32,
}

impl Credentials {
    /// The credentials, which name a process, a user and a group.
    fn passed(self) -> io::Result<Arc<PassedCredentials>> {
        // No process has an ID of 0 or below, and -1 names no user or group.
        if self.pid <= 0 || self.uid == u32::MAX || self.gid == u32::MAX {
            return Err(malformed());
        }
        let pid = Pid::from_raw(self.pid).ok_or_else(malformed)?;
        let creds = PassedCredentials::new(Uid::from_raw(self.uid), Gid::from_raw(self.gid), pid);

        Ok(Arc::new(creds))
    }
}

/// Bytes of a record, which needn't be text, as a JSON string of their base64.
mod in_base64 {
    use alloc::{borrow::Cow, vec::Vec};
    use core::fmt;

    use base64::{Engine, display::Base64Display, engine::general_purpose::STANDARD};
    use serde::{Deserializer, Serializer, de};

    pub(super) fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&Base64Display::new(bytes, &STANDARD))
    }

    pub(super) fn deserialize<'de, 'b, D>(deserializer: D) -> Result<Cow<'b, [u8]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(Decoder).map(Cow::Owned)
    }

    /// What decodes the bytes out of their base64.
    struct Decoder;

    impl de::Visitor<'_> for Decoder {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string of base64")
        }

        fn visit_str<E>(self, base64: &str) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            STANDARD.decode(base64).map_err(E::custom)
        }
    }
}

/// What counts the bytes written to it, and keeps none of them.
struct Len(usize);

impl io::Write for Len {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The error of a record, or file descriptors, that aren't of a hand-over.
fn malformed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "not the record of a hand-over of a connection and its file descriptors",
    )
}

/// The error of a record that a later version of zlink made.
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the record of a hand-over of a connection that a later version of zlink made",
    )
}

/// The version of the record's format: zlink refuses a record of a later version of the format,
/// which it can't read.
const VERSION: u32 = 1;

#[cfg(test)]
mod tests;
