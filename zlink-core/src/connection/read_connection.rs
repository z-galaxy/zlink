//! Contains connection related API.

use core::{fmt::Debug, str::from_utf8_unchecked};

use crate::{varlink_service, Result};

use super::{
    reply::{self, Reply},
    socket::ReadHalf,
    Call, BUFFER_SIZE, MAX_BUFFER_SIZE,
};
use alloc::vec::Vec;
use serde::Deserialize;
use serde_json::Deserializer;

/// A connection that can only be used for reading.
///
/// # Cancel safety
///
/// All async methods of this type are cancel safe unless explicitly stated otherwise in its
/// documentation.
#[derive(Debug)]
pub struct ReadConnection<Read: ReadHalf> {
    socket: Read,
    read_pos: usize,
    msg_pos: usize,
    buffer: Vec<u8>,
    id: usize,
}

impl<Read: ReadHalf> ReadConnection<Read> {
    /// Create a new connection.
    pub(super) fn new(socket: Read, id: usize) -> Self {
        Self {
            socket,
            read_pos: 0,
            msg_pos: 0,
            id,
            buffer: alloc::vec![0; BUFFER_SIZE],
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
    pub async fn receive_reply<'r, ReplyParams, ReplyError>(
        &'r mut self,
    ) -> Result<reply::Result<ReplyParams, ReplyError>>
    where
        ReplyParams: Deserialize<'r> + Debug,
        ReplyError: Deserialize<'r> + Debug,
    {
        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum ReplyMsg<ReplyParams, ReplyError> {
            Varlink(varlink_service::Error),
            Error(ReplyError),
            Reply(Reply<ReplyParams>),
        }

        match self
            .read_message::<ReplyMsg<ReplyParams, ReplyError>>()
            .await?
        {
            // Varlink service interface error need to be returned as the top-level error.
            ReplyMsg::Varlink(e) => Err(crate::Error::VarlinkService(e)),
            ReplyMsg::Error(e) => Ok(Err(e)),
            ReplyMsg::Reply(reply) => {
                // It's a success response.
                Ok(Ok(reply))
            }
        }
    }

    /// Receive a method call over the socket.
    ///
    /// The generic `Method` is the type of the method name and its input parameters. This should be
    /// a type that can deserialize itself from a complete method call message, i-e an object
    /// containing `method` and `parameter` fields. This can be easily achieved using the
    /// `serde::Deserialize` derive (See the code snippet in [`super::WriteConnection::send_call`]
    /// documentation for an example).
    pub async fn receive_call<'m, Method>(&'m mut self) -> Result<Call<Method>>
    where
        Method: Deserialize<'m> + Debug,
    {
        self.read_message::<Call<Method>>().await
    }

    // Reads at least one full message from the socket and return a single message bytes.
    async fn read_message<'m, M>(&'m mut self) -> Result<M>
    where
        M: Deserialize<'m> + Debug,
    {
        self.read_from_socket().await?;

        let mut stream = Deserializer::from_slice(&self.buffer[self.msg_pos..]).into_iter::<M>();
        let msg = stream.next();
        let null_index = self.msg_pos + stream.byte_offset();
        let buffer = &self.buffer[self.msg_pos..null_index];
        if self.buffer[null_index + 1] == b'\0' {
            // This means we're reading the last message and can now reset the indices.
            self.read_pos = 0;
            self.msg_pos = 0;
        } else {
            self.msg_pos = null_index + 1;
        }

        match msg {
            Some(Ok(msg)) => {
                // SAFETY: Since the parsing from JSON already succeeded, we can be sure that the
                // buffer contains a valid UTF-8 string.
                trace!("connection {}: received a message: {}", self.id, unsafe {
                    from_utf8_unchecked(buffer)
                });
                Ok(msg)
            }
            Some(Err(e)) => Err(e.into()),
            None => Err(crate::Error::UnexpectedEof),
        }
    }

    // Reads at least one full message from the socket.
    async fn read_from_socket(&mut self) -> Result<()> {
        if self.msg_pos > 0 {
            // This means we already have at least one message in the buffer so no need to read.
            return Ok(());
        }

        loop {
            let bytes_read = self.socket.read(&mut self.buffer[self.read_pos..]).await?;
            if bytes_read == 0 {
                return Err(crate::Error::UnexpectedEof);
            }
            self.read_pos += bytes_read;

            if self.read_pos == self.buffer.len() {
                if self.read_pos >= MAX_BUFFER_SIZE {
                    return Err(crate::Error::BufferOverflow);
                }

                self.buffer.extend(core::iter::repeat_n(0, BUFFER_SIZE));
            }

            // This marks end of all messages. After this loop is finished, we'll have 2 consecutive
            // null bytes at the end. This is then used by the callers to determine that they've
            // read all messages and can now reset the `read_pos`.
            self.buffer[self.read_pos] = b'\0';

            if self.buffer[self.read_pos - 1] == b'\0' {
                // One or more full messages were read.
                break;
            }
        }

        Ok(())
    }

    /// The underlying read half of the socket.
    pub fn read_half(&self) -> &Read {
        &self.socket
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{connection::socket::Socket, test_utils::mock_socket::MockSocket};
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
        // This tests that the deserializer handles large strings without
        // panicking and returns proper errors.
        let mut malicious = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
        // Create a very long string (10MB worth of 'A's).
        malicious.push_str(&"A".repeat(10 * 1024 * 1024));
        malicious.push_str(r#"}}"#);

        let socket = MockSocket::new(&[&malicious]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        // Should fail with JSON deserialization error, not panic.
        assert!(matches!(result, Err(crate::Error::Json(_))));
    }

    #[tokio::test]
    async fn malformed_json_deeply_nested_objects() {
        // Create deeply nested JSON objects to test for stack overflow or
        // excessive memory usage in the deserializer.
        let mut malicious = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
        // Add 10000 nested objects.
        for _ in 0..10000 {
            malicious.push_str(r#"{"a":"#);
        }
        malicious.push_str("0");
        for _ in 0..10000 {
            malicious.push('}');
        }
        malicious.push_str(r#"}}"#);

        let socket = MockSocket::new(&[&malicious]);
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
        malicious.push_str("0");
        for _ in 0..10000 {
            malicious.push(']');
        }
        malicious.push_str(r#"}}"#);

        let socket = MockSocket::new(&[&malicious]);
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

        let socket = MockSocket::new(&[malicious]);
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

        let socket = MockSocket::new(&[malicious]);
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
        // Test invalid escape sequences that might cause excessive
        // backtracking.
        let malicious = r#"{"method":"org.example.Test","parameters":{"value":"\x\x\x\x\x\x"}}"#;

        let socket = MockSocket::new(&[malicious]);
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

        let socket = MockSocket::new(&[malicious]);
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

        let socket = MockSocket::new(&[&large_msg]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        // This should fail with Json error due to type mismatch, but
        // shouldn't panic.
        let result = conn.receive_call::<TestMethod>().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn buffer_overflow_exceeds_max_size() {
        // Test a message that definitely exceeds MAX_BUFFER_SIZE.
        // Since MockSocket reads all data at once, we need to simulate
        // multiple reads that would exceed the limit.
        let size = 50 * 1024 * 1024; // 50MB chunk.
        let chunk = "X".repeat(size);
        let mut large_msg = r#"{"method":"org.example.Test","parameters":{"value":"#.to_string();
        large_msg.push_str(&chunk);
        large_msg.push_str(&chunk);
        large_msg.push_str(&chunk); // Total > 150MB.
        large_msg.push_str(r#"}}"#);

        let socket = MockSocket::new(&[&large_msg]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        // Should return BufferOverflow error.
        assert!(matches!(result, Err(crate::Error::BufferOverflow)));
    }

    #[tokio::test]
    async fn missing_null_terminator() {
        // Test message without proper null byte termination.
        // MockSocket automatically adds null bytes, so we need to test the
        // read_from_socket logic indirectly by ensuring empty stream case.
        let valid_msg = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
        let socket = MockSocket::new(&[valid_msg]);
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
        // Test message with embedded null bytes (which shouldn't happen in
        // valid JSON).
        let malicious = "{\0\"method\":\"org.example.Test\"\0}";
        let socket = MockSocket::new(&[malicious]);
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
        // Test the None case from StreamDeserializer when buffer is
        // effectively empty. Note: read_from_socket will return UnexpectedEof
        // before we even get to the deserializer since the socket returns 0
        // bytes on first read.
        let malicious = "";
        let socket = MockSocket::new(&[malicious]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        // MockSocket with empty string results in immediate EOF from socket.
        // This will trigger UnexpectedEof in read_from_socket, which happens
        // before deserialization.
        assert!(result.is_err());
        // The actual error could be UnexpectedEof from socket or from
        // deserializer.
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
        let socket = MockSocket::new(&[malicious]);
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

        let socket = MockSocket::new(&[&malicious]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        // Should either succeed or fail gracefully.
        // The TestMethod type expects only a "value" field, so this will
        // likely fail during deserialization to TestMethod.
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

        let socket = MockSocket::new(&[&malicious]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        // Should fail with type error but not panic.
        assert!(matches!(result, Err(crate::Error::Json(_))));
    }

    #[tokio::test]
    async fn byte_offset_calculation_with_valid_json() {
        // Verify that stream.byte_offset() correctly identifies the end of
        // valid JSON.
        let valid_msg = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
        let socket = MockSocket::new(&[valid_msg]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        assert!(result.is_ok());
        let call = result.unwrap();
        assert_eq!(call.method, TestMethod::Test { value: 42 });
    }

    #[tokio::test]
    async fn byte_offset_calculation_with_trailing_data() {
        // Test that byte_offset correctly handles when there's trailing
        // data after valid JSON.
        let valid_msg1 = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
        let valid_msg2 = r#"{"method":"org.example.Test","parameters":{"value":99}}"#;
        let socket = MockSocket::new(&[valid_msg1, valid_msg2]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        // First message.
        let result = conn.receive_call::<TestMethod>().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().method, TestMethod::Test { value: 42 });

        // Second message.
        let result = conn.receive_call::<TestMethod>().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().method, TestMethod::Test { value: 99 });
    }

    #[tokio::test]
    async fn error_propagation_json_to_error() {
        // Verify JSON deserialization errors are properly converted to
        // crate::Error::Json.
        let invalid_json = r#"{"method":"org.example.Test","parameters":{"value":"not_a_number"}}"#;
        let socket = MockSocket::new(&[invalid_json]);
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
        let socket = MockSocket::new(&[malformed]);
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
        let socket = MockSocket::new(&[&reply]);
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
        let socket = MockSocket::new(&[malicious]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        assert!(matches!(result, Err(crate::Error::Json(_))));
    }

    #[tokio::test]
    async fn malformed_json_number_overflow() {
        // Test with extremely large numbers that might cause overflow.
        let malicious = r#"{"method":"org.example.Test","parameters":{"value":999999999999999999999999999999999999999999999999999}}"#;
        let socket = MockSocket::new(&[malicious]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        // Should fail with JSON deserialization error.
        assert!(matches!(result, Err(crate::Error::Json(_))));
    }

    #[tokio::test]
    async fn malformed_json_duplicate_keys() {
        // Test JSON with duplicate keys in the same object.
        let malicious = r#"{"method":"org.example.Test","method":"org.example.Other","parameters":{"value":42}}"#;
        let socket = MockSocket::new(&[malicious]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        // serde_json typically accepts the last duplicate key, so this
        // might succeed or fail depending on the "Other" method name.
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

        let socket = MockSocket::new(&[&malicious]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        let result = conn.receive_call::<TestMethod>().await;
        // Should handle unicode escapes gracefully.
        assert!(result.is_err()); // Will fail type checking.
    }

    #[tokio::test]
    async fn partial_message_at_buffer_boundary() {
        // Test handling of partial messages that arrive at buffer
        // boundaries. MockSocket reads everything at once, so we test the
        // logic by ensuring the connection properly handles the end of data.
        let valid_msg = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
        let socket = MockSocket::new(&[valid_msg]);
        let (read, _write) = socket.split();
        let mut conn = ReadConnection::new(read, 0);

        // First read succeeds.
        assert!(conn.receive_call::<TestMethod>().await.is_ok());

        // Second read should fail with UnexpectedEof.
        let result = conn.receive_call::<TestMethod>().await;
        assert!(matches!(result, Err(crate::Error::UnexpectedEof)));
    }
}
