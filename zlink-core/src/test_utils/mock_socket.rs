//! Mock socket implementations for testing.
//!
//! This module provides a full-featured mock socket implementation that can be
//! used in tests to simulate socket behavior without requiring actual network
//! connections.

use crate::connection::socket::{ReadHalf, Socket, WriteHalf};
use alloc::vec::Vec;

/// Mock socket implementation for testing.
///
/// This socket pre-loads response data and allows tests to verify what was written.
/// Responses should be provided as individual strings, and the mock will automatically
/// add null byte separators between them.
#[derive(Debug)]
#[doc(hidden)]
pub struct MockSocket {
    read_data: Vec<u8>,
    read_pos: usize,
}

impl MockSocket {
    /// Create a new mock socket with pre-configured responses.
    ///
    /// Each response string will be automatically null-terminated.
    /// An additional null byte is added at the end to mark the end of all messages.
    pub fn new(responses: &[&str]) -> Self {
        let mut data = Vec::new();

        for response in responses {
            data.extend_from_slice(response.as_bytes());
            data.push(b'\0');
        }
        // Add an extra null byte to mark end of all messages
        data.push(b'\0');

        Self {
            read_data: data,
            read_pos: 0,
        }
    }
}

impl Socket for MockSocket {
    type ReadHalf = MockReadHalf;
    type WriteHalf = MockWriteHalf;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        (
            MockReadHalf {
                data: self.read_data,
                pos: self.read_pos,
            },
            MockWriteHalf {
                written: Vec::new(),
            },
        )
    }
}

/// Mock read half implementation.
#[derive(Debug)]
#[doc(hidden)]
pub struct MockReadHalf {
    data: Vec<u8>,
    pos: usize,
}

impl MockReadHalf {
    /// Get the remaining unread data in the buffer.
    pub fn remaining_data(&self) -> &[u8] {
        &self.data[self.pos..]
    }
}

impl ReadHalf for MockReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> crate::Result<usize> {
        let remaining = self.data.len().saturating_sub(self.pos);
        if remaining == 0 {
            return Ok(0);
        }

        let to_read = remaining.min(buf.len());
        buf[..to_read].copy_from_slice(&self.data[self.pos..self.pos + to_read]);
        self.pos += to_read;
        Ok(to_read)
    }
}

/// Mock write half implementation.
#[derive(Debug)]
#[doc(hidden)]
pub struct MockWriteHalf {
    written: Vec<u8>,
}

impl MockWriteHalf {
    /// Get all data that has been written to this mock.
    pub fn written_data(&self) -> &[u8] {
        &self.written
    }
}

impl WriteHalf for MockWriteHalf {
    async fn write(&mut self, buf: &[u8]) -> crate::Result<()> {
        self.written.extend_from_slice(buf);
        Ok(())
    }
}

/// Mock write half that asserts the expected write length.
///
/// This is useful for testing that writes are exactly the expected size.
#[derive(Debug)]
#[doc(hidden)]
pub struct TestWriteHalf {
    expected_len: usize,
}

impl TestWriteHalf {
    /// Create a new test write half that expects writes of the given length.
    pub fn new(expected_len: usize) -> Self {
        Self { expected_len }
    }
}

impl WriteHalf for TestWriteHalf {
    async fn write(&mut self, buf: &[u8]) -> crate::Result<()> {
        assert_eq!(buf.len(), self.expected_len);
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
    async fn write(&mut self, _buf: &[u8]) -> crate::Result<()> {
        self.count += 1;
        Ok(())
    }
}
