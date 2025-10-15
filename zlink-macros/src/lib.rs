#![doc(
    html_logo_url = "https://raw.githubusercontent.com/z-galaxy/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.png"
)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    missing_docs
)]
#![warn(unreachable_pub)]
#![cfg_attr(not(doctest), doc = include_str!("../README.md"))]

mod utils;

#[cfg(feature = "introspection")]
mod introspect;

mod reply_error;

#[cfg(feature = "proxy")]
mod proxy;

/// Derives `Type` for structs and enums, generating appropriate `Type::Object` or `Type::Enum`
/// representation.
///
/// **Requires the `introspection` feature to be enabled.**
///
/// ## Structs
///
/// For structs, this macro supports named fields and unit structs. It will generate a
/// `Type` implementation that creates a `Type::Object` containing all the fields with their
/// names and types. Tuple structs are not supported as Varlink does not support unnamed fields.
///
/// ## Enums
///
/// For enums, this macro only supports unit variants (variants without associated data). It will
/// generate a `Type` implementation that creates a `Type::Enum` containing all the variant
/// names.
///
/// # Supported Attributes
///
/// The following attributes can be used to customize the behavior of this derive macro:
///
/// * `#[zlink(crate = "path")]` - Specifies the crate path to use for zlink types. Defaults to
///   `::zlink`.
///
/// # Limitations
///
/// The following types are **not** supported by this macro:
///
/// - **Tuple structs**: Varlink does not support unnamed fields
/// - **Enums with data**: Only unit enums (variants without associated data) are supported
/// - **Unions**: Not supported by Varlink
///
/// ```rust,compile_fail
/// # use zlink::introspect::Type;
/// #[derive(Type)]  // This will fail to compile
/// struct Point(f32, f32, f32);
/// ```
///
/// ```rust,compile_fail
/// # use zlink::introspect::Type;
/// #[derive(Type)]  // This will fail to compile
/// enum Status {
///     Active(String),  // Variants with data are not supported
///     Inactive,
/// }
/// ```
///
/// # Examples
///
/// ## Named Structs
///
/// ```rust
/// use zlink::introspect::Type;
/// use zlink::idl;
///
/// #[derive(Type)]
/// struct Person {
///     name: String,
///     age: i32,
///     active: bool,
/// }
///
/// // Access the generated type information
/// match Person::TYPE {
///     idl::Type::Object(fields) => {
///         let field_vec: Vec<_> = fields.iter().collect();
///         assert_eq!(field_vec.len(), 3);
///
///         assert_eq!(field_vec[0].name(), "name");
///         assert_eq!(field_vec[0].ty(), &idl::Type::String);
///
///         assert_eq!(field_vec[1].name(), "age");
///         assert_eq!(field_vec[1].ty(), &idl::Type::Int);
///
///         assert_eq!(field_vec[2].name(), "active");
///         assert_eq!(field_vec[2].ty(), &idl::Type::Bool);
///     }
///     _ => panic!("Expected struct type"),
/// }
/// ```
///
/// ## Unit Structs
///
/// ```rust
/// # use zlink::introspect::Type;
/// # use zlink::idl;
/// #[derive(Type)]
/// struct Unit;
///
/// // Unit structs generate empty field lists
/// match Unit::TYPE {
///     idl::Type::Object(fields) => {
///         assert_eq!(fields.len(), 0);
///     }
///     _ => panic!("Expected struct type"),
/// }
/// ```
///
/// ## Complex Types
///
/// ```rust
/// # use zlink::introspect::Type;
/// # use zlink::idl;
/// #[derive(Type)]
/// struct Complex {
///     id: u64,
///     description: Option<String>,
///     tags: Vec<String>,
/// }
///
/// // The macro handles nested types like Option<T> and Vec<T>
/// match Complex::TYPE {
///     idl::Type::Object(fields) => {
///         let field_vec: Vec<_> = fields.iter().collect();
///
///         // Optional field becomes Type::Optional
///         match field_vec[1].ty() {
///             idl::Type::Optional(inner) => assert_eq!(inner.inner(), &idl::Type::String),
///             _ => panic!("Expected optional type"),
///         }
///
///         // Vec field becomes Type::Array
///         match field_vec[2].ty() {
///             idl::Type::Array(inner) => assert_eq!(inner.inner(), &idl::Type::String),
///             _ => panic!("Expected array type"),
///         }
///     }
///     _ => panic!("Expected struct type"),
/// }
/// ```
///
/// ## Unit Enums
///
/// ```rust
/// # use zlink::introspect::Type;
/// # use zlink::idl;
/// #[derive(Type)]
/// enum Status {
///     Active,
///     Inactive,
///     Pending,
/// }
///
/// // Unit enums generate variant lists
/// match Status::TYPE {
///     idl::Type::Enum(variants) => {
///         let variant_vec: Vec<_> = variants.iter().collect();
///         assert_eq!(variant_vec.len(), 3);
///         assert_eq!(variant_vec[0].name(), "Active");
///         assert_eq!(variant_vec[1].name(), "Inactive");
///         assert_eq!(variant_vec[2].name(), "Pending");
///     }
///     _ => panic!("Expected enum type"),
/// }
/// ```
#[cfg(feature = "introspection")]
#[proc_macro_derive(IntrospectType, attributes(zlink))]
pub fn derive_introspect_type(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    introspect::r#type::derive_type(input)
}

/// Derives `Type` for structs and enums, generating named custom type definitions.
///
/// **Requires the `introspection` feature to be enabled.**
///
/// This macro generates implementations of the `CustomType` trait, which provides named
/// custom type definitions suitable for IDL generation. It also generates a `Type` implementation
/// and therefore is mutually exclusive to `zlink::introspect::Type` derive macro.
///
/// ## Structs
///
/// For structs, this macro generates a `custom::Type::Object` containing the struct name and
/// all fields with their names and types.
///
/// ## Enums
///
/// For enums, this macro only supports unit variants and generates a `custom::Type::Enum`
/// containing the enum name and all variant names.
///
/// # Supported Attributes
///
/// The following attributes can be used to customize the behavior of this derive macro:
///
/// * `#[zlink(crate = "path")]` - Specifies the crate path to use for zlink types. Defaults to
///   `::zlink`.
///
/// # Examples
///
/// ## Named Structs
///
/// ```rust
/// use zlink::introspect::{CustomType, Type};
/// use zlink::idl;
///
/// #[derive(CustomType)]
/// struct Point {
///     x: f64,
///     y: f64,
/// }
///
/// // Access the generated custom type information
/// match Point::CUSTOM_TYPE {
///     idl::CustomType::Object(obj) => {
///         assert_eq!(obj.name(), "Point");
///         let fields: Vec<_> = obj.fields().collect();
///         assert_eq!(fields.len(), 2);
///         assert_eq!(fields[0].name(), "x");
///         assert_eq!(fields[1].name(), "y");
///     }
///     _ => panic!("Expected custom object type"),
/// }
///
/// match Point::TYPE {
///     idl::Type::Custom(name) => {
///         assert_eq!(*name, "Point");
///     }
///     _ => panic!("Expected custom type"),
/// }
/// ```
///
/// ## Unit Enums
///
/// ```rust
/// # use zlink::introspect::{CustomType, Type};
/// # use zlink::idl;
/// #[derive(CustomType)]
/// enum Status {
///     Active,
///     Inactive,
///     Pending,
/// }
///
/// // Access the generated custom enum type information
/// match Status::CUSTOM_TYPE {
///     idl::CustomType::Enum(enm) => {
///         assert_eq!(enm.name(), "Status");
///         let variants: Vec<_> = enm.variants().collect();
///         assert_eq!(variants.len(), 3);
///         assert_eq!(variants[0].name(), "Active");
///         assert_eq!(variants[1].name(), "Inactive");
///         assert_eq!(variants[2].name(), "Pending");
///     }
///     _ => panic!("Expected custom enum type"),
/// }
///
/// match Status::TYPE {
///     idl::Type::Custom(name) => {
///         assert_eq!(*name, "Status");
///     }
///     _ => panic!("Expected custom type"),
/// }
/// ```
#[cfg(feature = "introspection")]
#[proc_macro_derive(IntrospectCustomType, attributes(zlink))]
pub fn derive_introspect_custom_type(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    introspect::custom_type::derive_custom_type(input)
}

/// Derives `ReplyError` for enums, generating error definitions for Varlink service errors.
///
/// **Requires the `introspection` feature to be enabled.**
///
/// This macro generates implementations of the `ReplyError` trait, which provides a list of
/// error variants that can be returned by a Varlink service method. It supports unit variants,
/// variants with named fields, and single-field tuple variants (where the field type implements
/// `Type` and has a `Type::Object`).
///
/// # Supported Attributes
///
/// The following attributes can be used to customize the behavior of this derive macro:
///
/// * `#[zlink(crate = "path")]` - Specifies the crate path to use for zlink types. Defaults to
///   `::zlink`.
///
/// # Example
///
/// ```rust
/// use zlink::introspect::ReplyError;
///
/// #[derive(ReplyError)]
/// enum ServiceError {
///     // Unit variant - no parameters
///     NotFound,
///
///     // Named field variant - multiple parameters
///     InvalidQuery {
///         message: String,
///         line: u32,
///     },
///
///     // Single tuple variant - uses fields from the wrapped type
///     ValidationFailed(ValidationDetails),
/// }
///
/// // Example struct for tuple variant
/// #[derive(zlink::introspect::Type)]
/// struct ValidationDetails {
///     field_name: String,
///     expected: String,
/// }
///
/// // Access the generated error variants
/// assert_eq!(ServiceError::VARIANTS.len(), 3);
/// assert_eq!(ServiceError::VARIANTS[0].name(), "NotFound");
/// assert!(ServiceError::VARIANTS[0].has_no_fields());
///
/// assert_eq!(ServiceError::VARIANTS[1].name(), "InvalidQuery");
/// assert!(!ServiceError::VARIANTS[1].has_no_fields());
/// ```
#[cfg(feature = "introspection")]
#[proc_macro_derive(IntrospectReplyError, attributes(zlink))]
pub fn derive_introspect_reply_error(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    introspect::reply_error::derive_reply_error(input)
}

/// Creates a client-side proxy for calling Varlink methods on a connection.
///
/// **Requires the `proxy` feature to be enabled.**
///
/// This attribute macro generates an implementation of the provided trait for `Connection<S>`,
/// automatically handling the serialization of method calls and deserialization of responses.
/// Each proxy trait targets a single Varlink interface.
///
/// The macro also generates a chain extension trait that allows you to chain multiple method
/// calls together for efficient batching across multiple interfaces.
///
/// # Supported Attributes
///
/// The following attributes can be used to customize the behavior of this macro:
///
/// * `interface` (required) - The Varlink interface name (e.g., `"org.varlink.service"`).
/// * `crate` - Specifies the crate path to use for zlink types. Defaults to `::zlink`.
/// * `chain_name` - Custom name for the generated chain extension trait. Defaults to
///   `{TraitName}Chain`.
///
/// # Example
///
/// ```rust
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use zlink::proxy;
/// use serde::{Deserialize, Serialize};
/// use serde_prefix_all::prefix_all;
/// use futures_util::stream::Stream;
///
/// #[proxy("org.example.MyService")]
/// trait MyServiceProxy {
///     async fn get_status(&mut self) -> zlink::Result<Result<Status<'_>, MyError<'_>>>;
///     async fn set_value(
///         &mut self,
///         key: &str,
///         value: i32,
///     ) -> zlink::Result<Result<(), MyError<'_>>>;
///     // This will call the `io.systemd.Machine.List` method when `list_machines()` is invoked.
///     #[zlink(rename = "ListMachines")]
///     async fn list_machines(&mut self) -> zlink::Result<Result<Vec<Machine<'_>>, MyError<'_>>>;
///     // Streaming version of get_status - calls the same method but returns a stream
///     #[zlink(rename = "GetStatus", more)]
///     async fn stream_status(
///         &mut self,
///     ) -> zlink::Result<
///         impl Stream<Item = zlink::Result<Result<Status<'_>, MyError<'_>>>>,
///     >;
/// }
///
/// // The macro generates:
/// // impl<S: Socket> MyServiceProxy for Connection<S> { ... }
///
/// #[derive(Debug, Serialize, Deserialize)]
/// struct Status<'m> {
///     active: bool,
///     message: &'m str,
/// }
///
/// #[derive(Debug, Serialize, Deserialize)]
/// struct Machine<'m> { name: &'m str }
///
/// #[prefix_all("org.example.MyService.")]
/// #[derive(Debug, Serialize, Deserialize)]
/// #[serde(tag = "error", content = "parameters")]
/// enum MyError<'a> {
///     NotFound,
///     InvalidRequest,
///     // Parameters must be named.
///     CodedError { code: u32, message: &'a str },
/// }
///
/// // Example usage:
/// # use zlink::test_utils::mock_socket::MockSocket;
/// # let responses = vec![
/// #     r#"{"parameters":{"active":true,"message":"System running"}}"#,
/// # ];
/// # let socket = MockSocket::new(&responses);
/// # let mut conn = zlink::Connection::new(socket);
/// let result = conn.get_status().await?.unwrap();
/// assert_eq!(result.active, true);
/// assert_eq!(result.message, "System running");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// ```
///
/// # Chaining Method Calls
///
/// The proxy macro generates chain extension traits that allow you to batch multiple method calls
/// together. This is useful for reducing round trips and efficiently calling methods across
/// multiple interfaces. Each method gets a `chain_` prefixed variant that starts a chain.
///
/// ## Example: Chaining Method Calls
///
/// ```rust
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use zlink::proxy;
/// # use serde::{Deserialize, Serialize};
/// # use futures_util::{pin_mut, TryStreamExt};
/// #
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct User<'a> { id: u64, name: &'a str }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct Post<'a> { id: u64, user_id: u64, content: &'a str }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # #[serde(untagged)]
/// # enum BlogReply<'a> {
/// #     #[serde(borrow)]
/// #     User(User<'a>),
/// #     #[serde(borrow)]
/// #     Post(Post<'a>),
/// #     #[serde(borrow)]
/// #     Posts(Vec<Post<'a>>)
/// # }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # #[serde(tag = "error")]
/// # enum BlogError { NotFound, InvalidInput }
/// # impl std::fmt::Display for BlogError {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
/// #         match self {
/// #             Self::NotFound => write!(f, "Not found"),
/// #             Self::InvalidInput => write!(f, "Invalid input")
/// #         }
/// #     }
/// # }
/// # impl std::error::Error for BlogError {}
/// #
/// // Define proxies for two different services
/// #[proxy("org.example.blog.Users")]
/// trait UsersProxy {
///     async fn get_user(&mut self, id: u64) -> zlink::Result<Result<BlogReply<'_>, BlogError>>;
///     async fn create_user(&mut self, name: &str)
///         -> zlink::Result<Result<BlogReply<'_>, BlogError>>;
/// }
///
/// #[proxy("org.example.blog.Posts")]
/// trait PostsProxy {
///     async fn get_posts_by_user(&mut self, user_id: u64)
///         -> zlink::Result<Result<BlogReply<'_>, BlogError>>;
///     async fn create_post(&mut self, user_id: u64, content: &str)
///         -> zlink::Result<Result<BlogReply<'_>, BlogError>>;
/// }
///
/// # use zlink::test_utils::mock_socket::MockSocket;
/// # let responses = vec![
/// #     r#"{"parameters":{"id":1,"name":"Alice"}}"#,
/// #     r#"{"parameters":{"id":1,"user_id":1,"content":"My first post!"}}"#,
/// #     r#"{"parameters":[{"id":1,"user_id":1,"content":"My first post!"}]}"#,
/// #     r#"{"parameters":{"id":1,"name":"Alice"}}"#,
/// # ];
/// # let socket = MockSocket::new(&responses);
/// # let mut conn = zlink::Connection::new(socket);
/// // Chain calls across both interfaces in a single batch
/// let chain = conn
///     .chain_create_user::<BlogReply<'_>, BlogError>("Alice")? // Start with Users interface
///     .create_post(1, "My first post!")?                       // Chain Posts interface
///     .get_posts_by_user(1)?                                   // Get all posts
///     .get_user(1)?;                                           // Get user details
///
/// // Send all calls in a single batch
/// let replies = chain.send().await?;
/// pin_mut!(replies);
///
/// // Process replies in order
/// let mut reply_count = 0;
/// while let Some(reply) = replies.try_next().await? {
///     let reply = reply?;
///     reply_count += 1;
///     match reply.parameters() {
///         Some(BlogReply::User(user)) => assert_eq!(user.name, "Alice"),
///         Some(BlogReply::Post(post)) => assert_eq!(post.content, "My first post!"),
///         Some(BlogReply::Posts(posts)) => assert_eq!(posts.len(), 1),
///         None => {} // set_value returns empty response
///     }
/// }
/// assert_eq!(reply_count, 4); // We made 4 calls
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// ```
///
/// ## Combining with Standard Varlink Service
///
/// When the `idl-parse` feature is enabled, you can also chain calls between your custom
/// interfaces and the standard Varlink service interface for introspection:
///
/// ```rust
/// # #[cfg(feature = "idl-parse")] {
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use zlink::{proxy, varlink_service};
/// # use serde::{Deserialize, Serialize};
/// #
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct Status { active: bool, message: String }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # #[serde(tag = "error")]
/// # enum MyError { NotFound, InvalidRequest }
/// #
/// #[proxy("com.example.MyService")]
/// trait MyServiceProxy {
///     async fn get_status(&mut self) -> zlink::Result<Result<Status, MyError>>;
/// }
///
/// // Combined types for cross-interface chaining
/// #[derive(Debug, Deserialize)]
/// #[serde(untagged)]
/// enum CombinedReply<'a> {
///     #[serde(borrow)]
///     VarlinkService(varlink_service::Reply<'a>),
///     MyService(Status),
/// }
///
/// #[derive(Debug, Deserialize)]
/// #[serde(untagged)]
/// enum CombinedError {
///     VarlinkService(varlink_service::Error),
///     MyService(MyError),
/// }
///
/// // Example usage:
/// # use zlink::test_utils::mock_socket::MockSocket;
/// # let responses = vec![
/// #     concat!(
/// #         r#"{"parameters":{"vendor":"Test","product":"Example","version":"1.0","#,
/// #         r#""url":"https://example.com","interfaces":["com.example.MyService","#,
/// #         r#""org.varlink.service"]}}"#
/// #     ),
/// #     r#"{"parameters":{"active":true,"message":"Running"}}"#,
/// #     r#"{"parameters":{"description":"interface com.example.MyService\n..."}}"#,
/// # ];
/// # let socket = MockSocket::new(&responses);
/// # let mut conn = zlink::Connection::new(socket);
/// use varlink_service::Proxy;
/// use zlink::varlink_service::Chain;
///
/// // Get service info and custom status in one batch
/// let chain = conn
///     .chain_get_info::<CombinedReply<'_>, CombinedError>()? // Varlink service interface
///     .get_status()?                                         // MyService interface
///     .get_interface_description("com.example.MyService")?;  // Back to Varlink service
///
/// let replies = chain.send().await?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// # }
/// ```
///
/// ## Chain Extension Traits
///
/// For each proxy trait, the macro generates a corresponding chain extension trait. For example,
/// `FtlProxy` gets `FtlProxyChain`. This trait is automatically implemented for `Chain` types,
/// allowing seamless method chaining across interfaces.
///
/// # Method Requirements
///
/// Proxy methods must:
/// - Take `&mut self` as the first parameter
/// - Can be either `async fn` or return `impl Future`
/// - Return `zlink::Result<Result<ReplyType, ErrorType>>` (outer Result for connection errors,
///   inner for method errors)
/// - The arguments can be any type that implement `serde::Serialize`
/// - The reply type (`Ok` case of the inner `Result`) must be a type that implements
///   `serde::Deserialize` and deserializes itself from a JSON object. Typically you'd just use a
///   struct that derives `serde::Deserialize`.
/// - The reply error type (`Err` case of the inner `Result`) must be a type `serde::Deserialize`
///   that deserializes itself from a JSON object with two fields:
///   - `error`: a string containing the fully qualified error name
///   - `parameters`: an optional object containing all the fields of the error
///
/// # Method Names
///
/// By default, method names are converted from snake_case to PascalCase for the Varlink call.
/// To specify a different Varlink method name, use the `#[zlink(rename = "...")]` attribute. See
/// `list_machines` in the example above.
///
/// # Streaming Methods
///
/// For methods that support streaming (the 'more' flag), use the `#[zlink(more)]` attribute.
/// Streaming methods must return `Result<impl Stream<Item = Result<Result<ReplyType,
/// ErrorType>>>>`. The proxy will automatically set the 'more' flag on the call and return a
/// stream of replies.
///
/// # Generic Parameters
///
/// The proxy macro supports generic type parameters on individual methods. Note that generic
/// parameters on the trait itself are not currently supported.
///
/// ```rust
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use zlink::proxy;
/// # use serde::{Deserialize, Serialize};
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct StoredValue<T> { data: T }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct ProcessReply<'a> { result: &'a str }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # #[serde(tag = "error")]
/// # enum StorageError { NotFound }
/// # impl std::fmt::Display for StorageError {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
/// #         write!(f, "Storage error")
/// #     }
/// # }
/// # impl std::error::Error for StorageError {}
/// #
/// #[proxy("org.example.Storage")]
/// trait StorageProxy {
///     // Method-level generics with trait bounds
///     async fn store<'a, T: Serialize + std::fmt::Debug>(
///         &mut self,
///         key: &'a str,
///         value: T,
///     ) -> zlink::Result<Result<(), StorageError>>;
///
///     // Generic methods with where clauses
///     async fn process<T>(&mut self, data: T)
///         -> zlink::Result<Result<ProcessReply<'_>, StorageError>>
///     where
///         T: Serialize + std::fmt::Debug;
///
///     // Methods can use generic type parameters in both input and output
///     async fn store_and_return<'a, T>(&mut self, key: &'a str, value: T)
///         -> zlink::Result<Result<StoredValue<T>, StorageError>>
///     where
///         T: Serialize + for<'de> Deserialize<'de> + std::fmt::Debug;
/// }
///
/// // Example usage:
/// # use zlink::test_utils::mock_socket::MockSocket;
/// # let responses = vec![
/// #     r#"{"parameters":null}"#, // store returns empty Ok
/// # ];
/// # let socket = MockSocket::new(&responses);
/// # let mut conn = zlink::Connection::new(socket);
/// // Store a value with generic type
/// let result = conn.store("my-key", 42i32).await?;
/// assert!(result.is_ok());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// ```
#[cfg(feature = "proxy")]
#[proc_macro_attribute]
pub fn proxy(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    proxy::proxy(attr.into(), input.into()).into()
}

/// Implements `serde::{Serialize, Deserialize}` for service error enums.
///
/// This macro automatically generates both `Serialize` and `Deserialize` implementations for error
/// types that are used in Varlink service replies.
///
/// The macro works in both `std` and `no_std` environments and requires the "error" field
/// to appear before "parameters" field in JSON for efficient parsing.
///
/// # Supported Enum Variants
///
/// The macro supports:
/// - **Unit variants**: Variants without any data
/// - **Named field variants**: Variants with named fields
///
/// Tuple variants are **not** supported.
///
/// # Attributes
///
/// - `interface` - This mandatory attribute specifies the Varlink interface name (e.g.,
///   "org.varlink.service")
///
/// # Example
///
/// ```rust
/// use zlink::ReplyError;
///
/// #[derive(ReplyError)]
/// #[zlink(interface = "com.example.MyService")]
/// enum ServiceError {
///     // Unit variant - no parameters
///     NotFound,
///     PermissionDenied,
///
///     // Named field variant - multiple parameters
///     InvalidInput {
///         field: String,
///         reason: String,
///     },
///
///     // Another variant with a single field
///     Timeout {
///         seconds: u32,
///     },
/// }
///
/// // The macro generates:
/// // - `Serialize` impl that creates properly tagged enum format
/// // - `Deserialize` impl that handles the tagged enum format efficiently
/// ```
///
/// # Serialization Format
///
/// The generated serialization uses a tagged enum format:
///
/// ```json
/// // Unit variant:
/// {"error": "NotFound"}
/// // or with empty parameters:
/// {"error": "NotFound", "parameters": null}
///
/// // Variant with fields:
/// {
///   "error": "InvalidInput",
///   "parameters": {
///     "field": "username",
///     "reason": "too short"
///   }
/// }
/// ```
#[proc_macro_derive(ReplyError, attributes(zlink))]
pub fn derive_reply_error(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    reply_error::derive_reply_error(input)
}
