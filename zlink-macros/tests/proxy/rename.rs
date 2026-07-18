#[tokio::test]
async fn rename_test() {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[proxy("org.example.Rename")]
    trait RenameProxy {
        #[zlink(rename = "GetData")]
        async fn get_data(&mut self) -> zlink::Result<Result<GetDataReply<'_>, Error>>;

        #[zlink(rename = "SetValue")]
        async fn update_value(&mut self, value: i32) -> zlink::Result<Result<(), Error>>;

        // Test snake_case to PascalCase conversion
        async fn snake_case_method(&mut self) -> zlink::Result<Result<(), Error>>;
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[derive(Debug, Serialize, Deserialize)]
    struct GetDataReply<'a> {
        #[serde(borrow)]
        data: &'a str,
    }

    // Test get_data with renamed method
    let responses = json!({"parameters": {"data": "test data"}}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    let result = conn.get_data().await.unwrap().unwrap();
    assert_eq!(result.data, "test data");

    // Verify the renamed method name is used
    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    assert_eq!(written["method"], "org.example.Rename.GetData");

    // Test update_value
    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.update_value(42).await.unwrap().unwrap();

    // Test snake_case_method (should be converted to PascalCase)
    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.snake_case_method().await.unwrap().unwrap();

    // Verify snake_case was converted to PascalCase
    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    assert_eq!(written["method"], "org.example.Rename.SnakeCaseMethod");
}

#[tokio::test]
async fn param_rename_chain_test() {
    use futures_util::{pin_mut, stream::StreamExt};
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[proxy("org.example.ParamRename")]
    trait ParamRenameProxy {
        #[allow(unused)]
        async fn set_config(
            &mut self,
            #[zlink(rename = "dryRun")] dry_run: bool,
            #[zlink(rename = "configValue")] config_value: String,
        ) -> zlink::Result<Result<(), Error>>;
    }

    // Test chain_* method with renamed parameters
    let reply1 = json!({}).to_string();
    let reply2 = json!({}).to_string();
    let socket = MockSocket::new(&[&reply1, &reply2], vec![vec![]]);
    let mut conn = Connection::new(socket);

    {
        let replies = conn
            .chain_set_config(true, "test_value".to_string())
            .unwrap()
            .set_config(false, "another_value".to_string())
            .unwrap()
            .send::<(), Error>()
            .await
            .unwrap();

        pin_mut!(replies);

        let (reply1, _fds) = replies.next().await.unwrap().unwrap();
        reply1.unwrap();
        let (reply2, _fds) = replies.next().await.unwrap().unwrap();
        reply2.unwrap();
    }

    // Verify both chain_* and chain extension methods use renamed parameters
    let bytes_written = conn.write().write_half().written_data();

    // Parse the two JSON messages (separated by null bytes)
    let messages: Vec<&[u8]> = bytes_written
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(messages.len(), 2);

    // Check first message (from chain_set_config)
    let written1: serde_json::Value = serde_json::from_slice(messages[0]).unwrap();
    assert_eq!(written1["method"], "org.example.ParamRename.SetConfig");
    assert_eq!(written1["parameters"]["dryRun"], true);
    assert_eq!(written1["parameters"]["configValue"], "test_value");

    // Check second message (from chain extension .set_config)
    let written2: serde_json::Value = serde_json::from_slice(messages[1]).unwrap();
    assert_eq!(written2["method"], "org.example.ParamRename.SetConfig");
    assert_eq!(written2["parameters"]["dryRun"], false);
    assert_eq!(written2["parameters"]["configValue"], "another_value");
}

#[tokio::test]
async fn raw_ident_method_name() {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[proxy("org.example.Raw")]
    trait RawProxy {
        // The generated fn has to keep the raw ident to match this declaration, while the name it
        // carries on the wire must not: `r#` is Rust syntax, never part of the name.
        async fn r#type(&mut self, r#move: i32) -> zlink::Result<Result<(), Error>>;
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.r#type(42).await.unwrap().unwrap();

    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    assert_eq!(written["method"], "org.example.Raw.Type");
    assert_eq!(written["parameters"]["move"], 42);
}

#[tokio::test]
async fn raw_ident_chain_method() {
    use futures_util::{pin_mut, stream::StreamExt};
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[proxy("org.example.RawChain")]
    trait RawChainProxy {
        #[allow(unused)]
        async fn r#type(&mut self, r#move: i32) -> zlink::Result<Result<(), Error>>;
    }

    let reply1 = json!({}).to_string();
    let reply2 = json!({}).to_string();
    let socket = MockSocket::new(&[&reply1, &reply2], vec![vec![]]);
    let mut conn = Connection::new(socket);

    {
        // `chain_type`, not `chain_r#type`; the chain extension method stays raw-named.
        let replies = conn
            .chain_type(1)
            .unwrap()
            .r#type(2)
            .unwrap()
            .send::<(), Error>()
            .await
            .unwrap();

        pin_mut!(replies);

        let (reply1, _fds) = replies.next().await.unwrap().unwrap();
        reply1.unwrap();
        let (reply2, _fds) = replies.next().await.unwrap().unwrap();
        reply2.unwrap();
    }

    let bytes_written = conn.write().write_half().written_data();
    let messages: Vec<&[u8]> = bytes_written
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(messages.len(), 2);

    for (message, expected) in messages.iter().zip([1, 2]) {
        let written: serde_json::Value = serde_json::from_slice(message).unwrap();
        assert_eq!(written["method"], "org.example.RawChain.Type");
        assert_eq!(written["parameters"]["move"], expected);
    }
}

#[tokio::test]
async fn rename_all_pascal_case() {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[proxy(
        interface = "org.example.RenameAll",
        rename_all_arguments = "PascalCase"
    )]
    trait RenameAllProxy {
        #[allow(unused)]
        async fn set_config(
            &mut self,
            dry_run: bool,
            config_value: String,
        ) -> zlink::Result<Result<(), Error>>;
    }

    // Test that rename_all_arguments = "PascalCase" converts parameter names
    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.set_config(true, "test_value".to_string())
        .await
        .unwrap()
        .unwrap();

    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    assert_eq!(written["method"], "org.example.RenameAll.SetConfig");
    assert_eq!(written["parameters"]["DryRun"], true);
    assert_eq!(written["parameters"]["ConfigValue"], "test_value");
}

#[tokio::test]
async fn rename_all_with_explicit_override() {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    // Explicit #[zlink(rename)] should override rename_all_arguments
    #[proxy(
        interface = "org.example.RenameAllOverride",
        rename_all_arguments = "PascalCase"
    )]
    trait RenameAllOverrideProxy {
        #[allow(unused)]
        async fn update_setting(
            &mut self,
            #[zlink(rename = "customName")] setting_name: String,
            setting_value: i32,
        ) -> zlink::Result<Result<(), Error>>;
    }

    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.update_setting("test".to_string(), 42)
        .await
        .unwrap()
        .unwrap();

    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    // Explicit rename takes precedence
    assert_eq!(written["parameters"]["customName"], "test");
    // rename_all_arguments applies to the other parameter
    assert_eq!(written["parameters"]["SettingValue"], 42);
}

#[tokio::test]
async fn rename_all_camel_case() {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[proxy(
        interface = "org.example.CamelCase",
        rename_all_arguments = "camelCase"
    )]
    trait CamelCaseProxy {
        #[allow(unused)]
        async fn get_user_info(
            &mut self,
            user_name: String,
            include_details: bool,
        ) -> zlink::Result<Result<(), Error>>;
    }

    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.get_user_info("alice".to_string(), true)
        .await
        .unwrap()
        .unwrap();

    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    assert_eq!(written["parameters"]["userName"], "alice");
    assert_eq!(written["parameters"]["includeDetails"], true);
}

#[tokio::test]
async fn rename_all_chain_methods() {
    use futures_util::{pin_mut, stream::StreamExt};
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[proxy(
        interface = "org.example.RenameAllChain",
        rename_all_arguments = "PascalCase"
    )]
    trait RenameAllChainProxy {
        #[allow(unused)]
        async fn set_config(
            &mut self,
            dry_run: bool,
            config_value: String,
        ) -> zlink::Result<Result<(), Error>>;
    }

    let reply1 = json!({}).to_string();
    let reply2 = json!({}).to_string();
    let socket = MockSocket::new(&[&reply1, &reply2], vec![vec![]]);
    let mut conn = Connection::new(socket);

    {
        let replies = conn
            .chain_set_config(true, "val1".to_string())
            .unwrap()
            .set_config(false, "val2".to_string())
            .unwrap()
            .send::<(), Error>()
            .await
            .unwrap();

        pin_mut!(replies);

        let (reply1, _fds) = replies.next().await.unwrap().unwrap();
        reply1.unwrap();
        let (reply2, _fds) = replies.next().await.unwrap().unwrap();
        reply2.unwrap();
    }

    let bytes_written = conn.write().write_half().written_data();
    let messages: Vec<&[u8]> = bytes_written
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(messages.len(), 2);

    for message in &messages {
        let written: serde_json::Value = serde_json::from_slice(message).unwrap();
        // Verify PascalCase parameter names in both chain and chain extension methods
        assert!(written["parameters"]["DryRun"].is_boolean());
        assert!(written["parameters"]["ConfigValue"].is_string());
    }
}

#[tokio::test]
async fn rename_all_raw_identifiers() {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    // `r#` is Rust syntax, not part of the name: rename_all_arguments must apply to
    // `type`/`move`, never `r#type`/`r#move`.
    #[proxy(
        interface = "org.example.RenameAllRaw",
        rename_all_arguments = "PascalCase"
    )]
    trait RenameAllRawProxy {
        #[allow(unused)]
        async fn set_kind(
            &mut self,
            r#type: String,
            r#move: bool,
        ) -> zlink::Result<Result<(), Error>>;
    }

    let responses = json!({}).to_string();
    let socket = MockSocket::with_responses(&[&responses]);
    let mut conn = Connection::new(socket);

    conn.set_kind("disk".to_string(), true)
        .await
        .unwrap()
        .unwrap();

    let bytes_written = conn.write().write_half().written_data();
    let written: serde_json::Value =
        serde_json::from_slice(&bytes_written[..bytes_written.len() - 1]).unwrap();
    assert_eq!(written["method"], "org.example.RenameAllRaw.SetKind");
    assert_eq!(written["parameters"]["Type"], "disk");
    assert_eq!(written["parameters"]["Move"], true);
}

#[tokio::test]
async fn rename_all_raw_identifiers_chain_methods() {
    use futures_util::{pin_mut, stream::StreamExt};
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use zlink::{Connection, proxy, test_utils::mock_socket::MockSocket};

    #[derive(Debug, Serialize, Deserialize)]
    struct Error;

    #[proxy(
        interface = "org.example.RenameAllRawChain",
        rename_all_arguments = "PascalCase"
    )]
    trait RenameAllRawChainProxy {
        #[allow(unused)]
        async fn set_kind(
            &mut self,
            r#type: String,
            r#move: bool,
        ) -> zlink::Result<Result<(), Error>>;
    }

    let reply1 = json!({}).to_string();
    let reply2 = json!({}).to_string();
    let socket = MockSocket::new(&[&reply1, &reply2], vec![vec![]]);
    let mut conn = Connection::new(socket);

    {
        let replies = conn
            .chain_set_kind("disk".to_string(), true)
            .unwrap()
            .set_kind("net".to_string(), false)
            .unwrap()
            .send::<(), Error>()
            .await
            .unwrap();

        pin_mut!(replies);

        let (reply1, _fds) = replies.next().await.unwrap().unwrap();
        reply1.unwrap();
        let (reply2, _fds) = replies.next().await.unwrap().unwrap();
        reply2.unwrap();
    }

    let bytes_written = conn.write().write_half().written_data();
    let messages: Vec<&[u8]> = bytes_written
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(messages.len(), 2);

    // Both the chain and chain extension methods must apply rename_all_arguments to the unraw'd
    // names.
    for message in &messages {
        let written: serde_json::Value = serde_json::from_slice(message).unwrap();
        assert!(written["parameters"]["Type"].is_string());
        assert!(written["parameters"]["Move"].is_boolean());
    }
}
