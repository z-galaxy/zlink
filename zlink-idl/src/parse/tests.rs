//! Parsers for Varlink IDL using winnow.
//!
//! This module provides parsers for converting IDL strings into the corresponding
//! Rust types defined in the parent module. Uses byte-based parsing to avoid UTF-8 overhead.

use super::*;

#[test]
fn parse_primitive_types() {
    assert_eq!(parse_type("bool").unwrap(), Type::Bool);
    assert_eq!(parse_type("int").unwrap(), Type::Int);
    assert_eq!(parse_type("float").unwrap(), Type::Float);
    assert_eq!(parse_type("string").unwrap(), Type::String);
    assert_eq!(parse_type("object").unwrap(), Type::ForeignObject);
    assert_eq!(parse_type("any").unwrap(), Type::Any);
}

#[test]
fn parse_custom_types() {
    match parse_type("Person").unwrap() {
        Type::Custom(name) => {
            assert_eq!(name, "Person");
        }
        _ => panic!("Expected custom type"),
    }
}

#[test]
fn parse_composite_types() {
    // Test optional type
    match parse_type("?int").unwrap() {
        Type::Optional(optional) => {
            assert_eq!(*optional, Type::Int);
        }
        _ => panic!("Expected optional type"),
    }

    // Test array type
    match parse_type("[]string").unwrap() {
        Type::Array(array) => {
            assert_eq!(*array, Type::String);
        }
        _ => panic!("Expected array type"),
    }

    // Test map type
    match parse_type("[string]bool").unwrap() {
        Type::Map(map) => {
            assert_eq!(*map, Type::Bool);
        }
        _ => panic!("Expected map type"),
    }
}

#[test]
fn parse_nested_types() {
    // Test nested optional array
    match parse_type("?[]string").unwrap() {
        Type::Optional(optional) => match &*optional {
            Type::Array(array) => {
                assert_eq!(*array, Type::String);
            }
            _ => panic!("Expected array inside optional"),
        },
        _ => panic!("Expected optional type"),
    }
}

#[test]
fn parse_inline_enum() {
    match parse_type("(one, two, three)").unwrap() {
        Type::Enum(variants) => {
            let collected: Vec<_> = variants.iter().collect();
            assert_eq!(collected.len(), 3);
        }
        _ => panic!("Expected enum type"),
    }
}

#[test]
fn parse_inline_struct() {
    match parse_type("(x: float, y: float)").unwrap() {
        Type::Object(fields) => {
            let collected: Vec<_> = fields.iter().collect();
            assert_eq!(collected.len(), 2);
            assert_eq!(collected[0].name(), "x");
            assert_eq!(collected[0].ty(), &Type::Float);
            assert_eq!(collected[1].name(), "y");
            assert_eq!(collected[1].ty(), &Type::Float);
        }
        _ => panic!("Expected struct type"),
    }
}

#[test]
fn parse_inline_empty_struct() {
    // Empty `()` must be parsed as a (empty) struct per the Varlink grammar.
    // An empty enum cannot be instantiated, so the grammar reserves `()` for
    // structs.
    match parse_type("()").unwrap() {
        Type::Object(fields) => {
            assert_eq!(fields.iter().count(), 0);
        }
        other => panic!("Expected empty struct, got: {other:?}"),
    }

    // Whitespace-only and comment-only content is also empty per the
    // grammar's `_` production.
    assert!(matches!(parse_type("(   )").unwrap(), Type::Object(_)));
    assert!(matches!(
        parse_type("(# comment\n)").unwrap(),
        Type::Object(_)
    ));

    // Also exercise the issue's reported context: an inline empty struct used
    // as a field type within a custom type.
    let custom_type = parse_custom_type("type T (x: ())").unwrap();
    let object = custom_type
        .as_object()
        .expect("expected struct-like custom type");
    let field = object.fields().next().expect("expected field x");
    assert_eq!(field.name(), "x");
    match field.ty() {
        Type::Object(fields) => {
            assert_eq!(fields.iter().count(), 0);
        }
        other => panic!("Expected x to be an empty struct, got: {other:?}"),
    }
}

#[test]
fn parse_whitespace() {
    // Test that whitespace is handled correctly
    assert_eq!(parse_type("  bool  ").unwrap(), Type::Bool);
    assert_eq!(parse_type("\tbool\n").unwrap(), Type::Bool);
}

#[test]
fn parse_errors() {
    assert!(parse_type("").is_err());
    assert!(parse_type("invalid").is_err());
    assert!(parse_type("bool extra").is_err());
}

#[test]
fn parse_interface_name() {
    let input = b"org.example.test";
    let mut input_mut = input.as_slice();
    let result = interface_name(&mut input_mut).unwrap();
    assert_eq!(result, "org.example.test");
    assert!(input_mut.is_empty());

    let input = b"com.example.foo.bar";
    let mut input_mut = input.as_slice();
    let result = interface_name(&mut input_mut).unwrap();
    assert_eq!(result, "com.example.foo.bar");
    assert!(input_mut.is_empty());

    // Invalid: no dot
    let mut input_mut = b"example".as_slice();
    assert!(interface_name(&mut input_mut).is_err());

    // Invalid: starts with number
    let mut input_mut = b"1example.test".as_slice();
    assert!(interface_name(&mut input_mut).is_err());
}

#[test]
fn test_parse_method() {
    let input = "method GetInfo() -> (info: string)";
    let method = parse_method(input).unwrap();
    assert_eq!(method.name(), "GetInfo");
    assert_eq!(method.inputs().count(), 0);
    let mut outputs = method.outputs();
    assert_eq!(
        outputs.next().unwrap(),
        &Parameter::new("info", &Type::String, &[])
    );
    assert!(outputs.next().is_none());

    let input = "method Add(a: int, b: int) -> (sum: int)";
    let method = parse_method(input).unwrap();
    assert_eq!(method.name(), "Add");
    let mut inputs = method.inputs();
    assert_eq!(
        inputs.next().unwrap(),
        &Parameter::new("a", &Type::Int, &[])
    );
    assert_eq!(
        inputs.next().unwrap(),
        &Parameter::new("b", &Type::Int, &[])
    );
    assert!(inputs.next().is_none());
    let mut outputs = method.outputs();
    assert_eq!(
        outputs.next().unwrap(),
        &Parameter::new("sum", &Type::Int, &[])
    );
    assert!(outputs.next().is_none());
}

#[test]
fn test_parse_error() {
    let input = "error NotFound(resource: string)";
    let error = parse_error(input).unwrap();
    assert_eq!(error.name(), "NotFound");
    assert_eq!(error.fields().count(), 1);
    let mut fields = error.fields();
    assert_eq!(
        fields.next().unwrap(),
        &Field::new("resource", &Type::String, &[])
    );
    assert!(fields.next().is_none());

    let input = "error InvalidInput()";
    let error = parse_error(input).unwrap();
    assert_eq!(error.name(), "InvalidInput");
    assert_eq!(error.fields().count(), 0);
}

#[test]
fn test_parse_custom_type() {
    let input = "type Person (name: string, age: int)";
    let custom_type = parse_custom_type(input).unwrap();
    assert_eq!(custom_type.name(), "Person");
    let mut fields = custom_type.as_object().unwrap().fields();
    assert_eq!(
        fields.next().unwrap(),
        &Field::new("name", &Type::String, &[])
    );
    assert_eq!(fields.next().unwrap(), &Field::new("age", &Type::Int, &[]));
    assert!(fields.next().is_none());

    let input = "type Config (host: string, port: int, enabled: bool)";
    let custom_type = parse_custom_type(input).unwrap();
    assert_eq!(custom_type.name(), "Config");
    assert_eq!(custom_type.as_object().unwrap().fields().count(), 3);
    let mut fields = custom_type.as_object().unwrap().fields();
    assert_eq!(
        fields.next().unwrap(),
        &Field::new("host", &Type::String, &[])
    );
    assert_eq!(fields.next().unwrap(), &Field::new("port", &Type::Int, &[]));
    assert_eq!(
        fields.next().unwrap(),
        &Field::new("enabled", &Type::Bool, &[])
    );
    assert!(fields.next().is_none());

    // Enum-style type definitions are correctly parsed as enums
    let enum_type = parse_custom_type("type Color (red, green, blue)").unwrap();
    assert_eq!(enum_type.name(), "Color");
    assert!(enum_type.is_enum()); // Correctly parsed as enum
    assert_eq!(enum_type.as_enum().unwrap().variants().count(), 3);
    let mut variants = enum_type.as_enum().unwrap().variants();
    assert_eq!(variants.next().unwrap().name(), "red");
    assert_eq!(variants.next().unwrap().name(), "green");
    assert_eq!(variants.next().unwrap().name(), "blue");
    assert!(variants.next().is_none());
}

#[test]
fn parse_mixed_field_types() {
    // Mixed field types (some with types, some without) should be treated as an error
    let input = "type Mixed (field1, field2: string, field3)";
    let result = parse_custom_type(input);
    assert!(
        result.is_err(),
        "Mixed field types should be a parsing error"
    );
}

#[test]
fn parse_empty_custom_type() {
    // Empty custom types should be treated as objects
    let input = "type Empty ()";
    let custom_type = parse_custom_type(input).unwrap();
    assert_eq!(custom_type.name(), "Empty");
    assert!(custom_type.is_object()); // Should be treated as object
    assert!(!custom_type.is_enum()); // Should not be treated as enum
    assert_eq!(custom_type.as_object().unwrap().fields().count(), 0);
}

#[test]
fn test_parse_field() {
    let input = "name: string";
    let field = parse_field(input).unwrap();
    assert_eq!(field.name(), "name");
    assert_eq!(field.ty(), &Type::String);

    let input = "items: []int";
    let field = parse_field(input).unwrap();
    assert_eq!(field.name(), "items");
    assert_eq!(field.ty(), &Type::Array(TypeRef::new(&Type::Int)));
}

#[test]
fn parse_field_with_comments() {
    let input = r#"# Field comment
name: string"#;
    let field = parse_field(input).unwrap();
    assert_eq!(field.name(), "name");
    assert_eq!(field.ty(), &Type::String);
    let comments: Vec<_> = field.comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Field comment");

    let input = r#"# First comment
# Second comment
items: []int"#;
    let field = parse_field(input).unwrap();
    assert_eq!(field.name(), "items");
    assert_eq!(field.ty(), &Type::Array(TypeRef::new(&Type::Int)));
    let comments: Vec<_> = field.comments().collect();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].text(), "First comment");
    assert_eq!(comments[1].text(), "Second comment");
}

#[test]
fn parse_parameter_with_comments() {
    let input = r#"# Parameter comment
name: string"#;
    let param = parse_field(input).unwrap(); // Parameter is alias for Field
    assert_eq!(param.name(), "name");
    assert_eq!(param.ty(), &Type::String);
    let comments: Vec<_> = param.comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Parameter comment");
}

#[test]
fn test_parse_interface() {
    let input = r#"
interface org.example.test

type Person (name: string, age: int)

method GetPerson(id: int) -> (person: Person)

error NotFound(id: int)
        "#;

    let interface = parse_interface(input).unwrap();
    assert_eq!(interface.name(), "org.example.test");
    assert_eq!(interface.custom_types().count(), 1);
    assert_eq!(interface.methods().count(), 1);
    assert_eq!(interface.errors().count(), 1);
}

#[test]
fn parse_error_messages() {
    // Test with invalid syntax
    let invalid_interface = "invalid syntax here";
    let result = parse_interface(invalid_interface);
    assert!(result.is_err());
    match result.unwrap_err() {
        Error::Parse(_) => (),
        other => panic!("Expected Parse error, got: {:?}", other),
    }

    // Test with unexpected remaining input
    let incomplete_interface = "interface com.example.Test\nmethod Test() -> ()\nextra junk";
    let result = parse_interface(incomplete_interface);
    assert!(result.is_err());
    match result.unwrap_err() {
        Error::TrailingInput(_) | Error::Parse(_) => (),
        other => panic!("Expected TrailingInput or Parse error, got: {:?}", other),
    }

    // Test with empty input
    let empty_interface = "";
    let result = parse_interface(empty_interface);
    assert!(result.is_err());
    match result.unwrap_err() {
        Error::Empty => (),
        other => panic!("Expected Empty error, got: {:?}", other),
    }
}

#[test]
fn parse_comment() {
    let input = "# This is a comment";
    let mut input_bytes = input.as_bytes();
    let comment = comment_def(&mut input_bytes).unwrap();
    assert_eq!(comment.content(), "This is a comment");
    assert_eq!(comment.text(), "This is a comment");
}

#[test]
fn parse_interface_with_comments() {
    let input = r#"
interface org.example.test

# This is a comment
type Person (name: string, age: int)

# Another comment
method GetPerson(id: int) -> (person: Person)

# Final comment
error NotFound(id: int)
        "#;

    let interface = parse_interface(input).unwrap();
    assert_eq!(interface.name(), "org.example.test");

    // Check that we have the expected number of each type of member
    assert_eq!(interface.custom_types().count(), 1);
    assert_eq!(interface.methods().count(), 1);
    assert_eq!(interface.errors().count(), 1);

    // Check custom type
    let custom_types: Vec<_> = interface.custom_types().collect();
    assert_eq!(custom_types[0].name(), "Person");
    // TODO: Check custom type comments when CustomType supports comments

    // Check method
    let methods: Vec<_> = interface.methods().collect();
    assert_eq!(methods[0].name(), "GetPerson");
    let comments: Vec<_> = methods[0].comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Another comment");

    // Check error
    let errors: Vec<_> = interface.errors().collect();
    assert_eq!(errors[0].name(), "NotFound");
    let comments: Vec<_> = errors[0].comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Final comment");
}

#[test]
fn ws_with_comments() {
    let input = "  # This is a comment\n  \t# Another comment\n  some_text";
    let mut input_bytes = input.as_bytes();

    // Call ws function
    ws(&mut input_bytes).unwrap();

    // Check that ws consumed whitespace and comments, leaving only "some_text"
    let remaining = core::str::from_utf8(input_bytes).unwrap();
    assert_eq!(remaining, "some_text");

    // Test with just whitespace
    let input2 = "   \t\n   ";
    let mut input_bytes2 = input2.as_bytes();
    ws(&mut input_bytes2).unwrap();
    assert!(input_bytes2.is_empty());

    // Test with just a comment
    let input3 = "# Just a comment\n";
    let mut input_bytes3 = input3.as_bytes();
    ws(&mut input_bytes3).unwrap();
    assert!(input_bytes3.is_empty());
}

#[test]
fn parse_simple_enum() {
    let input = "(one, two, three)";
    let mut input_bytes = input.as_bytes();
    if let Err(e) = enum_type(&mut input_bytes) {
        panic!("Should be able to parse simple enum: {:?}", e);
    }
}

#[test]
fn parse_acquiremetadata_enum_directly() {
    let input = r#"(
	# Do not include metadata in the output
	no,
	# Include metadata in the output
	yes,
	# Include metadata in the output, but gracefully eat up errors
	graceful
)"#;

    let mut input_bytes = input.as_bytes();
    if let Err(e) = enum_type(&mut input_bytes) {
        panic!(
            "Should be able to parse AcquireMetadata enum: {:?}. Remaining input: {:?}",
            e,
            core::str::from_utf8(input_bytes).unwrap_or("<invalid UTF-8>")
        );
    }
}

#[test]
fn parse_enum_with_comments() {
    let input = r#"type AcquireMetadata(
	# Do not include metadata in the output
	no,
	# Include metadata in the output
	yes,
	# Include metadata in the output, but gracefully eat up errors
	graceful
)"#;

    match parse_custom_type(input) {
        Ok(custom_type) => {
            assert_eq!(custom_type.name(), "AcquireMetadata");
            assert!(custom_type.is_enum());

            let enum_def = custom_type.as_enum().unwrap();
            assert_eq!(enum_def.variants().count(), 3);

            let variants: Vec<_> = enum_def.variants().collect();
            assert_eq!(variants[0].name(), "no");
            assert!(variants[0].has_comments());
            assert_eq!(
                variants[0].comments().next().unwrap().content(),
                "Do not include metadata in the output"
            );

            assert_eq!(variants[1].name(), "yes");
            assert!(variants[1].has_comments());
            assert_eq!(
                variants[1].comments().next().unwrap().content(),
                "Include metadata in the output"
            );

            assert_eq!(variants[2].name(), "graceful");
            assert!(variants[2].has_comments());
            assert_eq!(
                variants[2].comments().next().unwrap().content(),
                "Include metadata in the output, but gracefully eat up errors"
            );
        }
        Err(e) => panic!("Should be able to parse enum with comments: {}", e),
    }
}

#[test]
fn multiple_consecutive_comments() {
    let input = r#"
interface org.example.test

# First comment
# Second comment
# Third comment
method SimpleMethod() -> ()

# Fourth comment
# Fifth comment
error SimpleError()
        "#;

    let interface = parse_interface(input).unwrap();
    assert_eq!(interface.name(), "org.example.test");

    // Should have: method + error = 2 members (comments attached to them)
    assert_eq!(interface.methods().count(), 1);
    assert_eq!(interface.errors().count(), 1);

    // Verify method has multiple comments attached
    let methods: Vec<_> = interface.methods().collect();
    assert_eq!(methods[0].name(), "SimpleMethod");
    let comments: Vec<_> = methods[0].comments().collect();
    assert_eq!(comments.len(), 3);
    assert_eq!(comments[0].text(), "First comment");
    assert_eq!(comments[1].text(), "Second comment");
    assert_eq!(comments[2].text(), "Third comment");

    // Verify error has multiple comments attached
    let errors: Vec<_> = interface.errors().collect();
    assert_eq!(errors[0].name(), "SimpleError");
    let comments: Vec<_> = errors[0].comments().collect();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].text(), "Fourth comment");
    assert_eq!(comments[1].text(), "Fifth comment");
}

#[test]
fn comments_attached_to_members() {
    let input = r#"
interface org.example.test

# Documentation for Person type
type Person (name: string, age: int)

# Documentation for GetPerson method
method GetPerson(id: int) -> (person: Person)

# Documentation for NotFound error
error NotFound(id: int)
        "#;

    let interface = parse_interface(input).unwrap();

    // Check that we have the expected number of each type of member
    assert_eq!(interface.custom_types().count(), 1);
    assert_eq!(interface.methods().count(), 1);
    assert_eq!(interface.errors().count(), 1);

    // Check custom type
    let custom_types: Vec<_> = interface.custom_types().collect();
    assert_eq!(custom_types[0].name(), "Person");
    // Note: CustomType doesn't support comments yet, this will be implemented later

    // Check method
    let methods: Vec<_> = interface.methods().collect();
    assert_eq!(methods[0].name(), "GetPerson");
    let comments: Vec<_> = methods[0].comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Documentation for GetPerson method");

    // Check error
    let errors: Vec<_> = interface.errors().collect();
    assert_eq!(errors[0].name(), "NotFound");
    let comments: Vec<_> = errors[0].comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Documentation for NotFound error");
}

#[test]
fn comprehensive_comment_parsing() {
    let input = r#"
interface org.example.comprehensive

# Type documentation - first line
# Type documentation - second line
type Config (
    # Host configuration
    host: string,
    port: int,
    # Enable/disable flag
    enabled: bool
)

# Method documentation - line 1
# Method documentation - line 2
# Method documentation - line 3
method Configure(config: Config) -> (success: bool)

# Single error comment
error ConfigurationError(message: string, code: int)

# Reset method documentation
method Reset() -> ()

# GetStatus method documentation
method GetStatus() -> (status: string)
        "#;

    let interface = parse_interface(input).unwrap();
    assert_eq!(interface.name(), "org.example.comprehensive");

    // Check that we have the expected number of each type of member
    assert_eq!(interface.custom_types().count(), 1);
    assert_eq!(interface.methods().count(), 3);
    assert_eq!(interface.errors().count(), 1);

    // Check that the type was parsed correctly (comments ignored for now)
    let custom_types: Vec<_> = interface.custom_types().collect();
    let custom_type = custom_types[0];
    assert_eq!(custom_type.name(), "Config");
    let object = custom_type.as_object().unwrap();
    let fields: Vec<_> = object.fields().collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].name(), "host");
    assert_eq!(fields[1].name(), "port");
    assert_eq!(fields[2].name(), "enabled");
    // Verify field comments are now parsed and attached
    let field0_comments: Vec<_> = fields[0].comments().collect();
    assert_eq!(field0_comments.len(), 1);
    assert_eq!(field0_comments[0].text(), "Host configuration");

    assert_eq!(fields[1].comments().count(), 0); // port has no comment

    let field2_comments: Vec<_> = fields[2].comments().collect();
    assert_eq!(field2_comments.len(), 1);
    assert_eq!(field2_comments[0].text(), "Enable/disable flag");

    // Check that the method has attached comments
    let methods: Vec<_> = interface.methods().collect();
    let method = methods[0];
    assert_eq!(method.name(), "Configure");
    let comments: Vec<_> = method.comments().collect();
    assert_eq!(comments.len(), 3);
    assert_eq!(comments[0].text(), "Method documentation - line 1");
    assert_eq!(comments[1].text(), "Method documentation - line 2");
    assert_eq!(comments[2].text(), "Method documentation - line 3");

    // Verify method parameters have no comments (internal comments ignored)
    let inputs: Vec<_> = method.inputs().collect();
    let outputs: Vec<_> = method.outputs().collect();
    assert_eq!(inputs[0].comments().count(), 0);
    assert_eq!(outputs[0].comments().count(), 0);

    // Check that the error has attached comments
    let errors: Vec<_> = interface.errors().collect();
    let error = errors[0];
    assert_eq!(error.name(), "ConfigurationError");
    let comments: Vec<_> = error.comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Single error comment");

    // Verify error fields have no comments (no field comments in this test)
    let fields: Vec<_> = error.fields().collect();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name(), "message");
    assert_eq!(fields[0].comments().count(), 0);
    assert_eq!(fields[1].name(), "code");
    assert_eq!(fields[1].comments().count(), 0);

    // Check Reset method with single comment
    // Check that the second method (Reset method) has attached comments
    let method = methods[1];
    assert_eq!(method.name(), "Reset");
    let comments: Vec<_> = method.comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Reset method documentation");

    // Verify method has no parameters and no comments on parameters
    assert!(method.has_no_inputs());
    assert!(method.has_no_outputs());

    // Check GetStatus method with single comment
    let method = methods[2];
    assert_eq!(method.name(), "GetStatus");
    let comments: Vec<_> = method.comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "GetStatus method documentation");

    // Verify method structure
    assert!(method.has_no_inputs());
    assert!(!method.has_no_outputs());
    let outputs: Vec<_> = method.outputs().collect();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].name(), "status");
    assert_eq!(outputs[0].comments().count(), 0);
}

#[test]
fn comment_content_edge_cases() {
    let input = r#"
interface org.example.edgecases

# Comment with special characters: !@#$%^&*()
# Comment with unicode: 🚀 UTF-8 тест
# Comment with whitespace:   spaces   and   tabs
method SpecialMethod() -> ()

#No space after hash
error SpecialError()

#    Leading spaces after hash
method AnotherMethod() -> ()
        "#;

    let interface = parse_interface(input).unwrap();

    // Check that we have the expected number of each type of member
    assert_eq!(interface.methods().count(), 2);
    assert_eq!(interface.errors().count(), 1);

    // Check method with special characters in comments
    let methods: Vec<_> = interface.methods().collect();
    assert_eq!(methods[0].name(), "SpecialMethod");
    let comments: Vec<_> = methods[0].comments().collect();
    assert_eq!(comments.len(), 3);
    assert_eq!(
        comments[0].text(),
        "Comment with special characters: !@#$%^&*()"
    );
    assert_eq!(comments[1].text(), "Comment with unicode: 🚀 UTF-8 тест");
    assert_eq!(
        comments[2].text(),
        "Comment with whitespace:   spaces   and   tabs"
    );

    // Check method with leading spaces after hash
    assert_eq!(methods[1].name(), "AnotherMethod");
    let comments: Vec<_> = methods[1].comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "Leading spaces after hash");

    // Check error with no space after hash
    let errors: Vec<_> = interface.errors().collect();
    assert_eq!(errors[0].name(), "SpecialError");
    let comments: Vec<_> = errors[0].comments().collect();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text(), "No space after hash");
}

#[test]
fn parse_any_composite_types() {
    // Test optional any
    match parse_type("?any").unwrap() {
        Type::Optional(inner) => assert_eq!(*inner, Type::Any),
        other => panic!("Expected optional any, got: {:?}", other),
    }

    // Test array of any
    match parse_type("[]any").unwrap() {
        Type::Array(inner) => assert_eq!(*inner, Type::Any),
        other => panic!("Expected array of any, got: {:?}", other),
    }

    // Test map of any
    match parse_type("[string]any").unwrap() {
        Type::Map(inner) => assert_eq!(*inner, Type::Any),
        other => panic!("Expected map of any, got: {:?}", other),
    }

    // Test nested: optional array of any
    match parse_type("?[]any").unwrap() {
        Type::Optional(optional) => match &*optional {
            Type::Array(array) => assert_eq!(*array, Type::Any),
            other => panic!("Expected array inside optional, got: {:?}", other),
        },
        other => panic!("Expected optional type, got: {:?}", other),
    }
}

#[test]
fn parse_interface_with_any_type() {
    let input = r#"
interface org.example.anytest

type Config (
    name: string,
    metadata: any,
    extra: ?any
)

method SetConfig(config: Config, data: any) -> (result: any)
method GetData() -> (items: []any, map: [string]any)
    "#;

    let interface = parse_interface(input).unwrap();
    assert_eq!(interface.name(), "org.example.anytest");
    assert_eq!(interface.custom_types().count(), 1);
    assert_eq!(interface.methods().count(), 2);

    // Check custom type fields
    let custom_types: Vec<_> = interface.custom_types().collect();
    let config = custom_types[0].as_object().unwrap();
    let fields: Vec<_> = config.fields().collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].name(), "name");
    assert_eq!(fields[0].ty(), &Type::String);
    assert_eq!(fields[1].name(), "metadata");
    assert_eq!(fields[1].ty(), &Type::Any);
    assert_eq!(fields[2].name(), "extra");
    assert_eq!(fields[2].ty(), &Type::Optional(TypeRef::new(&Type::Any)));

    // Check SetConfig method
    let methods: Vec<_> = interface.methods().collect();
    let set_config = methods[0];
    assert_eq!(set_config.name(), "SetConfig");
    let inputs: Vec<_> = set_config.inputs().collect();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[1].name(), "data");
    assert_eq!(inputs[1].ty(), &Type::Any);
    let outputs: Vec<_> = set_config.outputs().collect();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].name(), "result");
    assert_eq!(outputs[0].ty(), &Type::Any);

    // Check GetData method
    let get_data = methods[1];
    assert_eq!(get_data.name(), "GetData");
    let outputs: Vec<_> = get_data.outputs().collect();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].name(), "items");
    assert_eq!(outputs[0].ty(), &Type::Array(TypeRef::new(&Type::Any)));
    assert_eq!(outputs[1].name(), "map");
    assert_eq!(outputs[1].ty(), &Type::Map(TypeRef::new(&Type::Any)));
}

/// Parse a Varlink type from a string.
fn parse_type(input: &str) -> Result<Type<'_>, Error> {
    parse_from_str(input, varlink_type)
}

/// Parse a method from a string.
fn parse_method(input: &str) -> Result<Method<'_>, Error> {
    parse_from_str(input, method_def)
}

/// Parse an error from a string.
fn parse_error(input: &str) -> Result<crate::Error<'_>, Error> {
    parse_from_str(input, error_def)
}

/// Parse a custom type from a string.
fn parse_custom_type(input: &str) -> Result<CustomType<'_>, Error> {
    parse_from_str(input, type_def)
}

/// Parse a field from a string.
fn parse_field(input: &str) -> Result<Field<'_>, Error> {
    parse_from_str(input, field)
}

#[test]
fn trailing_comment_after_parameter() {
    // The parser used to fail when a comment appeared between a parameter's
    // type and the closing `)` (or comma).
    let input = r#"
interface a.b

method M (
    x: bool #
) -> ()
    "#;
    let interface = parse_interface(input).expect("trailing `#` should parse");
    let methods: Vec<_> = interface.methods().collect();
    assert_eq!(methods[0].name(), "M");
    let inputs: Vec<_> = methods[0].inputs().collect();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].name(), "x");
    assert_eq!(inputs[0].ty(), &Type::Bool);

    // Same with content, between params and before closing paren of an inline
    // struct, plus between a custom-type field and `)`.
    let input = r#"
interface a.b

type T (
    a: bool, # trailing for a
    b: int # trailing for b
)

method M (
    x: (
        y: bool # trailing inside nested struct
    ),
    z: int # trailing for z
) -> ()
    "#;
    let interface = parse_interface(input).expect("trailing comments should parse");
    let custom = interface.custom_types().next().unwrap();
    assert_eq!(custom.name(), "T");
    assert_eq!(custom.as_object().unwrap().fields().count(), 2);

    let methods: Vec<_> = interface.methods().collect();
    let inputs: Vec<_> = methods[0].inputs().collect();
    assert_eq!(inputs.len(), 2);
    let Type::Object(nested) = inputs[0].ty() else {
        panic!("Expected x to be an inline struct");
    };
    assert_eq!(nested.iter().count(), 1);
}

#[test]
fn nested_struct_field_comments() {
    // Comments on fields of a nested inline struct were dropped because the
    // struct parser consumed them as plain whitespace.
    let input = r#"
interface a.b

method M (
    # works
    x: (
        # doesn't work
        y: bool
    )
) -> ()
    "#;

    let interface = parse_interface(input).unwrap();
    let methods: Vec<_> = interface.methods().collect();
    let inputs: Vec<_> = methods[0].inputs().collect();
    assert_eq!(inputs.len(), 1);

    let outer_comments: Vec<_> = inputs[0].comments().collect();
    assert_eq!(outer_comments.len(), 1);
    assert_eq!(outer_comments[0].text(), "works");

    let Type::Object(nested_fields) = inputs[0].ty() else {
        panic!(
            "Expected x to be an inline struct, got: {:?}",
            inputs[0].ty()
        );
    };
    let nested_fields: Vec<_> = nested_fields.iter().collect();
    assert_eq!(nested_fields.len(), 1);
    assert_eq!(nested_fields[0].name(), "y");
    let nested_comments: Vec<_> = nested_fields[0].comments().collect();
    assert_eq!(nested_comments.len(), 1);
    assert_eq!(nested_comments[0].text(), "doesn't work");
}

#[test]
fn nested_enum_variant_comments() {
    // Variants of a nested inline enum should retain their preceding comments.
    let input = r#"
interface a.b

method M (
    x: (
        # the no variant
        no,
        # the yes variant
        yes
    )
) -> ()
    "#;

    let interface = parse_interface(input).unwrap();
    let methods: Vec<_> = interface.methods().collect();
    let inputs: Vec<_> = methods[0].inputs().collect();
    let Type::Enum(variants) = inputs[0].ty() else {
        panic!("Expected x to be an inline enum, got: {:?}", inputs[0].ty());
    };
    let variants: Vec<_> = variants.iter().collect();
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].name(), "no");
    let no_comments: Vec<_> = variants[0].comments().collect();
    assert_eq!(no_comments.len(), 1);
    assert_eq!(no_comments[0].text(), "the no variant");
    assert_eq!(variants[1].name(), "yes");
    let yes_comments: Vec<_> = variants[1].comments().collect();
    assert_eq!(yes_comments.len(), 1);
    assert_eq!(yes_comments[0].text(), "the yes variant");
}

#[test]
fn method_with_parameter_comments() {
    let input = r#"
interface org.example.test

method TestMethod(
# The user's name
name: string,
# The user's age in years
age: int,
# Optional email address
email: ?string
) -> (
# Success message
message: string
)
    "#;

    let interface = parse_interface(input).unwrap();

    // Check that we have the expected number of each type of member
    assert_eq!(interface.methods().count(), 1);

    let methods: Vec<_> = interface.methods().collect();
    let method = methods[0];

    assert_eq!(method.name(), "TestMethod");

    // Check input parameters
    let inputs: Vec<_> = method.inputs().collect();
    assert_eq!(inputs.len(), 3);

    assert_eq!(inputs[0].name(), "name");
    let name_comments: Vec<_> = inputs[0].comments().collect();
    assert_eq!(name_comments.len(), 1);
    assert_eq!(name_comments[0].text(), "The user's name");

    assert_eq!(inputs[1].name(), "age");
    let age_comments: Vec<_> = inputs[1].comments().collect();
    assert_eq!(age_comments.len(), 1);
    assert_eq!(age_comments[0].text(), "The user's age in years");

    assert_eq!(inputs[2].name(), "email");
    let email_comments: Vec<_> = inputs[2].comments().collect();
    assert_eq!(email_comments.len(), 1);
    assert_eq!(email_comments[0].text(), "Optional email address");

    // Check output parameters
    let outputs: Vec<_> = method.outputs().collect();
    assert_eq!(outputs.len(), 1);

    assert_eq!(outputs[0].name(), "message");
    let message_comments: Vec<_> = outputs[0].comments().collect();
    assert_eq!(message_comments.len(), 1);
    assert_eq!(message_comments[0].text(), "Success message");
}

#[test]
fn error_with_field_comments() {
    let input = r#"
interface org.example.test

error TestError(
# Error code
code: int,
# Error message
message: string
)
    "#;

    let interface = parse_interface(input).unwrap();

    // Check that we have the expected number of each type of member
    assert_eq!(interface.errors().count(), 1);

    let errors: Vec<_> = interface.errors().collect();
    let error = errors[0];

    assert_eq!(error.name(), "TestError");

    let fields: Vec<_> = error.fields().collect();
    assert_eq!(fields.len(), 2);

    assert_eq!(fields[0].name(), "code");
    let code_comments: Vec<_> = fields[0].comments().collect();
    assert_eq!(code_comments.len(), 1);
    assert_eq!(code_comments[0].text(), "Error code");

    assert_eq!(fields[1].name(), "message");
    let message_comments: Vec<_> = fields[1].comments().collect();
    assert_eq!(message_comments.len(), 1);
    assert_eq!(message_comments[0].text(), "Error message");
}

#[test]
fn struct_type_with_field_comments() {
    let input = r#"
interface org.example.test

type Person(
# Person's full name
name: string,
# Age in years
age: int,
# Contact information
email: ?string
)
    "#;

    let interface = parse_interface(input).unwrap();

    // Check that we have the expected number of each type of member
    assert_eq!(interface.custom_types().count(), 1);

    let custom_types: Vec<_> = interface.custom_types().collect();
    let custom_type = custom_types[0];
    let Some(object) = custom_type.as_object() else {
        panic!("Expected custom type to be an object");
    };

    assert_eq!(object.name(), "Person");

    let fields: Vec<_> = object.fields().collect();
    assert_eq!(fields.len(), 3);

    assert_eq!(fields[0].name(), "name");
    let name_comments: Vec<_> = fields[0].comments().collect();
    assert_eq!(name_comments.len(), 1);
    assert_eq!(name_comments[0].text(), "Person's full name");

    assert_eq!(fields[1].name(), "age");
    let age_comments: Vec<_> = fields[1].comments().collect();
    assert_eq!(age_comments.len(), 1);
    assert_eq!(age_comments[0].text(), "Age in years");

    assert_eq!(fields[2].name(), "email");
    let email_comments: Vec<_> = fields[2].comments().collect();
    assert_eq!(email_comments.len(), 1);
    assert_eq!(email_comments[0].text(), "Contact information");
}

#[test]
fn interface_with_comments() {
    let input = r#"
# Interface documentation - line 1
# Interface documentation - line 2
interface org.example.test

method SimpleMethod() -> ()
    "#;

    let interface = parse_interface(input).unwrap();
    assert_eq!(interface.name(), "org.example.test");

    // Check interface comments
    let comments: Vec<_> = interface.comments().collect();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].text(), "Interface documentation - line 1");
    assert_eq!(comments[1].text(), "Interface documentation - line 2");

    // Verify the interface still has its members
    assert_eq!(interface.methods().count(), 1);
}

#[test]
fn custom_type_with_comments() {
    let input = r#"
# Type documentation - line 1
# Type documentation - line 2
type Person(
    name: string,
    age: int
)
    "#;

    let custom_type = parse_custom_type(input).unwrap();
    assert_eq!(custom_type.name(), "Person");

    // Check that this is an object type
    let Some(object) = custom_type.as_object() else {
        panic!("Expected custom type to be an object");
    };

    // Check type comments
    let comments: Vec<_> = object.comments().collect();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].text(), "Type documentation - line 1");
    assert_eq!(comments[1].text(), "Type documentation - line 2");

    // Verify the type still has its fields
    let fields: Vec<_> = object.fields().collect();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name(), "name");
    assert_eq!(fields[1].name(), "age");
}

#[test]
fn interface_and_type_comments_integration() {
    let input = r#"
# Main interface documentation
# Describes the core API
interface org.example.core

# User data structure
# Contains basic user information
type User(
    # User's display name
    name: string,
    # User's email address
    email: string
)

# Get user information
# Returns user details by ID
method GetUser(id: int) -> (user: User)
    "#;

    let interface = parse_interface(input).unwrap();
    assert_eq!(interface.name(), "org.example.core");

    // Check interface comments
    let interface_comments: Vec<_> = interface.comments().collect();
    assert_eq!(interface_comments.len(), 2);
    assert_eq!(interface_comments[0].text(), "Main interface documentation");
    assert_eq!(interface_comments[1].text(), "Describes the core API");

    // Check members
    assert_eq!(interface.custom_types().count(), 1);
    assert_eq!(interface.methods().count(), 1);

    // Check the custom type member and its comments
    let custom_types: Vec<_> = interface.custom_types().collect();
    let custom_type = custom_types[0];
    let Some(object) = custom_type.as_object() else {
        panic!("Expected custom type to be an object");
    };

    let type_comments: Vec<_> = object.comments().collect();
    assert_eq!(type_comments.len(), 2);
    assert_eq!(type_comments[0].text(), "User data structure");
    assert_eq!(type_comments[1].text(), "Contains basic user information");

    // Check field comments
    let fields: Vec<_> = object.fields().collect();
    assert_eq!(fields.len(), 2);

    let name_comments: Vec<_> = fields[0].comments().collect();
    assert_eq!(name_comments.len(), 1);
    assert_eq!(name_comments[0].text(), "User's display name");

    let email_comments: Vec<_> = fields[1].comments().collect();
    assert_eq!(email_comments.len(), 1);
    assert_eq!(email_comments[0].text(), "User's email address");

    // Check the method member and its comments
    let methods: Vec<_> = interface.methods().collect();
    let method = methods[0];

    let method_comments: Vec<_> = method.comments().collect();
    assert_eq!(method_comments.len(), 2);
    assert_eq!(method_comments[0].text(), "Get user information");
    assert_eq!(method_comments[1].text(), "Returns user details by ID");
}
