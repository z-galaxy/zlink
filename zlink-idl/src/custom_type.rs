//! Type definition for Varlink IDL custom types.

use core::fmt;

use zlink_names::TypeName;

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
    pub fn name(&self) -> &TypeName<'a> {
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
    pub const fn as_object(&self) -> Option<&CustomObject<'a>> {
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
mod tests {
    use alloc::vec;

    use super::*;
    use crate::{EnumVariant, Field, Type};
    use zlink_names::FieldName;

    #[test]
    fn object_creation() {
        let field_x = Field::new("x".try_into().unwrap(), &Type::Float, &[]);
        let field_y = Field::new("y".try_into().unwrap(), &Type::Float, &[]);
        let fields = [&field_x, &field_y];

        let custom_obj = CustomObject::new("Point".try_into().unwrap(), &fields, &[]);
        assert_eq!(custom_obj.name(), "Point");
        assert_eq!(custom_obj.fields().count(), 2);

        // Check the fields individually - order and values.
        let fields = custom_obj.fields().collect::<Vec<_>>();
        assert_eq!(fields[0].name(), "x");
        assert_eq!(fields[0].ty(), &Type::Float);
        assert_eq!(fields[1].name(), "y");
        assert_eq!(fields[1].ty(), &Type::Float);
    }

    #[test]
    fn enum_creation() {
        let red = EnumVariant::new_owned("red", vec![]).unwrap();
        let green = EnumVariant::new_owned("green", vec![]).unwrap();
        let blue = EnumVariant::new_owned("blue", vec![]).unwrap();
        let custom_enum = CustomEnum::new_owned("Color", vec![red, green, blue], vec![]).unwrap();

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
        let field_x = Field::new("x".try_into().unwrap(), &Type::Float, &[]);
        let field_y = Field::new("y".try_into().unwrap(), &Type::Float, &[]);
        let fields = [&field_x, &field_y];

        let custom_obj = CustomObject::new("Point".try_into().unwrap(), &fields, &[]);
        let custom_type = CustomType::from(custom_obj);

        assert_eq!(custom_type.name(), "Point");
        assert!(custom_type.is_object());
        assert!(!custom_type.is_enum());
        assert!(custom_type.as_object().is_some());
        assert!(custom_type.as_enum().is_none());
    }

    #[test]
    fn type_from_enum() {
        let red = EnumVariant::new_owned("red", vec![]).unwrap();
        let green = EnumVariant::new_owned("green", vec![]).unwrap();
        let blue = EnumVariant::new_owned("blue", vec![]).unwrap();
        let custom_enum = CustomEnum::new_owned("Color", vec![red, green, blue], vec![]).unwrap();
        let custom_type = CustomType::from(custom_enum);

        assert_eq!(custom_type.name(), "Color");
        assert!(!custom_type.is_object());
        assert!(custom_type.is_enum());
        assert!(custom_type.as_object().is_none());
        assert!(custom_type.as_enum().is_some());
    }

    #[test]
    fn type_display_object() {
        // Test object display
        let field_x = Field::new("x".try_into().unwrap(), &Type::Int, &[]);
        let field_y = Field::new("y".try_into().unwrap(), &Type::Int, &[]);
        let fields = [&field_x, &field_y];
        let custom_obj = CustomObject::new("Point".try_into().unwrap(), &fields, &[]);
        let custom_type = CustomType::from(custom_obj);
        use core::fmt::Write;
        let mut buf = String::new();
        write!(buf, "{}", custom_type).unwrap();
        assert_eq!(buf.as_str(), "type Point (x: int, y: int)");
    }

    #[test]
    fn type_display_enum() {
        // Test enum display. `const` items can't use the validating conversions.
        const DIRECTION_VARIANTS: &[&EnumVariant<'static>] = &[
            &EnumVariant::new(FieldName::from_static_str_unchecked("north"), &[]),
            &EnumVariant::new(FieldName::from_static_str_unchecked("south"), &[]),
            &EnumVariant::new(FieldName::from_static_str_unchecked("east"), &[]),
            &EnumVariant::new(FieldName::from_static_str_unchecked("west"), &[]),
        ];
        let custom_enum = CustomEnum::new("Direction".try_into().unwrap(), DIRECTION_VARIANTS, &[]);
        let custom_type = CustomType::from(custom_enum);
        use core::fmt::Write;
        let mut buf = String::new();
        write!(buf, "{}", custom_type).unwrap();
        assert_eq!(buf.as_str(), "type Direction (north, south, east, west)");
    }

    #[test]
    fn owned_types() {
        // Test owned object
        let fields = vec![
            Field::new("name".try_into().unwrap(), &Type::String, &[]),
            Field::new("age".try_into().unwrap(), &Type::Int, &[]),
        ];
        let custom_obj = CustomObject::new_owned("Person", fields, vec![]).unwrap();
        assert_eq!(custom_obj.name(), "Person");
        use core::fmt::Write;
        let mut buf = String::new();
        write!(buf, "{}", custom_obj).unwrap();
        assert_eq!(buf.as_str(), "type Person (name: string, age: int)");

        // Test owned enum
        let custom_enum = CustomEnum::new_owned(
            "Size",
            vec![
                EnumVariant::new_owned("small", vec![]).unwrap(),
                EnumVariant::new_owned("medium", vec![]).unwrap(),
                EnumVariant::new_owned("large", vec![]).unwrap(),
            ],
            vec![],
        )
        .unwrap();
        assert_eq!(custom_enum.name(), "Size");
        let mut buf = String::new();
        write!(buf, "{}", custom_enum).unwrap();
        assert_eq!(buf.as_str(), "type Size (small, medium, large)");
    }
}
