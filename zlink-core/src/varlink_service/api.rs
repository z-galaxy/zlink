use alloc::borrow::Cow;
use serde::{Deserialize, Serialize};
use zlink_names::InterfaceName;

#[cfg(feature = "introspection")]
use crate::introspect;

use crate::ReplyError;

#[cfg(feature = "idl")]
use super::InterfaceDescription;
use super::{Info, OwnedInfo};

/// `org.varlink.service` interface methods.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "parameters")]
pub enum Method<'a> {
    /// Get information about the Varlink service.
    #[serde(rename = "org.varlink.service.GetInfo")]
    GetInfo,
    /// Get the description of the specified interface.
    #[serde(rename = "org.varlink.service.GetInterfaceDescription")]
    GetInterfaceDescription {
        /// The interface to get the description for.
        #[serde(borrow)]
        interface: InterfaceName<'a>,
    },
}

/// `org.varlink.service` interface replies.
///
/// This enum represents all possible replies from the varlink service interface methods.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "idl-parse", derive(Deserialize))]
#[serde(untagged)]
pub enum Reply<'a> {
    /// Reply for `GetInfo` method.
    #[serde(borrow)]
    Info(Info<'a>),
    /// Reply for `GetInterfaceDescription` method.
    /// Note: InterfaceDescription only supports 'static lifetime for deserialization.
    #[cfg(feature = "idl")]
    InterfaceDescription(InterfaceDescription<'static>),
}

/// Owned version of [`Reply`] for use with the chain API.
///
/// This type uses owned types ([`OwnedInfo`]) instead of borrowed types, allowing it to be
/// deserialized as owned data. This is required for the chain API because the internal buffer
/// may be reused between stream iterations.
#[derive(Debug, Serialize)]
#[cfg_attr(any(not(feature = "idl"), feature = "idl-parse"), derive(Deserialize))]
#[serde(untagged)]
pub enum OwnedReply {
    /// Reply for `GetInfo` method.
    Info(OwnedInfo),
    /// Reply for `GetInterfaceDescription` method.
    #[cfg(feature = "idl")]
    InterfaceDescription(InterfaceDescription<'static>),
}

#[cfg(feature = "idl")]
impl<'a> From<Reply<'a>> for OwnedReply {
    fn from(reply: Reply<'a>) -> Self {
        match reply {
            Reply::Info(info) => OwnedReply::Info(info.into()),
            Reply::InterfaceDescription(desc) => OwnedReply::InterfaceDescription(desc),
        }
    }
}

#[cfg(not(feature = "idl"))]
impl<'a> From<Reply<'a>> for OwnedReply {
    fn from(reply: Reply<'a>) -> Self {
        match reply {
            Reply::Info(info) => OwnedReply::Info(info.into()),
        }
    }
}

/// Errors that can be returned by the `org.varlink.service` interface.
#[derive(Debug, Clone, PartialEq, ReplyError)]
#[cfg_attr(feature = "introspection", derive(introspect::ReplyError))]
#[zlink(interface = "org.varlink.service")]
#[cfg_attr(feature = "introspection", zlink(crate = "crate"))]
pub enum Error<'a> {
    /// The requested interface was not found.
    InterfaceNotFound {
        /// The interface that was not found.
        #[zlink(borrow)]
        interface: Cow<'a, str>,
    },
    /// The requested method was not found.
    MethodNotFound {
        /// The method that was not found.
        #[zlink(borrow)]
        method: Cow<'a, str>,
    },
    /// The interface defines the requested method, but the service does not implement it.
    MethodNotImplemented {
        /// The method that is not implemented.
        #[zlink(borrow)]
        method: Cow<'a, str>,
    },
    /// One of the passed parameters is invalid.
    InvalidParameter {
        /// The parameter that is invalid.
        #[zlink(borrow)]
        parameter: Cow<'a, str>,
    },
    /// Client is denied access.
    PermissionDenied,
    /// Method is expected to be called with 'more' set to true, but wasn't.
    ExpectedMore,
}

impl Error<'_> {
    /// Convert this error into an owned version with `'static` lifetime.
    ///
    /// This is useful when you need to store or propagate the error.
    pub fn into_owned(self) -> Error<'static> {
        match self {
            Error::InterfaceNotFound { interface } => Error::InterfaceNotFound {
                interface: Cow::Owned(interface.into_owned()),
            },
            Error::MethodNotFound { method } => Error::MethodNotFound {
                method: Cow::Owned(method.into_owned()),
            },
            Error::MethodNotImplemented { method } => Error::MethodNotImplemented {
                method: Cow::Owned(method.into_owned()),
            },
            Error::InvalidParameter { parameter } => Error::InvalidParameter {
                parameter: Cow::Owned(parameter.into_owned()),
            },
            Error::PermissionDenied => Error::PermissionDenied,
            Error::ExpectedMore => Error::ExpectedMore,
        }
    }
}

impl core::error::Error for Error<'_> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

impl core::fmt::Display for Error<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::InterfaceNotFound { interface } => {
                write!(f, "Interface not found: {interface}")
            }
            Error::MethodNotFound { method } => {
                write!(f, "Method not found: {method}")
            }
            Error::InvalidParameter { parameter } => {
                write!(f, "Invalid parameter: {parameter}")
            }
            Error::PermissionDenied => {
                write!(f, "Permission denied")
            }
            Error::ExpectedMore => {
                write!(f, "Expected more")
            }
            Error::MethodNotImplemented { method } => {
                write!(f, "Method not implemented: {method}")
            }
        }
    }
}

/// Owned version of [`Error`] for use with the chain API.
///
/// This is a newtype wrapper around `Error<'static>`, allowing it to be deserialized as owned data.
/// This is required for the chain API because the internal buffer may be reused between stream
/// iterations.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedError(Error<'static>);

impl OwnedError {
    /// Returns a reference to the inner `Error`.
    pub fn inner(&self) -> &Error<'static> {
        &self.0
    }

    /// Consumes self and returns the inner `Error`.
    pub fn into_inner(self) -> Error<'static> {
        self.0
    }
}

impl core::ops::Deref for OwnedError {
    type Target = Error<'static>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for OwnedError {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl core::error::Error for OwnedError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.0.source()
    }
}

impl core::fmt::Display for OwnedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a> From<Error<'a>> for OwnedError {
    fn from(err: Error<'a>) -> Self {
        Self(err.into_owned())
    }
}

impl Serialize for OwnedError {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OwnedError {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use alloc::string::String;

        // Helper enum that deserializes into owned strings.
        #[derive(Deserialize)]
        #[serde(tag = "error", content = "parameters")]
        enum ErrorHelper {
            #[serde(rename = "org.varlink.service.InterfaceNotFound")]
            InterfaceNotFound { interface: String },
            #[serde(rename = "org.varlink.service.MethodNotFound")]
            MethodNotFound { method: String },
            #[serde(rename = "org.varlink.service.MethodNotImplemented")]
            MethodNotImplemented { method: String },
            #[serde(rename = "org.varlink.service.InvalidParameter")]
            InvalidParameter { parameter: String },
            #[serde(rename = "org.varlink.service.PermissionDenied")]
            PermissionDenied,
            #[serde(rename = "org.varlink.service.ExpectedMore")]
            ExpectedMore,
        }

        let helper = ErrorHelper::deserialize(deserializer)?;
        let error = match helper {
            ErrorHelper::InterfaceNotFound { interface } => Error::InterfaceNotFound {
                interface: Cow::Owned(interface),
            },
            ErrorHelper::MethodNotFound { method } => Error::MethodNotFound {
                method: Cow::Owned(method),
            },
            ErrorHelper::MethodNotImplemented { method } => Error::MethodNotImplemented {
                method: Cow::Owned(method),
            },
            ErrorHelper::InvalidParameter { parameter } => Error::InvalidParameter {
                parameter: Cow::Owned(parameter),
            },
            ErrorHelper::PermissionDenied => Error::PermissionDenied,
            ErrorHelper::ExpectedMore => Error::ExpectedMore,
        };
        Ok(Self(error))
    }
}

/// Result type for Varlink service methods.
pub type Result<'a, T> = core::result::Result<T, Error<'a>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_serialization() {
        let err = Error::InterfaceNotFound {
            interface: Cow::Borrowed("com.example.missing"),
        };

        let json = serialize_error(&err);
        assert!(json.contains("org.varlink.service.InterfaceNotFound"));
        assert!(json.contains("com.example.missing"));

        let err = Error::PermissionDenied;

        let json = serialize_error(&err);
        assert!(json.contains("org.varlink.service.PermissionDenied"));
    }

    #[test]
    fn error_deserialization() {
        // Test error with parameter.
        let json = r#"{"error":"org.varlink.service.InterfaceNotFound","parameters":{"interface":"com.example.missing"}}"#;
        let err: Error<'_> = deserialize_error(json);
        assert_eq!(
            err,
            Error::InterfaceNotFound {
                interface: Cow::Borrowed("com.example.missing")
            }
        );

        // Test error without parameters.
        let json = r#"{"error":"org.varlink.service.PermissionDenied"}"#;
        let err: Error<'_> = deserialize_error(json);
        assert_eq!(err, Error::PermissionDenied);

        // Test MethodNotFound error.
        let json = r#"{"error":"org.varlink.service.MethodNotFound","parameters":{"method":"NonExistentMethod"}}"#;
        let err: Error<'_> = deserialize_error(json);
        assert_eq!(
            err,
            Error::MethodNotFound {
                method: Cow::Borrowed("NonExistentMethod")
            }
        );

        // Test InvalidParameter error.
        let json = r#"{"error":"org.varlink.service.InvalidParameter","parameters":{"parameter":"invalid_param"}}"#;
        let err: Error<'_> = deserialize_error(json);
        assert_eq!(
            err,
            Error::InvalidParameter {
                parameter: Cow::Borrowed("invalid_param")
            }
        );

        // Test MethodNotImplemented error.
        let json = r#"{"error":"org.varlink.service.MethodNotImplemented","parameters":{"method":"UnimplementedMethod"}}"#;
        let err: Error<'_> = deserialize_error(json);
        assert_eq!(
            err,
            Error::MethodNotImplemented {
                method: Cow::Borrowed("UnimplementedMethod")
            }
        );

        // Test ExpectedMore error.
        let json = r#"{"error":"org.varlink.service.ExpectedMore"}"#;
        let err: Error<'_> = deserialize_error(json);
        assert_eq!(err, Error::ExpectedMore);
    }

    #[test]
    fn error_round_trip_serialization() {
        // Test with error that has parameters.
        let original = Error::InterfaceNotFound {
            interface: Cow::Borrowed("com.example.missing"),
        };

        test_round_trip_serialize(&original);

        // Test with error that has no parameters.
        let original = Error::PermissionDenied;

        test_round_trip_serialize(&original);
    }

    #[test]
    fn into_owned() {
        let borrowed = Error::InterfaceNotFound {
            interface: Cow::Borrowed("test.interface"),
        };
        let owned = borrowed.into_owned();
        assert_eq!(
            owned,
            Error::InterfaceNotFound {
                interface: Cow::Owned("test.interface".into())
            }
        );
    }

    // Helper function to serialize Error to JSON string.
    fn serialize_error(err: &Error<'_>) -> String {
        serde_json::to_string(err).unwrap()
    }

    // Helper function to deserialize JSON string to Error.
    fn deserialize_error(json: &str) -> Error<'_> {
        serde_json::from_str(json).unwrap()
    }

    // Helper function for round-trip serialization test.
    fn test_round_trip_serialize(original: &Error<'_>) {
        let json = serde_json::to_string(original).unwrap();
        let deserialized: Error<'_> = serde_json::from_str(&json).unwrap();
        assert_eq!(*original, deserialized);
    }
}
