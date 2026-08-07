#[cfg(feature = "introspection")]
use crate::introspect::Type;
use alloc::{borrow::Cow, vec::Vec};
use serde::{Deserialize, Serialize};
use zlink_names::InterfaceName;

/// Information about a Varlink service implementation.
///
/// This is the return type for the `GetInfo` method of the `org.varlink.service` interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "introspection", derive(Type))]
#[cfg_attr(feature = "introspection", zlink(crate = "crate"))]
pub struct Info<'a> {
    /// The vendor of the service.
    #[serde(borrow)]
    pub vendor: Cow<'a, str>,
    /// The product name of the service.
    #[serde(borrow)]
    pub product: Cow<'a, str>,
    /// The version of the service.
    #[serde(borrow)]
    pub version: Cow<'a, str>,
    /// The URL associated with the service.
    #[serde(borrow)]
    pub url: Cow<'a, str>,
    /// List of interfaces provided by the service.
    #[serde(borrow)]
    pub interfaces: Vec<InterfaceName<'a>>,
}

impl<'a> Info<'a> {
    /// Create a new `Info` instance.
    pub fn new(
        vendor: impl Into<Cow<'a, str>>,
        product: impl Into<Cow<'a, str>>,
        version: impl Into<Cow<'a, str>>,
        url: impl Into<Cow<'a, str>>,
        interfaces: impl IntoIterator<Item = impl Into<InterfaceName<'static>>>,
    ) -> Self {
        Self {
            vendor: vendor.into(),
            product: product.into(),
            version: version.into(),
            url: url.into(),
            interfaces: interfaces.into_iter().map(Into::into).collect(),
        }
    }

    /// Internal function only. Do not call this directly in your own code!
    pub fn from_static_str_unchecked(
        vendor: impl Into<Cow<'a, str>>,
        product: impl Into<Cow<'a, str>>,
        version: impl Into<Cow<'a, str>>,
        url: impl Into<Cow<'a, str>>,
        interfaces: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        let interfaces = interfaces
            .into_iter()
            .map(InterfaceName::from_static_str_unchecked);
        Self::new(vendor, product, version, url, interfaces)
    }

    /// Convert this info into an owned version with `'static` lifetime.
    pub fn into_owned(self) -> Info<'static> {
        Info {
            vendor: Cow::Owned(self.vendor.into_owned()),
            product: Cow::Owned(self.product.into_owned()),
            version: Cow::Owned(self.version.into_owned()),
            url: Cow::Owned(self.url.into_owned()),
            interfaces: self
                .interfaces
                .into_iter()
                .map(InterfaceName::into_owned)
                .collect(),
        }
    }
}

/// Owned version of [`Info`] for use with the chain API.
///
/// This is a newtype wrapper around `Info<'static>`, allowing it to be deserialized as owned data.
/// This is required for the chain API because the internal buffer may be reused between stream
/// iterations.
///
/// # Example
///
/// ```no_run
/// use zlink_core::varlink_service::OwnedInfo;
///
/// // OwnedInfo can be deserialized from JSON without borrowing.
/// let json = r#"{"vendor":"Test","product":"Product","version":"1.0","url":"https://example.com","interfaces":["org.varlink.service"]}"#;
/// let info: OwnedInfo = serde_json::from_str(json).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OwnedInfo(Info<'static>);

impl OwnedInfo {
    /// Create a new `OwnedInfo` instance.
    pub fn new(
        vendor: impl Into<Cow<'static, str>>,
        product: impl Into<Cow<'static, str>>,
        version: impl Into<Cow<'static, str>>,
        url: impl Into<Cow<'static, str>>,
        interfaces: impl IntoIterator<Item = impl Into<InterfaceName<'static>>>,
    ) -> Self {
        Self(Info::new(vendor, product, version, url, interfaces))
    }

    /// Internal function only. Do not call this directly in your own code!
    pub fn from_static_str_unchecked(
        vendor: impl Into<Cow<'static, str>>,
        product: impl Into<Cow<'static, str>>,
        version: impl Into<Cow<'static, str>>,
        url: impl Into<Cow<'static, str>>,
        interfaces: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        let interfaces = interfaces
            .into_iter()
            .map(InterfaceName::from_static_str_unchecked);
        Self::new(vendor, product, version, url, interfaces)
    }

    /// Returns a reference to the inner `Info`.
    pub fn inner(&self) -> &Info<'static> {
        &self.0
    }

    /// Consumes self and returns the inner `Info`.
    pub fn into_inner(self) -> Info<'static> {
        self.0
    }
}

impl core::ops::Deref for OwnedInfo {
    type Target = Info<'static>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for OwnedInfo {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a> From<Info<'a>> for OwnedInfo {
    fn from(info: Info<'a>) -> Self {
        Self(info.into_owned())
    }
}

impl<'de> Deserialize<'de> for OwnedInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use alloc::string::String;
        use zlink_names::OwnedInterfaceName;

        // Deserialize into owned names directly.
        #[derive(Deserialize)]
        struct InfoOwned {
            vendor: String,
            product: String,
            version: String,
            url: String,
            interfaces: Vec<OwnedInterfaceName>,
        }

        let info = InfoOwned::deserialize(deserializer)?;
        Ok(Self(Info {
            vendor: Cow::Owned(info.vendor),
            product: Cow::Owned(info.product),
            version: Cow::Owned(info.version),
            url: Cow::Owned(info.url),
            interfaces: info
                .interfaces
                .into_iter()
                .map(InterfaceName::from)
                .collect(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization() {
        let interfaces = alloc::vec!["com.example.test"];

        let info = Info::from_static_str_unchecked(
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
        assert_eq!(info.interfaces[0].as_str(), "com.example.test");
        assert_eq!(info.interfaces[1].as_str(), "com.example.other");
    }

    #[test]
    fn round_trip_serialization() {
        let interfaces = alloc::vec!["com.example.test", "com.example.other"];

        let original = Info::from_static_str_unchecked(
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
