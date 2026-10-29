// Include the generated code from the build script.
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{TryStreamExt, pin_mut};
    use serde_json::json;
    use zlink::{Connection, test_utils::mock_socket::MockSocket};

    #[tokio::test]
    async fn test_example_proxy() {
        // Prepare all responses for ExampleProxy tests.
        let responses = [
            // Test successful get_person call.
            json!({
                "parameters": {
                    "person": {
                        "name": "Alice Smith",
                        "age": 32,
                        "email": "alice@example.com"
                    }
                }
            })
            .to_string(),
            // Test person with no email (optional field).
            json!({
                "parameters": {
                    "person": {
                        "name": "Bob Jones",
                        "age": 25,
                        "email": null
                    }
                }
            })
            .to_string(),
            // Test NotFound error.
            json!({
                "error": "test.Example.NotFound",
                "parameters": {
                    "message": "Person not found: unknown"
                }
            })
            .to_string(),
            // Test InvalidInput error with correct field names.
            json!({
                "error": "test.Example.InvalidInput",
                "parameters": {
                    "code": 400,
                    "details": "Invalid input provided"
                }
            })
            .to_string(),
            // Test Unknown error (no fields).
            json!({
                "error": "test.Example.Unknown"
            })
            .to_string(),
        ];

        let socket =
            MockSocket::with_responses(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let mut conn = Connection::new(socket);

        // Test successful get_person call.
        let result = conn.get_person("alice").await.unwrap().unwrap();
        assert_eq!(result.person.name, "Alice Smith");
        assert_eq!(result.person.age, 32);
        assert_eq!(result.person.email, Some("alice@example.com".to_string()));

        // Test person with no email (optional field).
        let result = conn.get_person("bob").await.unwrap().unwrap();
        assert_eq!(result.person.name, "Bob Jones");
        assert_eq!(result.person.age, 25);
        assert_eq!(result.person.email, None);

        // Test NotFound error.
        let result = conn.get_person("unknown").await.unwrap();
        assert!(matches!(
            result,
            Err(ExampleError::NotFound { message }) if message == "Person not found: unknown"
        ));

        // Test InvalidInput error with correct field names.
        let result = conn.get_person("").await.unwrap();
        assert!(matches!(
            result,
            Err(ExampleError::InvalidInput { code: 400, details }) if details == "Invalid input provided"
        ));

        // Test Unknown error (no fields).
        let result = conn.get_person("test").await.unwrap();
        assert!(matches!(result, Err(ExampleError::Unknown)));
    }

    #[tokio::test]
    async fn test_calc_proxy() {
        // Prepare all responses for CalcProxy tests.
        let responses = [
            // Test add method.
            json!({ "parameters": { "result": 15 } }).to_string(),
            // Test subtract method.
            json!({ "parameters": { "result": 5 } }).to_string(),
            // Test subtract with negative result.
            json!({ "parameters": { "result": -7 } }).to_string(),
            // Test multiply method.
            json!({ "parameters": { "result": 50 } }).to_string(),
            // Test multiply by zero.
            json!({ "parameters": { "result": 0 } }).to_string(),
            // Test divide method with valid division.
            json!({ "parameters": { "result": 2 } }).to_string(),
            // Test divide by zero - returns None.
            json!({ "parameters": { "result": null } }).to_string(),
        ];

        let socket =
            MockSocket::with_responses(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let mut conn = Connection::new(socket);

        // Test add method.
        let result = conn.add(10, 5).await.unwrap().unwrap();
        assert_eq!(result.result, 15);

        // Test subtract method.
        let result = conn.subtract(10, 5).await.unwrap().unwrap();
        assert_eq!(result.result, 5);

        // Test subtract with negative result.
        let result = conn.subtract(3, 10).await.unwrap().unwrap();
        assert_eq!(result.result, -7);

        // Test multiply method.
        let result = conn.multiply(10, 5).await.unwrap().unwrap();
        assert_eq!(result.result, 50);

        // Test multiply by zero.
        let result = conn.multiply(100, 0).await.unwrap().unwrap();
        assert_eq!(result.result, 0);

        // Test divide method with valid division.
        let result = conn.divide(10, 5).await.unwrap().unwrap();
        assert_eq!(result.result, Some(2));

        // Test divide by zero - returns None.
        let result = conn.divide(10, 0).await.unwrap().unwrap();
        assert_eq!(result.result, None);
    }

    #[tokio::test]
    async fn test_storage_proxy() {
        // Prepare all responses for StorageProxy tests.
        let responses = [
            // Test store method with a document that has tags.
            json!({ "parameters": { "id": "doc-123" } }).to_string(),
            // Test store with document without tags (empty vec).
            json!({ "parameters": { "id": "doc-456" } }).to_string(),
            // Test retrieve method.
            json!({
                "parameters": {
                    "doc": {
                        "id": "doc-789",
                        "title": "Retrieved Document",
                        "content": "Retrieved content",
                        "tags": ["archived", "2024"]
                    }
                }
            })
            .to_string(),
            // Test search method with string array parameter.
            json!({
                "parameters": {
                    "docs": [
                        {
                            "id": "doc-1",
                            "title": "Document 1",
                            "content": "Content 1",
                            "tags": ["tag1"]
                        },
                        {
                            "id": "doc-2",
                            "title": "Document 2",
                            "content": "Content 2",
                            "tags": ["tag2"]
                        },
                        {
                            "id": "doc-3",
                            "title": "Document 3",
                            "content": "Content 3",
                            "tags": ["tag3"]
                        }
                    ]
                }
            })
            .to_string(),
            // Test search with empty tags array.
            json!({ "parameters": { "docs": [] } }).to_string(),
        ];

        let socket =
            MockSocket::with_responses(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let mut conn = Connection::new(socket);

        // Test store method with a document that has tags.
        let doc = Document {
            id: "doc-123".to_string(),
            title: "My Document".to_string(),
            content: "This is the content".to_string(),
            tags: vec!["important".to_string(), "draft".to_string()],
        };
        let result = conn.store(&doc).await.unwrap().unwrap();
        assert_eq!(result.id, "doc-123");

        // Test store with document without tags (empty vec).
        let doc_no_tags = Document {
            id: "doc-456".to_string(),
            title: "Another Document".to_string(),
            content: "More content".to_string(),
            tags: vec![],
        };
        let result = conn.store(&doc_no_tags).await.unwrap().unwrap();
        assert_eq!(result.id, "doc-456");

        // Test retrieve method.
        let result = conn.retrieve("doc-789").await.unwrap().unwrap();
        assert_eq!(result.doc.id, "doc-789");
        assert_eq!(result.doc.title, "Retrieved Document");
        assert_eq!(result.doc.content, "Retrieved content");
        assert_eq!(
            result.doc.tags,
            vec!["archived".to_string(), "2024".to_string()]
        );

        // Test search method with string array parameter.
        // This is important for testing the string reference handling in arrays.
        // The search method takes query: &str and tags: &[&str] for the tags parameter.
        // The proxy macro should handle the conversion properly.
        let tags_to_search = ["important".to_string(), "urgent".to_string()];
        let tags_refs: Vec<&str> = tags_to_search.iter().map(|s| s.as_str()).collect();

        let result = conn
            .search("test query", &tags_refs)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.docs.len(), 3);
        assert_eq!(result.docs[0].id, "doc-1");
        assert_eq!(result.docs[1].id, "doc-2");
        assert_eq!(result.docs[2].id, "doc-3");

        // Test search with empty tags array.
        let empty_tags: Vec<&str> = vec![];
        let result = conn
            .search("another query", &empty_tags)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.docs.len(), 0);
    }

    #[tokio::test]
    async fn test_error_handling() {
        // Test connection-level errors separately since they each need their own connection.
        // Test connection-level error (invalid JSON response).
        let invalid_json = "this is not valid JSON";
        let socket = MockSocket::with_responses(&[invalid_json]);
        let mut conn: Connection<MockSocket> = Connection::new(socket);

        // This should result in a connection error, not a service error.
        let result = conn.get_person("test").await;
        assert!(result.is_err()); // Connection-level error

        // Test empty response.
        let empty_response = "";
        let socket = MockSocket::with_responses(&[empty_response]);
        let mut conn: Connection<MockSocket> = Connection::new(socket);

        let result = conn.get_person("test").await;
        assert!(result.is_err()); // Connection-level error

        // Test response with neither parameters nor error.
        let weird_response = json!({
            "something": "else"
        })
        .to_string();

        let socket = MockSocket::with_responses(&[&weird_response]);
        let mut conn: Connection<MockSocket> = Connection::new(socket);

        let result = conn.get_person("test").await;
        assert!(result.is_err()); // Should be an error since no parameters or error field

        // Prepare all valid responses for edge case tests.
        let responses = [
            // Test with Unicode strings.
            json!({
                "parameters": {
                    "person": {
                        "name": "李明",
                        "age": 30,
                        "email": "李明@example.com"
                    }
                }
            })
            .to_string(),
            // Test with very long strings.
            json!({
                "parameters": {
                    "doc": {
                        "id": "long-doc",
                        "title": "x".repeat(1000),
                        "content": "content",
                        "tags": []
                    }
                }
            })
            .to_string(),
            // Test with many tags in array.
            json!({
                "parameters": {
                    "docs": (0..100).map(|i| json!({
                        "id": format!("doc-{}", i),
                        "title": format!("Title {}", i),
                        "content": format!("Content {}", i),
                        "tags": [format!("tag{}", i)]
                    })).collect::<Vec<_>>()
                }
            })
            .to_string(),
            // Test large numbers in calc operations.
            json!({
                "parameters": {
                    "result": i64::MAX
                }
            })
            .to_string(),
        ];

        let socket =
            MockSocket::with_responses(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let mut conn = Connection::new(socket);

        // Test with Unicode strings.
        let result = conn.get_person("李明").await.unwrap().unwrap();
        assert_eq!(result.person.name, "李明");
        assert_eq!(result.person.email, Some("李明@example.com".to_string()));

        // Test with very long strings.
        let result = conn.retrieve("id").await.unwrap().unwrap();
        assert_eq!(result.doc.title.len(), 1000);

        // Test with many tags in array.
        let tags: Vec<&str> = vec!["test"];
        let result = conn.search("query", &tags).await.unwrap().unwrap();
        assert_eq!(result.docs.len(), 100);

        // Test large numbers in calc operations.
        let result = conn.add(i64::MAX - 1, 1).await.unwrap().unwrap();
        assert_eq!(result.result, i64::MAX);
    }

    #[tokio::test]
    async fn test_anytype_proxy() {
        let responses = [
            // Response for SetConfig: returns an id.
            json!({
                "parameters": {
                    "id": "config-1"
                }
            })
            .to_string(),
            // Response for GetConfig: returns a Config with various any values.
            json!({
                "parameters": {
                    "config": {
                        "name": "my-config",
                        "metadata": {
                            "version": 2,
                            "features": ["a", "b"]
                        },
                        "extra": 42
                    }
                }
            })
            .to_string(),
            // Response for GetConfig: metadata is a string, extra is null.
            json!({
                "parameters": {
                    "config": {
                        "name": "simple-config",
                        "metadata": "just a string",
                        "extra": null
                    }
                }
            })
            .to_string(),
            // Response for ProcessData: returns an array as the any result.
            json!({
                "parameters": {
                    "result": [1, "two", true, null]
                }
            })
            .to_string(),
            // Response for ProcessData: returns a nested object.
            json!({
                "parameters": {
                    "result": {
                        "nested": {
                            "deep": true
                        }
                    }
                }
            })
            .to_string(),
        ];

        let socket =
            MockSocket::with_responses(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
        let mut conn = Connection::new(socket);

        // Test SetConfig with structured metadata.
        let config = Config {
            name: "my-config".to_string(),
            metadata: json!({"version": 2, "features": ["a", "b"]}),
            extra: Some(json!(42)),
        };
        let result = conn
            .set_config(&config, &json!("extra-data"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.id, "config-1");

        // Test GetConfig returns Config with object metadata.
        let result = conn.get_config("config-1").await.unwrap().unwrap();
        assert_eq!(result.config.name, "my-config");
        assert_eq!(result.config.metadata["version"], 2);
        assert_eq!(result.config.metadata["features"][0], "a");
        assert_eq!(result.config.extra, Some(json!(42)));

        // Test GetConfig with string metadata and null extra.
        let result = conn.get_config("config-2").await.unwrap().unwrap();
        assert_eq!(result.config.name, "simple-config");
        assert_eq!(result.config.metadata, json!("just a string"));
        assert_eq!(result.config.extra, None);

        // Test ProcessData returning an array.
        let result = conn.process_data(&json!("input")).await.unwrap().unwrap();
        assert_eq!(result.result, json!([1, "two", true, null]));

        // Test ProcessData returning a nested object.
        let result = conn
            .process_data(&json!({"key": "value"}))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.result["nested"]["deep"], true);
    }

    #[tokio::test]
    async fn more_supported_method() {
        let item = |name, size| ListOutput {
            item: Item {
                name: String::from(name),
                size,
            },
        };

        // The method can be called without the `more` flag, getting a single reply.
        let mut conn = connection(&[json!({"parameters": {"item": {"name": "a", "size": 1}}})]);
        assert_eq!(conn.list().await.unwrap().unwrap(), item("a", 1));
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "List", None);

        // It can also be called with the flag, getting a stream of replies. The stream has to be
        // dropped before the connection can be used again.
        let mut conn = connection(&[
            json!({"continues": true, "parameters": {"item": {"name": "a", "size": 1}}}),
            json!({"parameters": {"item": {"name": "b", "size": 2}}}),
        ]);
        {
            let stream = conn.list_more().await.unwrap();
            pin_mut!(stream);
            let replies = stream.try_collect::<Vec<_>>().await.unwrap();
            assert_eq!(replies, [Ok(item("a", 1)), Ok(item("b", 2))]);
        }
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "List", Some("more"));
    }

    #[tokio::test]
    async fn more_required_method() {
        let mut conn = connection(&[
            json!({"continues": true, "parameters": {"name": "a", "removed": false}}),
            json!({"parameters": {"name": "a", "removed": true}}),
        ]);
        {
            let stream = conn.watch().await.unwrap();
            pin_mut!(stream);
            let reply = |name: &str, removed| {
                Ok(WatchOutput {
                    name: name.to_string(),
                    removed,
                })
            };
            assert_eq!(stream.try_next().await.unwrap(), Some(reply("a", false)));
            assert_eq!(stream.try_next().await.unwrap(), Some(reply("a", true)));
            assert_eq!(stream.try_next().await.unwrap(), None);
        }
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "Watch", Some("more"));

        // The errors of the interface are the items of the stream.
        let mut conn = connection(&[
            json!({"error": "org.example.flags.NoSuchItem", "parameters": {"name": "a"}}),
        ]);
        let stream = conn.watch().await.unwrap();
        pin_mut!(stream);
        let error = FlagsError::NoSuchItem {
            name: "a".to_string(),
        };
        assert_eq!(stream.try_next().await.unwrap(), Some(Err(error)));
        assert_eq!(stream.try_next().await.unwrap(), None);
    }

    #[tokio::test]
    async fn upgrade_required_method() {
        let mut conn = connection(&[json!({"parameters": {"size": 3, "checksum": "abc"}})]);
        let download = conn.download("blob").await.unwrap().unwrap();
        assert_eq!(
            download,
            DownloadOutput {
                size: 3,
                checksum: "abc".to_string(),
            }
        );
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "Download", Some("upgrade"));
        assert_eq!(calls[0]["parameters"], json!({"name": "blob"}));

        // The error reply refuses the upgrade.
        let mut conn = connection(&[
            json!({"error": "org.example.flags.NoSuchItem", "parameters": {"name": "blob"}}),
        ]);
        let error = conn.download("blob").await.unwrap().unwrap_err();
        let expected = FlagsError::NoSuchItem {
            name: "blob".to_string(),
        };
        assert_eq!(error, expected);
    }

    #[tokio::test]
    async fn upgrade_supported_method() {
        // The method can be called without the `upgrade` flag.
        let mut conn = connection(&[json!({"parameters": {"size": 3}})]);
        let fetch = conn.fetch("blob").await.unwrap().unwrap();
        assert_eq!(fetch, FetchOutput { size: 3 });
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "Fetch", None);
        assert_eq!(calls[0]["parameters"], json!({"name": "blob"}));

        // It can also be called with the flag, with the same name on the wire.
        let mut conn = connection(&[json!({"parameters": {"size": 3}})]);
        let fetch = conn.fetch_upgrade("blob").await.unwrap().unwrap();
        assert_eq!(fetch, FetchOutput { size: 3 });
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "Fetch", Some("upgrade"));
        assert_eq!(calls[0]["parameters"], json!({"name": "blob"}));
    }

    #[tokio::test]
    async fn method_without_flags() {
        let mut conn = connection(&[json!({})]);
        conn.ping().await.unwrap().unwrap();
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "Ping", None);
    }

    #[tokio::test]
    async fn more_chain_calls() {
        // A chain starting with a streaming method asks for more replies too, and gets them all.
        let mut conn = connection(&[
            json!({"continues": true, "parameters": {"item": {"name": "a", "size": 1}}}),
            json!({"parameters": {"item": {"name": "b", "size": 2}}}),
        ]);
        {
            let replies = conn
                .chain_list_more()
                .unwrap()
                .send::<ListOutput, FlagsError>()
                .await
                .unwrap();
            pin_mut!(replies);
            let replies = replies.try_collect::<Vec<_>>().await.unwrap();
            let names: Vec<_> = replies
                .into_iter()
                .map(|(reply, _fds)| reply.unwrap().into_parameters().unwrap().item.name)
                .collect();
            assert_eq!(names, ["a", "b"]);
        }
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "List", Some("more"));

        let mut conn = connection(&[
            json!({"continues": true, "parameters": {"name": "a", "removed": false}}),
            json!({"parameters": {"name": "a", "removed": true}}),
        ]);
        {
            let replies = conn
                .chain_watch()
                .unwrap()
                .send::<WatchOutput, FlagsError>()
                .await
                .unwrap();
            pin_mut!(replies);
            let replies = replies.try_collect::<Vec<_>>().await.unwrap();
            let removed: Vec<_> = replies
                .into_iter()
                .map(|(reply, _fds)| reply.unwrap().into_parameters().unwrap().removed)
                .collect();
            assert_eq!(removed, [false, true]);
        }
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_call(&calls[0], "Watch", Some("more"));
    }

    #[tokio::test]
    async fn keyword_params_and_outputs() {
        let mut conn = connection(&[json!({"parameters": {"super": "s"}})]);
        let reply = conn.get_uuid("a", 1).await.unwrap().unwrap();
        assert_eq!(reply.super_, "s");

        // The parameters are named as in the IDL, not as their identifiers.
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["parameters"], json!({"self": "a", "crate": 1}));
    }

    #[tokio::test]
    async fn acronym_method() {
        let mut conn = connection(&[json!({"parameters": {"super": "s"}})]);
        conn.get_uuid("a", 1).await.unwrap().unwrap();

        // The macro makes `GetUuid` of `get_uuid`, which isn't the name of the method.
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["method"], "org.example.names.GetUUID");
    }

    #[tokio::test]
    async fn keyword_method() {
        let mut conn = connection(&[json!({})]);
        conn.self_().await.unwrap().unwrap();

        // The method that `self_` calls is `Self`.
        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["method"], "org.example.names.Self");
    }

    #[test]
    fn keyword_fields() {
        // The fields are named as in the IDL, not as their identifiers.
        assert_eq!(serde_json::to_value(node()).unwrap(), node_json());
        assert_eq!(serde_json::from_value::<Node>(node_json()).unwrap(), node());
    }

    #[test]
    fn keyword_error_fields() {
        let error = NamesError::HttpError {
            self_: "a".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();

        // The field is named as in the IDL, not as its identifier.
        assert_eq!(json["parameters"], json!({"self": "a"}));
    }

    #[tokio::test]
    async fn acronym_error() {
        // The variant is named `HttpError`, which isn't the name of the error.
        let mut conn = connection(&[
            json!({"error": "org.example.names.HTTPError", "parameters": {"self": "a"}}),
        ]);
        let error = conn.self_().await.unwrap().unwrap_err();
        let expected = NamesError::HttpError {
            self_: "a".to_string(),
        };
        assert_eq!(error, expected);

        // The name of the IDL is the one that the error is sent with, too.
        let json = serde_json::to_value(&expected).unwrap();
        assert_eq!(json["error"], "org.example.names.HTTPError");
        assert_eq!(json["parameters"], json!({"self": "a"}));
    }

    #[test]
    fn enum_values() {
        // The values are named as in the IDL, whatever the case of their variants is.
        let modes = [
            (Mode::Fast, "Fast"),
            (Mode::SlowDown, "slow_down"),
            (Mode::Http, "HTTP"),
        ];
        for (mode, value) in modes {
            assert_eq!(serde_json::to_value(&mode).unwrap(), json!(value));
            assert_eq!(serde_json::from_value::<Mode>(json!(value)).unwrap(), mode);
        }
    }

    #[tokio::test]
    async fn custom_types_in_calls() {
        let reply = json!({"parameters": {"node": node_json(), "mode": "slow_down"}});
        let mut conn = connection(&[reply]);
        let reply = conn.update(&node(), &Mode::Http).await.unwrap().unwrap();
        assert_eq!(reply.node, node());
        assert_eq!(reply.mode, Mode::SlowDown);

        let calls = sent_calls(&conn);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["method"], "org.example.names.Update");
        assert_eq!(
            calls[0]["parameters"],
            json!({"node": node_json(), "mode": "HTTP"})
        );
    }

    /// A connection that gets the `replies`.
    fn connection(replies: &[serde_json::Value]) -> Connection<MockSocket> {
        let replies: Vec<_> = replies.iter().map(|reply| reply.to_string()).collect();
        let replies: Vec<_> = replies.iter().map(|reply| reply.as_str()).collect();

        Connection::new(MockSocket::with_responses(&replies))
    }

    /// The calls that were sent through the `conn`, in order.
    fn sent_calls(conn: &Connection<MockSocket>) -> Vec<serde_json::Value> {
        // Each call is terminated by a NUL byte.
        conn.write()
            .write_half()
            .written_data()
            .split(|byte| *byte == 0)
            .filter(|call| !call.is_empty())
            .map(|call| serde_json::from_slice(call).unwrap())
            .collect()
    }

    /// Assert that the `call` is for the `method` of the flags interface, with the `flag` set if
    /// there's one, and the other flag unset.
    #[track_caller]
    fn assert_call(call: &serde_json::Value, method: &str, flag: Option<&str>) {
        assert_eq!(call["method"], format!("org.example.flags.{method}"));
        for name in ["more", "upgrade"] {
            if flag == Some(name) {
                assert_eq!(call[name], true);
            } else {
                assert!(
                    call.get(name).is_none(),
                    "unexpected `{name}` flag in {call}"
                );
            }
        }
    }

    /// A node of the names interface.
    fn node() -> Node {
        Node {
            self_: "a".to_string(),
            crate_: 1,
            super_: Some("b".to_string()),
            r#type: "c".to_string(),
            r#try: true,
            r#gen: Some("d".to_string()),
        }
    }

    /// The JSON of the `node`, with the names of the IDL.
    fn node_json() -> serde_json::Value {
        json!({
            "self": "a",
            "crate": 1,
            "super": "b",
            "type": "c",
            "try": true,
            "gen": "d",
        })
    }
}
