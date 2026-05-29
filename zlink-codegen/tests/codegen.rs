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
fn test_upgrade_method_codegen() {
    let idl = r#"
interface org.example.upgrade

upgrade method DoUpgrade() -> (success: bool)
"#;

    let interface = Interface::try_from(idl).unwrap();
    let code = generate_interface(&interface).unwrap();

    // Verify #[zlink(upgrade)] attribute is present
    assert!(code.contains("#[zlink(upgrade)]"));

    // Verify signature with by-value `self` receiver and UpgradeReply return type
    assert!(code.contains("async fn do_upgrade(self) -> zlink::Result<zlink::connection::UpgradeReply<Self::Socket, DoUpgradeOutput, UpgradeError>>;"));
}
