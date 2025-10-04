//! Interface definitions for Varlink IDL.

use core::fmt;

use alloc::vec::Vec;

#[cfg(feature = "idl-parse")]
use crate::Error;

use super::List;

/// A Varlink interface definition.
#[derive(Debug, Clone, Eq)]
pub struct Interface<'a> {
    /// The name of the interface in reverse-domain notation.
    name: &'a str,
    /// The methods of the interface.
    methods: List<'a, super::Method<'a>>,
    /// The custom types of the interface.
    custom_types: List<'a, super::CustomType<'a>>,
    /// The errors of the interface.
    errors: List<'a, super::Error<'a>>,
    /// The comments associated with this interface.
    comments: List<'a, super::Comment<'a>>,
}

impl<'a> Interface<'a> {
    /// Creates a new interface with the given name, borrowed collections, and comments.
    pub const fn new(
        name: &'a str,
        methods: &'a [&'a super::Method<'a>],
        custom_types: &'a [&'a super::CustomType<'a>],
        errors: &'a [&'a super::Error<'a>],
        comments: &'a [&'a super::Comment<'a>],
    ) -> Self {
        Self {
            name,
            methods: List::Borrowed(methods),
            custom_types: List::Borrowed(custom_types),
            errors: List::Borrowed(errors),
            comments: List::Borrowed(comments),
        }
    }

    /// Creates a new interface with the given name, owned collections, and comments.
    pub fn new_owned(
        name: &'a str,
        methods: Vec<super::Method<'a>>,
        custom_types: Vec<super::CustomType<'a>>,
        errors: Vec<super::Error<'a>>,
        comments: Vec<super::Comment<'a>>,
    ) -> Self {
        Self {
            name,
            methods: List::Owned(methods),
            custom_types: List::Owned(custom_types),
            errors: List::Owned(errors),
            comments: List::from(comments),
        }
    }

    /// Returns the name of the interface.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns an iterator over the methods of the interface.
    pub fn methods(&self) -> impl Iterator<Item = &super::Method<'a>> {
        self.methods.iter()
    }

    /// Returns an iterator over the custom types of the interface.
    pub fn custom_types(&self) -> impl Iterator<Item = &super::CustomType<'a>> {
        self.custom_types.iter()
    }

    /// Returns an iterator over the errors of the interface.
    pub fn errors(&self) -> impl Iterator<Item = &super::Error<'a>> {
        self.errors.iter()
    }

    /// Returns an iterator over the comments associated with this interface.
    pub fn comments(&self) -> impl Iterator<Item = &super::Comment<'a>> {
        self.comments.iter()
    }

    /// Returns true if the interface has no members.
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.custom_types.is_empty() && self.errors.is_empty()
    }
}

impl<'a> fmt::Display for Interface<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Comments first
        for comment in self.comments.iter() {
            writeln!(f, "{comment}")?;
        }
        write!(f, "interface {}", self.name)?;
        for custom_type in self.custom_types.iter() {
            write!(f, "\n\n{custom_type}")?;
        }
        for method in self.methods.iter() {
            write!(f, "\n\n{method}")?;
        }
        for error in self.errors.iter() {
            write!(f, "\n\n{error}")?;
        }
        Ok(())
    }
}

#[cfg(feature = "idl-parse")]
impl<'a> TryFrom<&'a str> for Interface<'a> {
    type Error = Error;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        super::parse::parse_interface(value)
    }
}

impl PartialEq for Interface<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.custom_types == other.custom_types
            && self.methods == other.methods
            && self.errors == other.errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idl::{Error, Field, Method, Parameter, Type};

    #[test]
    fn org_varlink_service_interface() {
        use crate::idl::TypeRef;

        // Build the org.varlink.service interface as our test case
        let interfaces_type = Type::Array(TypeRef::new(&Type::String));
        let get_info_outputs = [
            &Parameter::new("vendor", &Type::String, &[]),
            &Parameter::new("product", &Type::String, &[]),
            &Parameter::new("version", &Type::String, &[]),
            &Parameter::new("url", &Type::String, &[]),
            &Parameter::new("interfaces", &interfaces_type, &[]),
        ];
        let get_info = Method::new("GetInfo", &[], &get_info_outputs, &[]);

        let get_interface_desc_inputs = [&Parameter::new("interface", &Type::String, &[])];
        let get_interface_desc_outputs = [&Parameter::new("description", &Type::String, &[])];
        let get_interface_desc = Method::new(
            "GetInterfaceDescription",
            &get_interface_desc_inputs,
            &get_interface_desc_outputs,
            &[],
        );

        let interface_not_found_fields = [&Field::new("interface", &Type::String, &[])];
        let interface_not_found = Error::new("InterfaceNotFound", &interface_not_found_fields, &[]);

        let method_not_found_fields = [&Field::new("method", &Type::String, &[])];
        let method_not_found = Error::new("MethodNotFound", &method_not_found_fields, &[]);

        let method_not_impl_fields = [&Field::new("method", &Type::String, &[])];
        let method_not_impl = Error::new("MethodNotImplemented", &method_not_impl_fields, &[]);

        let invalid_param_fields = [&Field::new("parameter", &Type::String, &[])];
        let invalid_param = Error::new("InvalidParameter", &invalid_param_fields, &[]);

        let permission_denied = Error::new("PermissionDenied", &[], &[]);
        let expected_more = Error::new("ExpectedMore", &[], &[]);

        let methods = &[&get_info, &get_interface_desc];
        let errors = &[
            &interface_not_found,
            &method_not_found,
            &method_not_impl,
            &invalid_param,
            &permission_denied,
            &expected_more,
        ];

        let interface = Interface::new("org.varlink.service", methods, &[], errors, &[]);

        assert_eq!(interface.name(), "org.varlink.service");
        assert_eq!(interface.methods().count(), 2);
        assert_eq!(interface.errors().count(), 6);
        assert!(!interface.is_empty());

        // Check method count
        assert_eq!(interface.methods().count(), 2);

        // Check error count
        assert_eq!(interface.errors().count(), 6);

        // Test Display output
        use core::fmt::Write;
        let mut idl = mayheap::String::<2048>::new();
        write!(idl, "{}", interface).unwrap();
        assert!(idl.as_str().starts_with("interface org.varlink.service"));
        assert!(idl.as_str().contains("method GetInfo()"));
        assert!(idl
            .as_str()
            .contains("method GetInterfaceDescription(interface: string)"));
        assert!(idl
            .as_str()
            .contains("error InterfaceNotFound (interface: string)"));
        assert!(idl.as_str().contains("error PermissionDenied ()"));

        // Test parsing the official org.varlink.service IDL and compare with manually constructed
        #[cfg(feature = "idl-parse")]
        {
            use crate::idl::parse;

            const ORG_VARLINK_SERVICE_IDL: &str = r#"interface org.varlink.service

method GetInfo() -> (
  vendor: string,
  product: string,
  version: string,
  url: string,
  interfaces: []string
)

method GetInterfaceDescription(interface: string) -> (description: string)

error InterfaceNotFound (interface: string)

error MethodNotFound (method: string)

error MethodNotImplemented (method: string)

error InvalidParameter (parameter: string)

error PermissionDenied ()

error ExpectedMore ()
"#;

            let parsed_interface = parse::parse_interface(ORG_VARLINK_SERVICE_IDL)
                .expect("Failed to parse org.varlink.service IDL");

            // Compare the parsed interface with our manually constructed one
            assert_eq!(parsed_interface, interface);
        }
    }

    #[test]
    fn empty_interface() {
        let interface = Interface::new("com.example.empty", &[], &[], &[], &[]);
        assert!(interface.is_empty());
        assert_eq!(interface.methods().count(), 0);
        assert_eq!(interface.errors().count(), 0);
        assert_eq!(interface.custom_types().count(), 0);
    }

    #[cfg(feature = "idl-parse")]
    #[test]
    fn systemd_resolved_interface_parsing() {
        use alloc::vec::Vec;

        use crate::idl::{parse, CustomObject, CustomType, TypeRef};

        // Manually construct the systemd-resolved interface for comparison.

        // Define types used in the interface.
        let optional_int_type = Type::Optional(TypeRef::new(&Type::Int));
        let int_array_type = Type::Array(TypeRef::new(&Type::Int));
        let optional_string_type = Type::Optional(TypeRef::new(&Type::String));
        let optional_int_array_type = Type::Optional(TypeRef::new(&int_array_type));

        // Build ResolvedAddress custom type.
        let resolved_address_fields = [
            &Field::new("ifindex", &optional_int_type, &[]),
            &Field::new("family", &Type::Int, &[]),
            &Field::new("address", &int_array_type, &[]),
        ];
        let resolved_address = CustomType::from(CustomObject::new(
            "ResolvedAddress",
            &resolved_address_fields,
            &[],
        ));

        // Build ResolvedName custom type.
        let resolved_name_fields = [
            &Field::new("ifindex", &optional_int_type, &[]),
            &Field::new("name", &Type::String, &[]),
        ];
        let resolved_name = CustomType::from(CustomObject::new(
            "ResolvedName",
            &resolved_name_fields,
            &[],
        ));

        // Build ResourceKey custom type.
        let resource_key_fields = [
            &Field::new("class", &Type::Int, &[]),
            &Field::new("type", &Type::Int, &[]),
            &Field::new("name", &Type::String, &[]),
        ];
        let resource_key =
            CustomType::from(CustomObject::new("ResourceKey", &resource_key_fields, &[]));

        // Build ResourceRecord custom type (references ResourceKey).
        let resource_key_type = Type::Custom("ResourceKey");
        let resource_record_fields = [
            &Field::new("key", &resource_key_type, &[]),
            &Field::new("priority", &optional_int_type, &[]),
            &Field::new("weight", &optional_int_type, &[]),
            &Field::new("port", &optional_int_type, &[]),
            &Field::new("name", &optional_string_type, &[]),
            &Field::new("address", &optional_int_array_type, &[]),
        ];
        let resource_record = CustomType::from(CustomObject::new(
            "ResourceRecord",
            &resource_record_fields,
            &[],
        ));

        // Build methods.
        let resolved_address_array_type =
            Type::Array(TypeRef::new(&Type::Custom("ResolvedAddress")));
        let resolved_name_array_type = Type::Array(TypeRef::new(&Type::Custom("ResolvedName")));

        let resolve_hostname_inputs = [
            &Parameter::new("ifindex", &optional_int_type, &[]),
            &Parameter::new("name", &Type::String, &[]),
            &Parameter::new("family", &optional_int_type, &[]),
            &Parameter::new("flags", &optional_int_type, &[]),
        ];
        let resolve_hostname_outputs = [
            &Parameter::new("addresses", &resolved_address_array_type, &[]),
            &Parameter::new("name", &Type::String, &[]),
            &Parameter::new("flags", &Type::Int, &[]),
        ];
        let resolve_hostname = Method::new(
            "ResolveHostname",
            &resolve_hostname_inputs,
            &resolve_hostname_outputs,
            &[],
        );

        let resolve_address_inputs = [
            &Parameter::new("ifindex", &optional_int_type, &[]),
            &Parameter::new("family", &Type::Int, &[]),
            &Parameter::new("address", &int_array_type, &[]),
            &Parameter::new("flags", &optional_int_type, &[]),
        ];
        let resolve_address_outputs = [
            &Parameter::new("names", &resolved_name_array_type, &[]),
            &Parameter::new("flags", &Type::Int, &[]),
        ];
        let resolve_address = Method::new(
            "ResolveAddress",
            &resolve_address_inputs,
            &resolve_address_outputs,
            &[],
        );

        // Build errors.
        let no_name_servers = Error::new("NoNameServers", &[], &[]);
        let query_timed_out = Error::new("QueryTimedOut", &[], &[]);

        let dnssec_validation_failed_fields = [
            &Field::new("result", &Type::String, &[]),
            &Field::new("extendedDNSErrorCode", &optional_int_type, &[]),
            &Field::new("extendedDNSErrorMessage", &optional_string_type, &[]),
        ];
        let dnssec_validation_failed = Error::new(
            "DNSSECValidationFailed",
            &dnssec_validation_failed_fields,
            &[],
        );

        let dns_error_fields = [
            &Field::new("rcode", &Type::Int, &[]),
            &Field::new("extendedDNSErrorCode", &optional_int_type, &[]),
            &Field::new("extendedDNSErrorMessage", &optional_string_type, &[]),
        ];
        let dns_error = Error::new("DNSError", &dns_error_fields, &[]);

        // Build the complete interface.
        let custom_types = &[
            &resolved_address,
            &resolved_name,
            &resource_key,
            &resource_record,
        ];
        let methods = &[&resolve_hostname, &resolve_address];
        let errors = &[
            &no_name_servers,
            &query_timed_out,
            &dnssec_validation_failed,
            &dns_error,
        ];

        let interface = Interface::new("io.systemd.Resolve", methods, custom_types, errors, &[]);

        // Test parsing the IDL and compare with manually constructed interface.
        const SYSTEMD_RESOLVED_IDL: &str = r#"interface io.systemd.Resolve

type ResolvedAddress(
    ifindex: ?int,
    family: int,
    address: []int
)

type ResolvedName(
    ifindex: ?int,
    name: string
)

type ResourceKey(
    class: int,
    type: int,
    name: string
)

type ResourceRecord(
    key: ResourceKey,
    priority: ?int,
    weight: ?int,
    port: ?int,
    name: ?string,
    address: ?[]int
)

method ResolveHostname(
    ifindex: ?int,
    name: string,
    family: ?int,
    flags: ?int
) -> (
    addresses: []ResolvedAddress,
    name: string,
    flags: int
)

method ResolveAddress(
    ifindex: ?int,
    family: int,
    address: []int,
    flags: ?int
) -> (
    names: []ResolvedName,
    flags: int
)

error NoNameServers()

error QueryTimedOut()

error DNSSECValidationFailed(
    result: string,
    extendedDNSErrorCode: ?int,
    extendedDNSErrorMessage: ?string
)

error DNSError(
    rcode: int,
    extendedDNSErrorCode: ?int,
    extendedDNSErrorMessage: ?string
)
"#;

        let parsed_interface = parse::parse_interface(SYSTEMD_RESOLVED_IDL)
            .expect("Failed to parse systemd-resolved interface");

        // Verify basic interface properties match.
        assert_eq!(parsed_interface.name(), interface.name());
        assert_eq!(
            parsed_interface.custom_types().count(),
            interface.custom_types().count()
        );
        assert_eq!(
            parsed_interface.methods().count(),
            interface.methods().count()
        );
        assert_eq!(
            parsed_interface.errors().count(),
            interface.errors().count()
        );

        // Check specific type validation - ResolvedAddress.
        let parsed_resolved_address = parsed_interface
            .custom_types()
            .find(|t| t.name() == "ResolvedAddress")
            .expect("ResolvedAddress type should exist");
        let manual_resolved_address = interface
            .custom_types()
            .find(|t| t.name() == "ResolvedAddress")
            .expect("ResolvedAddress type should exist in manual interface");

        // Verify field types in ResolvedAddress.
        let parsed_fields: Vec<_> = parsed_resolved_address
            .as_object()
            .unwrap()
            .fields()
            .collect();
        let manual_fields: Vec<_> = manual_resolved_address
            .as_object()
            .unwrap()
            .fields()
            .collect();
        assert_eq!(parsed_fields.len(), manual_fields.len());

        assert_eq!(parsed_fields[0].name(), "ifindex");
        assert_eq!(
            *parsed_fields[0].ty(),
            Type::Optional(TypeRef::new(&Type::Int))
        );
        assert_eq!(parsed_fields[1].name(), "family");
        assert_eq!(*parsed_fields[1].ty(), Type::Int);
        assert_eq!(parsed_fields[2].name(), "address");
        assert_eq!(
            *parsed_fields[2].ty(),
            Type::Array(TypeRef::new(&Type::Int))
        );

        // Check method parameter types - ResolveHostname.
        let parsed_resolve_hostname = parsed_interface
            .methods()
            .find(|m| m.name() == "ResolveHostname")
            .expect("ResolveHostname method should exist");
        let manual_resolve_hostname = interface
            .methods()
            .find(|m| m.name() == "ResolveHostname")
            .expect("ResolveHostname method should exist in manual interface");

        let parsed_inputs: Vec<_> = parsed_resolve_hostname.inputs().collect();
        let manual_inputs: Vec<_> = manual_resolve_hostname.inputs().collect();
        assert_eq!(parsed_inputs.len(), manual_inputs.len());

        // Verify input parameter types.
        assert_eq!(parsed_inputs[0].name(), "ifindex");
        assert_eq!(
            *parsed_inputs[0].ty(),
            Type::Optional(TypeRef::new(&Type::Int))
        );
        assert_eq!(parsed_inputs[1].name(), "name");
        assert_eq!(*parsed_inputs[1].ty(), Type::String);
        assert_eq!(parsed_inputs[2].name(), "family");
        assert_eq!(
            *parsed_inputs[2].ty(),
            Type::Optional(TypeRef::new(&Type::Int))
        );

        // Verify output parameter types.
        let parsed_outputs: Vec<_> = parsed_resolve_hostname.outputs().collect();
        assert_eq!(parsed_outputs[0].name(), "addresses");
        assert_eq!(
            *parsed_outputs[0].ty(),
            Type::Array(TypeRef::new(&Type::Custom("ResolvedAddress")))
        );
        assert_eq!(parsed_outputs[1].name(), "name");
        assert_eq!(*parsed_outputs[1].ty(), Type::String);
        assert_eq!(parsed_outputs[2].name(), "flags");
        assert_eq!(*parsed_outputs[2].ty(), Type::Int);

        // Check error field types - DNSError.
        let parsed_dns_error = parsed_interface
            .errors()
            .find(|e| e.name() == "DNSError")
            .expect("DNSError should exist");
        let dns_error_fields: Vec<_> = parsed_dns_error.fields().collect();

        assert_eq!(dns_error_fields[0].name(), "rcode");
        assert_eq!(*dns_error_fields[0].ty(), Type::Int);
        assert_eq!(dns_error_fields[1].name(), "extendedDNSErrorCode");
        assert_eq!(
            *dns_error_fields[1].ty(),
            Type::Optional(TypeRef::new(&Type::Int))
        );
        assert_eq!(dns_error_fields[2].name(), "extendedDNSErrorMessage");
        assert_eq!(
            *dns_error_fields[2].ty(),
            Type::Optional(TypeRef::new(&Type::String))
        );

        // Verify no-field errors work.
        let parsed_no_name_servers = parsed_interface
            .errors()
            .find(|e| e.name() == "NoNameServers")
            .expect("NoNameServers should exist");
        assert_eq!(parsed_no_name_servers.fields().count(), 0);

        // Compare the parsed interface with our manually constructed one.
        assert_eq!(parsed_interface, interface);
    }

    #[test]
    fn display_with_comments() {
        use crate::idl::{Comment, Method};
        use core::fmt::Write;

        let comment1 = Comment::new("Interface documentation");
        let comment2 = Comment::new("Version 1.0");
        let interface_comments = [&comment1, &comment2];

        let method_comment = Comment::new("Test method");
        let method_comments = [&method_comment];
        let method = Method::new("Test", &[], &[], &method_comments);
        let methods = [&method];

        let interface = Interface::new("org.example.test", &methods, &[], &[], &interface_comments);

        let mut output = mayheap::String::<256>::new();
        write!(&mut output, "{}", interface).unwrap();

        let expected = "# Interface documentation\n# Version 1.0\ninterface org.example.test\n\n# Test method\nmethod Test() -> ()";
        assert_eq!(output, expected);
    }

    #[test]
    fn comprehensive_display_with_nested_comments() {
        use crate::idl::{
            Comment, CustomObject, CustomType, Error, Field, Method, Parameter, Type,
        };
        use core::fmt::Write;

        // Interface comments
        let interface_comment = Comment::new("Comprehensive test interface");
        let interface_comments = [&interface_comment];

        // Custom type with comments
        let type_comment = Comment::new("User data structure");
        let type_comments = [&type_comment];
        let name_field_comment = Comment::new("Full name");
        let name_field_comments = [&name_field_comment];
        let name_field = Field::new("name", &Type::String, &name_field_comments);
        let age_field = Field::new("age", &Type::Int, &[]);
        let fields = [&name_field, &age_field];
        let user_object = CustomObject::new("User", &fields, &type_comments);
        let user_type = CustomType::from(user_object);
        let custom_types = [&user_type];

        // Method with comments
        let method_comment = Comment::new("Get user by ID");
        let method_comments = [&method_comment];
        let id_param_comment = Comment::new("User ID");
        let id_param_comments = [&id_param_comment];
        let id_param = Parameter::new("id", &Type::Int, &id_param_comments);
        let user_param = Parameter::new("user", &Type::Custom("User"), &[]);
        let inputs = [&id_param];
        let outputs = [&user_param];
        let method = Method::new("GetUser", &inputs, &outputs, &method_comments);
        let methods = [&method];

        // Error with comments
        let error_comment = Comment::new("User not found error");
        let error_comments = [&error_comment];
        let msg_field = Field::new("message", &Type::String, &[]);
        let error_fields = [&msg_field];
        let error = Error::new("UserNotFound", &error_fields, &error_comments);
        let errors = [&error];

        let interface = Interface::new(
            "org.example.comprehensive",
            &methods,
            &custom_types,
            &errors,
            &interface_comments,
        );

        let mut output = mayheap::String::<512>::new();
        write!(&mut output, "{}", interface).unwrap();

        let expected = "# Comprehensive test interface\ninterface org.example.comprehensive\n\n# User data structure\ntype User (# Full name\nname: string, age: int)\n\n# Get user by ID\nmethod GetUser(# User ID\nid: int) -> (user: User)\n\n# User not found error\nerror UserNotFound (message: string)";
        assert_eq!(output, expected);
    }

    #[test]
    #[cfg(feature = "idl-parse")]
    fn parse_and_display_round_trip_with_comments() {
        use core::fmt::Write;

        let input = r#"# Main interface documentation
# Version 1.0
interface org.example.test

# User data structure
# Contains basic information
type User (# User's full name
name: string, age: int)

# Get user by ID
# Returns user details
method GetUser(# User identifier
id: int) -> (user: User)

# User not found error
error UserNotFound (id: int)"#;

        let parsed = Interface::try_from(input).unwrap();
        let mut output = mayheap::String::<512>::new();
        write!(&mut output, "{}", parsed).unwrap();

        // The output should exactly match the input (normalized whitespace)
        assert_eq!(output.trim(), input.trim());
    }
}
