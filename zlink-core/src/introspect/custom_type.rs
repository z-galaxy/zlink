/// Custom type introspection.
///
/// This trait is similar to [`crate::introspect::Type`] but provides custom type definitions
/// that include the type name and are suitable for IDL generation. While
/// [`crate::introspect::Type`] returns inline type information, this trait returns named custom
/// type definitions.
///
/// # Examples
///
/// ```rust
/// use zlink_core::introspect::CustomType;
/// use zlink_core::idl::{self, Field, CustomObject};
///
/// struct Point {
///     x: f64,
///     y: f64,
/// }
///
/// impl CustomType for Point {
///     const CUSTOM_TYPE: &'static idl::CustomType<'static> = &{
///         static FIELD_X: Field<'static> = Field::new("x", &zlink_core::idl::Type::Float, &[]);
///         static FIELD_Y: Field<'static> = Field::new("y", &zlink_core::idl::Type::Float, &[]);
///         static FIELDS: &[&Field<'static>] = &[&FIELD_X, &FIELD_Y];
///
///         idl::CustomType::Object(CustomObject::new("Point", FIELDS, &[]))
///     };
/// }
/// ```
pub trait CustomType {
    /// The custom type information including the type name.
    const CUSTOM_TYPE: &'static crate::idl::CustomType<'static>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        idl::{self, CustomEnum, CustomObject, EnumVariant, Field},
        introspect,
    };

    // Test implementation for a custom struct type
    struct TestPoint;

    impl CustomType for TestPoint {
        const CUSTOM_TYPE: &'static idl::CustomType<'static> = &{
            static FIELD_X: Field<'static> = Field::new("x", <f64 as introspect::Type>::TYPE, &[]);
            static FIELD_Y: Field<'static> = Field::new("y", <f64 as introspect::Type>::TYPE, &[]);
            static FIELDS: &[&Field<'static>] = &[&FIELD_X, &FIELD_Y];

            idl::CustomType::Object(CustomObject::new("Point", FIELDS, &[]))
        };
    }

    // Test implementation for a custom enum type
    struct TestStatus;

    impl CustomType for TestStatus {
        const CUSTOM_TYPE: &'static idl::CustomType<'static> = &{
            static VARIANT_ACTIVE: EnumVariant<'static> = EnumVariant::new("Active", &[]);
            static VARIANT_INACTIVE: EnumVariant<'static> = EnumVariant::new("Inactive", &[]);
            static VARIANT_PENDING: EnumVariant<'static> = EnumVariant::new("Pending", &[]);
            static VARIANTS: &[&'static EnumVariant<'static>] =
                &[&VARIANT_ACTIVE, &VARIANT_INACTIVE, &VARIANT_PENDING];

            idl::CustomType::Enum(CustomEnum::new("Status", VARIANTS, &[]))
        };
    }

    #[test]
    fn custom_struct_type() {
        match TestPoint::CUSTOM_TYPE {
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
    }

    #[test]
    fn custom_enum_type() {
        match TestStatus::CUSTOM_TYPE {
            idl::CustomType::Enum(enm) => {
                assert_eq!(enm.name(), "Status");

                let variants: Vec<_> = enm.variants().collect();
                assert_eq!(variants.len(), 3);

                assert_eq!(variants[0].name(), "Active");
                assert_eq!(variants[1].name(), "Inactive");
                assert_eq!(variants[2].name(), "Pending");
            }
            _ => panic!("Expected custom enum type"),
        }
    }

    #[test]
    fn const_compatibility() {
        // Verify that TYPE can be used in const contexts
        const _POINT_TYPE: &idl::CustomType<'static> = TestPoint::CUSTOM_TYPE;
        const _STATUS_TYPE: &idl::CustomType<'static> = TestStatus::CUSTOM_TYPE;
    }

    #[test]
    fn type_name_access() {
        assert_eq!(TestPoint::CUSTOM_TYPE.name(), "Point");
        assert_eq!(TestStatus::CUSTOM_TYPE.name(), "Status");
    }

    #[test]
    fn type_variant_checking() {
        assert!(TestPoint::CUSTOM_TYPE.is_object());
        assert!(!TestPoint::CUSTOM_TYPE.is_enum());

        assert!(!TestStatus::CUSTOM_TYPE.is_object());
        assert!(TestStatus::CUSTOM_TYPE.is_enum());
    }
}
