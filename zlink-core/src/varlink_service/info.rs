#[cfg(feature = "introspection")]
use crate::introspect::Type;
use mayheap::Vec;
use serde::{Deserialize, Serialize};

/// Information about a Varlink service implementation.
///
/// This is the return type for the `GetInfo` method of the `org.varlink.service` interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "introspection", derive(Type))]
#[cfg_attr(feature = "introspection", zlink(crate = "crate"))]
pub struct Info<'a> {
    /// The vendor of the service.
    pub vendor: &'a str,
    /// The product name of the service.
    pub product: &'a str,
    /// The version of the service.
    pub version: &'a str,
    /// The URL associated with the service.
    pub url: &'a str,
    /// List of interfaces provided by the service.
    pub interfaces: Vec<&'a str, 8>,
}

impl<'a> Info<'a> {
    /// Create a new `Info` instance.
    pub fn new(
        vendor: &'a str,
        product: &'a str,
        version: &'a str,
        url: &'a str,
        interfaces: Vec<&'a str, 8>,
    ) -> Self {
        Self {
            vendor,
            product,
            version,
            url,
            interfaces,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization() {
        let mut interfaces = Vec::new();
        interfaces.push("com.example.test").unwrap();

        let info = Info::new(
            "Test Vendor",
            "Test Product",
            "1.0.0",
            "https://example.com",
            interfaces,
        );

        let json = serde_json::to_string(&info).unwrap();

        assert!(json.contains("Test Vendor"));
        assert!(json.contains("com.example.test"));
    }

    #[test]
    fn deserialization() {
        let json = r#"{
            "vendor": "Test Vendor",
            "product": "Test Product",
            "version": "1.0.0",
            "url": "https://example.com",
            "interfaces": ["com.example.test", "com.example.other"]
        }"#;

        let info: Info<'_> = serde_json::from_str(json).unwrap();

        assert_eq!(info.vendor, "Test Vendor");
        assert_eq!(info.product, "Test Product");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.url, "https://example.com");
        assert_eq!(info.interfaces.len(), 2);
        assert_eq!(info.interfaces[0], "com.example.test");
        assert_eq!(info.interfaces[1], "com.example.other");
    }

    #[test]
    fn round_trip_serialization() {
        let mut interfaces = Vec::new();
        interfaces.push("com.example.test").unwrap();
        interfaces.push("com.example.other").unwrap();

        let original = Info::new(
            "Test Vendor",
            "Test Product",
            "1.0.0",
            "https://example.com",
            interfaces,
        );

        // Serialize to JSON
        let json = serde_json::to_string(&original).unwrap();

        // Deserialize back from JSON
        let deserialized: Info<'_> = serde_json::from_str(&json).unwrap();

        // Verify they are equal
        assert_eq!(original, deserialized);
    }
}
