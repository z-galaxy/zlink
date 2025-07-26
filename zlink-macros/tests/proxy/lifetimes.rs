#[tokio::test]
async fn lifetimes_test() {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{proxy, test_utils::mock_socket::MockSocket, Connection};

    #[proxy("org.example.Lifetimes")]
    trait LifetimeProxy {
        async fn process<'a>(
            &mut self,
            data: &'a str,
        ) -> zlink::Result<Result<Response<'_>, Error>>;
        async fn with_lifetime<'b>(
            &mut self,
            input: &'b str,
            count: i32,
        ) -> zlink::Result<Result<Vec<&str>, Error>>;
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Response<'a> {
        data: &'a str,
        length: usize,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    // Test process method
    let responses = json!({
        "parameters": {
            "data": "test response",
            "length": 13
        }
    })
    .to_string();
    let socket = MockSocket::new(&[&responses]);
    let mut conn = Connection::new(socket);

    let result = conn.process("test input").await.unwrap().unwrap();
    assert_eq!(result.data, "test response");
    assert_eq!(result.length, 13);

    // Test with_lifetime method
    let responses = json!({
        "parameters": ["item1", "item2", "item3"]
    })
    .to_string();
    let socket = MockSocket::new(&[&responses]);
    let mut conn = Connection::new(socket);

    let result = conn.with_lifetime("test", 3).await.unwrap().unwrap();
    assert_eq!(result, vec!["item1", "item2", "item3"]);
}
