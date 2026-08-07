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

mod naming;

mod attr_mode;

#[cfg(feature = "introspection")]
mod introspect;

mod reply_error;

#[cfg(feature = "proxy")]
mod proxy;

#[cfg(feature = "service")]
mod service;

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
/// # Renaming
///
/// - `#[zlink(rename = "...")]` on a field or variant sets its name in the IDL.
/// - `#[zlink(rename_all = "...")]` on a struct applies a case convention to its fields, and on an
///   enum to its variant names. An explicit `rename` on an item overrides it.
///
/// Valid `rename_all` values are `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`,
/// `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case` and `SCREAMING-KEBAB-CASE`. They mean the
/// same as they do in serde.
///
/// These attributes do not affect serialization. On a type that also derives serde traits, state
/// the same renaming to serde, or the IDL will not describe what is actually sent:
///
/// ```rust
/// use serde::Serialize;
/// use zlink::introspect::Type;
///
/// #[derive(Serialize, Type)]
/// #[serde(rename_all = "camelCase")]
/// #[zlink(rename_all = "camelCase")]
/// struct Membership {
///     user_name: String,
///     group_name: String,
/// }
/// ```
///
/// ## Field attributes
///
/// - `#[zlink(skip)]` — omit the field (or enum variant) from the emitted IDL. Use it to mirror
///   `#[serde(skip)]` so the described shape matches what serde actually sends.
/// - `#[zlink(flatten)]` — inline the field type's members in place, mirroring `#[serde(flatten)]`.
///   The field's type must be an inline object type (a `#[derive(Type)]` struct); a named custom
///   type or a scalar is a compile-time error.
///
/// Flattening a scalar is rejected:
/// ```rust,compile_fail
/// use zlink::introspect::Type;
/// #[derive(Type)]
/// struct Bad {
///     #[zlink(flatten)]
///     n: u32,
/// }
/// let _ = Bad::TYPE;
/// ```
///
/// `skip` and `rename` together are rejected:
/// ```rust,compile_fail
/// use zlink::introspect::Type;
/// #[derive(Type)]
/// struct Bad {
///     #[zlink(skip, rename = "x")]
///     n: u32,
/// }
/// ```
///
/// `skip` and `flatten` together are rejected:
/// ```rust,compile_fail
/// use zlink::introspect::Type;
/// #[derive(Type)]
/// struct Bad {
///     #[zlink(skip, flatten)]
///     n: u32,
/// }
/// ```
///
/// `flatten` and `rename` together are rejected:
/// ```rust,compile_fail
/// use zlink::introspect::Type;
/// #[derive(Type)]
/// struct Bad {
///     #[zlink(flatten, rename = "x")]
///     n: u32,
/// }
/// ```
///
/// Flattening a named custom type is rejected — its fields live behind a name, not inline:
/// ```rust,compile_fail
/// use zlink::introspect::{CustomType, Type};
/// #[derive(CustomType)]
/// struct Named {
///     a: u32,
/// }
/// #[derive(Type)]
/// struct Bad {
///     #[zlink(flatten)]
///     named: Named,
/// }
/// let _ = Bad::TYPE;
/// ```
///
/// `flatten` on an enum variant is rejected:
/// ```rust,compile_fail
/// use zlink::introspect::Type;
/// #[derive(Type)]
/// enum Bad {
///     #[zlink(flatten)]
///     Variant,
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
/// # Renaming
///
/// - `#[zlink(rename = "...")]` on a field or variant sets its name in the IDL.
/// - `#[zlink(rename_all = "...")]` on a struct applies a case convention to its fields, and on an
///   enum to its variant names. An explicit `rename` on an item overrides it.
/// - `#[zlink(rename = "...")]` on the type itself sets the name of the custom type in the IDL.
///
/// Valid `rename_all` values are `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`,
/// `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case` and `SCREAMING-KEBAB-CASE`. They mean the
/// same as they do in serde.
///
/// These attributes do not affect serialization. On a type that also derives serde traits, state
/// the same renaming to serde, or the IDL will not describe what is actually sent:
///
/// ```rust
/// use serde::Serialize;
/// use zlink::introspect::CustomType;
///
/// #[derive(Serialize, CustomType)]
/// #[serde(rename_all = "camelCase")]
/// #[zlink(rename_all = "camelCase")]
/// struct Membership {
///     user_name: String,
///     group_name: String,
/// }
/// ```
///
/// ## Field attributes
///
/// - `#[zlink(skip)]` — omit the field (or enum variant) from the emitted IDL. Use it to mirror
///   `#[serde(skip)]` so the described shape matches what serde actually sends.
/// - `#[zlink(flatten)]` — inline the field type's members in place, mirroring `#[serde(flatten)]`.
///   The field's type must be an inline object type (a `#[derive(Type)]` struct); a named custom
///   type or a scalar is a compile-time error.
///
/// Flattening a scalar is rejected:
/// ```rust,compile_fail
/// use zlink::introspect::CustomType;
/// #[derive(CustomType)]
/// struct Bad {
///     #[zlink(flatten)]
///     n: u32,
/// }
/// let _ = Bad::CUSTOM_TYPE;
/// ```
///
/// `skip` and `rename` together are rejected:
/// ```rust,compile_fail
/// use zlink::introspect::CustomType;
/// #[derive(CustomType)]
/// struct Bad {
///     #[zlink(skip, rename = "x")]
///     n: u32,
/// }
/// ```
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
/// # Renaming
///
/// - `#[zlink(rename = "...")]` on a field or variant sets its name in the IDL.
/// - `#[zlink(rename_all = "...")]` on the enum applies to error names, and on a variant to that
///   variant's fields. An explicit `rename` on an item overrides it. Error names in the IDL are
///   unqualified; the interface comes from `#[zlink(interface)]`.
///
/// Valid `rename_all` values are `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`,
/// `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case` and `SCREAMING-KEBAB-CASE`. They mean the
/// same as they do in serde.
///
/// These attributes do not affect serialization on their own. [`macro@ReplyError`] reads the same
/// `#[zlink(rename)]` and `#[zlink(rename_all)]` attributes, so deriving both on the same enum
/// (the usual pairing) keeps the wire format and the IDL in sync automatically. Any other type
/// that separately derives serde traits still needs the same renaming stated to serde, or the IDL
/// will not describe what is actually sent:
///
/// ```rust
/// use zlink::{ReplyError, introspect};
///
/// // `UPPERCASE`, not `SCREAMING_SNAKE_CASE`: an error name has no place for underscores
/// // (`[A-Z][A-Za-z0-9]*`), so the screaming-snake form would not be expressible in Varlink.
/// #[derive(ReplyError, introspect::ReplyError)]
/// #[zlink(interface = "org.example.Test", rename_all = "UPPERCASE")]
/// enum ServiceError {
///     NotFound,
/// }
/// ```
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
///
/// `#[zlink(skip)]` and `#[zlink(flatten)]` are not supported on error fields; they only apply to
/// `Type`/`CustomType`, where they mirror a `#[serde(..)]` attribute.
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
/// * `rename_all_arguments` - Applies a case convention to all method argument names. Valid values
///   are `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`,
///   `kebab-case`, `SCREAMING-KEBAB-CASE`. systemd's Varlink APIs, for instance, use `camelCase`
///   argument names (with `PascalCase` where a name mirrors a documented option name), so a proxy
///   for one typically wants `rename_all_arguments = "camelCase"`. Per-argument `#[zlink(rename =
///   "...")]` takes precedence over `rename_all_arguments`. Every produced name must still fit the
///   Varlink field-name grammar (`[A-Za-z][A-Za-z0-9_]*`), so a convention that steps outside it --
///   `kebab-case` on a multi-word argument, say -- is rejected at compile time:
///
/// ```rust,compile_fail
/// # use zlink::proxy;
/// #[proxy(interface = "org.example.Config", rename_all_arguments = "kebab-case")]
/// trait ConfigProxy {
///     // `dry_run` would become `dry-run`, which Varlink cannot express.
///     async fn set_config(&mut self, dry_run: bool) -> zlink::Result<Result<(), MyError>>;
/// }
/// # #[derive(Debug, serde::Serialize, serde::Deserialize)]
/// # enum MyError {}
/// ```
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
///     // Non-streaming methods can use borrowed types.
///     async fn get_status(&mut self) -> zlink::Result<Result<Status<'_>, MyError<'_>>>;
///     async fn set_value(
///         &mut self,
///         key: &str,
///         value: i32,
///     ) -> zlink::Result<Result<(), MyError<'_>>>;
///     #[zlink(rename = "ListMachines")]
///     async fn list_machines(&mut self) -> zlink::Result<Result<Vec<Machine<'_>>, MyError<'_>>>;
///     // Streaming methods must use owned types (DeserializeOwned) because the internal buffer may
///     // be reused between stream iterations.
///     #[zlink(rename = "GetStatus", more)]
///     async fn stream_status(
///         &mut self,
///     ) -> zlink::Result<
///         impl Stream<Item = zlink::Result<Result<OwnedStatus, OwnedMyError>>>,
///     >;
/// }
///
/// // The macro generates:
/// // impl<S: Socket> MyServiceProxy for Connection<S> { ... }
///
/// // Borrowed types for non-streaming methods.
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
/// // Owned types for streaming methods (required by the `more` attribute).
/// #[derive(Debug, Serialize, Deserialize)]
/// struct OwnedStatus {
///     active: bool,
///     message: String,
/// }
///
/// #[prefix_all("org.example.MyService.")]
/// #[derive(Debug, Serialize, Deserialize)]
/// #[serde(tag = "error", content = "parameters")]
/// enum OwnedMyError {
///     NotFound,
///     InvalidRequest,
///     CodedError { code: u32, message: String },
/// }
///
/// // Example usage:
/// # use zlink::test_utils::mock_socket::MockSocket;
/// # let responses = vec![
/// #     r#"{"parameters":{"active":true,"message":"System running"}}"#,
/// # ];
/// # let socket = MockSocket::with_responses(&responses);
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
/// **Important**: Chain methods are only generated for proxy methods that use owned types
/// (`DeserializeOwned`) in their return type. Methods with borrowed types (non-static lifetimes)
/// don't get chain variants since the internal buffer may be reused between stream iterations.
/// Input arguments can still use borrowed types.
///
/// ## Example: Chaining Method Calls
///
/// ```rust
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use zlink::proxy;
/// # use serde::{Deserialize, Serialize};
/// # use futures_util::{pin_mut, TryStreamExt};
/// #
/// // Owned reply types for chain API.
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct User { id: u64, name: String }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct Post { id: u64, user_id: u64, content: String }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # #[serde(untagged)]
/// # enum BlogReply {
/// #     User(User),
/// #     Post(Post),
/// #     Posts(Vec<Post>)
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
/// // Define proxies with owned return types - chain methods are generated.
/// #[proxy("org.example.blog.Users")]
/// trait UsersProxy {
///     async fn get_user(&mut self, id: u64)
///         -> zlink::Result<Result<BlogReply, BlogError>>;
///     async fn create_user(&mut self, name: &str)
///         -> zlink::Result<Result<BlogReply, BlogError>>;
/// }
///
/// #[proxy("org.example.blog.Posts")]
/// trait PostsProxy {
///     async fn get_posts_by_user(&mut self, user_id: u64)
///         -> zlink::Result<Result<BlogReply, BlogError>>;
///     async fn create_post(&mut self, user_id: u64, content: &str)
///         -> zlink::Result<Result<BlogReply, BlogError>>;
/// }
///
/// # use zlink::test_utils::mock_socket::MockSocket;
/// # let responses = vec![
/// #     r#"{"parameters":{"id":1,"name":"Alice"}}"#,
/// #     r#"{"parameters":{"id":1,"user_id":1,"content":"My first post!"}}"#,
/// #     r#"{"parameters":[{"id":1,"user_id":1,"content":"My first post!"}]}"#,
/// #     r#"{"parameters":{"id":1,"name":"Alice"}}"#,
/// # ];
/// # let socket = MockSocket::with_responses(&responses);
/// # let mut conn = zlink::Connection::new(socket);
/// let chain = conn
///     .chain_create_user("Alice")?
///     .create_post(1, "My first post!")?
///     .get_posts_by_user(1)?
///     .get_user(1)?;
///
/// // Send all calls in a single batch.
/// let replies = chain.send::<BlogReply, BlogError>().await?;
/// pin_mut!(replies);
///
/// // Process replies in order.
/// let mut reply_count = 0;
/// while let Some((reply, _fds)) = replies.try_next().await? {
///     reply_count += 1;
///     if let Ok(response) = reply {
///         match response.parameters() {
///             Some(BlogReply::User(user)) => assert_eq!(user.name, "Alice"),
///             Some(BlogReply::Post(post)) => assert_eq!(post.content, "My first post!"),
///             Some(BlogReply::Posts(posts)) => assert_eq!(posts.len(), 1),
///             None => {} // set_value returns empty response
///         }
///     }
/// }
/// assert_eq!(reply_count, 4); // We made 4 calls
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// ```
///
/// ## Combining Multiple Services
///
/// You can chain calls across multiple custom services. Define a combined reply type that can
/// deserialize responses from all interfaces:
///
/// ```rust
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use zlink::proxy;
/// # use serde::{Deserialize, Serialize};
/// # use futures_util::{pin_mut, TryStreamExt};
/// #
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct Status { active: bool, message: String }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct HealthInfo { healthy: bool, uptime: u64 }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # #[serde(tag = "error")]
/// # enum ServiceError { NotFound, InvalidRequest }
/// # impl std::fmt::Display for ServiceError {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
/// #         match self {
/// #             Self::NotFound => write!(f, "Not found"),
/// #             Self::InvalidRequest => write!(f, "Invalid input")
/// #         }
/// #     }
/// # }
/// # impl std::error::Error for ServiceError {}
/// #
/// // Multiple proxies with owned return types.
/// #[proxy("com.example.StatusService")]
/// trait StatusProxy {
///     async fn get_status(&mut self) -> zlink::Result<Result<Status, ServiceError>>;
/// }
///
/// #[proxy("com.example.HealthService")]
/// trait HealthProxy {
///     async fn get_health(&mut self) -> zlink::Result<Result<HealthInfo, ServiceError>>;
/// }
///
/// // Combined reply type for cross-interface chaining.
/// #[derive(Debug, Deserialize)]
/// #[serde(untagged)]
/// enum CombinedReply {
///     Status(Status),
///     Health(HealthInfo),
/// }
///
/// # use zlink::test_utils::mock_socket::MockSocket;
/// # let responses = vec![
/// #     r#"{"parameters":{"active":true,"message":"Running"}}"#,
/// #     r#"{"parameters":{"healthy":true,"uptime":12345}}"#,
/// # ];
/// # let socket = MockSocket::with_responses(&responses);
/// # let mut conn = zlink::Connection::new(socket);
/// // Chain calls across both services.
/// let chain = conn
///     .chain_get_status()?
///     .get_health()?;
///
/// let replies = chain.send::<CombinedReply, ServiceError>().await?;
/// pin_mut!(replies);
///
/// let mut count = 0;
/// while let Some((reply, _fds)) = replies.try_next().await? {
///     count += 1;
///     if let Ok(response) = reply {
///         match response.parameters() {
///             Some(CombinedReply::Status(s)) => println!("Status: {}", s.message),
///             Some(CombinedReply::Health(h)) => println!("Uptime: {}", h.uptime),
///             None => {}
///         }
///     }
/// }
/// assert_eq!(count, 2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
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
/// # One-way Methods
///
/// For fire-and-forget methods that don't expect a reply, use the `#[zlink(oneway)]` attribute.
/// One-way methods send the call and return immediately without waiting for a response. The method
/// must return `zlink::Result<()>` (just the outer Result for connection errors, no inner Result
/// since there's no reply to process).
///
/// One-way methods cannot be combined with `#[zlink(more)]` or `#[zlink(return_fds)]`.
///
/// This attribute is particularly useful in combination with chaining method calls. When you chain
/// oneway methods with regular methods, the oneway calls are sent but don't contribute to the reply
/// stream. For example, if you chain 4 calls where 2 are regular and 2 are oneway, you'll only
/// receive 2 replies. This allows you to efficiently batch side-effect operations (like resets or
/// notifications) alongside queries in a single round-trip.
///
/// ```rust
/// # use zlink::proxy;
/// # use serde::{Deserialize, Serialize};
/// #[proxy("org.example.Notifications")]
/// trait NotificationsProxy {
///     /// Fire-and-forget notification - returns immediately without waiting for a reply.
///     #[zlink(oneway)]
///     async fn notify(&mut self, message: &str) -> zlink::Result<()>;
///
///     /// Another one-way method with multiple parameters.
///     #[zlink(oneway)]
///     async fn log_event(&mut self, level: &str, message: &str, timestamp: u64)
///         -> zlink::Result<()>;
/// }
/// ```
///
/// # File Descriptor Passing
///
/// **Requires the `std` feature to be enabled.**
///
/// Methods can send and receive file descriptors using the following attributes:
///
/// ## Sending File Descriptors
///
/// Use `#[zlink(fds)]` on a parameter of type `Vec<OwnedFd>` to send file descriptors with the
/// method call. Only one FD parameter is allowed per method.
///
/// ## Receiving File Descriptors
///
/// Use `#[zlink(return_fds)]` on a method to indicate it returns file descriptors. The method's
/// return type must be `Result<(Result<ReplyType, ErrorType>, Vec<OwnedFd>)>` - a tuple containing
/// both the method result and the received file descriptors. The FDs are available regardless of
/// whether the method succeeded or failed.
///
/// ## Example: File Descriptor Passing
///
/// File descriptors are passed out-of-band from the encoded JSON parameters. The typical pattern
/// is to include integer indexes in your JSON parameters that reference positions in the FD
/// vector. This is similar to how D-Bus handles FD passing.
///
/// ```rust
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// use zlink::proxy;
/// use serde::{Deserialize, Serialize};
/// use std::os::fd::OwnedFd;
///
/// #[proxy("org.example.FileService")]
/// trait FileServiceProxy {
///     // Send file descriptors to the service
///     // The stdin/stdout parameters are indexes into the FDs vector
///     async fn spawn_process(
///         &mut self,
///         command: String,
///         stdin_fd: u32,
///         stdout_fd: u32,
///         #[zlink(fds)] fds: Vec<OwnedFd>,
///     ) -> zlink::Result<Result<ProcessInfo, FileError>>;
///
///     // Receive file descriptors from the service
///     // Returns metadata with FD indexes and the actual FDs
///     #[zlink(return_fds)]
///     async fn open_files(
///         &mut self,
///         paths: Vec<String>,
///     ) -> zlink::Result<(Result<Vec<FileInfo>, FileError>, Vec<OwnedFd>)>;
/// }
///
/// #[derive(Debug, Serialize, Deserialize)]
/// struct ProcessInfo {
///     pid: u32,
/// }
///
/// // Response contains FD indexes referencing the FD vector
/// #[derive(Debug, Serialize, Deserialize)]
/// struct FileInfo {
///     path: String,
///     fd: u32,  // Index into the FD vector (0, 1, 2, etc.)
/// }
///
/// #[derive(Debug, Serialize, Deserialize)]
/// #[serde(tag = "error")]
/// enum FileError {
///     NotFound { path: String },
///     PermissionDenied { path: String },
/// }
/// # impl std::error::Error for FileError {}
/// # impl std::fmt::Display for FileError {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
/// #         match self {
/// #             FileError::NotFound { path } => write!(f, "File not found: {}", path),
/// #             FileError::PermissionDenied { path } =>
/// #                 write!(f, "Permission denied: {}", path),
/// #         }
/// #     }
/// # }
///
/// // Example usage:
/// # use std::os::unix::net::UnixStream;
/// # use zlink::test_utils::mock_socket::MockSocket;
/// // Sending FDs: Pass indexes as regular parameters
/// # let (stdin_pipe, _w1) = UnixStream::pair()?;
/// # let (stdout_pipe, _w2) = UnixStream::pair()?;
/// # let send_response = r#"{"parameters":{"pid":1234}}"#;
/// # let send_socket = MockSocket::with_responses(&[send_response]);
/// # let mut send_conn = zlink::Connection::new(send_socket);
/// let fds = vec![stdin_pipe.into(), stdout_pipe.into()];
/// // Parameters reference FD indexes: stdin_fd=0, stdout_fd=1
/// let result = send_conn.spawn_process("/bin/cat".to_string(), 0, 1, fds).await?;
/// let process_info = result?;
/// assert_eq!(process_info.pid, 1234);
///
/// // Receiving FDs: Response contains indexes that reference the FD vector
/// # let (file1, _w3) = UnixStream::pair()?;
/// # let (file2, _w4) = UnixStream::pair()?;
/// # let recv_response = r#"{
/// #   "parameters": [
/// #     {"path": "/etc/config.txt", "fd": 0},
/// #     {"path": "/var/data.bin", "fd": 1}
/// #   ]
/// # }"#;
/// # let recv_fds = vec![vec![file1.into(), file2.into()]];
/// # let recv_socket = MockSocket::new(&[recv_response], recv_fds);
/// # let mut recv_conn = zlink::Connection::new(recv_socket);
/// let (result, received_fds) = recv_conn
///     .open_files(vec!["/etc/config.txt".to_string(), "/var/data.bin".to_string()])
///     .await?;
/// let file_list = result?;
/// assert_eq!(file_list.len(), 2);
/// assert_eq!(received_fds.len(), 2);
/// // Use the fd field to match file info with actual FDs
/// for file_info in &file_list {
///     let fd = &received_fds[file_info.fd as usize];
///     println!("File {} has FD at index {}", file_info.path, file_info.fd);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// ```
///
/// ## Parameter Renaming
///
/// Use `#[zlink(rename = "name")]` on parameters to customize their serialized names in the
/// Varlink protocol. This is useful when the Rust parameter name doesn't match the expected
/// Varlink parameter name.
///
/// ```rust
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use zlink::proxy;
/// # use serde::{Deserialize, Serialize};
/// #[proxy("org.example.Users")]
/// trait UsersProxy {
///     async fn create_user(
///         &mut self,
///         #[zlink(rename = "user_name")] name: String,
///         #[zlink(rename = "user_email")] email: String,
///     ) -> zlink::Result<Result<UserId, UserError>>;
/// }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # struct UserId { id: u32 }
/// # #[derive(Debug, Serialize, Deserialize)]
/// # #[serde(tag = "error")]
/// # enum UserError { InvalidInput }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// ```
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
/// # let socket = MockSocket::with_responses(&responses);
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
/// ## Enum-level attributes
///
/// - `interface` - This mandatory attribute specifies the Varlink interface name (e.g.,
///   "org.varlink.service")
/// - `rename_all = "..."` - Applies a case convention to every variant name
///
/// ## Variant-level attributes
///
/// - `rename = "..."` - Specifies a custom name for the variant in the serialized error name
/// - `rename_all = "..."` - Applies a case convention to this variant's fields
///
/// ## Field-level attributes
///
/// - `rename = "..."` - Specifies a custom name for the field in the JSON representation
/// - `borrow` - Enables zero-copy deserialization for types like `Cow<'_, str>`
///
/// # Example
///
/// ```rust
/// use std::borrow::Cow;
/// use zlink::ReplyError;
///
/// #[derive(ReplyError)]
/// #[zlink(interface = "com.example.MyService")]
/// enum ServiceError<'a> {
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
///     // Variant with zero-copy deserialization using borrow
///     CustomError {
///         #[zlink(borrow)]
///         message: Cow<'a, str>,
///     },
///
///     // Variant with a renamed error name: serialized as
///     // "com.example.MyService.TimedOut".
///     #[zlink(rename = "TimedOut")]
///     Timeout {
///         #[zlink(rename = "timeoutSeconds")]
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

/// Transforms an impl block into a `Service` trait implementation.
///
/// **Requires the `service` feature to be enabled.** The `service` feature automatically enables
/// the `introspection` feature, allowing the macro to generate interface descriptions and handle
/// the standard `org.varlink.service` interface automatically.
///
/// This attribute macro takes a regular impl block and generates the necessary code to implement
/// the `Service` trait, enabling the type to handle Varlink method calls.
///
/// # Automatic Introspection Support
///
/// The generated service automatically handles the `org.varlink.service` interface:
///
/// - **`GetInfo`**: Returns service metadata (vendor, product, version, URL) and a list of all
///   implemented interfaces.
/// - **`GetInterfaceDescription`**: Returns the IDL description for any implemented interface,
///   generated at compile time from the method signatures and types.
/// - **Unknown methods**: Return `MethodNotFound` error with the method name.
/// - **Unknown interfaces** (in `GetInterfaceDescription`): Return `InterfaceNotFound` error.
///
/// # Supported Attributes
///
/// ## On the impl block:
///
/// * `crate = "path"` - Specifies the crate path to use for zlink types. Defaults to `::zlink`.
/// * `interface = "..."` - Sets the default interface name for all methods. Useful for services
///   that implement a single interface. Methods can still override this with method-level
///   `#[zlink(interface = "...")]`.
/// * `types = [Type1, Type2, ...]` - Custom types to include in interface descriptions. These types
///   must implement `CustomType` (typically via `#[derive(CustomType)]`). For single-interface
///   services, these types are included in that interface's IDL output. For multi-interface
///   services, prefer specifying types per interface using the method-level `#[zlink(interface =
///   "...", types = [...])]` attribute instead.
/// * `vendor = <expr>` - The vendor name for `GetInfo` response. Defaults to empty string.
/// * `product = <expr>` - The product name for `GetInfo` response. Defaults to empty string.
/// * `version = <expr>` - The version string for `GetInfo` response. Defaults to empty string. E.g.
///   `version = env!("CARGO_PKG_VERSION")`.
/// * `url = <expr>` - The URL for `GetInfo` response. Defaults to empty string.
///
/// ## On methods:
///
/// * `#[zlink(interface = "...")]` - Set the interface name for this and subsequent methods. If an
///   interface is specified at the impl block level, this overrides it for the current method.
/// * `#[zlink(interface = "...", types = [Type1, ...])]` - Set the interface and associate custom
///   types with it. This scopes the types to the specified interface so they only appear in that
///   interface's introspection output. Particularly useful for multi-interface services.
/// * `#[zlink(rename = "MethodName")]` - Custom Varlink method name.
///
/// ## On parameters:
///
/// * `#[zlink(rename = "paramName")]` - Custom serialized parameter name. **Required** for
///   parameter names starting with `_` (the Rust convention for unused parameters), since such
///   names are not valid in the Varlink IDL.
/// * `#[zlink(connection)]` - Mark this parameter to receive a mutable reference to the connection.
///   This is useful for accessing peer credentials or other connection-specific functionality.
///   **Requires an explicit generic socket type parameter** (e.g., `impl<Sock> MyService`).
///
/// # Compile-time Type Checking
///
/// The macro verifies at compile time that any custom type referenced in a method's input *or*
/// output parameters is declared in `types = [...]`. If you forget to list a type, you'll get
/// a compile error:
///
/// ```rust,compile_fail
/// use serde::{Deserialize, Serialize};
/// use zlink::{introspect::CustomType, service};
///
/// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
/// struct Book { title: String }
///
/// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
/// struct BookList { books: Vec<Book> }
///
/// struct LibraryService;
///
/// // Missing `BookList` in `types = [...]` even though it's returned by `list_books`.
/// #[service(interface = "org.example.library", types = [Book])]
/// impl LibraryService {
///     async fn list_books(&self) -> BookList {
///         BookList { books: vec![] }
///     }
/// }
/// ```
///
/// Similarly, parameter names starting with `_` are rejected because they are not valid in the
/// Varlink IDL. Specify the wire name explicitly with `#[zlink(rename = "...")]`.
///
/// # Generated Code
///
/// The macro generates an `impl<Sock: Socket> Service<Sock> for YourType` with the `handle` method,
/// along with internal helper types for serialization/deserialization.
///
/// # Error Handling
///
/// Methods can return `Result<T, E>` with any error type `E` that implements `Serialize` and
/// `Debug`. Different methods can use different error types - the macro automatically generates
/// internal wrapper types to handle all unique error types.
///
/// When a method returns `Err(e)`, the macro generates code that wraps it in the appropriate
/// combo enum variant and returns `MethodReply::Error(...)`.
/// When a method returns `Ok(v)`, it returns `MethodReply::Single(Some(v))`.
///
/// Methods can also return plain values (not wrapped in Result) - these are always treated as
/// successful responses.
///
/// # Custom Socket Bounds
///
/// By default, the generated `Service` impl uses a generic socket parameter with just the `Socket`
/// bound. If you need additional bounds (e.g., for peer credential checking), you can provide your
/// own generics on the impl block:
///
/// ```rust
/// use zlink::{service, connection::socket::FetchPeerCredentials, introspect};
///
/// struct MyService;
///
/// #[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
/// #[zlink(interface = "org.example.service")]
/// enum MyError {
///     ServiceError,
/// }
///
/// #[service]
/// impl<Sock> MyService
/// where
///     Sock::ReadHalf: FetchPeerCredentials,
/// {
///     #[zlink(interface = "org.example.service")]
///     async fn get_status(&self) -> Result<(), MyError> {
///         Ok(())
///     }
/// }
/// ```
///
/// The first type parameter is used as the socket type for the generated `Service` impl. The
/// `Socket` bound is automatically added, so you only need to specify additional bounds.
///
/// # Connection Parameter
///
/// Methods can receive a mutable reference to the connection using `#[zlink(connection)]`:
///
/// ```rust
/// use zlink::{service, Connection, connection::socket::FetchPeerCredentials, introspect};
///
/// struct MyService;
///
/// #[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
/// #[zlink(interface = "org.example.service")]
/// enum MyError {
///     ServiceError,
/// }
///
/// #[service]
/// impl<Sock> MyService
/// where
///     Sock::ReadHalf: FetchPeerCredentials,
/// {
///     #[zlink(interface = "org.example.service")]
///     async fn check_credentials(
///         &self,
///         #[zlink(connection)] conn: &mut Connection<Sock>,
///     ) -> Result<(), MyError> {
///         let _creds = conn.peer_credentials().await;
///         Ok(())
///     }
/// }
/// ```
///
/// Methods with connection parameters are only callable through the `Service` trait (not directly
/// on the type), since they require the socket type to be known.
///
/// # Streaming Methods
///
/// Methods that send multiple replies (the Varlink `more` flag) are marked with `#[zlink(more)]`.
/// Such a method must:
///
/// - Take `more: bool` as the first parameter after `self`. This receives the value of the call's
///   `more` flag, allowing the method to behave differently when the client only wants a single
///   reply.
/// - Return `impl Stream<Item = Reply<T>>` (or `impl Stream<Item = (Reply<T>, Vec<OwnedFd>)>` when
///   combined with `#[zlink(return_fds)]`). A concrete stream type is also accepted; in that case
///   the macro infers `Reply<T>` from the type's first generic parameter.
/// - Set `Reply::set_continues(Some(true))` on every intermediate item and `Some(false)` on the
///   final one so that the client knows when the stream ends.
///
/// ## Returning Method Errors From Streams
///
/// Streaming methods can also opt in to emitting Varlink error replies as stream items. To do so,
/// return a stream whose item is `Result<Reply<T>, E>` (or `(Result<Reply<T>, E>, Vec<OwnedFd>)`
/// for `#[zlink(return_fds)]`). When the stream yields `Err(e)`, the server sends an error reply
/// to the client. The error type `E` must implement `serde::Serialize` and `Debug`, just like for
/// non-streaming methods, and (because the stream outlives `&self`) cannot borrow from the
/// service. Different streaming methods may use different error types; the macro combines them
/// into a single internal enum exposed as `Service::ReplyStreamError`.
///
/// Note that, on the wire, an error reply terminates the stream — a client that receives an
/// error reply should not expect any further items. The macro does not enforce this on the
/// server side, so callers can still drain the rest of the stream if the server emits more items
/// after an error.
///
/// ## Example
///
/// ```rust
/// use futures_util::Stream;
/// use serde::{Deserialize, Serialize};
/// use zlink::{introspect::Type, service, Reply};
///
/// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
/// struct Tick {
///     value: u32,
/// }
///
/// struct Counter;
///
/// #[service(interface = "org.example.counter")]
/// impl Counter {
///     // Streams `Tick` values from 1 to `to`. If the caller did not set the `more` flag, only a
///     // single tick is emitted.
///     #[zlink(more)]
///     async fn count(
///         &self,
///         more: bool,
///         to: u32,
///     ) -> impl Stream<Item = Reply<Tick>> + Unpin {
///         let to = if more { to.max(1) } else { 1 };
///         futures_util::stream::iter((1..=to).map(move |value| {
///             Reply::new(Some(Tick { value })).set_continues(Some(value < to))
///         }))
///     }
/// }
/// ```
///
/// ## Streaming With Errors
///
/// ```rust
/// use futures_util::Stream;
/// use serde::{Deserialize, Serialize};
/// use zlink::{introspect, service, Reply};
///
/// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, introspect::Type)]
/// struct Tick { value: u32 }
///
/// #[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
/// #[zlink(interface = "org.example.counter")]
/// enum CountError {
///     AtZero,
/// }
///
/// struct Counter;
///
/// #[service(interface = "org.example.counter")]
/// impl Counter {
///     #[zlink(more)]
///     async fn count(
///         &self,
///         _more: bool,
///         to: u32,
///     ) -> impl Stream<Item = Result<Reply<Tick>, CountError>> + Unpin {
///         if to == 0 {
///             return futures_util::stream::iter(vec![Err(CountError::AtZero)]);
///         }
///         let last = to;
///         let items: Vec<Result<Reply<Tick>, CountError>> = (1..=to)
///             .map(move |value| {
///                 Ok(Reply::new(Some(Tick { value })).set_continues(Some(value < last)))
///             })
///             .collect();
///         futures_util::stream::iter(items)
///     }
/// }
/// ```
///
/// # Example
///
/// ```rust
/// use zlink::{
///     introspect::{self, Type},
///     service,
///     tokio::unix::{bind, connect},
///     Server,
/// };
/// use serde::{Deserialize, Serialize};
///
/// // Response type for balance operations.
/// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
/// struct Balance {
///     amount: i64,
/// }
///
/// // Error type - must derive zlink::ReplyError for proper serialization.
/// #[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
/// #[zlink(interface = "org.example.bank")]
/// enum BankError {
///     InsufficientFunds { available: i64, requested: i64 },
///     InvalidAmount { amount: i64 },
///     AccountLocked,
/// }
///
/// struct BankAccount {
///     balance: i64,
///     locked: bool,
/// }
///
/// impl BankAccount {
///     fn new(initial_balance: i64) -> Self {
///         Self { balance: initial_balance, locked: false }
///     }
/// }
///
/// // Service implementation with error handling.
/// #[service]
/// impl BankAccount {
///     // Method that returns a plain value (not Result) - always succeeds.
///     #[zlink(interface = "org.example.bank")]
///     async fn get_balance(&self) -> Balance {
///         Balance { amount: self.balance }
///     }
///
///     // Method that can fail - returns Result<Balance, BankError>.
///     async fn deposit(&mut self, amount: i64) -> Result<Balance, BankError> {
///         if self.locked {
///             return Err(BankError::AccountLocked);
///         }
///         if amount <= 0 {
///             return Err(BankError::InvalidAmount { amount });
///         }
///         self.balance += amount;
///         Ok(Balance { amount: self.balance })
///     }
///
///     async fn withdraw(&mut self, amount: i64) -> Result<Balance, BankError> {
///         if self.locked {
///             return Err(BankError::AccountLocked);
///         }
///         if amount <= 0 {
///             return Err(BankError::InvalidAmount { amount });
///         }
///         if amount > self.balance {
///             return Err(BankError::InsufficientFunds {
///                 available: self.balance,
///                 requested: amount,
///             });
///         }
///         self.balance -= amount;
///         Ok(Balance { amount: self.balance })
///     }
///
///     // Method returning Result<(), E> - void success, can fail.
///     async fn lock_account(&mut self) -> Result<(), BankError> {
///         if self.locked {
///             return Err(BankError::AccountLocked);
///         }
///         self.locked = true;
///         Ok(())
///     }
/// }
///
/// // Client-side proxy definition.
/// #[zlink::proxy("org.example.bank")]
/// trait BankProxy {
///     async fn get_balance(&mut self) -> zlink::Result<Result<Balance, BankError>>;
///     async fn deposit(&mut self, amount: i64) -> zlink::Result<Result<Balance, BankError>>;
///     async fn withdraw(&mut self, amount: i64) -> zlink::Result<Result<Balance, BankError>>;
///     async fn lock_account(&mut self) -> zlink::Result<Result<(), BankError>>;
/// }
///
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// // Server setup.
/// let dir = tempfile::tempdir()?;
/// let socket_path = dir.path().join("service-example.sock");
/// let listener = bind(&socket_path)?;
/// let service = BankAccount::new(1000);
/// let server = Server::new(listener, service);
///
/// // Run server and client concurrently.
/// tokio::select! {
///     res = server.run() => res?,
///     res = async {
///         let mut conn = connect(&socket_path).await?;
///
///         // Check initial balance.
///         let balance = conn.get_balance().await?.unwrap();
///         assert_eq!(balance.amount, 1000);
///
///         // Successful deposit.
///         let balance = conn.deposit(500).await?.unwrap();
///         assert_eq!(balance.amount, 1500);
///
///         // Successful withdrawal.
///         let balance = conn.withdraw(200).await?.unwrap();
///         assert_eq!(balance.amount, 1300);
///
///         // Error: withdraw more than available.
///         let err = conn.withdraw(5000).await?.unwrap_err();
///         assert_eq!(err, BankError::InsufficientFunds { available: 1300, requested: 5000 });
///
///         // Error: invalid amount.
///         let err = conn.deposit(-100).await?.unwrap_err();
///         assert_eq!(err, BankError::InvalidAmount { amount: -100 });
///
///         // Lock account and verify subsequent operations fail.
///         conn.lock_account().await?.unwrap();
///         let err = conn.withdraw(100).await?.unwrap_err();
///         assert_eq!(err, BankError::AccountLocked);
///
///         Ok::<(), Box<dyn std::error::Error>>(())
///     } => res?,
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # })?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Introspection Example
///
/// The service automatically provides introspection via the `org.varlink.service` interface:
///
/// ```rust
/// use zlink::{
///     introspect::{self, CustomType, Type},
///     service,
///     varlink_service::Proxy,
/// };
/// use serde::{Deserialize, Serialize};
///
/// // Custom type - must derive CustomType to be included in IDL.
/// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
/// struct Balance {
///     amount: i64,
/// }
///
/// #[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
/// #[zlink(interface = "org.example.bank")]
/// enum BankError {
///     InsufficientFunds { available: i64 },
/// }
///
/// struct BankService;
///
/// // Include custom types in the service for IDL generation.
/// #[service(
///     types = [Balance],
///     vendor = "Example Corp",
///     product = "Bank Service",
///     version = env!("CARGO_PKG_VERSION"),
///     url = "https://example.com/bank"
/// )]
/// impl BankService {
///     #[zlink(interface = "org.example.bank")]
///     async fn get_balance(&self) -> Result<Balance, BankError> {
///         Ok(Balance { amount: 1000 })
///     }
/// }
///
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use zlink::test_utils::mock_socket::MockSocket;
/// // GetInfo returns metadata and list of interfaces.
/// # let response = format!(
/// #     r#"{{"parameters":{{"vendor":"Example Corp","product":"Bank Service","version":"{}","url":"https://example.com/bank","interfaces":["org.example.bank","org.varlink.service"]}}}}"#,
/// #     env!("CARGO_PKG_VERSION"),
/// # );
/// # let socket = MockSocket::with_responses(&[&response]);
/// # let mut conn = zlink::Connection::new(socket);
/// let info = conn.get_info().await?.unwrap();
/// assert_eq!(info.vendor, "Example Corp");
/// let interfaces: Vec<&str> = info.interfaces.iter().map(|s| s.as_ref()).collect();
/// assert_eq!(interfaces.as_slice(), ["org.example.bank", "org.varlink.service"]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
///
/// # tokio::runtime::Runtime::new().unwrap().block_on(async {
/// # use zlink::test_utils::mock_socket::MockSocket;
/// // GetInterfaceDescription returns the IDL, which can be parsed to verify methods and types.
/// # let responses = [
/// #     r#"{"parameters":{"description":"interface org.example.bank\n\ntype Balance (amount: int)\n\nmethod GetBalance() -> (amount: int)\n\nerror InsufficientFunds (available: int)"}}"#,
/// # ];
/// # let socket = MockSocket::with_responses(&responses);
/// # let mut conn = zlink::Connection::new(socket);
/// let desc = conn.get_interface_description("org.example.bank").await?.unwrap();
/// let interface = desc.parse()?;
/// assert_eq!(interface.name().as_str(), "org.example.bank");
///
/// // Verify methods are present.
/// let method_names: Vec<_> = interface.methods().map(|m| m.name()).collect();
/// assert_eq!(method_names.as_slice(), ["GetBalance"]);
///
/// // Verify custom types are included.
/// let type_names: Vec<_> = interface.custom_types().map(|t| t.name()).collect();
/// assert_eq!(type_names.as_slice(), ["Balance"]);
///
/// // Verify errors are present.
/// let error_names: Vec<_> = interface.errors().map(|e| e.name()).collect();
/// assert_eq!(error_names.as_slice(), ["InsufficientFunds"]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// ```
///
/// # Method Name Conversion
///
/// By default, method names are converted from snake_case to PascalCase for the Varlink call.
/// For example, `get_balance` becomes `GetBalance`. Use `#[zlink(rename = "...")]` to override
/// this.
///
/// # Full Method Path
///
/// The full Varlink method path is constructed as `{interface}.{MethodName}`. For example,
/// if the interface is `org.example.bank` and the method is `GetBalance`, the full path
/// will be `org.example.bank.GetBalance`.
///
/// # Interface Propagation
///
/// The interface for methods is determined in this order:
/// 1. If the method has `#[zlink(interface = "...")]`, that interface is used.
/// 2. Otherwise, the interface is inherited from the previous method or from the macro-level
///    `interface = "..."` attribute.
///
/// For services implementing a single interface, specifying `interface = "..."` at the macro level
/// is the simplest approach - all methods automatically use that interface without needing
/// individual attributes.
#[cfg(feature = "service")]
#[proc_macro_attribute]
pub fn service(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    service::service(attr.into(), input.into()).into()
}
