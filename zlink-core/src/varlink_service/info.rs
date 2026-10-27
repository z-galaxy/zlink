#[cfg(feature = "introspection")]
use crate::introspect::Type;
use crate::names::{InterfaceName, OwnedInterfaceName};
use alloc::{borrow::Cow, vec::Vec};
use serde::{Deserialize, Serialize};

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
    ///
    /// The interface names can be anything that converts into an [`InterfaceName`], e.g. string
    /// slices, which are validated, or `InterfaceName`s.
    ///
    /// # Errors
    ///
    /// If one of the interface names doesn't convert into an [`InterfaceName`], e.g. because it
    /// is a string that is not a valid interface name.
    pub fn new<I>(
        vendor: impl Into<Cow<'a, str>>,
        product: impl Into<Cow<'a, str>>,
        version: impl Into<Cow<'a, str>>,
        url: impl Into<Cow<'a, str>>,
        interfaces: I,
    ) -> Result<Self, <I::Item as TryInto<InterfaceName<'a>>>::Error>
    where
        I: IntoIterator,
        I::Item: TryInto<InterfaceName<'a>>,
    {
        Ok(Self {
            vendor: vendor.into(),
            product: product.into(),
            version: version.into(),
            url: url.into(),
            interfaces: interfaces
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        })
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
                .map(|name| name.into_owned().into_inner())
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
    ///
    /// The interface names can be anything that converts into an [`InterfaceName`], e.g. string
    /// slices, which are validated, or `InterfaceName`s.
    ///
    /// # Errors
    ///
    /// If one of the interface names doesn't convert into an [`InterfaceName`], e.g. because it
    /// is a string that is not a valid interface name.
    pub fn new<I>(
        vendor: impl Into<Cow<'static, str>>,
        product: impl Into<Cow<'static, str>>,
        version: impl Into<Cow<'static, str>>,
        url: impl Into<Cow<'static, str>>,
        interfaces: I,
    ) -> Result<Self, <I::Item as TryInto<InterfaceName<'static>>>::Error>
    where
        I: IntoIterator,
        I::Item: TryInto<InterfaceName<'static>>,
    {
        Info::new(vendor, product, version, url, interfaces).map(Self)
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

        // Deserialize into owned strings directly.
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
                .map(OwnedInterfaceName::into_inner)
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

        let info = Info::new(
            "Test Vendor",
            "Test Product",
            "1.0.0",
            "https://example.com",
            interfaces,
        )
        .unwrap();

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
        let interfaces = alloc::vec!["com.example.test", "com.example.other"];

        let original = Info::new(
            "Test Vendor",
            "Test Product",
            "1.0.0",
            "https://example.com",
            interfaces,
        )
        .unwrap();

        // Serialize to JSON
        let json = serde_json::to_string(&original).unwrap();

        // Deserialize back from JSON
        let deserialized: Info<'_> = serde_json::from_str(&json).unwrap();

        // Verify they are equal
        assert_eq!(original, deserialized);
    }

    #[test]
    fn invalid_interface_names() {
        // The constructor validates the names it converts.
        assert_eq!(
            Info::new("Vendor", "Product", "1.0", "https://example.com", ["foo"]).unwrap_err(),
            crate::names::Error::InvalidInterfaceName
        );

        // Names that are `InterfaceName`s already need no conversion, so it can't fail.
        let name = InterfaceName::try_from("com.example.test").unwrap();
        let Ok(info) = Info::new("Vendor", "Product", "1.0", "https://example.com", [name]);
        assert_eq!(info.interfaces, ["com.example.test"]);

        // Deserialization validates the names too.
        let json = r#"{
            "vendor": "Test Vendor",
            "product": "Test Product",
            "version": "1.0.0",
            "url": "https://example.com",
            "interfaces": ["com.example.test", "foo"]
        }"#;
        let err = serde_json::from_str::<Info<'_>>(json).unwrap_err();
        assert!(
            err.to_string()
                .contains("expected a Varlink interface name"),
            "unexpected error: {err}"
        );
    }

    // Whichever form the interface names come in, the error of the constructor converts into the
    // crate's own.
    #[test]
    fn constructor_error_conversion() -> crate::Result<()> {
        let info = Info::new("Vendor", "Product", "1.0", "", ["com.example.test"])?;
        let name = InterfaceName::try_from("com.example.test")?;
        assert_eq!(Info::new("Vendor", "Product", "1.0", "", [name])?, info);
        let owned = OwnedInfo::new("Vendor", "Product", "1.0", "", ["com.example.test"])?;
        let name = InterfaceName::try_from("com.example.test")?;
        assert_eq!(
            OwnedInfo::new("Vendor", "Product", "1.0", "", [name])?,
            owned
        );

        Ok(())
    }

    #[test]
    fn owned_info() {
        let json = r#"{
            "vendor": "Test Vendor",
            "product": "Test Product",
            "version": "1.0.0",
            "url": "https://example.com",
            "interfaces": ["com.example.test", "com.example.other"]
        }"#;
        let info: OwnedInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.vendor, "Test Vendor");
        assert_eq!(info.interfaces, ["com.example.test", "com.example.other"]);

        // The names are validated, whether deserialized or passed to the constructor.
        let json = json.replace("com.example.other", "other");
        serde_json::from_str::<OwnedInfo>(&json).unwrap_err();
        assert_eq!(
            OwnedInfo::new("Vendor", "Product", "1.0", "https://example.com", ["other"])
                .unwrap_err(),
            crate::names::Error::InvalidInterfaceName
        );

        // An `Info` can be turned into an owned one, which no longer borrows.
        let source = alloc::string::String::from("com.example.test");
        let info = Info::new(
            "Vendor",
            "Product",
            "1.0",
            "https://example.com",
            [source.as_str()],
        )
        .unwrap();
        let owned = OwnedInfo::from(info);
        drop(source);
        assert_eq!(owned.interfaces, ["com.example.test"]);
    }
}
