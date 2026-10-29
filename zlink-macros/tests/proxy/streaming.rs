use futures_util::TryStreamExt;

#[tokio::test]
async fn streaming_test() {
    use futures_util::stream::Stream;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    // Non-streaming methods can use borrowed types.
    // Streaming methods must use owned types (DeserializeOwned) because the internal buffer may be
    // reused between stream iterations, which would invalidate borrowed references.
    #[proxy("org.example.Stream")]
    trait StreamProxy {
        async fn get_single(&mut self) -> zlink::Result<Result<SingleReply<'_>, Error>>;

        #[zlink(more)]
        async fn get_stream(
            &mut self,
        ) -> zlink::Result<impl Stream<Item = zlink::Result<Result<StreamReply, Error>>>>;

        #[zlink(rename = "CustomStream", more)]
        async fn custom_stream(
            &mut self,
            count: i32,
        ) -> zlink::Result<impl Stream<Item = zlink::Result<Result<Item, Error>>>>;
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Item {
        id: u32,
        name: String,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Error;

    // Non-streaming reply can borrow from the connection buffer.
    #[derive(Debug, Serialize, Deserialize)]
    struct SingleReply<'a> {
        #[serde(borrow)]
        result: &'a str,
    }

    // Streaming reply must be owned (no lifetime parameter).
    #[derive(Debug, Serialize, Deserialize)]
    struct StreamReply {
        result: String,
    }

    // Test get_single
    let responses = json!({"parameters": {"result": "single result"}}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    let result = conn.get_single().await.unwrap().unwrap();
    assert_eq!(result.result, "single result");

    // Test streaming method
    let responses = [
        json!({"continues": true, "parameters": {"result": "stream item 1"}}).to_string(),
        json!({"continues": true, "parameters": {"result": "stream item 2"}}).to_string(),
        json!({"continues": false, "parameters": {"result": "stream item 3"}}).to_string(),
    ];
    let socket =
        MockSocket::with_responses(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    let mut conn = Connection::new(socket);

    let stream = conn.get_stream().await.unwrap();
    futures_util::pin_mut!(stream);
    let items = stream
        .try_collect::<Vec<Result<StreamReply, Error>>>()
        .await
        .unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].as_ref().unwrap().result, "stream item 1");
    assert_eq!(items[1].as_ref().unwrap().result, "stream item 2");
    assert_eq!(items[2].as_ref().unwrap().result, "stream item 3");

    // Test custom_stream method
    let responses = [
        json!({"continues": true, "parameters": {"id": 1, "name": "Item 1"}}).to_string(),
        json!({"continues": true, "parameters": {"id": 2, "name": "Item 2"}}).to_string(),
        json!({"continues": false, "parameters": {"id": 3, "name": "Item 3"}}).to_string(),
    ];
    let socket =
        MockSocket::with_responses(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    let mut conn = Connection::new(socket);

    let stream = conn.custom_stream(3).await.unwrap();
    futures_util::pin_mut!(stream);
    let items = stream
        .try_collect::<Vec<Result<Item, Error>>>()
        .await
        .unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].as_ref().unwrap().id, 1);
    assert_eq!(items[0].as_ref().unwrap().name, "Item 1");
    assert_eq!(items[1].as_ref().unwrap().id, 2);
    assert_eq!(items[2].as_ref().unwrap().id, 3);

    // Test the chain variant of the streaming method: it asks for more replies too, and gets them
    // all.
    let responses = [
        json!({"continues": true, "parameters": {"result": "stream item 1"}}).to_string(),
        json!({"continues": false, "parameters": {"result": "stream item 2"}}).to_string(),
    ];
    let socket =
        MockSocket::with_responses(&responses.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    let mut conn = Connection::new(socket);

    {
        let replies = conn
            .chain_get_stream()
            .unwrap()
            .send::<StreamReply, Error>()
            .await
            .unwrap();
        futures_util::pin_mut!(replies);
        let replies = replies.try_collect::<Vec<_>>().await.unwrap();
        let results: Vec<_> = replies
            .into_iter()
            .map(|(reply, _fds)| reply.unwrap().into_parameters().unwrap().result)
            .collect();
        assert_eq!(results, ["stream item 1", "stream item 2"]);
    }

    let written = conn.write().write_half().written_data();
    let call: serde_json::Value = serde_json::from_slice(written.strip_suffix(&[0]).unwrap())
        .expect("a single call terminated by a NUL byte");
    assert_eq!(call["method"], "org.example.Stream.GetStream");
    assert_eq!(call["more"], true);
}
