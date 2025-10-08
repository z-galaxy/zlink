//! Type definition for Varlink IDL custom types.

use core::fmt;

use super::{CustomEnum, CustomObject};

/// A custom type definition in Varlink IDL.
///
/// This can be either a struct-like object type with named fields,
/// or an enum-like type with named variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomType<'a> {
    /// A struct-like custom type with named fields.
    Object(CustomObject<'a>),
    /// An enum-like custom type with named variants.
    Enum(CustomEnum<'a>),
}

impl<'a> CustomType<'a> {
    /// Returns the name of the custom type.
    pub fn name(&self) -> &'a str {
        match self {
            CustomType::Object(obj) => obj.name(),
            CustomType::Enum(enm) => enm.name(),
        }
    }

    /// Returns true if this is an object custom type.
    pub fn is_object(&self) -> bool {
        matches!(self, CustomType::Object(_))
    }

    /// Returns true if this is an enum custom type.
    pub fn is_enum(&self) -> bool {
        matches!(self, CustomType::Enum(_))
    }

    /// Returns the object if this is an object custom type.
    pub fn as_object(&self) -> Option<&CustomObject<'a>> {
        match self {
            CustomType::Object(obj) => Some(obj),
            CustomType::Enum(_) => None,
        }
    }

    /// Returns the enum if this is an enum custom type.
    pub fn as_enum(&self) -> Option<&CustomEnum<'a>> {
        match self {
            CustomType::Object(_) => None,
            CustomType::Enum(enm) => Some(enm),
        }
    }
}

impl<'a> From<CustomObject<'a>> for CustomType<'a> {
    fn from(obj: CustomObject<'a>) -> Self {
        CustomType::Object(obj)
    }
}

impl<'a> From<CustomEnum<'a>> for CustomType<'a> {
    fn from(enm: CustomEnum<'a>) -> Self {
        CustomType::Enum(enm)
    }
}

impl<'a> fmt::Display for CustomType<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CustomType::Object(obj) => write!(f, "{obj}"),
            CustomType::Enum(enm) => write!(f, "{enm}"),
        }
    }
}

#[cfg(test)]
#[cfg(feature = "introspection")]
mod tests {
    use alloc::vec;

    use super::*;
    use crate::{
        idl::{self, EnumVariant, Field},
        introspect,
    };

    #[test]
    fn object_creation() {
        let field_x = Field::new("x", <f64 as introspect::Type>::TYPE, &[]);
        let field_y = Field::new("y", <f64 as introspect::Type>::TYPE, &[]);
        let fields = [&field_x, &field_y];

        let custom_obj = CustomObject::new("Point", &fields, &[]);
        assert_eq!(custom_obj.name(), "Point");
        assert_eq!(custom_obj.fields().count(), 2);

        // Check the fields individually - order and values.
        let fields = custom_obj.fields().collect::<Vec<_>>();
        assert_eq!(fields[0].name(), "x");
        assert_eq!(fields[0].ty(), &idl::Type::Float);
        assert_eq!(fields[1].name(), "y");
        assert_eq!(fields[1].ty(), &idl::Type::Float);
    }

    #[test]
    fn enum_creation() {
        let red = EnumVariant::new_owned("red", vec![]);
        let green = EnumVariant::new_owned("green", vec![]);
        let blue = EnumVariant::new_owned("blue", vec![]);
        let custom_enum = CustomEnum::new_owned("Color", vec![red, green, blue], vec![]);

        assert_eq!(custom_enum.name(), "Color");
        assert_eq!(custom_enum.variants().count(), 3);

        // Check the variants individually - order and values.
        let variants = custom_enum.variants().collect::<Vec<_>>();
        assert_eq!(variants[0].name(), "red");
        assert_eq!(variants[1].name(), "green");
        assert_eq!(variants[2].name(), "blue");
    }

    #[test]
    fn type_from_object() {
        let field_x = Field::new("x", <f64 as introspect::Type>::TYPE, &[]);
        let field_y = Field::new("y", <f64 as introspect::Type>::TYPE, &[]);
        let fields = [&field_x, &field_y];

        let custom_obj = CustomObject::new("Point", &fields, &[]);
        let custom_type = idl::CustomType::from(custom_obj);

        assert_eq!(custom_type.name(), "Point");
        assert!(custom_type.is_object());
        assert!(!custom_type.is_enum());
        assert!(custom_type.as_object().is_some());
        assert!(custom_type.as_enum().is_none());
    }

    #[test]
    fn type_from_enum() {
        let red = EnumVariant::new_owned("red", vec![]);
        let green = EnumVariant::new_owned("green", vec![]);
        let blue = EnumVariant::new_owned("blue", vec![]);
        let custom_enum = CustomEnum::new_owned("Color", vec![red, green, blue], vec![]);
        let custom_type = idl::CustomType::from(custom_enum);

        assert_eq!(custom_type.name(), "Color");
        assert!(!custom_type.is_object());
        assert!(custom_type.is_enum());
        assert!(custom_type.as_object().is_none());
        assert!(custom_type.as_enum().is_some());
    }

    #[test]
    fn type_display_object() {
        // Test object display
        let field_x = Field::new("x", <i64 as introspect::Type>::TYPE, &[]);
        let field_y = Field::new("y", <i64 as introspect::Type>::TYPE, &[]);
        let fields = [&field_x, &field_y];
        let custom_obj = CustomObject::new("Point", &fields, &[]);
        let custom_type = idl::CustomType::from(custom_obj);
        use core::fmt::Write;
        let mut buf = String::new();
        write!(buf, "{}", custom_type).unwrap();
        assert_eq!(buf.as_str(), "type Point (x: int, y: int)");
    }

    #[test]
    fn type_display_enum() {
        // Test enum display
        const DIRECTION_VARIANTS: &[&EnumVariant<'static>] = &[
            &EnumVariant::new("north", &[]),
            &EnumVariant::new("south", &[]),
            &EnumVariant::new("east", &[]),
            &EnumVariant::new("west", &[]),
        ];
        let custom_enum = CustomEnum::new("Direction", DIRECTION_VARIANTS, &[]);
        let custom_type = idl::CustomType::from(custom_enum);
        use core::fmt::Write;
        let mut buf = String::new();
        write!(buf, "{}", custom_type).unwrap();
        assert_eq!(buf.as_str(), "type Direction (north, south, east, west)");
    }

    #[test]
    fn owned_types() {
        // Test owned object
        let fields = vec![
            Field::new("name", <&str as introspect::Type>::TYPE, &[]),
            Field::new("age", <i64 as introspect::Type>::TYPE, &[]),
        ];
        let custom_obj = CustomObject::new_owned("Person", fields, vec![]);
        assert_eq!(custom_obj.name(), "Person");
        use core::fmt::Write;
        let mut buf = String::new();
        write!(buf, "{}", custom_obj).unwrap();
        assert_eq!(buf.as_str(), "type Person (name: string, age: int)");

        // Test owned enum
        let custom_enum = CustomEnum::new_owned(
            "Size",
            vec![
                EnumVariant::new_owned("small", vec![]),
                EnumVariant::new_owned("medium", vec![]),
                EnumVariant::new_owned("large", vec![]),
            ],
            vec![],
        );
        assert_eq!(custom_enum.name(), "Size");
        let mut buf = String::new();
        write!(buf, "{}", custom_enum).unwrap();
        assert_eq!(buf.as_str(), "type Size (small, medium, large)");
    }
}
