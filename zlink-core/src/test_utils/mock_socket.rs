//! Mock socket implementations for testing.
//!
//! This module provides a full-featured mock socket implementation that can be
//! used in tests to simulate socket behavior without requiring actual network
//! connections.

#[cfg(feature = "std")]
use crate::connection;
use crate::connection::socket::{ReadHalf, ReadResult, Socket, WriteHalf};
use alloc::vec::Vec;
#[cfg(feature = "std")]
use core::cell::RefCell;
#[cfg(feature = "std")]
use rustix::fd::{BorrowedFd, OwnedFd};

/// Mock socket implementation for testing.
///
/// This socket pre-loads response data and allows tests to verify what was written.
/// Each message is stored separately and returned one at a time per read() call,
/// simulating non-pipelined socket behavior where each write is read separately.
///
/// In std mode, also supports file descriptor passing for testing FD-based IPC.
#[derive(Debug)]
#[doc(hidden)]
pub struct MockSocket {
    /// The data returned by each read.
    messages: Vec<Vec<u8>>,
    #[cfg(feature = "std")]
    fds: Vec<Vec<OwnedFd>>,
}

impl MockSocket {
    /// Create a new mock socket with pre-configured responses.
    ///
    /// Each response string will be null-terminated. Messages are stored separately and
    /// returned one at a time.
    ///
    /// In std mode, the `fds` parameter specifies which FDs to return with each message.
    /// The i-th FD vec is returned with the i-th message.
    pub fn new(responses: &[&str], #[cfg(feature = "std")] fds: Vec<Vec<OwnedFd>>) -> Self {
        let messages = responses
            .iter()
            .map(|r| {
                let mut msg = r.as_bytes().to_vec();
                msg.push(b'\0');
                msg
            })
            .collect();

        Self {
            messages,
            #[cfg(feature = "std")]
            fds,
        }
    }

    /// Create a mock socket whose read half returns each of `reads` verbatim, in a read of its own.
    ///
    /// Unlike [`MockSocket::new`], nothing is added to the data, so the reads can be split at any
    /// point and end with anything.
    ///
    /// In std mode, the i-th FD vec is returned with the i-th read.
    pub fn with_reads(reads: &[&[u8]], #[cfg(feature = "std")] fds: Vec<Vec<OwnedFd>>) -> Self {
        Self {
            messages: reads.iter().map(|r| r.to_vec()).collect(),
            #[cfg(feature = "std")]
            fds,
        }
    }

    /// Create a mock socket with responses and no file descriptors.
    ///
    /// This is a convenience method for tests that don't use FDs, working
    /// uniformly in both std and no_std modes.
    pub fn with_responses(responses: &[&str]) -> Self {
        Self::new(
            responses,
            #[cfg(feature = "std")]
            Vec::new(),
        )
    }
}

impl Socket for MockSocket {
    type ReadHalf = MockReadHalf;
    type WriteHalf = MockWriteHalf;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        (
            MockReadHalf {
                messages: self.messages,
                msg_index: 0,
                pos_in_msg: 0,
                #[cfg(feature = "std")]
                fds: self.fds,
            },
            MockWriteHalf {
                written: Vec::new(),
                #[cfg(feature = "std")]
                fds_written: RefCell::new(Vec::new()),
                #[cfg(all(feature = "std", target_os = "linux"))]
                credentials_written: Vec::new(),
            },
        )
    }
}

/// Mock read half implementation.
#[derive(Debug)]
#[doc(hidden)]
pub struct MockReadHalf {
    messages: Vec<Vec<u8>>,
    msg_index: usize,
    pos_in_msg: usize,
    #[cfg(feature = "std")]
    fds: Vec<Vec<OwnedFd>>,
}

impl MockReadHalf {
    /// Get the number of messages (or reads, see [`MockSocket::with_reads`]) remaining.
    pub fn messages_remaining(&self) -> usize {
        self.messages.len().saturating_sub(self.msg_index)
    }

    /// Get the number of FD sets that have been consumed (std only).
    #[cfg(feature = "std")]
    pub fn fds_consumed(&self) -> usize {
        self.msg_index
    }
}

impl ReadHalf for MockReadHalf {
    #[cfg(feature = "std")]
    async fn read(&mut self, buf: &mut [u8]) -> crate::Result<ReadResult> {
        // No more messages - EOF.
        if self.msg_index >= self.messages.len() {
            return Ok(ReadResult::new(0));
        }

        let msg = &self.messages[self.msg_index];
        let remaining = msg.len() - self.pos_in_msg;
        let to_read = remaining.min(buf.len());
        buf[..to_read].copy_from_slice(&msg[self.pos_in_msg..self.pos_in_msg + to_read]);
        self.pos_in_msg += to_read;

        // Return FDs only when message is fully read.
        let fds = if self.pos_in_msg >= msg.len() {
            let fds = if self.msg_index < self.fds.len() {
                core::mem::take(&mut self.fds[self.msg_index])
            } else {
                Vec::new()
            };
            self.msg_index += 1;
            self.pos_in_msg = 0;
            fds
        } else {
            Vec::new()
        };

        Ok(ReadResult::new(to_read).set_fds(fds))
    }

    #[cfg(not(feature = "std"))]
    async fn read(&mut self, buf: &mut [u8]) -> crate::Result<ReadResult> {
        // No more messages - EOF.
        if self.msg_index >= self.messages.len() {
            return Ok(ReadResult::new(0));
        }

        let msg = &self.messages[self.msg_index];
        let remaining = msg.len() - self.pos_in_msg;
        let to_read = remaining.min(buf.len());
        buf[..to_read].copy_from_slice(&msg[self.pos_in_msg..self.pos_in_msg + to_read]);
        self.pos_in_msg += to_read;

        // Move to next message when current is fully read.
        if self.pos_in_msg >= msg.len() {
            self.msg_index += 1;
            self.pos_in_msg = 0;
        }

        Ok(ReadResult::new(to_read))
    }
}

#[cfg(feature = "std")]
impl connection::socket::FetchPeerCredentials for MockReadHalf {
    async fn fetch_peer_credentials(&self) -> std::io::Result<connection::Credentials> {
        // For mock sockets, return credentials of the current process.
        let uid = rustix::process::getuid();
        let pid = rustix::process::getpid();
        let gid = rustix::process::getgid();

        #[cfg(target_os = "linux")]
        {
            Ok(connection::Credentials::new(
                connection::PassedCredentials::new(uid, gid, pid),
                vec![],
                None,
            ))
        }

        #[cfg(not(target_os = "linux"))]
        {
            Ok(connection::Credentials::new(
                connection::PassedCredentials::new(uid, gid, pid),
            ))
        }
    }
}

/// Mock write half implementation.
#[derive(Debug)]
#[doc(hidden)]
pub struct MockWriteHalf {
    written: Vec<u8>,
    #[cfg(feature = "std")]
    fds_written: RefCell<Vec<Vec<OwnedFd>>>,
    #[cfg(all(feature = "std", target_os = "linux"))]
    credentials_written: Vec<crate::connection::PassedCredentials>,
}

impl MockWriteHalf {
    /// Create a new mock write half.
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        Self {
            written: Vec::new(),
            fds_written: RefCell::new(Vec::new()),
            #[cfg(all(feature = "std", target_os = "linux"))]
            credentials_written: Vec::new(),
        }
    }

    /// Get all data that has been written to this mock.
    pub fn written_data(&self) -> &[u8] {
        &self.written
    }

    /// Get all file descriptors that have been written (std only).
    #[cfg(feature = "std")]
    pub fn fds_written(&self) -> core::cell::Ref<'_, Vec<Vec<OwnedFd>>> {
        self.fds_written.borrow()
    }

    /// Get the number of times FDs were written (std only).
    #[cfg(feature = "std")]
    pub fn fd_write_count(&self) -> usize {
        self.fds_written.borrow().len()
    }

    /// All credentials that have been written.
    #[cfg(all(feature = "std", target_os = "linux"))]
    pub fn credentials_written(&self) -> &[crate::connection::PassedCredentials] {
        &self.credentials_written
    }
}

#[cfg(feature = "std")]
impl Default for MockWriteHalf {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteHalf for MockWriteHalf {
    async fn write(
        &mut self,
        buf: &[u8],
        #[cfg(feature = "std")] fds: &[impl std::os::fd::AsFd],
        #[cfg(all(feature = "std", target_os = "linux"))] credentials: Option<
            &crate::connection::PassedCredentials,
        >,
    ) -> crate::Result<()> {
        self.written.extend_from_slice(buf);

        #[cfg(feature = "std")]
        {
            let borrowed_fds: Vec<BorrowedFd<'_>> = fds.iter().map(|f| f.as_fd()).collect();

            if !borrowed_fds.is_empty() {
                // For testing, we duplicate the FDs to take ownership.
                // In real implementation, the OS would transfer them.
                let owned_fds: Vec<OwnedFd> = borrowed_fds
                    .iter()
                    .map(|fd| {
                        rustix::io::fcntl_dupfd_cloexec(fd, 0)
                            .map_err(|e| crate::Error::Io(e.into()))
                    })
                    .collect::<crate::Result<Vec<_>>>()?;
                self.fds_written.borrow_mut().push(owned_fds);
            }

            #[cfg(target_os = "linux")]
            if let Some(credentials) = credentials {
                // `PassedCredentials` doesn't implement `Clone`, so reconstruct from its fields.
                // This is fine for test-only code.
                self.credentials_written
                    .push(crate::connection::PassedCredentials::new(
                        credentials.unix_user_id(),
                        credentials.unix_primary_group_id(),
                        credentials.process_id(),
                    ));
            }
        }

        Ok(())
    }
}

/// Mock write half that asserts the expected write length.
///
/// This is useful for testing that writes are exactly the expected size.
/// In std mode, can also validate expected FD count.
#[derive(Debug)]
#[doc(hidden)]
pub struct TestWriteHalf {
    expected_len: usize,
    #[cfg(feature = "std")]
    expected_fd_count: Option<usize>,
    #[cfg(feature = "std")]
    write_count: usize,
}

impl TestWriteHalf {
    /// Create a new test write half that expects writes of the given length.
    #[cfg(not(feature = "std"))]
    pub fn new(expected_len: usize) -> Self {
        Self { expected_len }
    }

    /// Create a new test write half that expects writes of the given length (std version).
    #[cfg(feature = "std")]
    pub fn new(expected_len: usize) -> Self {
        Self {
            expected_len,
            expected_fd_count: None,
            write_count: 0,
        }
    }

    /// Create a new test write half expecting specific write length and FD count (std only).
    #[cfg(feature = "std")]
    pub fn new_with_fds(expected_len: usize, expected_fd_count: usize) -> Self {
        Self {
            expected_len,
            expected_fd_count: Some(expected_fd_count),
            write_count: 0,
        }
    }

    /// Get the number of writes performed (std only).
    #[cfg(feature = "std")]
    pub fn write_count(&self) -> usize {
        self.write_count
    }
}

impl WriteHalf for TestWriteHalf {
    async fn write(
        &mut self,
        buf: &[u8],
        #[cfg(feature = "std")] fds: &[impl std::os::fd::AsFd],
        #[cfg(all(feature = "std", target_os = "linux"))] _credentials: Option<
            &crate::connection::PassedCredentials,
        >,
    ) -> crate::Result<()> {
        assert_eq!(buf.len(), self.expected_len);

        #[cfg(feature = "std")]
        {
            let fd_count = fds.len();
            if let Some(expected_count) = self.expected_fd_count {
                assert_eq!(fd_count, expected_count);
            } else {
                assert_eq!(fd_count, 0, "Expected no FDs to be passed");
            }

            self.write_count += 1;
        }

        Ok(())
    }
}

/// Mock write half that counts the number of write operations.
///
/// This is useful for testing pipelining behavior or write frequency.
#[derive(Debug)]
#[doc(hidden)]
pub struct CountingWriteHalf {
    count: usize,
}

impl Default for CountingWriteHalf {
    fn default() -> Self {
        Self::new()
    }
}

impl CountingWriteHalf {
    /// Create a new counting write half.
    pub fn new() -> Self {
        Self { count: 0 }
    }

    /// Get the number of write operations that have been performed.
    pub fn count(&self) -> usize {
        self.count
    }
}

impl WriteHalf for CountingWriteHalf {
    async fn write(
        &mut self,
        _buf: &[u8],
        #[cfg(feature = "std")] _fds: &[impl std::os::fd::AsFd],
        #[cfg(all(feature = "std", target_os = "linux"))] _credentials: Option<
            &crate::connection::PassedCredentials,
        >,
    ) -> crate::Result<()> {
        self.count += 1;
        Ok(())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    #[tokio::test]
    async fn mock_socket_with_fds_basic() {
        use std::os::unix::net::UnixStream;

        let (r1, _w1) = UnixStream::pair().unwrap();
        let (r2, _w2) = UnixStream::pair().unwrap();

        let fds = vec![vec![r1.into()], vec![r2.into()]];
        let socket = MockSocket::new(&["test1", "test2"], fds);

        let (mut read, _write) = socket.split();

        let mut buf = [0u8; 10]; // Small buffer to force multiple reads
        let result = read.read(&mut buf).await.unwrap();
        assert!(result.bytes_read() > 0);
        assert_eq!(result.fds().len(), 1);

        let result = read.read(&mut buf).await.unwrap();
        assert!(result.bytes_read() > 0);
        assert_eq!(result.fds().len(), 1);
    }

    #[tokio::test]
    async fn mock_write_half_captures_fds() {
        use std::os::unix::net::UnixStream;

        let mut write = MockWriteHalf::new();

        let (r1, _w1) = UnixStream::pair().unwrap();
        let borrowed = r1.as_fd();

        write
            .write(
                b"test",
                &[borrowed],
                #[cfg(target_os = "linux")]
                None,
            )
            .await
            .unwrap();

        assert_eq!(write.written_data(), b"test");
        assert_eq!(write.fd_write_count(), 1);
        assert_eq!(write.fds_written().len(), 1);
        assert_eq!(write.fds_written()[0].len(), 1);
    }

    #[tokio::test]
    async fn test_write_half_validates_fd_count() {
        use std::os::unix::net::UnixStream;

        let mut write = TestWriteHalf::new_with_fds(4, 2);

        let (r1, _w1) = UnixStream::pair().unwrap();
        let (r2, _w2) = UnixStream::pair().unwrap();
        let borrowed = [r1.as_fd(), r2.as_fd()];

        write
            .write(
                b"test",
                &borrowed,
                #[cfg(target_os = "linux")]
                None,
            )
            .await
            .unwrap();

        assert_eq!(write.write_count(), 1);
    }

    #[tokio::test]
    #[should_panic(expected = "assertion `left == right` failed")]
    async fn test_write_half_panics_on_wrong_fd_count() {
        use std::os::unix::net::UnixStream;

        let mut write = TestWriteHalf::new_with_fds(4, 2);

        let (r1, _w1) = UnixStream::pair().unwrap();
        let borrowed = [r1.as_fd()];

        // This should panic because we expect 2 FDs but provide 1.
        write
            .write(
                b"test",
                &borrowed,
                #[cfg(target_os = "linux")]
                None,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn mock_read_half_multiple_fds_per_read() {
        use std::os::unix::net::UnixStream;

        let (r1, _w1) = UnixStream::pair().unwrap();
        let (r2, _w2) = UnixStream::pair().unwrap();
        let (r3, _w3) = UnixStream::pair().unwrap();

        let fds = vec![vec![r1.into(), r2.into(), r3.into()]];
        let socket = MockSocket::new(&["test"], fds);

        let (mut read, _write) = socket.split();

        let mut buf = [0u8; 1024];
        let result = read.read(&mut buf).await.unwrap();
        assert!(result.bytes_read() > 0);
        assert_eq!(result.fds().len(), 3);
    }

    #[tokio::test]
    async fn mock_read_half_mixed_fd_and_no_fd_reads() {
        use std::os::unix::net::UnixStream;

        let (r1, _w1) = UnixStream::pair().unwrap();

        let fds = vec![vec![r1.into()], vec![]];
        let socket = MockSocket::new(&["test1", "test2"], fds);

        let (mut read, _write) = socket.split();

        let mut buf = [0u8; 10]; // Small buffer to force multiple reads

        let result = read.read(&mut buf).await.unwrap();
        assert!(result.bytes_read() > 0);
        assert!(!result.fds().is_empty());

        let result = read.read(&mut buf).await.unwrap();
        assert!(result.bytes_read() > 0);
        assert!(result.fds().is_empty());
    }
}
