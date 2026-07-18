#![cfg(feature = "introspection")]

//! Integration test to verify end-to-end CustomType derive macro functionality.
//!
//! This test verifies that:
//! 1. The CustomType derive macro is available from zlink::idl::custom
//! 2. It generates correct custom type implementations for structs and enums
//! 3. The generated types include proper names and work with the API
//! 4. Both the trait and derive macro are available from the same module

use zlink::{idl, introspect::CustomType};

#[test]
fn custom_type_derive_integration() {
    // Test struct with CustomType derive
    #[derive(CustomType)]
    #[allow(dead_code)]
    struct Point {
        x: f64,
        y: f64,
    }

    // Verify the derive macro generated the correct implementation
    match Point::CUSTOM_TYPE {
        idl::CustomType::Object(obj) => {
            assert_eq!(obj.name(), "Point");

            let fields: Vec<_> = obj.fields().collect();
            assert_eq!(fields.len(), 2);

            assert_eq!(fields[0].name(), "x");
            assert_eq!(fields[1].name(), "y");
        }
        _ => panic!("Expected custom object type for Point"),
    }

    // Test that the type name is accessible
    assert_eq!(Point::CUSTOM_TYPE.name(), "Point");
    assert!(Point::CUSTOM_TYPE.is_object());
    assert!(!Point::CUSTOM_TYPE.is_enum());
}

#[test]
fn custom_enum_derive_integration() {
    // Test enum with CustomType derive
    #[derive(CustomType)]
    #[allow(dead_code)]
    enum Status {
        Active,
        Inactive,
        Pending,
    }

    // Verify the derive macro generated the correct implementation
    match Status::CUSTOM_TYPE {
        idl::CustomType::Enum(enm) => {
            assert_eq!(enm.name(), "Status");

            let variants: Vec<_> = enm.variants().collect();
            assert_eq!(variants.len(), 3);

            assert_eq!(variants[0].name(), "Active");
            assert_eq!(variants[1].name(), "Inactive");
            assert_eq!(variants[2].name(), "Pending");
        }
        _ => panic!("Expected custom enum type for Status"),
    }

    // Test that the type name is accessible
    assert_eq!(Status::CUSTOM_TYPE.name(), "Status");
    assert!(!Status::CUSTOM_TYPE.is_object());
    assert!(Status::CUSTOM_TYPE.is_enum());
}

#[test]
fn unit_struct_derive_integration() {
    // Test unit struct
    #[derive(CustomType)]
    #[allow(dead_code)]
    struct Unit;

    match Unit::CUSTOM_TYPE {
        idl::CustomType::Object(obj) => {
            assert_eq!(obj.name(), "Unit");

            let fields: Vec<_> = obj.fields().collect();
            assert_eq!(fields.len(), 0);
        }
        _ => panic!("Expected custom object type for Unit"),
    }
}

#[test]
fn complex_struct_derive_integration() {
    // Test complex struct with various field types
    #[derive(CustomType)]
    #[allow(dead_code)]
    struct Person {
        name: String,
        age: u32,
        email: Option<String>,
        tags: Vec<String>,
        active: bool,
    }

    match Person::CUSTOM_TYPE {
        idl::CustomType::Object(obj) => {
            assert_eq!(obj.name(), "Person");

            let fields: Vec<_> = obj.fields().collect();
            assert_eq!(fields.len(), 5);

            // Verify field names
            assert_eq!(fields[0].name(), "name");
            assert_eq!(fields[1].name(), "age");
            assert_eq!(fields[2].name(), "email");
            assert_eq!(fields[3].name(), "tags");
            assert_eq!(fields[4].name(), "active");
        }
        _ => panic!("Expected custom object type for Person"),
    }
}

#[test]
fn const_compatibility() {
    #[derive(CustomType)]
    #[allow(dead_code)]
    struct TestStruct {
        value: i32,
    }

    #[derive(CustomType)]
    #[allow(dead_code)]
    enum TestEnum {
        Variant1,
        Variant2,
    }

    // Verify that TYPE can be used in const contexts
    const _STRUCT_TYPE: &idl::CustomType<'static> = TestStruct::CUSTOM_TYPE;
    const _ENUM_TYPE: &idl::CustomType<'static> = TestEnum::CUSTOM_TYPE;
}

#[test]
fn trait_and_derive_same_import() {
    // This test verifies that we can import both the trait and derive macro
    // with the same name from the custom module
    use zlink::introspect::CustomType;

    #[derive(CustomType)]
    #[allow(dead_code)]
    struct LocalType {
        field: String,
    }

    // Verify we can use the trait method
    let r#type = LocalType::CUSTOM_TYPE;
    assert_eq!(r#type.name(), "LocalType");
}

#[test]
fn single_variant_enum() {
    #[derive(CustomType)]
    #[allow(dead_code)]
    enum SingleVariant {
        Only,
    }

    match SingleVariant::CUSTOM_TYPE {
        idl::CustomType::Enum(enm) => {
            assert_eq!(enm.name(), "SingleVariant");

            let variants: Vec<_> = enm.variants().collect();
            assert_eq!(variants.len(), 1);
            assert_eq!(variants[0].name(), "Only");
        }
        _ => panic!("Expected custom enum type for SingleVariant"),
    }
}

#[test]
fn type_names_preserved() {
    // Test that type names are used verbatim, with no case normalisation applied. `HTTPServer`
    // (rather than the earlier `snake_case_name`) makes the point while staying a valid Varlink
    // type name: a naive PascalCase normaliser would fold its run of capitals to `HttpServer`.
    #[derive(CustomType)]
    #[allow(dead_code)]
    struct HTTPServer {
        value: i32,
    }

    #[derive(CustomType)]
    #[allow(dead_code, non_camel_case_types)]
    enum MixedCaseEnum {
        VariantOne,
        variant_two,
        VARIANT_THREE,
    }

    assert_eq!(HTTPServer::CUSTOM_TYPE.name(), "HTTPServer");
    assert_eq!(MixedCaseEnum::CUSTOM_TYPE.name(), "MixedCaseEnum");

    if let idl::CustomType::Enum(enm) = MixedCaseEnum::CUSTOM_TYPE {
        let variants: Vec<_> = enm.variants().collect();
        assert_eq!(variants[0].name(), "VariantOne");
        assert_eq!(variants[1].name(), "variant_two");
        assert_eq!(variants[2].name(), "VARIANT_THREE");
    }
}

#[test]
fn derive_macro_available_from_main_module() {
    // Verify the derive macro is available from the expected location
    use zlink::introspect::CustomType;

    #[derive(CustomType)]
    #[allow(dead_code)]
    struct ExportTest {
        data: String,
    }

    // If this compiles and we can access TYPE, the export works
    assert_eq!(ExportTest::CUSTOM_TYPE.name(), "ExportTest");
}

#[test]
fn enum_variant_names_not_renamed_for_encoding() {
    // This test verifies that enum variant names in Type are preserved exactly
    // as written in the code, ignoring any serde renaming attributes
    use serde::{Deserialize, Serialize};

    #[derive(CustomType, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[allow(dead_code)]
    enum DriveState {
        Idle,
        Spooling,
        Busy,
        VeryBusy, // This would be "very_busy" in JSON but should be "VeryBusy" in Type
    }

    match DriveState::CUSTOM_TYPE {
        idl::CustomType::Enum(enm) => {
            assert_eq!(enm.name(), "DriveState");

            let variants: Vec<_> = enm.variants().collect();
            assert_eq!(variants.len(), 4);

            // Verify that the original variant names are preserved, not the serde-renamed ones
            assert_eq!(variants[0].name(), "Idle"); // Not "idle"
            assert_eq!(variants[1].name(), "Spooling"); // Not "spooling"
            assert_eq!(variants[2].name(), "Busy"); // Not "busy"
            assert_eq!(variants[3].name(), "VeryBusy"); // Not "very_busy"
        }
        _ => panic!("Expected custom enum type for DriveState"),
    }
}
