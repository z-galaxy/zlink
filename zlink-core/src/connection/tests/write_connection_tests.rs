#![cfg(test)]

use crate::{
    connection::{BUFFER_SIZE, write_connection::WriteConnection},
    test_utils::mock_socket::TestWriteHalf,
};

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
    #[cfg(feature = "std")]
    write_conn.write(&item, vec![]).await.unwrap();
    #[cfg(not(feature = "std"))]
    write_conn.write(&item).await.unwrap();
    assert_eq!(write_conn.buffer.len(), BUFFER_SIZE * 3);
    assert_eq!(write_conn.pos, 0); // Reset after flush.
}

#[tokio::test]
async fn enqueue_and_flush() {
    // Test enqueuing multiple small items.
    let mut write_conn = WriteConnection::new(TestWriteHalf::new(5), 1); // "42\03\0"

    #[cfg(feature = "std")]
    {
        write_conn.enqueue(&42u32, vec![]).unwrap();
        write_conn.enqueue(&3u32, vec![]).unwrap();
    }
    #[cfg(not(feature = "std"))]
    {
        write_conn.enqueue(&42u32).unwrap();
        write_conn.enqueue(&3u32).unwrap();
    }
    assert_eq!(write_conn.pos, 5); // "42\03\0"

    write_conn.flush().await.unwrap();
    assert_eq!(write_conn.pos, 0); // Reset after flush.
}

#[tokio::test]
async fn enqueue_null_terminators() {
    // Test that null terminators are properly placed.
    let mut write_conn = WriteConnection::new(TestWriteHalf::new(4), 1); // "1\02\0"

    #[cfg(feature = "std")]
    {
        write_conn.enqueue(&1u32, vec![]).unwrap();
        assert_eq!(write_conn.buffer[write_conn.pos - 1], b'\0');

        write_conn.enqueue(&2u32, vec![]).unwrap();
        assert_eq!(write_conn.buffer[write_conn.pos - 1], b'\0');
    }
    #[cfg(not(feature = "std"))]
    {
        write_conn.enqueue(&1u32).unwrap();
        assert_eq!(write_conn.buffer[write_conn.pos - 1], b'\0');

        write_conn.enqueue(&2u32).unwrap();
        assert_eq!(write_conn.buffer[write_conn.pos - 1], b'\0');
    }

    write_conn.flush().await.unwrap();
}

#[tokio::test]
async fn enqueue_buffer_extension() {
    // Test buffer extension when enqueuing large items.
    let mut write_conn = WriteConnection::new(TestWriteHalf::new(0), 1);
    let initial_len = write_conn.buffer.len();

    // Fill up the buffer.
    let large_item: Vec<u8> = vec![0u8; BUFFER_SIZE];
    #[cfg(feature = "std")]
    write_conn.enqueue(&large_item, vec![]).unwrap();
    #[cfg(not(feature = "std"))]
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

    #[cfg(feature = "std")]
    write_conn.enqueue(&1u32, vec![]).unwrap();
    #[cfg(not(feature = "std"))]
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

    #[cfg(feature = "std")]
    write_conn.enqueue(&1u32, vec![]).unwrap();
    #[cfg(not(feature = "std"))]
    write_conn.enqueue(&1u32).unwrap();
    write_conn.flush().await.unwrap();

    // Should be able to enqueue again after flush.
    #[cfg(feature = "std")]
    write_conn.enqueue(&2u32, vec![]).unwrap();
    #[cfg(not(feature = "std"))]
    write_conn.enqueue(&2u32).unwrap();
    assert_eq!(write_conn.pos, 2); // "2\0"

    write_conn.flush().await.unwrap();
    assert_eq!(write_conn.pos, 0);
}

#[tokio::test]
async fn call_pipelining() {
    use super::super::super::Call;
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
    #[cfg(feature = "std")]
    write_conn.enqueue_call(&call1, vec![]).unwrap();
    #[cfg(not(feature = "std"))]
    write_conn.enqueue_call(&call1).unwrap();

    let call2 = Call::new(TestMethod {
        name: "method2",
        value: 2,
    });
    #[cfg(feature = "std")]
    write_conn.enqueue_call(&call2, vec![]).unwrap();
    #[cfg(not(feature = "std"))]
    write_conn.enqueue_call(&call2).unwrap();

    let call3 = Call::new(TestMethod {
        name: "method3",
        value: 3,
    });
    #[cfg(feature = "std")]
    write_conn.enqueue_call(&call3, vec![]).unwrap();
    #[cfg(not(feature = "std"))]
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
    for &pos in &null_positions[..null_count] {
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
    use super::super::super::Call;
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
        #[cfg(feature = "std")]
        write_conn_individual
            .send_call(&call, vec![])
            .await
            .unwrap();
        #[cfg(not(feature = "std"))]
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
        #[cfg(feature = "std")]
        write_conn_pipelined.enqueue_call(&call, vec![]).unwrap();
        #[cfg(not(feature = "std"))]
        write_conn_pipelined.enqueue_call(&call).unwrap();
    }
    write_conn_pipelined.flush().await.unwrap();
    assert_eq!(write_conn_pipelined.socket.count(), 1);
}

#[tokio::test]
async fn into_write_half() {
    use crate::test_utils::mock_socket::CountingWriteHalf;

    let mut write_conn = WriteConnection::new(CountingWriteHalf::new(), 1);
    #[cfg(feature = "std")]
    {
        write_conn.write(&42u32, vec![]).await.unwrap();
        write_conn.enqueue(&3u32, vec![]).unwrap();
    }
    #[cfg(not(feature = "std"))]
    {
        write_conn.write(&42u32).await.unwrap();
        write_conn.enqueue(&3u32).unwrap();
    }

    // The message that was enqueued but not flushed is dropped.
    let write = write_conn.into_write_half();
    assert_eq!(write.count(), 1);
}
