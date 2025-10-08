//! Contains connection related API.

use core::fmt::Debug;

use alloc::vec::Vec;
use serde::Serialize;

use super::{socket::WriteHalf, Call, Reply, BUFFER_SIZE};

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
    socket: Write,
    buffer: Vec<u8>,
    pos: usize,
    id: usize,
}

impl<Write: WriteHalf> WriteConnection<Write> {
    /// Create a new connection.
    pub(super) fn new(socket: Write, id: usize) -> Self {
        Self {
            socket,
            id,
            buffer: Vec::new(),
            pos: 0,
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
    pub async fn send_call<Method>(&mut self, call: &Call<Method>) -> crate::Result<()>
    where
        Method: Serialize + Debug,
    {
        trace!("connection {}: sending call: {:?}", self.id, call);
        self.write(call).await
    }

    /// Send a reply over the socket.
    ///
    /// The generic parameter `Params` is the type of the successful reply. This should be a type
    /// that can serialize itself as the `parameters` field of the reply.
    pub async fn send_reply<Params>(&mut self, reply: &Reply<Params>) -> crate::Result<()>
    where
        Params: Serialize + Debug,
    {
        trace!("connection {}: sending reply: {:?}", self.id, reply);
        self.write(reply).await
    }

    /// Send an error reply over the socket.
    ///
    /// The generic parameter `ReplyError` is the type of the error reply. This should be a type
    /// that can serialize itself to the whole reply object, containing `error` and `parameter`
    /// fields. This can be easily achieved using the `serde::Serialize` derive (See the code
    /// snippet in [`super::ReadConnection::receive_reply`] documentation for an example).
    pub async fn send_error<ReplyError>(&mut self, error: &ReplyError) -> crate::Result<()>
    where
        ReplyError: Serialize + Debug,
    {
        trace!("connection {}: sending error: {:?}", self.id, error);
        self.write(error).await
    }

    /// Enqueue a call to be sent over the socket.
    ///
    /// Similar to [`WriteConnection::send_call`], except that the call is not sent immediately but
    /// enqueued for later sending. This is useful when you want to send multiple calls in a
    /// batch.
    pub fn enqueue_call<Method>(&mut self, call: &Call<Method>) -> crate::Result<()>
    where
        Method: Serialize + Debug,
    {
        trace!("connection {}: enqueuing call: {:?}", self.id, call);
        self.enqueue(call)
    }

    /// Send out the enqueued calls.
    pub async fn flush(&mut self) -> crate::Result<()> {
        if self.pos == 0 {
            return Ok(());
        }

        trace!("connection {}: flushing {} bytes", self.id, self.pos);
        self.socket.write(&self.buffer[..self.pos]).await?;
        self.pos = 0;
        Ok(())
    }

    /// The underlying write half of the socket.
    pub fn write_half(&self) -> &Write {
        &self.socket
    }

    async fn write<T>(&mut self, value: &T) -> crate::Result<()>
    where
        T: Serialize + ?Sized + Debug,
    {
        self.enqueue(value)?;
        self.flush().await
    }

    fn enqueue<T>(&mut self, value: &T) -> crate::Result<()>
    where
        T: Serialize + ?Sized + Debug,
    {
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
        Ok(())
    }

    fn grow_buffer(&mut self) -> crate::Result<()> {
        if self.buffer.len() >= super::MAX_BUFFER_SIZE {
            return Err(crate::Error::BufferOverflow);
        }

        self.buffer.extend_from_slice(&[0; BUFFER_SIZE]);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::mock_socket::TestWriteHalf;

    #[tokio::test]
    async fn write() {
        const WRITE_LEN: usize =
            // Every `0u8` is one byte.
            BUFFER_SIZE +
            // `,` separators.
            (BUFFER_SIZE - 1) +
            // `[` and `]`.
            2 +
            // null byte from enqueue.
            1;
        let mut write_conn = WriteConnection::new(TestWriteHalf::new(WRITE_LEN), 1);
        // An item that serializes into `> BUFFER_SIZE * 2` bytes.
        let item: Vec<u8> = vec![0u8; BUFFER_SIZE];
        write_conn.write(&item).await.unwrap();
        assert_eq!(write_conn.buffer.len(), BUFFER_SIZE * 3);
        assert_eq!(write_conn.pos, 0); // Reset after flush.
    }

    #[tokio::test]
    async fn enqueue_and_flush() {
        // Test enqueuing multiple small items.
        let mut write_conn = WriteConnection::new(TestWriteHalf::new(5), 1); // "42\03\0"

        write_conn.enqueue(&42u32).unwrap();
        write_conn.enqueue(&3u32).unwrap();
        assert_eq!(write_conn.pos, 5); // "42\03\0"

        write_conn.flush().await.unwrap();
        assert_eq!(write_conn.pos, 0); // Reset after flush.
    }

    #[tokio::test]
    async fn enqueue_null_terminators() {
        // Test that null terminators are properly placed.
        let mut write_conn = WriteConnection::new(TestWriteHalf::new(4), 1); // "1\02\0"

        write_conn.enqueue(&1u32).unwrap();
        assert_eq!(write_conn.buffer[write_conn.pos - 1], b'\0');

        write_conn.enqueue(&2u32).unwrap();
        assert_eq!(write_conn.buffer[write_conn.pos - 1], b'\0');

        write_conn.flush().await.unwrap();
    }

    #[tokio::test]
    async fn enqueue_buffer_extension() {
        // Test buffer extension when enqueuing large items.
        let mut write_conn = WriteConnection::new(TestWriteHalf::new(0), 1);
        let initial_len = write_conn.buffer.len();

        // Fill up the buffer.
        let large_item: Vec<u8> = vec![0u8; BUFFER_SIZE];
        write_conn.enqueue(&large_item).unwrap();

        assert!(write_conn.buffer.len() > initial_len);
    }

    #[tokio::test]
    async fn flush_empty_buffer() {
        // Test that flushing an empty buffer is a no-op.
        let mut write_conn = WriteConnection::new(TestWriteHalf::new(0), 1);

        // Should not call write since buffer is empty.
        write_conn.flush().await.unwrap();
        assert_eq!(write_conn.pos, 0);
    }

    #[tokio::test]
    async fn multiple_flushes() {
        // Test multiple flushes in a row.
        let mut write_conn = WriteConnection::new(TestWriteHalf::new(2), 1); // "1\0"

        write_conn.enqueue(&1u32).unwrap();
        write_conn.flush().await.unwrap();
        assert_eq!(write_conn.pos, 0);

        // Second flush should be a no-op.
        write_conn.flush().await.unwrap();
        assert_eq!(write_conn.pos, 0);
    }

    #[tokio::test]
    async fn enqueue_after_flush() {
        // Test that enqueuing works properly after a flush.
        let mut write_conn = WriteConnection::new(TestWriteHalf::new(2), 1); // "2\0"

        write_conn.enqueue(&1u32).unwrap();
        write_conn.flush().await.unwrap();

        // Should be able to enqueue again after flush.
        write_conn.enqueue(&2u32).unwrap();
        assert_eq!(write_conn.pos, 2); // "2\0"

        write_conn.flush().await.unwrap();
        assert_eq!(write_conn.pos, 0);
    }

    #[tokio::test]
    async fn call_pipelining() {
        use super::super::Call;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Serialize, Deserialize)]
        struct TestMethod {
            name: &'static str,
            value: u32,
        }

        let mut write_conn = WriteConnection::new(TestWriteHalf::new(0), 1);

        // Test pipelining multiple method calls.
        let call1 = Call::new(TestMethod {
            name: "method1",
            value: 1,
        });
        write_conn.enqueue_call(&call1).unwrap();

        let call2 = Call::new(TestMethod {
            name: "method2",
            value: 2,
        });
        write_conn.enqueue_call(&call2).unwrap();

        let call3 = Call::new(TestMethod {
            name: "method3",
            value: 3,
        });
        write_conn.enqueue_call(&call3).unwrap();

        assert!(write_conn.pos > 0);

        // Verify that all calls are properly queued with null terminators.
        let buffer = &write_conn.buffer[..write_conn.pos];
        let mut null_positions = [0usize; 3];
        let mut null_count = 0;

        for (i, &byte) in buffer.iter().enumerate() {
            if byte == b'\0' {
                assert!(null_count < 3, "Found more than 3 null terminators");
                null_positions[null_count] = i;
                null_count += 1;
            }
        }

        // Should have exactly 3 null terminators for 3 calls.
        assert_eq!(null_count, 3);

        // Verify each null terminator is at the end of a complete JSON object.
        for i in 0..null_count {
            let pos = null_positions[i];
            assert!(
                pos > 0,
                "Null terminator at position {pos} should not be at start"
            );
            let preceding_byte = buffer[pos - 1];
            assert!(
                preceding_byte == b'}' || preceding_byte == b'"' || preceding_byte.is_ascii_digit(),
                "Null terminator at position {pos} should be after valid JSON ending, found byte: {preceding_byte}"
            );
        }

        // Verify the last null terminator is at the very end.
        assert_eq!(null_positions[2], write_conn.pos - 1);
    }

    #[tokio::test]
    async fn pipelining_vs_individual_sends() {
        use super::super::Call;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Serialize, Deserialize)]
        struct TestMethod {
            operation: &'static str,
            id: u32,
        }

        // Use consolidated counting write half from test_utils.
        use crate::test_utils::mock_socket::CountingWriteHalf;

        // Test individual sends (3 write calls expected).
        let counting_write = CountingWriteHalf::new();
        let mut write_conn_individual = WriteConnection::new(counting_write, 1);

        for i in 1..=3 {
            let call = Call::new(TestMethod {
                operation: "fetch",
                id: i,
            });
            write_conn_individual.send_call(&call).await.unwrap();
        }
        assert_eq!(write_conn_individual.socket.count(), 3);

        // Test pipelined sends (1 write call expected).
        let counting_write = CountingWriteHalf::new();
        let mut write_conn_pipelined = WriteConnection::new(counting_write, 2);

        for i in 1..=3 {
            let call = Call::new(TestMethod {
                operation: "fetch",
                id: i,
            });
            write_conn_pipelined.enqueue_call(&call).unwrap();
        }
        write_conn_pipelined.flush().await.unwrap();
        assert_eq!(write_conn_pipelined.socket.count(), 1);
    }
}
