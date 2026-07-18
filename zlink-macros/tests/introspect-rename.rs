#![cfg(feature = "introspection")]

use zlink::{
    idl,
    introspect::{CustomType, Type},
};

#[test]
fn struct_field_rename_all() {
    let idl::Type::Object(fields) = Membership::TYPE else {
        panic!("expected an object type");
    };
    let fields: Vec<_> = fields.iter().collect();

    assert_eq!(fields[0].name(), "userName");
    assert_eq!(fields[1].name(), "groupName");
}

#[test]
fn explicit_rename_overrides_rename_all() {
    let idl::Type::Object(fields) = Overridden::TYPE else {
        panic!("expected an object type");
    };
    let fields: Vec<_> = fields.iter().collect();

    assert_eq!(fields[0].name(), "ID", "explicit rename must win");
    assert_eq!(fields[1].name(), "groupName", "rename_all still applies");
}

#[test]
fn enum_variant_rename_all() {
    let idl::Type::Enum(variants) = Status::TYPE else {
        panic!("expected an enum type");
    };
    let variants: Vec<_> = variants.iter().collect();

    assert_eq!(variants[0].name(), "active");
    assert_eq!(variants[1].name(), "notReady");
    assert_eq!(variants[2].name(), "GONE", "explicit rename must win");
}

#[test]
fn custom_type_container_rename() {
    let idl::CustomType::Object(obj) = Record::CUSTOM_TYPE else {
        panic!("expected an object custom type");
    };

    assert_eq!(obj.name(), "Membership");

    let fields: Vec<_> = obj.fields().collect();
    assert_eq!(fields[0].name(), "userName");
}

#[test]
fn custom_type_rename_keeps_type_reference_in_sync() {
    // The `Type` impl must point at the same name the `CustomType` impl declares, or the IDL
    // reference dangles.
    assert_eq!(Record::TYPE, &idl::Type::Custom("Membership"));
}

#[test]
fn custom_enum_container_rename() {
    let idl::CustomType::Enum(e) = Level::CUSTOM_TYPE else {
        panic!("expected an enum custom type");
    };

    assert_eq!(e.name(), "Severity");

    let variants: Vec<_> = e.variants().collect();
    assert_eq!(variants[0].name(), "low");
}

#[test]
fn custom_type_raw_ident_container_name_is_unraw_d() {
    let idl::CustomType::Object(obj) = Foo::CUSTOM_TYPE else {
        panic!("expected an object custom type");
    };

    assert_eq!(obj.name(), "Foo", "must not be r#Foo");
}

#[test]
fn raw_ident_custom_type_keeps_type_reference_in_sync() {
    // Same sync property as `custom_type_rename_keeps_type_reference_in_sync` above, but for the
    // no-explicit-rename path: the container name comes straight from the (unraw'd) Rust ident.
    assert_eq!(Foo::TYPE, &idl::Type::Custom("Foo"));
}

#[test]
fn raw_ident_field_name_is_unraw_d() {
    // `r#type` must not panic (the static ident derived from it must not carry `r#`), and the
    // IDL name must not carry `r#` either: `r#type` is not valid Varlink.
    let idl::Type::Object(fields) = RawField::TYPE else {
        panic!("expected an object type");
    };
    let fields: Vec<_> = fields.iter().collect();

    assert_eq!(fields[0].name(), "type");
}

#[test]
fn raw_ident_field_rename_all_applies_to_the_unraw_d_name() {
    let idl::Type::Object(fields) = RawFieldUppercased::TYPE else {
        panic!("expected an object type");
    };
    let fields: Vec<_> = fields.iter().collect();

    assert_eq!(fields[0].name(), "TYPE", "must not be R#TYPE");
}

#[test]
fn raw_ident_field_explicit_rename_wins() {
    // This asserted value ("kind") is produced by `resolve` whether or not the ident is unraw'd
    // first, so it does not by itself pin the fix. What this test actually guards is that the
    // `RawFieldRenamed` fixture below compiles at all: the static field ident is derived from the
    // (unraw'd) Rust ident `r#type`, not from the resolved "kind" name. Reverting the fix makes
    // this fail to compile with `` `"FIELD_R#TYPE"` is not a valid identifier ``.
    let idl::Type::Object(fields) = RawFieldRenamed::TYPE else {
        panic!("expected an object type");
    };
    let fields: Vec<_> = fields.iter().collect();

    assert_eq!(fields[0].name(), "kind");
}

#[test]
fn raw_ident_variant_name_is_unraw_d() {
    let idl::Type::Enum(variants) = RawVariant::TYPE else {
        panic!("expected an enum type");
    };
    let variants: Vec<_> = variants.iter().collect();

    assert_eq!(variants[0].name(), "fn");
}

// `#[allow(unused)]` on test types matches the convention already used throughout
// `tests/introspect-type.rs`: the fields exist only to be described, never read.

#[derive(Type)]
#[allow(unused)]
#[zlink(rename_all = "camelCase")]
struct Membership {
    user_name: String,
    group_name: String,
}

#[derive(Type)]
#[allow(unused)]
#[zlink(rename_all = "camelCase")]
struct Overridden {
    #[zlink(rename = "ID")]
    user_id: String,
    group_name: String,
}

#[derive(Type)]
#[allow(unused)]
#[zlink(rename_all = "camelCase")]
enum Status {
    Active,
    NotReady,
    #[zlink(rename = "GONE")]
    Gone,
}

#[derive(CustomType)]
#[allow(unused)]
#[zlink(rename = "Membership", rename_all = "camelCase")]
struct Record {
    user_name: String,
}

#[derive(CustomType)]
#[allow(unused)]
#[zlink(rename = "Severity", rename_all = "lowercase")]
enum Level {
    Low,
    High,
}

#[derive(Type)]
#[allow(unused)]
struct RawField {
    r#type: String,
}

#[derive(Type)]
#[allow(unused)]
#[zlink(rename_all = "UPPERCASE")]
struct RawFieldUppercased {
    r#type: String,
}

#[derive(Type)]
#[allow(unused)]
struct RawFieldRenamed {
    #[zlink(rename = "kind")]
    r#type: String,
}

#[derive(Type)]
#[allow(unused, non_camel_case_types)]
enum RawVariant {
    r#fn,
}

#[derive(CustomType)]
#[allow(unused)]
struct r#Foo {
    value: String,
}
