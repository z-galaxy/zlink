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

    /// Regression test: untagged outer enum wrapping adjacently-tagged inner enum,
    /// where the matching variant has ALL-OPTIONAL fields and non-empty parameters.
    ///
    /// This reproduces the `machine_proxy` e2e failure:
    /// `{"method":"io.systemd.Machine.List","parameters":{"name":".host"}}`
    /// was failing with deserialization error due to a bug in EmptyParamsDeserializer.
    #[test]
    fn untagged_outer_all_optional_struct_variant_with_params() {
        // Outer untagged enum (what `service` macro generates)
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(untagged)]
        enum OuterEnum {
            VarlinkService(VarlinkServiceMethods),
            UserMethods(UserMethods),
        }

        // First inner: adjacently-tagged with some methods
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(tag = "method", content = "parameters")]
        enum VarlinkServiceMethods {
            #[serde(rename = "org.varlink.service.GetInfo")]
            GetInfo,
            #[serde(rename = "org.varlink.service.GetInterfaceDescription")]
            GetInterfaceDescription { interface: String },
        }

        // Second inner: multiple struct variants, several with ALL-OPTIONAL fields
        // (mirrors mock_machined_service structure)
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(tag = "method", content = "parameters")]
        enum UserMethods {
            #[serde(rename = "X.Register")]
            Register { name: String, class: String },
            #[serde(rename = "X.Unregister")]
            Unregister {
                name: Option<String>,
                pid: Option<i64>,
            },
            #[serde(rename = "X.Terminate")]
            Terminate {
                name: Option<String>,
                pid: Option<i64>,
            },
            #[serde(rename = "X.Kill")]
            Kill {
                name: Option<String>,
                pid: Option<i64>,
                whom: Option<String>,
                signal: Option<i64>,
            },
            #[serde(rename = "X.List")]
            List {
                name: Option<String>,
                pid: Option<i64>,
            },
            #[serde(rename = "X.Open")]
            Open {
                name: Option<String>,
                pid: Option<i64>,
                mode: String,
                user: Option<String>,
            },
        }

        // Case 1: unit variant with empty params (serde#2045 case — must still pass)
        let json = r#"{"method":"org.varlink.service.GetInfo","parameters":{}}"#;
        let result: Result<Call<OuterEnum>, _> = serde_json::from_str(json);
        let call: Call<OuterEnum> =
            result.unwrap_or_else(|e| panic!("unit+empty params failed: {e}"));
        assert!(
            matches!(
                call.method(),
                OuterEnum::VarlinkService(VarlinkServiceMethods::GetInfo)
            ),
            "expected GetInfo, got {:?}",
            call.method()
        );

        // Case 2: all-optional struct variant with non-empty params — THIS IS THE BUG
        let json = r#"{"method":"X.List","parameters":{"name":".host"}}"#;
        let call: Call<OuterEnum> = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("all-opt struct variant with params failed: {e}"));
        assert!(
            matches!(
                call.method(),
                OuterEnum::UserMethods(UserMethods::List {
                    name: Some(_),
                    pid: None
                })
            ),
            "expected List{{name: Some(\".host\")}}, got {:?}",
            call.method()
        );
        if let OuterEnum::UserMethods(UserMethods::List { name: Some(n), .. }) = call.method() {
            assert_eq!(n, ".host", "name field should be .host");
        }

        // Case 3: all-optional struct variant with EMPTY params (all None)
        let json = r#"{"method":"X.List","parameters":{}}"#;
        let call: Call<OuterEnum> = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("all-opt struct variant with empty params failed: {e}"));
        assert!(
            matches!(
                call.method(),
                OuterEnum::UserMethods(UserMethods::List {
                    name: None,
                    pid: None,
                })
            ),
            "expected List{{name: None, pid: None}}, got {:?}",
            call.method()
        );

        // Case 4: struct variant with required field still works
        let json = r#"{"method":"X.Register","parameters":{"name":"vm1","class":"container"}}"#;
        let call: Call<OuterEnum> = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("required-field struct variant failed: {e}"));
        assert!(
            matches!(
                call.method(),
                OuterEnum::UserMethods(UserMethods::Register { name, class })
                if name == "vm1" && class == "container"
            ),
            "expected Register{{name:\"vm1\"}}, got {:?}",
            call.method()
        );

        // Case 5 (regression): empty params on a struct variant, followed by `more:true`.
        // Attempt 1 aborts when the empty map is forwarded as a unit to the struct visitor;
        // the retry must still recover `more` from the outer map and leave the stream clean.
        let json = r#"{"method":"X.List","parameters":{},"more":true}"#;
        let call: Call<OuterEnum> = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("empty params then `more` flag failed: {e}"));
        assert!(
            matches!(
                call.method(),
                OuterEnum::UserMethods(UserMethods::List {
                    name: None,
                    pid: None,
                })
            ),
            "expected List{{None,None}}, got {:?}",
            call.method()
        );
        assert!(
            call.more(),
            "`more` flag after empty params must be preserved"
        );
    }

    /// Direct (non-untagged) adjacently-tagged enum: empty params on a struct variant followed
    /// by `more:true`. Unlike the untagged path (which buffers the whole map first), here the
    /// `FilterMap` streams the outer map, so this guards against the empty-params retry losing a
    /// trailing flag or corrupting the stream.
    #[test]
    fn direct_struct_variant_empty_params_then_flag() {
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(tag = "method", content = "parameters")]
        enum Method {
            #[serde(rename = "Ping")]
            Ping,
            #[serde(rename = "List")]
            List {
                name: Option<String>,
                pid: Option<i64>,
            },
        }

        // No-arg variant, empty params, trailing flag.
        let call: Call<Method> =
            serde_json::from_str(r#"{"method":"Ping","parameters":{},"more":true}"#)
                .expect("Ping with empty params and flag");
        assert!(matches!(call.method(), Method::Ping));
        assert!(call.more());

        // All-optional struct variant, empty params, trailing flag.
        let call: Call<Method> =
            serde_json::from_str(r#"{"method":"List","parameters":{},"oneway":true}"#)
                .expect("List with empty params and flag");
        assert!(matches!(
            call.method(),
            Method::List {
                name: None,
                pid: None
            }
        ));
        assert!(call.oneway());
    }

    /// A method enum exercising the three shapes that interact with the empty-`parameters`
    /// workaround (serde#2045): a no-argument unit variant, an all-optional struct variant,
    /// and a struct variant with a required field.
    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(tag = "method", content = "parameters")]
    enum WireMethod {
        #[serde(rename = "org.example.Ping")]
        Ping,
        #[serde(rename = "org.example.List")]
        List {
            name: Option<String>,
            limit: Option<u32>,
        },
        #[serde(rename = "org.example.Get")]
        Get { id: u32 },
    }

    /// What a parsed [`WireMethod`] call is expected to look like, kept simple so the test
    /// table below reads as plain data.
    #[derive(Debug, PartialEq)]
    struct Expected {
        method: WireMethod,
        oneway: bool,
        more: bool,
        upgrade: bool,
    }

    impl Expected {
        const fn new(method: WireMethod) -> Self {
            Self {
                method,
                oneway: false,
                more: false,
                upgrade: false,
            }
        }
        const fn oneway(mut self) -> Self {
            self.oneway = true;
            self
        }
        const fn more(mut self) -> Self {
            self.more = true;
            self
        }
        const fn upgrade(mut self) -> Self {
            self.upgrade = true;
            self
        }
    }

    /// Comprehensive table of concrete incoming JSON wire strings that MUST deserialize, paired
    /// with the exact `Call` they should produce. This is the canonical specification of how zlink
    /// accepts method calls — especially the empty/absent/null `parameters` matrix that motivated
    /// the serde#2045 workaround.
    #[test]
    fn parse_incoming_method_calls() {
        use WireMethod::*;

        let list_none = || List {
            name: None,
            limit: None,
        };

        let cases: &[(&str, Expected)] = &[
            // --- No-argument unit variant: parameters absent / empty / null all map to the unit.
            // ---
            (r#"{"method":"org.example.Ping"}"#, Expected::new(Ping)),
            (
                r#"{"method":"org.example.Ping","parameters":{}}"#,
                Expected::new(Ping),
            ),
            (
                r#"{"method":"org.example.Ping","parameters":null}"#,
                Expected::new(Ping),
            ),
            // Unit variant with trailing flags, including empty params before the flag.
            (
                r#"{"method":"org.example.Ping","oneway":true}"#,
                Expected::new(Ping).oneway(),
            ),
            (
                r#"{"method":"org.example.Ping","parameters":{},"more":true}"#,
                Expected::new(Ping).more(),
            ),
            (
                r#"{"method":"org.example.Ping","parameters":{},"upgrade":true}"#,
                Expected::new(Ping).upgrade(),
            ),
            // Flag before an empty-params unit variant (key order independence).
            (
                r#"{"oneway":true,"method":"org.example.Ping","parameters":{}}"#,
                Expected::new(Ping).oneway(),
            ),
            // --- All-optional struct variant: empty/absent/null => all fields None. ---
            (
                r#"{"method":"org.example.List","parameters":{}}"#,
                Expected::new(list_none()),
            ),
            (
                r#"{"method":"org.example.List","parameters":null}"#,
                Expected::new(list_none()),
            ),
            (
                r#"{"method":"org.example.List"}"#,
                Expected::new(list_none()),
            ),
            // Absent parameters on a struct variant, followed by a flag (retry path with no
            // mid-stream drain; the flag is captured during Attempt 1's full map walk).
            (
                r#"{"method":"org.example.List","oneway":true}"#,
                Expected::new(list_none()).oneway(),
            ),
            // All-optional struct variant with actual values.
            (
                r#"{"method":"org.example.List","parameters":{"name":".host"}}"#,
                Expected::new(List {
                    name: Some(".host".into()),
                    limit: None,
                }),
            ),
            (
                r#"{"method":"org.example.List","parameters":{"name":"vm1","limit":10}}"#,
                Expected::new(List {
                    name: Some("vm1".into()),
                    limit: Some(10),
                }),
            ),
            // Empty params on a struct variant, followed by each flag (the stream-drain
            // regression).
            (
                r#"{"method":"org.example.List","parameters":{},"oneway":true}"#,
                Expected::new(list_none()).oneway(),
            ),
            (
                r#"{"method":"org.example.List","parameters":{},"more":true}"#,
                Expected::new(list_none()).more(),
            ),
            (
                r#"{"method":"org.example.List","parameters":{},"upgrade":true}"#,
                Expected::new(list_none()).upgrade(),
            ),
            // Populated struct variant with all flags, in a shuffled key order.
            (
                r#"{"upgrade":true,"parameters":{"name":"x","limit":3},"method":"org.example.List","oneway":true}"#,
                Expected::new(List {
                    name: Some("x".into()),
                    limit: Some(3),
                })
                .oneway()
                .upgrade(),
            ),
            // --- Struct variant with a required field: normal path, unaffected by the workaround.
            // ---
            (
                r#"{"method":"org.example.Get","parameters":{"id":42}}"#,
                Expected::new(Get { id: 42 }),
            ),
            (
                r#"{"method":"org.example.Get","parameters":{"id":7},"more":true}"#,
                Expected::new(Get { id: 7 }).more(),
            ),
            // Unknown extra top-level fields are ignored.
            (
                r#"{"method":"org.example.Ping","extra":"ignored","trailing":[1,2,3]}"#,
                Expected::new(Ping),
            ),
        ];

        for (json, expected) in cases {
            let call: Call<WireMethod> = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("failed to parse {json}: {e}"));
            let actual = Expected {
                method: match call.method() {
                    Ping => Ping,
                    List { name, limit } => List {
                        name: name.clone(),
                        limit: *limit,
                    },
                    Get { id } => Get { id: *id },
                },
                oneway: call.oneway(),
                more: call.more(),
                upgrade: call.upgrade(),
            };
            assert_eq!(&actual, expected, "mismatch parsing {json}");
        }
    }

    /// Wire strings that MUST be rejected: a missing required field, an unknown method, a
    /// non-empty `parameters` on a no-argument method, and malformed flag values. These guard
    /// against the empty-params workaround becoming overly permissive or masking real errors.
    #[test]
    fn reject_invalid_method_calls() {
        let cases: &[&str] = &[
            // Missing required field `id`.
            r#"{"method":"org.example.Get","parameters":{}}"#,
            r#"{"method":"org.example.Get"}"#,
            // Unknown method name.
            r#"{"method":"org.example.Nope","parameters":{}}"#,
            // No-argument method given real parameters.
            r#"{"method":"org.example.Ping","parameters":{"unexpected":1}}"#,
            // Malformed flag values must not be silently swallowed by the empty-params retry.
            r#"{"method":"org.example.List","more":"not-a-bool"}"#,
            r#"{"method":"org.example.List","parameters":null,"more":123}"#,
            r#"{"method":"org.example.Ping","oneway":123}"#,
        ];

        for json in cases {
            let result: Result<Call<WireMethod>, _> = serde_json::from_str(json);
            assert!(
                result.is_err(),
                "expected {json} to be rejected, but it parsed as {:?}",
                result.unwrap().method()
            );
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
