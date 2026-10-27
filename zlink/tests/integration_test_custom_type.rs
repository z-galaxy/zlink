#![cfg(feature = "introspection")]

//! Integration test to verify end-to-end CustomType functionality.
//!
//! This test verifies that:
//! 1. The custom::Type trait is available from zlink::introspect::custom
//! 2. It can be implemented manually for custom types
//! 3. It generates correct Type implementations with type names
//! 4. Both object and enum custom types work as expected

use zlink::{
    idl::{self, CustomEnum, CustomObject, EnumVariant, Field},
    introspect::{CustomType, Type},
    names::{FieldName, TypeName},
};

// Test struct that implements custom::Type
struct Point;

impl CustomType for Point {
    const CUSTOM_TYPE: &'static idl::CustomType<'static> = &{
        static FIELD_X: Field = Field::new(
            FieldName::from_static_str_unchecked("x"),
            &idl::Type::Float,
            &[],
        );
        static FIELD_Y: Field = Field::new(
            FieldName::from_static_str_unchecked("y"),
            &idl::Type::Float,
            &[],
        );
        static FIELDS: &[&Field<'static>] = &[&FIELD_X, &FIELD_Y];

        idl::CustomType::Object(CustomObject::new(
            TypeName::from_static_str_unchecked("Point"),
            FIELDS,
            &[],
        ))
    };
}

impl Type for Point {
    const TYPE: &'static idl::Type<'static> =
        &{ idl::Type::Custom(TypeName::from_static_str_unchecked("Point")) };
}

// Test enum that implements custom::Type
struct Color;

impl CustomType for Color {
    const CUSTOM_TYPE: &'static idl::CustomType<'static> = &{
        static VARIANT_RED: EnumVariant<'static> =
            EnumVariant::new(FieldName::from_static_str_unchecked("Red"), &[]);
        static VARIANT_GREEN: EnumVariant<'static> =
            EnumVariant::new(FieldName::from_static_str_unchecked("Green"), &[]);
        static VARIANT_BLUE: EnumVariant<'static> =
            EnumVariant::new(FieldName::from_static_str_unchecked("Blue"), &[]);
        static VARIANTS: &[&EnumVariant<'static>] = &[&VARIANT_RED, &VARIANT_GREEN, &VARIANT_BLUE];

        idl::CustomType::Enum(CustomEnum::new(
            TypeName::from_static_str_unchecked("Color"),
            VARIANTS,
            &[],
        ))
    };
}

// Complex struct with various field types
struct Person;

impl CustomType for Person {
    const CUSTOM_TYPE: &'static idl::CustomType<'static> = &{
        static FIELD_NAME: Field = Field::new(
            FieldName::from_static_str_unchecked("name"),
            &idl::Type::String,
            &[],
        );
        static FIELD_AGE: Field = Field::new(
            FieldName::from_static_str_unchecked("age"),
            &idl::Type::Int,
            &[],
        );
        static FIELD_ACTIVE: Field = Field::new(
            FieldName::from_static_str_unchecked("active"),
            &idl::Type::Bool,
            &[],
        );
        static FIELDS: &[&Field<'static>] = &[&FIELD_NAME, &FIELD_AGE, &FIELD_ACTIVE];

        idl::CustomType::Object(CustomObject::new(
            TypeName::from_static_str_unchecked("Person"),
            FIELDS,
            &[],
        ))
    };
}

impl Type for Person {
    const TYPE: &'static idl::Type<'static> =
        &{ idl::Type::Custom(TypeName::from_static_str_unchecked("Person")) };
}

#[test]
fn custom_struct_type_integration() {
    // Test that TYPE is available and returns correct custom type
    match Point::CUSTOM_TYPE {
        idl::CustomType::Object(obj) => {
            assert_eq!(obj.name(), "Point");

            let fields: Vec<_> = obj.fields().collect();
            assert_eq!(fields.len(), 2);

            assert_eq!(fields[0].name(), "x");
            assert_eq!(fields[0].ty(), &idl::Type::Float);

            assert_eq!(fields[1].name(), "y");
            assert_eq!(fields[1].ty(), &idl::Type::Float);
        }
        _ => panic!("Expected custom object type"),
    }

    // Verify the type has a name (unlike regular Type)
    assert_eq!(Point::CUSTOM_TYPE.name(), "Point");
    assert!(Point::CUSTOM_TYPE.is_object());
    assert!(!Point::CUSTOM_TYPE.is_enum());
}

#[test]
fn custom_enum_type_integration() {
    // Test enum custom type
    match Color::CUSTOM_TYPE {
        idl::CustomType::Enum(enm) => {
            assert_eq!(enm.name(), "Color");

            let variants: Vec<_> = enm.variants().collect();
            assert_eq!(variants.len(), 3);

            assert_eq!(variants[0].name(), "Red");
            assert_eq!(variants[1].name(), "Green");
            assert_eq!(variants[2].name(), "Blue");
        }
        _ => panic!("Expected custom enum type"),
    }

    // Verify the type has a name and correct variant type
    assert_eq!(Color::CUSTOM_TYPE.name(), "Color");
    assert!(!Color::CUSTOM_TYPE.is_object());
    assert!(Color::CUSTOM_TYPE.is_enum());
}

#[test]
fn complex_custom_type_integration() {
    // Test complex struct with multiple field types
    match Person::CUSTOM_TYPE {
        idl::CustomType::Object(obj) => {
            assert_eq!(obj.name(), "Person");

            let fields: Vec<_> = obj.fields().collect();
            assert_eq!(fields.len(), 3);

            assert_eq!(fields[0].name(), "name");
            assert_eq!(fields[0].ty(), &idl::Type::String);

            assert_eq!(fields[1].name(), "age");
            assert_eq!(fields[1].ty(), &idl::Type::Int);

            assert_eq!(fields[2].name(), "active");
            assert_eq!(fields[2].ty(), &idl::Type::Bool);
        }
        _ => panic!("Expected custom object type"),
    }
}

#[test]
fn const_compatibility() {
    // Verify that TYPE can be used in const contexts
    const _POINT_TYPE: &idl::CustomType<'static> = Point::CUSTOM_TYPE;
    const _COLOR_TYPE: &idl::CustomType<'static> = Color::CUSTOM_TYPE;
    const _PERSON_TYPE: &idl::CustomType<'static> = Person::CUSTOM_TYPE;
}

#[test]
fn trait_imports() {
    // This test verifies that we can import the custom Type trait
    // and it doesn't conflict with the regular Type trait

    // Both traits should be available and distinct
    const _CUSTOM: &idl::CustomType<'static> = Point::CUSTOM_TYPE;
    const _REGULAR: &idl::Type<'static> = Point::TYPE;
}

#[test]
fn type_accessor_methods() {
    // Test the convenience methods on custom types
    let point_type = Point::CUSTOM_TYPE;
    assert!(point_type.is_object());
    assert!(!point_type.is_enum());
    assert!(point_type.as_object().is_some());
    assert!(point_type.as_enum().is_none());

    let color_type = Color::CUSTOM_TYPE;
    assert!(!color_type.is_object());
    assert!(color_type.is_enum());
    assert!(color_type.as_object().is_none());
    assert!(color_type.as_enum().is_some());
}
