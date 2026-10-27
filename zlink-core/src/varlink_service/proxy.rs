//! Client-side proxy API for the `org.varlink.service` interface.
//!
//! This module provides the [`Proxy`] trait which offers convenient methods to call
//! the standard Varlink service interface methods on any connection.

use crate::proxy;

use super::{Error, Info, InterfaceDescription, OwnedError, OwnedInfo};
use crate::names::InterfaceName;

/// Client-side proxy for the `org.varlink.service` interface.
///
/// This trait provides methods to call the standard Varlink service interface methods on a
/// connection.
///
/// # Borrowed vs Owned Methods
///
/// The trait provides both borrowed and owned variants of each method:
///
/// - **Borrowed methods** (`get_info`, `get_interface_description`): Return borrowed types for
///   efficient zero-copy deserialization. These are preferred for single calls.
///
/// - **Owned methods** (`owned_get_info`, `owned_get_interface_description`): Return owned types
///   ([`OwnedInfo`], [`OwnedError`]) required for the chain API. Chain methods (`chain_*`) are
///   generated only for these variants since `DeserializeOwned` is required for pipelining.
///
/// # Example: Basic Usage
///
/// ```no_run
/// use zlink_core::{Connection, varlink_service::Proxy};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let mut conn: Connection<zlink_core::connection::socket::impl_for_doc::Socket> = todo!();
/// // Get service information (borrowed - zero-copy).
/// let info = conn.get_info().await?.map_err(|e| e.to_string())?;
/// println!("Service: {} v{} by {}", info.product, info.version, info.vendor);
/// println!("URL: {}", info.url);
/// println!("Interfaces: {:?}", info.interfaces);
///
/// // Get interface description.
/// let desc = conn
///     .get_interface_description("org.varlink.service".try_into()?)
///     .await?
///     .map_err(|e| e.to_string())?;
/// println!("Interface description: {}", desc.as_raw().unwrap());
///
/// # Ok(())
/// # }
/// ```
///
/// # Example: Chaining (Pipelining)
///
/// ```no_run
/// use zlink_core::{
///     Connection,
///     varlink_service::{Chain, OwnedError, OwnedReply, Proxy},
/// };
/// use futures_util::{pin_mut, stream::StreamExt};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let mut conn: Connection<zlink_core::connection::socket::impl_for_doc::Socket> = todo!();
/// // Chain multiple calls using owned methods (required for pipelining).
/// let chain = conn
///     .chain_owned_get_info()?
///     .owned_get_interface_description("org.example.interface".try_into()?)?
///     .owned_get_info()?;
///
/// // Send the chain and process replies.
/// let replies = chain.send::<OwnedReply, OwnedError>().await?;
/// pin_mut!(replies);
///
/// // Process each reply in the order they were chained.
/// while let Some(result) = replies.next().await {
///     let (reply, _fds) = result?;
///     match reply.unwrap().into_parameters().unwrap() {
///         OwnedReply::Info(info) => {
///             println!("Service: {} v{} by {}", info.product, info.version, info.vendor);
///         }
///         OwnedReply::InterfaceDescription(desc) => {
///             println!("Interface description: {}", desc.as_raw().unwrap());
///         }
///     }
/// }
///
/// # Ok(())
/// # }
/// ```
#[proxy(
    interface = "org.varlink.service",
    crate = "crate",
    chain_name = "Chain"
)]
#[cfg(feature = "std")]
pub trait Proxy {
    /// Get information about a Varlink service.
    ///
    /// This method uses borrowed types for zero-copy deserialization. For chaining (pipelining),
    /// use [`owned_get_info`](Self::owned_get_info) instead.
    ///
    /// # Returns
    ///
    /// Two-layer result: outer for connection errors, inner for method errors. On success, contains
    /// service information as [`Info`].
    async fn get_info(&mut self) -> crate::Result<core::result::Result<Info<'_>, Error<'_>>>;

    /// Get information about a Varlink service (owned variant for chain API).
    ///
    /// This method returns owned types, which is required for the chain API (pipelining).
    /// For single calls, prefer [`get_info`](Self::get_info) for zero-copy deserialization.
    ///
    /// # Returns
    ///
    /// Two-layer result: outer for connection errors, inner for method errors. On success, contains
    /// service information as [`OwnedInfo`].
    #[zlink(rename = "GetInfo")]
    async fn owned_get_info(
        &mut self,
    ) -> crate::Result<core::result::Result<OwnedInfo, OwnedError>>;

    /// Get the IDL description of an interface.
    ///
    /// This method uses borrowed types for zero-copy deserialization. For chaining (pipelining),
    /// use [`owned_get_interface_description`](Self::owned_get_interface_description) instead.
    ///
    /// # Arguments
    ///
    /// * `interface` - The name of the interface to get the description for.
    ///
    /// # Returns
    ///
    /// Two-layer result: outer for connection errors, inner for method errors. On success, contains
    /// the unparsed interface definition as a [`InterfaceDescription`]. Use
    /// [`InterfaceDescription::parse`] to parse it.
    async fn get_interface_description(
        &mut self,
        interface: InterfaceName<'_>,
    ) -> crate::Result<core::result::Result<InterfaceDescription<'static>, Error<'_>>>;

    /// Get the IDL description of an interface (owned variant for chain API).
    ///
    /// This method returns owned types, which is required for the chain API (pipelining).
    /// For single calls, prefer [`get_interface_description`](Self::get_interface_description)
    /// for zero-copy deserialization.
    ///
    /// # Arguments
    ///
    /// * `interface` - The name of the interface to get the description for.
    ///
    /// # Returns
    ///
    /// Two-layer result: outer for connection errors, inner for method errors. On success, contains
    /// the unparsed interface definition as a [`InterfaceDescription`]. Use
    /// [`InterfaceDescription::parse`] to parse it.
    #[zlink(rename = "GetInterfaceDescription")]
    async fn owned_get_interface_description(
        &mut self,
        interface: InterfaceName<'_>,
    ) -> crate::Result<core::result::Result<InterfaceDescription<'static>, OwnedError>>;
}

#[cfg(test)]
mod tests {
    use super::{super::OwnedReply, *};
    use crate::{Connection, test_utils::mock_socket::MockSocket};
    use futures_util::{pin_mut, stream::StreamExt};

    #[tokio::test]
    async fn chain_api_creation() -> crate::Result<()> {
        // Test that we can create chains with the owned API.
        let responses = [
            r#"{"parameters":{"vendor":"Test","product":"TestProduct","version":"1.0","url":"https://test.com","interfaces":["org.varlink.service"]}}"#,
            r#"{"parameters":{"description":"interface org.varlink.service {}"}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        // Test that we can create the chain APIs.
        let _chain1 = conn.chain_owned_get_info()?;
        let _chain2 =
            conn.chain_owned_get_interface_description("org.varlink.service".try_into().unwrap())?;

        Ok(())
    }

    #[tokio::test]
    async fn chain_extension_methods() -> crate::Result<()> {
        // Test that we can use chain extension methods.
        let responses = [
            r#"{"parameters":{"vendor":"Test","product":"TestProduct","version":"1.0","url":"https://test.com","interfaces":["org.varlink.service"]}}"#,
            r#"{"parameters":{"description":"interface org.varlink.service {}"}}"#,
            r#"{"parameters":{"vendor":"Test","product":"TestProduct","version":"1.0","url":"https://test.com","interfaces":["org.varlink.service"]}}"#,
        ];
        let socket = MockSocket::with_responses(&responses);
        let mut conn = Connection::new(socket);

        // Test that we can chain calls using extension methods and actually read replies.
        let chained = conn
            .chain_owned_get_info()?
            .owned_get_interface_description("org.varlink.service".try_into().unwrap())?
            .owned_get_info()?;

        let replies = chained.send::<OwnedReply, OwnedError>().await?;
        pin_mut!(replies);

        // Read first reply (GetInfo).
        let (first_reply, _fds) = replies.next().await.unwrap()?;
        let first_reply = first_reply.unwrap();
        match first_reply.into_parameters().unwrap() {
            OwnedReply::Info(info) => {
                assert_eq!(info.vendor, "Test");
                assert_eq!(info.product, "TestProduct");
                assert_eq!(info.version, "1.0");
                assert_eq!(info.url, "https://test.com");
                assert_eq!(info.interfaces, ["org.varlink.service"]);
            }
            _ => panic!("Expected Info reply"),
        }

        // Read second reply (GetInterfaceDescription).
        let (second_reply, _fds) = replies.next().await.unwrap()?;
        let second_reply = second_reply.unwrap();
        match second_reply.into_parameters().unwrap() {
            OwnedReply::InterfaceDescription(desc) => {
                assert_eq!(desc.as_raw().unwrap(), "interface org.varlink.service {}");
            }
            _ => panic!("Expected InterfaceDescription reply"),
        }

        // Read third reply (GetInfo again).
        let (third_reply, _fds) = replies.next().await.unwrap()?;
        let third_reply = third_reply.unwrap();
        match third_reply.into_parameters().unwrap() {
            OwnedReply::Info(info) => {
                assert_eq!(info.vendor, "Test");
            }
            _ => panic!("Expected Info reply"),
        }

        // No more replies.
        assert!(replies.next().await.is_none());

        Ok(())
    }
}
