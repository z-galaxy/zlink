use mayheap::string::String;
use serde::{Deserialize, Serialize};

#[cfg(feature = "introspection")]
use crate::introspect;

use crate::ReplyError;

use super::Info;
#[cfg(feature = "idl")]
use super::InterfaceDescription;

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
        interface: &'a str,
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

/// Errors that can be returned by the `org.varlink.service` interface.
#[derive(Debug, Clone, PartialEq, ReplyError)]
#[cfg_attr(feature = "introspection", derive(introspect::ReplyError))]
#[zlink(interface = "org.varlink.service")]
#[cfg_attr(feature = "introspection", zlink(crate = "crate"))]
pub enum Error {
    /// The requested interface was not found.
    InterfaceNotFound {
        /// The interface that was not found.
        interface: String<MAX_INTERFACE_NAME_LENGTH>,
    },
    /// The requested method was not found.
    MethodNotFound {
        /// The method that was not found.
        method: String<MAX_METHOD_NAME_LENGTH>,
    },
    /// The interface defines the requested method, but the service does not implement it.
    MethodNotImplemented {
        /// The method that is not implemented.
        method: String<MAX_METHOD_NAME_LENGTH>,
    },
    /// One of the passed parameters is invalid.
    InvalidParameter {
        /// The parameter that is invalid.
        parameter: String<MAX_PARAMETER_NAME_LENGTH>,
    },
    /// Client is denied access.
    PermissionDenied,
    /// Method is expected to be called with 'more' set to true, but wasn't.
    ExpectedMore,
}

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

impl core::fmt::Display for Error {
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

/// Result type for Varlink service methods.
pub type Result<T> = core::result::Result<T, Error>;

const MAX_INTERFACE_NAME_LENGTH: usize = 64;
const MAX_METHOD_NAME_LENGTH: usize = 64;
const MAX_PARAMETER_NAME_LENGTH: usize = 24;

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;

    #[test]
    fn error_serialization() {
        let err = Error::InterfaceNotFound {
            interface: String::from_str("com.example.missing").unwrap(),
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
        // Test error with parameter
        let json = r#"{"error":"org.varlink.service.InterfaceNotFound","parameters":{"interface":"com.example.missing"}}"#;
        let err = deserialize_error(json);
        assert_eq!(
            err,
            Error::InterfaceNotFound {
                interface: String::from_str("com.example.missing").unwrap()
            }
        );

        // Test error without parameters
        let json = r#"{"error":"org.varlink.service.PermissionDenied"}"#;
        let err = deserialize_error(json);
        assert_eq!(err, Error::PermissionDenied);

        // Test MethodNotFound error
        let json = r#"{"error":"org.varlink.service.MethodNotFound","parameters":{"method":"NonExistentMethod"}}"#;
        let err = deserialize_error(json);
        assert_eq!(
            err,
            Error::MethodNotFound {
                method: String::from_str("NonExistentMethod").unwrap()
            }
        );

        // Test InvalidParameter error
        let json = r#"{"error":"org.varlink.service.InvalidParameter","parameters":{"parameter":"invalid_param"}}"#;
        let err = deserialize_error(json);
        assert_eq!(
            err,
            Error::InvalidParameter {
                parameter: String::from_str("invalid_param").unwrap()
            }
        );

        // Test MethodNotImplemented error
        let json = r#"{"error":"org.varlink.service.MethodNotImplemented","parameters":{"method":"UnimplementedMethod"}}"#;
        let err = deserialize_error(json);
        assert_eq!(
            err,
            Error::MethodNotImplemented {
                method: String::from_str("UnimplementedMethod").unwrap()
            }
        );

        // Test ExpectedMore error
        let json = r#"{"error":"org.varlink.service.ExpectedMore"}"#;
        let err = deserialize_error(json);
        assert_eq!(err, Error::ExpectedMore);
    }

    #[test]
    fn error_round_trip_serialization() {
        // Test with error that has parameters
        let original = Error::InterfaceNotFound {
            interface: String::from_str("com.example.missing").unwrap(),
        };

        test_round_trip_serialize(&original);

        // Test with error that has no parameters
        let original = Error::PermissionDenied;

        test_round_trip_serialize(&original);
    }

    // Helper function to serialize Error to JSON string, abstracting std vs nostd differences
    fn serialize_error(err: &Error) -> mayheap::string::String<256> {
        mayheap::string::String::from_str(&serde_json::to_string(err).unwrap()).unwrap()
    }

    // Helper function to deserialize JSON string to Error, abstracting std vs nostd differences
    fn deserialize_error(json: &str) -> Error {
        serde_json::from_str(json).unwrap()
    }

    // Helper function for round-trip serialization test, abstracting std vs nostd differences
    fn test_round_trip_serialize(original: &Error) {
        let json = serde_json::to_string(original).unwrap();
        let deserialized: Error = serde_json::from_str(&json).unwrap();
        assert_eq!(*original, deserialized);
    }
}
