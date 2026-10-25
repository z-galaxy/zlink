#![cfg(test)]

use crate::{
    connection::{
        read_connection::ReadConnection,
        socket::{ReadHalf, ReadResult, Socket},
    },
    test_utils::mock_socket::MockSocket,
};
use serde::{Deserialize, Serialize};

// Test method and reply types for deserialization testing.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "parameters")]
enum TestMethod {
    #[serde(rename = "org.example.Test")]
    Test { value: u32 },
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct TestReply {
    result: String,
}

// DOS Attack Protection Tests.

#[tokio::test]
async fn malformed_json_extremely_long_string() {
    // Create a malicious JSON with a string close to MAX_BUFFER_SIZE.
    // This tests that the deserializer handles large strings without panicking and returns proper
    // errors.
    let mut malicious = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
    // Create a very long string (10MB worth of 'A's).
    malicious.push_str(&"A".repeat(10 * 1024 * 1024));
    malicious.push_str(r#"}}"#);

    let socket = MockSocket::with_responses(&[&malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should fail with JSON deserialization error, not panic.
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn malformed_json_deeply_nested_objects() {
    // Create deeply nested JSON objects to test for stack overflow or excessive memory usage in
    // the deserializer.
    let mut malicious = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
    // Add 10000 nested objects.
    for _ in 0..10000 {
        malicious.push_str(r#"{"a":"#);
    }
    malicious.push('0');
    for _ in 0..10000 {
        malicious.push('}');
    }
    malicious.push_str(r#"}}"#);

    let socket = MockSocket::with_responses(&[&malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should fail gracefully, not cause stack overflow.
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn malformed_json_deeply_nested_arrays() {
    // Similar to nested objects but with arrays.
    let mut malicious = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
    // Add 10000 nested arrays.
    for _ in 0..10000 {
        malicious.push('[');
    }
    malicious.push('0');
    for _ in 0..10000 {
        malicious.push(']');
    }
    malicious.push_str(r#"}}"#);

    let socket = MockSocket::with_responses(&[&malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should fail gracefully.
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn malformed_json_unterminated_string() {
    // Test unterminated string in JSON.
    let malicious = r#"{"method":"org.example.Test","parameters":{"value":"#;

    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should return UnexpectedEof or Json error.
    assert!(matches!(
        result,
        Err(crate::Error::Json(_)) | Err(crate::Error::UnexpectedEof)
    ));
}

#[tokio::test]
async fn malformed_json_unterminated_object() {
    // Test unterminated JSON object.
    let malicious = r#"{"method":"org.example.Test","parameters":{"value":42"#;

    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(
        result,
        Err(crate::Error::Json(_)) | Err(crate::Error::UnexpectedEof)
    ));
}

#[tokio::test]
async fn malformed_json_invalid_escape_sequences() {
    // Test invalid escape sequences that might cause excessive backtracking.
    let malicious = r#"{"method":"org.example.Test","parameters":{"value":"\x\x\x\x\x\x"}}"#;

    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn malformed_json_invalid_utf8_sequences() {
    // Test invalid UTF-8 sequences in JSON string.
    // Note: JSON requires valid UTF-8, so this should fail.
    // We use escaped form since MockSocket::new requires &str.
    // Using invalid escape sequences will trigger JSON parsing errors.
    let malicious = r#"{"method":"org.example.Test","parameters":{"value":"\xFF\xFE\xFD"}}"#;

    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should fail with JSON error due to invalid escape sequences.
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn buffer_overflow_near_max_size() {
    // Test a message with a large payload to verify buffer handling.
    // Using 10MB to keep test fast while still testing large messages.
    let size = 10 * 1024 * 1024;
    let mut large_msg = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
    large_msg.push_str(&"X".repeat(size));
    large_msg.push_str(r#"}}"#);

    let socket = MockSocket::with_responses(&[&large_msg]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    // This should fail with Json error due to type mismatch, but shouldn't panic.
    let result = conn.receive_call::<TestMethod>().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn buffer_overflow_exceeds_max_size() {
    // Test a message that definitely exceeds MAX_BUFFER_SIZE.
    // Since MockSocket reads all data at once, we need to simulate multiple reads that would
    // exceed the limit.
    let size = 50 * 1024 * 1024; // 50MB chunk.
    let chunk = "X".repeat(size);
    let mut large_msg = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
    large_msg.push_str(&chunk);
    large_msg.push_str(&chunk);
    large_msg.push_str(&chunk); // Total > 150MB.
    large_msg.push_str(r#"}}"#);

    let socket = MockSocket::with_responses(&[&large_msg]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should return BufferOverflow error.
    assert!(matches!(result, Err(crate::Error::BufferOverflow)));
}

#[tokio::test]
async fn missing_null_terminator() {
    // Test message without proper null byte termination.
    // MockSocket automatically adds null bytes, so we need to test the read_from_socket logic
    // indirectly by ensuring empty stream case.
    let valid_msg = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let socket = MockSocket::with_responses(&[valid_msg]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    // First read should succeed.
    let result = conn.receive_call::<TestMethod>().await;
    assert!(result.is_ok());

    // Second read should fail with UnexpectedEof.
    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::UnexpectedEof)));
}

#[tokio::test]
async fn multiple_null_bytes_in_message() {
    // Test message with embedded null bytes (which shouldn't happen in valid JSON).
    let malicious = "{\0\"method\":\"org.example.Test\"\0}";
    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should fail due to invalid JSON structure.
    assert!(matches!(
        result,
        Err(crate::Error::Json(_)) | Err(crate::Error::UnexpectedEof)
    ));
}

#[tokio::test]
async fn stream_deserializer_empty_buffer() {
    // Test the None case from StreamDeserializer when buffer is effectively empty.
    // Note: read_from_socket will return UnexpectedEof before we even get to the deserializer
    // since the socket returns 0 bytes on first read.
    let malicious = "";
    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // MockSocket with empty string results in immediate EOF from socket.
    // This will trigger UnexpectedEof in read_from_socket, which happens before deserialization.
    assert!(result.is_err());
    // The actual error could be UnexpectedEof from socket or from deserializer.
    match result {
        Err(crate::Error::UnexpectedEof) => {}
        Err(crate::Error::Json(_)) => {}
        other => panic!("Expected error, got {:?}", other),
    }
}

#[tokio::test]
async fn stream_deserializer_whitespace_only() {
    // Test StreamDeserializer with only whitespace.
    let malicious = "     \n\n\t\t   ";
    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should return UnexpectedEof or Json error.
    assert!(matches!(
        result,
        Err(crate::Error::Json(_)) | Err(crate::Error::UnexpectedEof)
    ));
}

#[tokio::test]
async fn malformed_json_repeated_keys() {
    // Test JSON with many repeated keys to stress the parser.
    let mut malicious = r#"{"method":"org.example.Test","parameters":{"#.to_string();
    for i in 0..10000 {
        malicious.push_str(&format!(r#""key{i}":"value{i}","#));
    }
    malicious.push_str(r#""value":42}}"#);

    let socket = MockSocket::with_responses(&[&malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should either succeed or fail gracefully.
    // The TestMethod type expects only a "value" field, so this will likely fail during
    // deserialization to TestMethod.
    // But it shouldn't panic.
    if let Err(e) = result {
        assert!(matches!(e, crate::Error::Json(_)));
    }
}

#[tokio::test]
async fn malformed_json_very_long_array() {
    // Test very long array in parameters.
    let mut malicious = r#"{"method":"org.example.Test","parameters":{"value":["#.to_string();
    for i in 0..100000 {
        if i > 0 {
            malicious.push(',');
        }
        malicious.push_str(&i.to_string());
    }
    malicious.push_str(r#"]}}"#);

    let socket = MockSocket::with_responses(&[&malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should fail with type error but not panic.
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn byte_offset_calculation_with_valid_json() {
    // Verify that stream.byte_offset() correctly identifies the end of valid JSON.
    let valid_msg = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let socket = MockSocket::with_responses(&[valid_msg]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    assert!(result.is_ok());
    #[cfg(feature = "std")]
    let (call, _fds) = result.unwrap();
    #[cfg(not(feature = "std"))]
    let call = result.unwrap();
    assert_eq!(call.method, TestMethod::Test { value: 42 });
}

#[tokio::test]
async fn byte_offset_calculation_with_trailing_data() {
    // Test that byte_offset correctly handles when there's trailing data after valid JSON.
    let valid_msg1 = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let valid_msg2 = r#"{"method":"org.example.Test","parameters":{"value":99}}"#;
    let socket = MockSocket::with_responses(&[valid_msg1, valid_msg2]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    // First message.
    let result = conn.receive_call::<TestMethod>().await;
    assert!(result.is_ok());
    #[cfg(feature = "std")]
    let (call, _fds) = result.unwrap();
    #[cfg(not(feature = "std"))]
    let call = result.unwrap();
    assert_eq!(call.method, TestMethod::Test { value: 42 });

    // Second message.
    let result = conn.receive_call::<TestMethod>().await;
    assert!(result.is_ok());
    #[cfg(feature = "std")]
    let (call, _fds) = result.unwrap();
    #[cfg(not(feature = "std"))]
    let call = result.unwrap();
    assert_eq!(call.method, TestMethod::Test { value: 99 });
}

#[tokio::test]
async fn error_propagation_json_to_error() {
    // Verify JSON deserialization errors are properly converted to crate::Error::Json.
    let invalid_json = r#"{"method":"org.example.Test","parameters":{"value":"not_a_number"}}"#;
    let socket = MockSocket::with_responses(&[invalid_json]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    match result {
        Err(crate::Error::Json(e)) => {
            // Verify it's a proper JSON error.
            assert!(!e.to_string().is_empty());
        }
        _ => panic!("Expected Error::Json, got {:?}", result),
    }
}

#[tokio::test]
async fn receive_reply_with_malformed_json() {
    // Test receive_reply with malformed JSON.
    let malformed = r#"{"parameters":{"result":"incomplete"#;
    let socket = MockSocket::with_responses(&[malformed]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_reply::<TestReply, TestReply>().await;
    assert!(matches!(
        result,
        Err(crate::Error::Json(_)) | Err(crate::Error::UnexpectedEof)
    ));
}

#[tokio::test]
async fn receive_reply_with_large_payload() {
    // Test receive_reply with very large but valid reply.
    let large_result = "X".repeat(1024 * 1024); // 1MB string.
    let reply = format!(r#"{{"parameters":{{"result":"{}"}}}}"#, large_result);
    let socket = MockSocket::with_responses(&[&reply]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_reply::<TestReply, TestReply>().await;
    // Should succeed and return the large payload.
    assert!(result.is_ok());
}

#[tokio::test]
async fn malformed_json_mixed_control_characters() {
    // Test JSON with unusual control characters.
    let malicious = "{\x01\"method\"\x02:\x03\"org.example.Test\"}";
    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn malformed_json_number_overflow() {
    // Test with extremely large numbers that might cause overflow.
    let malicious = r#"{"method":"org.example.Test","parameters":{"value":999999999999999999999999999999999999999999999999999}}"#;
    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should fail with JSON deserialization error.
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn malformed_json_duplicate_keys() {
    // Test JSON with duplicate keys in the same object.
    let malicious =
        r#"{"method":"org.example.Test","method":"org.example.Other","parameters":{"value":42}}"#;
    let socket = MockSocket::with_responses(&[malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // serde_json typically accepts the last duplicate key, so this might succeed or fail
    // depending on the "Other" method name.
    // The important part is it doesn't panic.
    let _ = result;
}

#[tokio::test]
async fn malformed_json_unicode_escapes() {
    // Test excessive unicode escape sequences.
    let mut malicious = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
    for _ in 0..10000 {
        malicious.push_str(r#"\u0041"#); // 'A' in unicode.
    }
    malicious.push_str(r#"}}"#);

    let socket = MockSocket::with_responses(&[&malicious]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    // Should handle unicode escapes gracefully.
    assert!(result.is_err()); // Will fail type checking.
}

#[tokio::test]
async fn partial_message_at_buffer_boundary() {
    // Test handling of partial messages that arrive at buffer boundaries.
    // MockSocket reads everything at once, so we test the logic by ensuring the connection
    // properly handles the end of data.
    let valid_msg = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let socket = MockSocket::with_responses(&[valid_msg]);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    // First read succeeds.
    assert!(conn.receive_call::<TestMethod>().await.is_ok());

    // Second read should fail with UnexpectedEof.
    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::UnexpectedEof)));
}

#[tokio::test]
async fn message_before_incomplete_one() {
    // A complete message is received without waiting for the rest of the one after it.
    let call2 = call(2);
    let first_read = [call(1), call2[..10].to_vec()].concat();
    let reads: [&[u8]; 2] = [&first_read, &call2[10..]];
    let socket = MockSocket::with_reads(
        &reads,
        #[cfg(feature = "std")]
        vec![],
    );
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await.unwrap();
    #[cfg(feature = "std")]
    let (call, _fds) = result;
    #[cfg(not(feature = "std"))]
    let call = result;
    assert_eq!(call.method, TestMethod::Test { value: 1 });
    // The second read wasn't needed.
    assert_eq!(conn.read_half().messages_remaining(), 1);

    let result = conn.receive_call::<TestMethod>().await.unwrap();
    #[cfg(feature = "std")]
    let (call, _fds) = result;
    #[cfg(not(feature = "std"))]
    let call = result;
    assert_eq!(call.method, TestMethod::Test { value: 2 });
    assert_eq!(conn.read_half().messages_remaining(), 0);
}

#[tokio::test]
async fn messages_split_across_reads() {
    // Messages are received no matter where the reads split them, including inside a NUL-less
    // part in the middle of one.
    let call3 = call(3);
    let reads: [&[u8]; 4] = [
        &[call(1), call(2), call3[..10].to_vec()].concat(),
        &call3[10..20],
        &[&call3[20..], &call(4)[..]].concat(),
        &call(5),
    ];
    let socket = MockSocket::with_reads(
        &reads,
        #[cfg(feature = "std")]
        vec![],
    );
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    for value in 1..=5 {
        let result = conn.receive_call::<TestMethod>().await.unwrap();
        #[cfg(feature = "std")]
        let (call, _fds) = result;
        #[cfg(not(feature = "std"))]
        let call = result;
        assert_eq!(call.method, TestMethod::Test { value });
    }
}

#[tokio::test]
async fn unterminated_message() {
    // A message without its NUL terminator isn't complete, however complete it looks.
    let message = [&call(1)[..], b"{\"method\":\"org.example.Test\""].concat();
    let reads: [&[u8]; 1] = [&message];
    let socket = MockSocket::with_reads(
        &reads,
        #[cfg(feature = "std")]
        vec![],
    );
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    assert!(conn.receive_call::<TestMethod>().await.is_ok());
    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::UnexpectedEof)));
}

#[tokio::test]
async fn malformed_message_is_skipped() {
    // A message that can't be deserialized doesn't affect the ones after it.
    let malformed = br#"{"method":"org.example.Test","parameters":{"value":"one"}}"#;
    let read = [&malformed[..], b"\0", &call(2)].concat();
    let reads: [&[u8]; 1] = [&read];
    let socket = MockSocket::with_reads(
        &reads,
        #[cfg(feature = "std")]
        vec![],
    );
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::Json(_))));

    let result = conn.receive_call::<TestMethod>().await.unwrap();
    #[cfg(feature = "std")]
    let (call, _fds) = result;
    #[cfg(not(feature = "std"))]
    let call = result;
    assert_eq!(call.method, TestMethod::Test { value: 2 });
}

#[tokio::test]
async fn whitespace_after_message() {
    let message = br#"{"method":"org.example.Test","parameters":{"value":1}}"#;
    let read = [&message[..], b" \r\n\t\0"].concat();
    let reads: [&[u8]; 1] = [&read];
    let socket = MockSocket::with_reads(
        &reads,
        #[cfg(feature = "std")]
        vec![],
    );
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await.unwrap();
    #[cfg(feature = "std")]
    let (call, _fds) = result;
    #[cfg(not(feature = "std"))]
    let call = result;
    assert_eq!(call.method, TestMethod::Test { value: 1 });
}

#[tokio::test]
async fn trailing_characters_after_message() {
    // Anything but whitespace between the end of the JSON value and the NUL is an error.
    let message = br#"{"method":"org.example.Test","parameters":{"value":1}}"#;
    let read = [&message[..], b" trailing\0", &call(2)].concat();
    let reads: [&[u8]; 1] = [&read];
    let socket = MockSocket::with_reads(
        &reads,
        #[cfg(feature = "std")]
        vec![],
    );
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::Json(_))));

    let result = conn.receive_call::<TestMethod>().await.unwrap();
    #[cfg(feature = "std")]
    let (call, _fds) = result;
    #[cfg(not(feature = "std"))]
    let call = result;
    assert_eq!(call.method, TestMethod::Test { value: 2 });
}

#[tokio::test]
async fn empty_message() {
    let read = [b"\0", &call(1)[..]].concat();
    let reads: [&[u8]; 1] = [&read];
    let socket = MockSocket::with_reads(
        &reads,
        #[cfg(feature = "std")]
        vec![],
    );
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::Json(_))));

    let result = conn.receive_call::<TestMethod>().await.unwrap();
    #[cfg(feature = "std")]
    let (call, _fds) = result;
    #[cfg(not(feature = "std"))]
    let call = result;
    assert_eq!(call.method, TestMethod::Test { value: 1 });
}

// Deserializing a message doesn't check that the values its type ignores are valid UTF-8, so a
// message can be received even if it isn't valid UTF-8. It must be logged nonetheless.
#[test_log::test(tokio::test)]
#[test_log(default_log_filter = "trace")]
async fn receive_message_with_invalid_utf8_in_ignored_value() {
    let msg =
        b"{\"method\":\"org.example.Test\",\"parameters\":{\"value\":42,\"note\":\"\xff\"}}\0";
    let mut conn = ReadConnection::new(BytesReadHalf(msg), 0);

    let result = conn.receive_call::<TestMethod>().await.unwrap();
    #[cfg(feature = "std")]
    let (call, _) = result;
    #[cfg(not(feature = "std"))]
    let call = result;
    assert_eq!(call.method(), &TestMethod::Test { value: 42 });
}

/// A call to `org.example.Test` with `value`, as it's sent over the socket.
fn call(value: u32) -> Vec<u8> {
    let mut call =
        format!(r#"{{"method":"org.example.Test","parameters":{{"value":{value}}}}}"#).into_bytes();
    call.push(b'\0');

    call
}

/// A read half returning the given bytes in a single read.
#[derive(Debug)]
struct BytesReadHalf(&'static [u8]);

impl ReadHalf for BytesReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> crate::Result<ReadResult> {
        let len = self.0.len();
        buf[..len].copy_from_slice(self.0);
        self.0 = &[];

        Ok(ReadResult::new(len))
    }
}
