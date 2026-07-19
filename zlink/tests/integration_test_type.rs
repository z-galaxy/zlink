#![cfg(feature = "introspection")]

//! Integration test to verify end-to-end Type functionality.
//!
//! This test verifies that:
//! 1. The Type trait and derive macro are both available from zlink::introspect
//! 2. They work together seamlessly
//! 3. The macro generates correct Type implementations
//! 4. Both simple and complex types work as expected

use zlink::{idl, introspect::Type};

#[derive(Type)]
#[allow(dead_code)]
struct Person {
    name: String,
    age: u32,
    email: Option<String>,
}

#[derive(Type)]
struct Unit;

#[derive(Type)]
#[allow(dead_code)]
enum Status {
    Active,
    Inactive,
    Pending,
}

#[derive(Type)]
#[allow(dead_code)]
struct Complex {
    id: u64,
    tags: Vec<String>,
    metadata: Option<Vec<String>>,
    active: bool,
}

#[test]
fn type_integration() {
    // Test that TYPE is available and returns correct type
    match Person::TYPE {
        idl::Type::Object(fields) => {
            let field_vec: Vec<_> = fields.iter().collect();
            assert_eq!(field_vec.len(), 3);

            assert_eq!(field_vec[0].name(), "name");
            assert_eq!(field_vec[0].ty(), &idl::Type::String);

            assert_eq!(field_vec[1].name(), "age");
            assert_eq!(field_vec[1].ty(), &idl::Type::Int);

            assert_eq!(field_vec[2].name(), "email");
            match field_vec[2].ty() {
                idl::Type::Optional(inner) => assert_eq!(inner.inner(), &idl::Type::String),
                _ => panic!("Expected optional type"),
            }
        }
        _ => panic!("Expected struct type"),
    }

    // Test unit struct
    match Unit::TYPE {
        idl::Type::Object(fields) => {
            assert_eq!(fields.len(), 0);
        }
        _ => panic!("Expected struct type"),
    }

    // Test complex types
    match Complex::TYPE {
        idl::Type::Object(fields) => {
            let field_vec: Vec<_> = fields.iter().collect();
            assert_eq!(field_vec.len(), 4);

            // Test Vec<String> field
            assert_eq!(field_vec[1].name(), "tags");
            match field_vec[1].ty() {
                idl::Type::Array(inner) => assert_eq!(inner.inner(), &idl::Type::String),
                _ => panic!("Expected array type"),
            }

            // Test Option<Vec<String>> field
            assert_eq!(field_vec[2].name(), "metadata");
            match field_vec[2].ty() {
                idl::Type::Optional(optional_inner) => match optional_inner.inner() {
                    idl::Type::Array(array_inner) => {
                        assert_eq!(array_inner.inner(), &idl::Type::String)
                    }
                    _ => panic!("Expected array inside optional"),
                },
                _ => panic!("Expected optional type"),
            }
        }
        _ => panic!("Expected struct type"),
    }
}

#[test]
fn const_compatibility() {
    // Verify that TYPE can be used in const contexts
    const _PERSON_TYPE: &idl::Type<'static> = Person::TYPE;
    const _UNIT_TYPE: &idl::Type<'static> = Unit::TYPE;
    const _COMPLEX_TYPE: &idl::Type<'static> = Complex::TYPE;
}

#[test]
fn trait_and_macro_same_name() {
    // This test verifies that we can import both the trait and macro
    // with the same name from the same module, just like zvariant does
    use zlink::introspect::Type; // This imports both trait and macro

    #[derive(Type)]
    #[allow(dead_code)]
    struct Local {
        value: i32,
    }

    // Use the trait method
    match Local::TYPE {
        idl::Type::Object(fields) => {
            let field_vec: Vec<_> = fields.iter().collect();
            assert_eq!(field_vec.len(), 1);
            assert_eq!(field_vec[0].name(), "value");
            assert_eq!(field_vec[0].ty(), &idl::Type::Int);
        }
        _ => panic!("Expected struct type"),
    }
}

#[test]
fn enum_type_integration() {
    // This test verifies that enum Type works with the main API
    match Status::TYPE {
        idl::Type::Enum(variants) => {
            let variant_vec: Vec<_> = variants.iter().collect();
            assert_eq!(variant_vec.len(), 3);
            assert_eq!(variant_vec[0].name(), "Active");
            assert_eq!(variant_vec[1].name(), "Inactive");
            assert_eq!(variant_vec[2].name(), "Pending");
        }
        _ => panic!("Expected enum type for Status"),
    }
}

#[derive(Type)]
#[allow(dead_code)]
struct Inner {
    a: u32,
    b: String,
}

#[derive(Type)]
#[allow(dead_code)]
struct WithSkipAndFlatten {
    id: u64,
    #[zlink(skip)]
    internal: Vec<u8>,
    #[zlink(flatten)]
    inner: Inner,
    tail: bool,
}

#[test]
fn skip_and_flatten_reshape_the_object() {
    match WithSkipAndFlatten::TYPE {
        idl::Type::Object(fields) => {
            let names: Vec<_> = fields.iter().map(|f| f.name()).collect();
            // `internal` is skipped; `inner` is replaced by its members `a`, `b`, in place.
            assert_eq!(names, ["id", "a", "b", "tail"]);
        }
        _ => panic!("expected object"),
    }
}

#[derive(Type)]
#[allow(dead_code)]
enum WithSkippedVariant {
    Active,
    #[zlink(skip)]
    Hidden,
    Pending,
}

#[test]
fn skip_drops_an_enum_variant() {
    match WithSkippedVariant::TYPE {
        idl::Type::Enum(variants) => {
            let names: Vec<_> = variants.iter().map(|v| v.name()).collect();
            assert_eq!(names, ["Active", "Pending"]);
        }
        _ => panic!("expected enum"),
    }
}
