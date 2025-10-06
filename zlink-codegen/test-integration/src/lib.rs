// Include the generated code from the build script.
include!(concat!(env!("OUT_DIR"), "/generated.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zlink::{test_utils::mock_socket::MockSocket, Connection};

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

        let socket = MockSocket::new(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
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

        let socket = MockSocket::new(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
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

        let socket = MockSocket::new(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
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
        let tags_to_search = vec!["important".to_string(), "urgent".to_string()];
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
        let socket = MockSocket::new(&[invalid_json]);
        let mut conn: Connection<MockSocket> = Connection::new(socket);

        // This should result in a connection error, not a service error.
        let result = conn.get_person("test").await;
        assert!(result.is_err()); // Connection-level error

        // Test empty response.
        let empty_response = "";
        let socket = MockSocket::new(&[empty_response]);
        let mut conn: Connection<MockSocket> = Connection::new(socket);

        let result = conn.get_person("test").await;
        assert!(result.is_err()); // Connection-level error

        // Test response with neither parameters nor error.
        let weird_response = json!({
            "something": "else"
        })
        .to_string();

        let socket = MockSocket::new(&[&weird_response]);
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

        let socket = MockSocket::new(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
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
}
