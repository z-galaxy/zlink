use zlink_macros::ReplyError;

#[derive(ReplyError, Debug, PartialEq)]
#[zlink(interface = "com.example.Test")]
enum TestError<'a> {
    NotFound,
    PermissionDenied,
    InvalidInput {
        field: &'a str,
        reason: &'a str,
    },
    Timeout {
        seconds: u32,
    },
    // Test variant with renamed fields
    RenamedFields {
        #[zlink(rename = "actualName")]
        _internal_name: &'a str,
        #[zlink(rename = "errorCode")]
        _code: i32,
        #[zlink(rename = "optionalData")]
        _optional: Option<&'a str>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_variant_serialization() {
        let error = TestError::NotFound;
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"error":"com.example.Test.NotFound"}"#);
    }

    #[test]
    fn named_variant_serialization() {
        let error = TestError::InvalidInput {
            field: "username",
            reason: "too short",
        };

        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""error":"com.example.Test.InvalidInput""#));
        assert!(json.contains(r#""field":"username""#));
        assert!(json.contains(r#""reason":"too short""#));
    }

    #[test]
    fn round_trip() {
        // Test with error that has parameters
        let original = TestError::InvalidInput {
            field: "password",
            reason: "missing special character",
        };

        round_trip_serialize(&original);

        // Test with error that has no parameters
        let original = TestError::NotFound;
        round_trip_serialize(&original);
    }

    #[test]
    fn renamed_fields_serialization() {
        // Test that renamed fields are serialized with their renamed names
        let error = TestError::RenamedFields {
            _internal_name: "test_value",
            _code: 42,
            _optional: Some("optional_value"),
        };

        let json = serde_json::to_string(&error).unwrap();

        // Check that the JSON contains the renamed field names, not the original ones
        assert!(json.contains(r#""actualName":"test_value""#));
        assert!(json.contains(r#""errorCode":42"#));
        assert!(json.contains(r#""optionalData":"optional_value""#));

        // Ensure original field names are NOT in the JSON
        assert!(!json.contains("_internal_name"));
        assert!(!json.contains("_code"));
        assert!(!json.contains("_optional"));
    }

    #[test]
    fn renamed_fields_deserialization() {
        // Test deserialization with renamed fields
        let json = r#"{"error":"com.example.Test.RenamedFields","parameters":{"actualName":"test_value","errorCode":42,"optionalData":"optional_value"}}"#;

        let deserialized: TestError = serde_json::from_str(json).unwrap();
        assert_eq!(
            deserialized,
            TestError::RenamedFields {
                _internal_name: "test_value",
                _code: 42,
                _optional: Some("optional_value"),
            }
        );

        // Test with optional field missing (should deserialize as None)
        let json_no_optional = r#"{"error":"com.example.Test.RenamedFields","parameters":{"actualName":"test_value","errorCode":42}}"#;

        let deserialized: TestError = serde_json::from_str(json_no_optional).unwrap();
        assert_eq!(
            deserialized,
            TestError::RenamedFields {
                _internal_name: "test_value",
                _code: 42,
                _optional: None,
            }
        );

        // Test that using original field names fails
        let json_with_original_names = r#"{"error":"com.example.Test.RenamedFields","parameters":{"_internal_name":"test_value","_code":42}}"#;
        let result: Result<TestError, _> = serde_json::from_str(json_with_original_names);
        assert!(result.is_err());
    }

    #[test]
    fn renamed_fields_round_trip() {
        // Test round-trip with all fields present
        let original = TestError::RenamedFields {
            _internal_name: "round_trip_test",
            _code: 999,
            _optional: Some("with_optional"),
        };
        round_trip_serialize(&original);

        // Test round-trip with optional field as None
        let original_no_optional = TestError::RenamedFields {
            _internal_name: "no_optional",
            _code: 123,
            _optional: None,
        };
        round_trip_serialize(&original_no_optional);
    }

    #[test]
    fn field_order_requirements_with_lifetimes() {
        // For enums with lifetimes, we require error before parameters
        let json_parameters_first = r#"{"parameters":{"field":"test","reason":"fail"},"error":"com.example.Test.InvalidInput"}"#;

        // Parameters-first JSON should fail to deserialize
        let result: Result<TestError, _> = serde_json::from_str(json_parameters_first);
        match result {
            Err(e) if e.is_data() => {
                // Expected - our custom deserializer error becomes a "data" error in serde_json
                // This confirms the field order validation is working
            }
            Err(_) => panic!("Expected a data error for field order violation"),
            Ok(_) => panic!("Expected deserialization to fail for parameters-first JSON"),
        }

        // But error before parameters works fine
        let json_error_first = r#"{"error":"com.example.Test.InvalidInput","parameters":{"field":"test","reason":"fail"}}"#;

        let result: Result<TestError, _> = serde_json::from_str(json_error_first);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            TestError::InvalidInput {
                field: "test",
                reason: "fail"
            }
        );
    }

    #[derive(ReplyError, Debug, PartialEq)]
    #[zlink(interface = "com.example.Owned")]
    enum OwnedError {
        NotFound,
        InvalidInput { field: String, reason: String },
    }

    #[test]
    fn field_order_requirements_without_lifetimes() {
        // For enums without lifetimes, we now also require error field first for simplicity
        let json_parameters_first = r#"{"parameters":{"field":"test","reason":"fail"},"error":"com.example.Owned.InvalidInput"}"#;

        // Parameters-first JSON should fail to deserialize
        let result: Result<OwnedError, _> = serde_json::from_str(json_parameters_first);
        match result {
            Err(e) if e.is_data() => {
                // Expected - our custom deserializer error becomes a "data" error in serde_json
                // This confirms the field order validation is working
            }
            Err(_) => panic!("Expected a data error for field order violation"),
            Ok(_) => panic!("Expected deserialization to fail for parameters-first JSON"),
        }

        // But error before parameters works fine
        let json_error_first = r#"{"error":"com.example.Owned.InvalidInput","parameters":{"field":"test","reason":"fail"}}"#;

        let result: Result<OwnedError, _> = serde_json::from_str(json_error_first);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            OwnedError::InvalidInput {
                field: "test".to_string(),
                reason: "fail".to_string()
            }
        );
    }

    // Helper function for round-trip serialization test, abstracting std vs nostd differences
    fn round_trip_serialize(original: &TestError) {
        let json = serde_json::to_string(original).unwrap();
        let deserialized: TestError = serde_json::from_str(&json).unwrap();
        assert_eq!(*original, deserialized);
    }
}
