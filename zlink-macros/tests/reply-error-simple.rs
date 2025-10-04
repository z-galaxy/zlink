use zlink_macros::ReplyError;

#[derive(ReplyError, Debug, PartialEq)]
#[zlink(interface = "com.example.Simple")]
enum SimpleError<'a> {
    NotFound,
    InvalidInput { reason: &'a str },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_serialization() {
        let error = SimpleError::NotFound;
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"error":"com.example.Simple.NotFound"}"#);
    }

    #[test]
    fn simple_named_variant() {
        let error = SimpleError::InvalidInput { reason: "test" };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""error":"com.example.Simple.InvalidInput""#));
        assert!(json.contains(r#""reason":"test""#));
    }

    #[test]
    fn round_trip() {
        // Test with error that has parameters
        let original = SimpleError::InvalidInput {
            reason: "test reason",
        };
        round_trip_serialize(&original);

        // Test with error that has no parameters
        let original = SimpleError::NotFound;
        round_trip_serialize(&original);
    }

    // Helper function for round-trip serialization test, abstracting std vs nostd differences
    fn round_trip_serialize(original: &SimpleError) {
        let json = serde_json::to_string(original).unwrap();
        let deserialized: SimpleError = serde_json::from_str(&json).unwrap();
        assert_eq!(*original, deserialized);
    }
}
