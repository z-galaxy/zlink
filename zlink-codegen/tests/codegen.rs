use zlink::idl::Interface;
use zlink_codegen::generate_interface;

#[test]
fn test_simple_interface() {
    let idl = r#"
# Simple Ping interface
interface org.example.ping

method Ping(message: string) -> (reply: string)
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check that the generated code contains expected elements.
    assert!(code.contains("#[proxy(\"org.example.ping\")]"));
    assert!(code.contains("pub trait Ping"));
    assert!(code.contains("async fn ping"));
    // Check that string parameters use references
    assert!(code.contains("message: &str"));
}

#[test]
fn test_interface_with_types() {
    let idl = r#"
interface org.example.types

type Person (
    name: string,
    age: int,
    email: ?string
)

type Status (idle, busy, away)

method GetPerson(id: int) -> (person: Person)
method SetStatus(status: Status) -> ()
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check that custom types are generated.
    assert!(code.contains("pub struct Person"));
    assert!(code.contains("pub name: String"));
    assert!(code.contains("pub age: i64"));
    assert!(code.contains("pub email: Option<String>"));

    assert!(code.contains("pub enum Status"));
    assert!(code.contains("Idle,"));
    assert!(code.contains("Busy,"));
    assert!(code.contains("Away,"));
}

#[test]
fn test_interface_with_errors() {
    let idl = r#"
interface org.example.errors

error NotFound(id: int)
error InvalidInput(message: string)

method Get(id: int) -> (value: string)
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check that errors are generated.
    assert!(code.contains("#[derive(Debug, Clone, PartialEq, ReplyError)]"));
    assert!(code.contains("#[zlink(interface = \"org.example.errors\")]"));
    assert!(code.contains("pub enum ErrorsError"));
    assert!(code.contains("NotFound"));
    assert!(code.contains("InvalidInput"));
}

#[test]
fn test_interface_with_arrays_and_dicts() {
    let idl = r#"
interface org.example.collections

method ListItems() -> (items: []string)
method GetConfig() -> (config: [string]string)
method ProcessData(numbers: []int) -> (results: [string]int)
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check that arrays and dicts are handled in output structs.
    // Output structs use references for strings for efficiency.
    assert!(code.contains("Vec<&'a str>"));
    assert!(code.contains("std::collections::HashMap<&'a str, &'a str>"));
    assert!(code.contains("std::collections::HashMap<&'a str, i64>"));

    // Check that parameters use references
    assert!(code.contains("numbers: &[i64]"));
}

#[test]
fn test_interface_with_optional_fields() {
    let idl = r#"
interface org.example.optional

type Config (
    name: string,
    value: ?string,
    enabled: ?bool
)

method GetConfig() -> (config: ?Config)
method SetConfig(config: ?Config) -> ()
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check that optional fields are handled.
    assert!(code.contains("pub value: Option<String>"));
    assert!(code.contains("pub enabled: Option<bool>"));
    assert!(code.contains("Option<Config>"));

    // Check that optional parameters are handled with references
    assert!(code.contains("config: Option<&Config>"));
}

#[test]
fn test_interface_with_multiple_outputs() {
    let idl = r#"
interface org.example.multi

method GetStats() -> (
    total: int,
    average: float,
    min: int,
    max: int
)
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check that multiple outputs create a struct (no lifetime needed for primitives)
    assert!(code.contains("pub struct GetStatsOutput"));
    assert!(code.contains("pub total: i64"));
    assert!(code.contains("pub average: f64"));
    assert!(code.contains("pub min: i64"));
    assert!(code.contains("pub max: i64"));
}

#[test]
fn test_org_varlink_service() {
    let idl = r#"
# The Varlink Service Interface is provided by every varlink service. It
# describes the service and the interfaces it implements.
interface org.varlink.service

# Get a list of all the interfaces a service provides and information
# about the service implementation.
method GetInfo() -> (
    vendor: string,
    product: string,
    version: string,
    url: string,
    interfaces: []string
)

# Get the description of an interface that is implemented by this service.
method GetInterfaceDescription(interface: string) -> (description: string)

# The requested interface was not found.
error InterfaceNotFound (interface: string)

# The requested method was not found
error MethodNotFound (method: string)

# The interface defines the requested method, but the service does not
# implement it.
error MethodNotImplemented (method: string)

# One of the passed parameters is invalid.
error InvalidParameter (parameter: string)
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check the generated code contains the expected elements.
    assert!(code.contains("#[proxy(\"org.varlink.service\")]"));
    assert!(code.contains("pub trait Service"));
    assert!(code.contains("async fn get_info"));
    assert!(code.contains("async fn get_interface_description"));
    assert!(code.contains("#[derive(Debug, Clone, PartialEq, ReplyError)]"));
    assert!(code.contains("#[zlink(interface = \"org.varlink.service\")]"));
    assert!(code.contains("pub enum ServiceError"));
    assert!(code.contains("InterfaceNotFound"));
    assert!(code.contains("MethodNotFound"));
    assert!(code.contains("MethodNotImplemented"));
    assert!(code.contains("InvalidParameter"));

    // Check that string parameters use references
    assert!(code.contains("interface: &str"));

    // Check that GetInfo creates an output struct for multiple outputs with lifetime
    assert!(code.contains("pub struct GetInfoOutput<'a>"));
    assert!(code.contains("pub vendor: &'a str"));
    assert!(code.contains("pub product: &'a str"));
    assert!(code.contains("pub version: &'a str"));
    assert!(code.contains("pub url: &'a str"));
    assert!(code.contains("pub interfaces: Vec<&'a str>"));
}

#[test]
fn test_reference_types_in_proxy() {
    let idl = r#"
interface org.example.refs

type CustomData (
    field1: string,
    field2: int
)

method SendString(text: string) -> ()
method SendArray(items: []string) -> ()
method SendMap(data: [string]int) -> ()
method SendCustom(data: CustomData) -> ()
method SendOptional(text: ?string, data: ?CustomData) -> ()
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check that all parameters use appropriate reference types
    assert!(code.contains("text: &str"));
    assert!(code.contains("items: &[&str]"));
    assert!(code.contains("data: &std::collections::HashMap<&str, i64>"));
    assert!(code.contains("data: &CustomData"));
    assert!(code.contains("text: Option<&str>"));
    assert!(code.contains("data: Option<&CustomData>"));
}

#[test]
fn test_camelcase_rename_attributes() {
    // Read the IDL file from the integration test to avoid duplication.
    let idl_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-integration")
        .join("camelcase.idl");
    let idl = std::fs::read_to_string(idl_path).expect("Failed to read camelcase.idl");

    let interface = Interface::try_from(idl.as_str()).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Verify method parameters have #[zlink(rename)] attributes.
    assert!(code.contains(r#"#[zlink(rename = "userId")] user_id: i64"#));
    assert!(code.contains(r#"#[zlink(rename = "includeEmail")] include_email: Option<bool>"#));

    // Verify struct fields have #[serde(rename)] attributes.
    assert!(code.contains(r#"#[serde(rename = "userName")]"#));
    assert!(code.contains("pub user_name: String"));
    assert!(code.contains(r#"#[serde(rename = "emailAddress")]"#));
    assert!(code.contains("pub email_address: Option<String>"));

    // Verify output struct fields have #[serde(rename)] attributes.
    assert!(code.contains(r#"#[serde(rename = "userData")]"#));
    assert!(code.contains("pub user_data: UserData"));
    assert!(code.contains(r#"#[serde(rename = "lastLogin")]"#));
    assert!(code.contains("pub last_login: &'a str"));

    // Verify error fields have #[zlink(rename)] attributes (not #[serde(rename)]).
    assert!(code.contains("UserNotFound {"));
    assert!(code.contains(r#"#[zlink(rename = "userId")]"#));
    assert!(code.contains("user_id: i64"));
}

#[test]
fn test_interface_with_any_type() {
    let idl = r#"
interface org.example.anytype

type Config (
    name: string,
    metadata: any,
    extra: ?any,
    items: []any,
    tags: [string]any
)

method SetConfig(data: any) -> (result: any)
method GetData() -> (items: []any, map: [string]any)
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Check struct fields use serde_json::Value for any type.
    assert!(code.contains("pub metadata: serde_json::Value"));
    assert!(code.contains("pub extra: Option<serde_json::Value>"));
    assert!(code.contains("pub items: Vec<serde_json::Value>"));
    assert!(code.contains("pub tags: std::collections::HashMap<String, serde_json::Value>"));

    // Check method input parameters use references.
    assert!(code.contains("data: &serde_json::Value"));

    // Check single output returns serde_json::Value.
    assert!(code.contains("serde_json::Value"));

    // Check GetData output struct contains array and map of any.
    assert!(code.contains("Vec<serde_json::Value>"));
    assert!(code.contains("std::collections::HashMap<"));
}

#[test]
fn flags_proxy_methods() {
    let code = flags_code();

    // The method supports the `more` flag: besides the regular method, there's a streaming one.
    assert_contains(
        &code,
        concat!(
            "    /// Lists the items.\n",
            "    async fn list(&mut self) -> zlink::Result<Result<ListOutput, FlagsError>>;\n",
            "    /// Lists the items.\n",
            "    ///\n",
            "    /// Calls the method with the `more` flag, streaming its replies.\n",
            "    #[zlink(more, rename = \"List\")]\n",
            "    async fn list_more(&mut self) -> zlink::Result<impl zlink::futures_util::Stream<",
            "Item = zlink::Result<Result<ListOutput, FlagsError>>>>;\n",
        ),
    );

    // The method requires the `more` flag: there's only a streaming method, with the plain name.
    assert_contains(
        &code,
        concat!(
            "    /// Watches the items for changes.\n",
            "    #[zlink(more)]\n",
            "    async fn watch(&mut self) -> zlink::Result<impl zlink::futures_util::Stream<",
            "Item = zlink::Result<Result<WatchOutput, FlagsError>>>>;\n",
        ),
    );

    // The method requires the `upgrade` flag: there's only a method for that, with the plain name.
    assert_contains(
        &code,
        concat!(
            "    /// Downloads an item over the upgraded connection.\n",
            "    #[zlink(upgrade)]\n",
            "    async fn download(&mut self, name: &str) -> ",
            "zlink::Result<Result<DownloadOutput, FlagsError>>;\n",
        ),
    );

    // The method supports the `upgrade` flag: besides the regular method, there's one for that.
    assert_contains(
        &code,
        concat!(
            "    /// Gets an item, or downloads it over the upgraded connection.\n",
            "    async fn fetch(&mut self, name: &str) -> ",
            "zlink::Result<Result<FetchOutput, FlagsError>>;\n",
            "    /// Gets an item, or downloads it over the upgraded connection.\n",
            "    ///\n",
            "    /// Calls the method with the `upgrade` flag, so a successful reply upgrades the ",
            "connection.\n",
            "    #[zlink(upgrade, rename = \"Fetch\")]\n",
            "    async fn fetch_upgrade(&mut self, name: &str) -> ",
            "zlink::Result<Result<FetchOutput, FlagsError>>;\n",
        ),
    );

    // The method without flags is as it always was.
    assert_contains(
        &code,
        concat!(
            "    /// Pings the service.\n",
            "    async fn ping(&mut self) -> zlink::Result<Result<(), FlagsError>>;\n",
        ),
    );

    // There are no other methods.
    assert_eq!(code.matches("async fn ").count(), 7);
}

#[test]
fn flags_owned_outputs() {
    // The replies to the calls with flags can't borrow from the connection.
    let code = flags_code();
    assert_contains(&code, "pub struct ListOutput {\n    pub item: Item,\n}");
    assert_contains(
        &code,
        "pub struct WatchOutput {\n    pub name: String,\n    pub removed: bool,\n}",
    );
    assert_contains(
        &code,
        "pub struct DownloadOutput {\n    pub size: i64,\n    pub checksum: String,\n}",
    );
    assert!(!code.contains("'a"));
    assert!(!code.contains("'_"));
    assert!(!code.contains("serde(borrow)"));

    // The replies to the calls without flags still borrow, in the same interface.
    let idl = r#"
interface org.example.mixed

# [Requires 'more' flag]
method Watch() -> (name: string)

# [Supports 'upgrade' flag]
method Fetch() -> (names: []string, kinds: [string]string)

method Name() -> (name: string)
"#;
    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    assert_contains(&code, "pub struct WatchOutput {\n    pub name: String,\n}");
    assert_contains(
        &code,
        concat!(
            "pub struct FetchOutput {\n",
            "    pub names: Vec<String>,\n",
            "    pub kinds: std::collections::HashMap<String, String>,\n",
            "}",
        ),
    );
    assert_contains(
        &code,
        "pub struct NameOutput<'a> {\n    #[serde(borrow)]\n    pub name: &'a str,\n}",
    );
    // The signatures agree with the structs.
    assert_contains(
        &code,
        "Item = zlink::Result<Result<WatchOutput, MixedError>>>>;",
    );
    assert_contains(
        &code,
        "async fn fetch(&mut self) -> zlink::Result<Result<FetchOutput, MixedError>>;",
    );
    assert_contains(
        &code,
        "async fn fetch_upgrade(&mut self) -> zlink::Result<Result<FetchOutput, MixedError>>;",
    );
    assert_contains(
        &code,
        "async fn name(&mut self) -> zlink::Result<Result<NameOutput<'_>, MixedError>>;",
    );
}

#[test]
fn flags_comments_are_not_docs() {
    // None of the comments that mark the flags is left in the generated code.
    let code = flags_code();
    for comment in [
        "[Supports 'more' flag]",
        "[Requires 'more' flag]",
        "[Supports 'upgrade' flag]",
        "[Requires 'upgrade' flag]",
    ] {
        assert!(!code.contains(comment), "`{comment}` found in:\n{code}");
    }

    // The other comments stay, in order, including the ones that merely look like the marking
    // ones. The marking ones are matched trimmed, so the trailing tab doesn't matter.
    let idl = concat!(
        "interface org.example.docs\n",
        "\n",
        "# Opens the file.\n",
        "# [Supports 'more' flag]\t\n",
        "# It stays open until it's closed.\n",
        "method Open() -> ()\n",
        "\n",
        "# [supports 'more' flag]\n",
        "# [Supports 'more' flags]\n",
        "# Supports 'more' flag\n",
        "method Close() -> ()\n",
    );
    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    assert_contains(
        &code,
        concat!(
            "    /// Opens the file.\n",
            "    /// It stays open until it's closed.\n",
            "    async fn open(&mut self) -> zlink::Result<Result<(), DocsError>>;\n",
            "    /// Opens the file.\n",
            "    /// It stays open until it's closed.\n",
            "    ///\n",
            "    /// Calls the method with the `more` flag, streaming its replies.\n",
            "    #[zlink(more, rename = \"Open\")]\n",
            "    async fn open_more(&mut self) -> zlink::Result<impl zlink::futures_util::Stream<",
            "Item = zlink::Result<Result<(), DocsError>>>>;\n",
        ),
    );
    assert_contains(
        &code,
        concat!(
            "    /// [supports 'more' flag]\n",
            "    /// [Supports 'more' flags]\n",
            "    /// Supports 'more' flag\n",
            "    async fn close(&mut self) -> zlink::Result<Result<(), DocsError>>;\n",
        ),
    );
    assert_eq!(code.matches("async fn ").count(), 3);
}

#[test]
fn flags_proxy_method_names() {
    let idl = r#"
interface org.example.names

# [Supports 'more' flag]
# [Supports 'upgrade' flag]
method Open(path: string) -> (fd: int)

# [Requires 'more' flag]
# [Requires 'upgrade' flag]
method Tunnel() -> ()

# [Supports 'more' flag]
method Type() -> ()
"#;
    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Both flags are supported: the regular method is followed by the one for each flag.
    assert_contains(
        &code,
        concat!(
            "    async fn open(&mut self, path: &str) -> ",
            "zlink::Result<Result<OpenOutput, NamesError>>;\n",
            "    /// Calls the method with the `more` flag, streaming its replies.\n",
            "    #[zlink(more, rename = \"Open\")]\n",
            "    async fn open_more(&mut self, path: &str) -> ",
            "zlink::Result<impl zlink::futures_util::Stream<",
            "Item = zlink::Result<Result<OpenOutput, NamesError>>>>;\n",
            "    /// Calls the method with the `upgrade` flag, so a successful reply upgrades the ",
            "connection.\n",
            "    #[zlink(upgrade, rename = \"Open\")]\n",
            "    async fn open_upgrade(&mut self, path: &str) -> ",
            "zlink::Result<Result<OpenOutput, NamesError>>;\n",
        ),
    );

    // Both flags are required: there's no regular method, and none has the plain name.
    assert_contains(
        &code,
        concat!(
            "    /// Calls the method with the `more` flag, streaming its replies.\n",
            "    #[zlink(more, rename = \"Tunnel\")]\n",
            "    async fn tunnel_more(&mut self) -> ",
            "zlink::Result<impl zlink::futures_util::Stream<",
            "Item = zlink::Result<Result<(), NamesError>>>>;\n",
            "    /// Calls the method with the `upgrade` flag, so a successful reply upgrades the ",
            "connection.\n",
            "    #[zlink(upgrade, rename = \"Tunnel\")]\n",
            "    async fn tunnel_upgrade(&mut self) -> zlink::Result<Result<(), NamesError>>;\n",
        ),
    );
    assert!(!code.contains("fn tunnel("));

    // The names that are Rust keywords are only escaped if they're not suffixed.
    assert_contains(
        &code,
        "    async fn r#type(&mut self) -> zlink::Result<Result<(), NamesError>>;\n",
    );
    assert_contains(
        &code,
        "    #[zlink(more, rename = \"Type\")]\n    async fn type_more(&mut self)",
    );
}

/// Generate the code for the IDL of the integration test, whose methods have all kinds of flags.
fn flags_code() -> String {
    // Read the IDL file from the integration test to avoid duplication.
    let idl_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test-integration")
        .join("flags.idl");
    let idl = std::fs::read_to_string(idl_path).expect("Failed to read flags.idl");

    let interface = Interface::try_from(idl.as_str()).unwrap();
    generate_interface(&interface).unwrap()
}

/// Assert that the `code` contains the `expected` code.
#[track_caller]
fn assert_contains(code: &str, expected: &str) {
    assert!(
        code.contains(expected),
        "`{expected}` not found in:\n{code}"
    );
}
