//! Unit tests for `Call` serialization and deserialization.

use super::Call;
use serde::{Deserialize, Serialize};

mod std {
    use serde_json::Value;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct ExtendedParams<'a> {
        #[serde(flatten)]
        middle: MiddleParams<'a>,
        metadata: &'a str,
        priority: u8,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct MiddleParams<'a> {
        #[serde(flatten)]
        base: BaseParams<'a>,
        category: &'a str,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct BaseParams<'a> {
        name: &'a str,
        value: i32,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "method", content = "parameters")]
    enum TestServiceMethods<'a> {
        #[serde(rename = "org.example.test.Simple")]
        Simple,
        #[serde(rename = "org.example.test.Method")]
        Method { name: &'a str, value: i32 },
        #[serde(rename = "org.example.test.GetInfo")]
        GetInfo { id: u32 },
        #[serde(rename = "org.example.test.Reset")]
        Reset,
        #[serde(rename = "org.example.test.WithFlattened")]
        WithFlattened(ExtendedParams<'a>),
    }

    #[test]
    fn serialize_call_with_method_only() {
        let method = TestServiceMethods::Method {
            name: "test",
            value: 42,
        };
        let call = Call::new(method);

        let json = serde_json::to_string(&call).unwrap();
        let expected =
            r#"{"method":"org.example.test.Method","parameters":{"name":"test","value":42}}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn serialize_call_with_oneway_true() {
        let method = TestServiceMethods::Simple;
        let call = Call::new(method).set_oneway(true);

        let json = serde_json::to_string(&call).unwrap();
        let expected = r#"{"method":"org.example.test.Simple","oneway":true}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn serialize_call_with_oneway_false() {
        let method = TestServiceMethods::Simple;
        let call = Call::new(method);

        let json = serde_json::to_string(&call).unwrap();
        let expected = r#"{"method":"org.example.test.Simple"}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn serialize_call_with_more_true() {
        let method = TestServiceMethods::Simple;
        let call = Call::new(method).set_more(true);

        let json = serde_json::to_string(&call).unwrap();
        let expected = r#"{"method":"org.example.test.Simple","more":true}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn serialize_call_with_upgrade_true() {
        let method = TestServiceMethods::Simple;
        let call = Call::new(method).set_upgrade(true);

        let json = serde_json::to_string(&call).unwrap();
        let expected = r#"{"method":"org.example.test.Simple","upgrade":true}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn serialize_call_with_all_flags() {
        let method = TestServiceMethods::Method {
            name: "test",
            value: 42,
        };
        let call = Call::new(method).set_oneway(true).set_upgrade(true);

        let json = serde_json::to_string(&call).unwrap();
        // Note: The order might vary, so we parse and check the structure.
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["method"], "org.example.test.Method");
        assert_eq!(parsed["parameters"]["name"], "test");
        assert_eq!(parsed["parameters"]["value"], 42);
        assert_eq!(parsed["oneway"], true);
        assert_eq!(parsed["more"], Value::Null);
        assert_eq!(parsed["upgrade"], true);
    }

    #[test]
    fn serialize_call_with_false_flags() {
        let method = TestServiceMethods::Simple;
        let call = Call::new(method)
            .set_oneway(false)
            .set_more(false)
            .set_upgrade(false);

        let json = serde_json::to_string(&call).unwrap();
        let expected = r#"{"method":"org.example.test.Simple"}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn deserialize_call_with_method_only() {
        let json =
            r#"{"method":"org.example.test.Method","parameters":{"name":"test","value":42}}"#;
        let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();

        match call.method() {
            TestServiceMethods::Method { name, value } => {
                assert_eq!(*name, "test");
                assert_eq!(*value, 42);
            }
            _ => panic!("Expected Method variant"),
        }
        assert!(!call.oneway());
        assert!(!call.more());
        assert!(!call.upgrade());
    }

    #[test]
    fn deserialize_call_with_oneway_true() {
        let json = r#"{"method":"org.example.test.Simple","oneway":true}"#;
        let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();

        assert!(matches!(call.method(), TestServiceMethods::Simple));
        assert!(call.oneway());
        assert!(!call.more());
        assert!(!call.upgrade());
    }

    #[test]
    fn deserialize_call_with_oneway_false() {
        let json = r#"{"method":"org.example.test.Simple","oneway":false}"#;
        let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();

        assert!(matches!(call.method(), TestServiceMethods::Simple));
        assert!(!call.oneway());
        assert!(!call.more());
        assert!(!call.upgrade());
    }

    #[test]
    fn deserialize_call_with_more_true() {
        let json = r#"{"method":"org.example.test.Simple","more":true}"#;
        let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();

        assert!(matches!(call.method(), TestServiceMethods::Simple));
        assert!(!call.oneway());
        assert!(call.more());
        assert!(!call.upgrade());
    }

    #[test]
    fn deserialize_call_with_upgrade_true() {
        let json = r#"{"method":"org.example.test.Simple","upgrade":true}"#;
        let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();

        assert!(matches!(call.method(), TestServiceMethods::Simple));
        assert!(!call.oneway());
        assert!(!call.more());
        assert!(call.upgrade());
    }

    #[test]
    fn deserialize_call_with_all_flags() {
        let json = r#"{"method":"org.example.test.Method","parameters":{"name":"test","value":42},"oneway":true,"more":false,"upgrade":true}"#;
        let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();

        match call.method() {
            TestServiceMethods::Method { name, value } => {
                assert_eq!(*name, "test");
                assert_eq!(*value, 42);
            }
            _ => panic!("Expected Method variant"),
        }
        assert!(call.oneway());
        assert!(!call.more());
        assert!(call.upgrade());
    }

    #[test]
    fn deserialize_call_with_extra_fields() {
        let json =
            r#"{"method":"org.example.test.Simple","extra":"ignored","oneway":true,"unknown":42}"#;
        let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();

        assert!(matches!(call.method(), TestServiceMethods::Simple));
        assert!(call.oneway());
        assert!(!call.more());
        assert!(!call.upgrade());
    }

    #[test]
    fn roundtrip_serialization() {
        let method = TestServiceMethods::Method {
            name: "roundtrip",
            value: 123,
        };
        let original = Call::new(method).set_more(true);

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Call<TestServiceMethods<'_>> = serde_json::from_str(&json).unwrap();

        match (original.method(), deserialized.method()) {
            (
                TestServiceMethods::Method {
                    name: name1,
                    value: value1,
                },
                TestServiceMethods::Method {
                    name: name2,
                    value: value2,
                },
            ) => {
                assert_eq!(name1, name2);
                assert_eq!(value1, value2);
            }
            _ => panic!("Expected Method variants"),
        }
        assert_eq!(original.oneway(), deserialized.oneway());
        assert_eq!(original.more(), deserialized.more());
        assert_eq!(original.upgrade(), deserialized.upgrade());
    }

    #[test]
    fn field_order_independence() {
        // Test with Simple method.
        let simple_jsons = [
            r#"{"method":"org.example.test.Simple","oneway":true,"more":false}"#,
            r#"{"oneway":true,"method":"org.example.test.Simple","more":false}"#,
            r#"{"more":false,"oneway":true,"method":"org.example.test.Simple"}"#,
        ];

        for json in &simple_jsons {
            let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();
            assert!(matches!(call.method(), TestServiceMethods::Simple));
            assert!(call.oneway());
            assert!(!call.more());
        }

        // Test with Method that has parameters - various field orderings.
        let method_jsons = [
            r#"{"method":"org.example.test.Method","parameters":{"name":"test","value":42},"oneway":true}"#,
            r#"{"parameters":{"name":"test","value":42},"method":"org.example.test.Method","oneway":true}"#,
            r#"{"oneway":true,"method":"org.example.test.Method","parameters":{"name":"test","value":42}}"#,
            r#"{"oneway":true,"parameters":{"name":"test","value":42},"method":"org.example.test.Method"}"#,
        ];

        for json in &method_jsons {
            let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();
            match call.method() {
                TestServiceMethods::Method { name, value } => {
                    assert_eq!(*name, "test");
                    assert_eq!(*value, 42);
                }
                _ => panic!("Expected Method variant"),
            }
            assert!(call.oneway());
        }

        // Test parameter field order within parameters object.
        let param_order_jsons = [
            r#"{"method":"org.example.test.Method","parameters":{"name":"test","value":42}}"#,
            r#"{"method":"org.example.test.Method","parameters":{"value":42,"name":"test"}}"#,
        ];

        for json in &param_order_jsons {
            let call: Call<TestServiceMethods<'_>> = serde_json::from_str(json).unwrap();
            match call.method() {
                TestServiceMethods::Method { name, value } => {
                    assert_eq!(*name, "test");
                    assert_eq!(*value, 42);
                }
                _ => panic!("Expected Method variant"),
            }
        }
    }

    #[test]
    fn comprehensive_service_methods() {
        // Demonstrates a complete service with multiple method types
        let methods = alloc::vec![
            TestServiceMethods::Simple,
            TestServiceMethods::Method {
                name: "complete",
                value: 456,
            },
            TestServiceMethods::GetInfo { id: 789 },
            TestServiceMethods::Reset,
        ];

        for method in methods {
            let call = Call::new(method.clone()).set_oneway(true);

            let json = serde_json::to_string(&call).unwrap();
            let deserialized: Call<TestServiceMethods<'_>> = serde_json::from_str(&json).unwrap();

            // Verify the method matches after roundtrip
            assert_eq!(call.oneway(), deserialized.oneway());
            assert_eq!(call.more(), deserialized.more());
            assert_eq!(call.upgrade(), deserialized.upgrade());

            // Method-specific verification
            match (call.method(), deserialized.method()) {
                (TestServiceMethods::Simple, TestServiceMethods::Simple) => {}
                (TestServiceMethods::Reset, TestServiceMethods::Reset) => {}
                (
                    TestServiceMethods::Method {
                        name: n1,
                        value: v1,
                    },
                    TestServiceMethods::Method {
                        name: n2,
                        value: v2,
                    },
                ) => {
                    assert_eq!(n1, n2);
                    assert_eq!(v1, v2);
                }
                (
                    TestServiceMethods::GetInfo { id: id1 },
                    TestServiceMethods::GetInfo { id: id2 },
                ) => {
                    assert_eq!(id1, id2);
                }
                (TestServiceMethods::WithFlattened(p1), TestServiceMethods::WithFlattened(p2)) => {
                    assert_eq!(p1, p2);
                }
                _ => panic!("Method variants don't match"),
            }
        }
    }

    #[test]
    fn serde_flatten_in_variant() {
        // Test serialization with multiple layers of flattened parameters.
        let extended_params = ExtendedParams {
            middle: MiddleParams {
                base: BaseParams {
                    name: "test_flatten",
                    value: 42,
                },
                category: "testing",
            },
            metadata: "important",
            priority: 5,
        };
        let method = TestServiceMethods::WithFlattened(extended_params);
        let call = Call::new(method).set_oneway(true);

        let json = serde_json::to_string(&call).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Verify that all flattened fields appear at the top level of parameters.
        assert_eq!(parsed["method"], "org.example.test.WithFlattened");
        assert_eq!(parsed["parameters"]["name"], "test_flatten");
        assert_eq!(parsed["parameters"]["value"], 42);
        assert_eq!(parsed["parameters"]["category"], "testing");
        assert_eq!(parsed["parameters"]["metadata"], "important");
        assert_eq!(parsed["parameters"]["priority"], 5);
        assert_eq!(parsed["oneway"], true);

        // Test deserialization with flattened parameters.
        let deserialized: Call<TestServiceMethods<'_>> = serde_json::from_str(&json).unwrap();

        match deserialized.method() {
            TestServiceMethods::WithFlattened(params) => {
                assert_eq!(params.middle.base.name, "test_flatten");
                assert_eq!(params.middle.base.value, 42);
                assert_eq!(params.middle.category, "testing");
                assert_eq!(params.metadata, "important");
                assert_eq!(params.priority, 5);
            }
            _ => panic!("Expected WithFlattened variant"),
        }
        assert!(deserialized.oneway());

        // Test roundtrip serialization maintains flattened structure.
        let json2 = serde_json::to_string(&deserialized).unwrap();
        let parsed2: serde_json::Value = serde_json::from_str(&json2).unwrap();
        assert_eq!(parsed, parsed2);
    }
}
