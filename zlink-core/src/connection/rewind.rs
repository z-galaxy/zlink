//! Contains the read half that the read connections are consumed into.

#[cfg(feature = "std")]
use alloc::collections::VecDeque;
use alloc::vec::Vec;

use super::socket::{ReadHalf, ReadResult};
#[cfg(feature = "std")]
use super::{read_connection::PendingFds, socket::UnixSocket};

#[cfg(feature = "std")]
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

/// The read half of a socket, along with the data that was read from it but not received.
///
/// A [`ReadConnection`] reads from the socket in chunks, so it may have read data that the peer
/// sent after the last message received through it, e.g. the data of another protocol after a
/// method call that asked for an upgrade (see [`Call::upgrade`]). Once the connection is consumed
/// into this read half (see [`ReadConnection::into_read_half`]), reads return that data first,
/// exactly as it was read and along with the file descriptors that came with it (`std` only).
/// They then read from the socket.
///
/// [`ReadConnection`]: super::ReadConnection
/// [`ReadConnection::into_read_half`]: super::ReadConnection::into_read_half
/// [`Call::upgrade`]: crate::Call::upgrade
#[derive(Debug)]
pub struct Rewind<Read> {
    socket: Read,
    // The data read from the socket that wasn't returned yet is `buffer[pos..]`.
    buffer: Vec<u8>,
    pos: usize,
    // The file descriptors that came with the data in `buffer`.
    #[cfg(feature = "std")]
    pending_fds: VecDeque<PendingFds>,
}

impl<Read> Rewind<Read> {
    /// Create a read half that returns `buffer` before reading from `socket`.
    pub(super) fn new(
        socket: Read,
        mut buffer: Vec<u8>,
        #[cfg(feature = "std")] pending_fds: VecDeque<PendingFds>,
    ) -> Self {
        // The buffer is usually much larger than the data it holds, if any.
        buffer.shrink_to_fit();

        Self {
            socket,
            buffer,
            pos: 0,
            #[cfg(feature = "std")]
            pending_fds,
        }
    }
}

impl<Read> ReadHalf for Rewind<Read>
where
    Read: ReadHalf,
{
    async fn read(&mut self, buf: &mut [u8]) -> crate::Result<ReadResult> {
        if self.pos == self.buffer.len() {
            return self.socket.read(buf).await;
        }

        let len = buf.len().min(self.buffer.len() - self.pos);
        #[cfg(feature = "std")]
        let (len, fds) = self.take_fds(len);
        buf[..len].copy_from_slice(&self.buffer[self.pos..self.pos + len]);
        self.pos += len;
        if self.pos == self.buffer.len() {
            // All the data was returned, so the buffer isn't needed anymore.
            self.buffer = Vec::new();
            self.pos = 0;
        }

        let result = ReadResult::new(len);
        #[cfg(feature = "std")]
        let result = result.set_fds(fds);

        Ok(result)
    }
}

#[cfg(feature = "std")]
impl<Read> Rewind<Read> {
    // The length of the next read from the buffer, given that it can return up to `len` bytes,
    // along with the file descriptors that it returns.
    fn take_fds(&mut self, len: usize) -> (usize, Vec<OwnedFd>) {
        match self.pending_fds.front() {
            // Stop right before the byte that came with the next file descriptors.
            Some(pending) if pending.pos > self.pos => {
                (len.min(pending.pos - self.pos), Vec::new())
            }
            // Return the file descriptors with the byte that came with them, and only that byte.
            // Depending on the OS, the file descriptors of a read come with its first byte or with
            // its last one, and that byte is then both.
            Some(_) if len > 0 => {
                let fds = self.pending_fds.pop_front().map(|pending| pending.fds);

                (1, fds.unwrap_or_default())
            }
            _ => (len, Vec::new()),
        }
    }
}

#[cfg(feature = "std")]
impl<Read> AsFd for Rewind<Read>
where
    Read: AsFd,
{
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }
}

#[cfg(feature = "std")]
impl<Read> UnixSocket for Rewind<Read> where Read: UnixSocket {}
