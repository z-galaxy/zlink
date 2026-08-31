//! Parsing of Varlink IDL.
//!
//! An [`Interface`] is parsed from a string through its [`TryFrom`] implementation, which
//! returns an [`Error`] on failure.

use winnow::{
    ModalResult, Parser,
    ascii::{multispace0, multispace1},
    combinator::{alt, delimited, opt, preceded, repeat, separated},
    error::{ErrMode, InputError, ParserError},
    token::{literal, take_while},
};

mod error;
pub use error::Error;
use zlink_names::{InterfaceName, parse_field_name, parse_interface_name, parse_type_name};

use super::{
    Comment, CustomEnum, CustomObject, CustomType, EnumVariant, Field, Interface, List, Method,
    Parameter, Type, TypeRef,
};

use alloc::{string::ToString, vec::Vec};

/// Parse a `# ... eol` comment as inter-token whitespace, discarding its
/// content. Used by [`ws`].
fn comment_as_ws<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    (
        literal("#"),
        take_while(0.., |c: u8| c != b'\n' && c != b'\r'),
        opt(alt((literal("\r\n"), literal("\n"), literal("\r")))),
    )
        .void()
        .parse_next(input)
}

/// Parse whitespace and comments according to Varlink grammar.
/// The `_` production in Varlink grammar: whitespace / comment / eol_r.
fn ws<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    repeat(0.., alt((multispace1.void(), comment_as_ws))).parse_next(input)
}

/// Parse only whitespace (not comments) - used in interface parsing where comments are members.
fn whitespace_only<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    multispace0.void().parse_next(input)
}

/// Convert bytes to str with input lifetime.
fn bytes_to_str(bytes: &[u8]) -> &str {
    // SAFETY: We only accept ASCII characters in our parsers
    core::str::from_utf8(bytes).unwrap()
}

/// Parse a primitive type.
fn primitive_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((
        literal("bool").map(|_| Type::Bool),
        literal("int").map(|_| Type::Int),
        literal("float").map(|_| Type::Float),
        literal("string").map(|_| Type::String),
        literal("object").map(|_| Type::ForeignObject),
        literal("any").map(|_| Type::Any),
    ))
    .parse_next(input)
}

/// Parse a field in a struct or parameter list.
fn field<'a>(input: &mut &'a [u8]) -> ModalResult<Field<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    let name = parse_field_name(input)?;
    ws(input)?;
    literal(":").parse_next(input)?;
    ws(input)?;
    let ty = varlink_type(input)?;
    Ok(Field::new_owned(name, ty, comments))
}

/// Separator between fields/variants of a comma-separated list.
///
/// `ws` before the comma consumes any whitespace and trailing comments
/// that follow the previous item (e.g. `x: bool # trailing`). After the
/// comma, `whitespace_only` consumes only whitespace, leaving any comments
/// in the input for the next item's `parse_preceding_comments` to attach.
fn field_separator<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    (ws, literal(","), whitespace_only).void().parse_next(input)
}

/// Parse an inline struct type: (field1: type1, field2: type2).
fn struct_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    delimited(
        // Leading whitespace is consumed here so empty/whitespace-only structs
        // parse without being mistaken for a field.
        (literal("("), whitespace_only),
        separated(0.., field, field_separator),
        // `ws` (not `whitespace_only`) so a final comment before `)` (e.g.
        // `(# comment\n)`) is consumed.
        (ws, literal(")")),
    )
    .map(|fields: Vec<Field<'a>>| Type::Object(List::from(fields)))
    .parse_next(input)
}

/// Parse an enum variant: any preceding comments followed by the variant name.
fn enum_variant<'a>(input: &mut &'a [u8]) -> ModalResult<EnumVariant<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;
    let name = parse_field_name(input)?;
    Ok(EnumVariant::new_owned(name, comments))
}

/// Parse an inline enum type: (variant1, variant2, variant3).
fn enum_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    delimited(
        (literal("("), whitespace_only),
        separated(0.., enum_variant, field_separator),
        (ws, literal(")")),
    )
    .map(|variants: Vec<EnumVariant<'a>>| Type::Enum(List::from(variants)))
    .parse_next(input)
}

/// Parse an inline type (struct or enum).
///
/// `struct_type` is tried first: it accepts `()` (and any whitespace- or
/// comment-only body) as an empty struct, per the Varlink grammar
/// (`struct = "(" struct_fields ")" | "(" ")"` — an empty enum cannot be
/// instantiated). Content with `:` matches `struct_type`; bare-name
/// content falls through to `enum_type` via `alt`'s backtracking.
fn inline_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((struct_type, enum_type)).parse_next(input)
}

/// Parse an element type (primitive, custom, or inline).
fn element_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((
        primitive_type,
        parse_type_name.map(Type::Custom),
        inline_type,
    ))
    .parse_next(input)
}

/// Parse an optional type: ?type.
fn optional_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    literal("?").parse_next(input)?;
    let inner = non_optional_type(input)?;
    Ok(Type::Optional(TypeRef::new_owned(inner)))
}

/// Parse any type except optional (to avoid recursion).
fn non_optional_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((array_type, map_type, element_type)).parse_next(input)
}

/// Parse an array type: []type.
fn array_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    literal("[]").parse_next(input)?;
    let inner = varlink_type(input)?;
    Ok(Type::Array(TypeRef::new_owned(inner)))
}

/// Parse a map type: [string]type.
fn map_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    literal("[string]").parse_next(input)?;
    let inner = varlink_type(input)?;
    Ok(Type::Map(TypeRef::new_owned(inner)))
}

/// Parse any Varlink type.
fn varlink_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((optional_type, array_type, map_type, element_type)).parse_next(input)
}

/// Parse a parameter list: (param1: type1, param2: type2).
fn parameter_list<'a>(
    input: &mut &'a [u8],
) -> ModalResult<Vec<Parameter<'a>>, InputError<&'a [u8]>> {
    delimited(
        (literal("("), whitespace_only),
        separated(0.., field, field_separator),
        (ws, literal(")")),
    )
    .parse_next(input)
}

/// Parse a method definition: method Name(inputs) -> (outputs).
fn method_def<'a>(input: &mut &'a [u8]) -> ModalResult<Method<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("method").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = parse_type_name(input)?;
    ws(input)?;
    let input_params = parameter_list(input)?;
    ws(input)?;
    literal("->").parse_next(input)?;
    ws(input)?;
    let output_params = parameter_list(input)?;

    Ok(Method::new_owned(
        name,
        input_params,
        output_params,
        comments,
    ))
}

/// Parse an error definition: error Name (fields).
fn error_def<'a>(input: &mut &'a [u8]) -> ModalResult<super::Error<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("error").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = parse_type_name(input)?;
    ws(input)?;
    let params = parameter_list(input)?;

    Ok(super::Error::new_owned(name, params, comments))
}

/// A typed field or an untyped enum variant. Custom types may be either a
/// struct (all items typed) or an enum (all items untyped); used inside
/// `type_def` to parse both forms uniformly and decide afterwards.
enum FieldOrVariant<'a> {
    Field(Field<'a>),
    Variant(EnumVariant<'a>),
}

/// Parse a single member of a `type Name (...)` body — either `name: type`
/// (a struct field) or `name` (an enum variant).
fn field_or_variant<'a>(
    input: &mut &'a [u8],
) -> ModalResult<FieldOrVariant<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;
    let name = parse_field_name(input)?;
    let ty = opt(preceded((ws, literal(":"), ws), varlink_type)).parse_next(input)?;
    Ok(match ty {
        Some(ty) => FieldOrVariant::Field(Field::new_owned(name, ty, comments)),
        None => FieldOrVariant::Variant(EnumVariant::new_owned(name, comments)),
    })
}

/// Parse a type definition: type Name <definition>.
fn type_def<'a>(input: &mut &'a [u8]) -> ModalResult<CustomType<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("type").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = parse_type_name(input)?;
    ws(input)?;

    let items: Vec<FieldOrVariant<'a>> = delimited(
        (literal("("), whitespace_only),
        separated(0.., field_or_variant, field_separator),
        (ws, literal(")")),
    )
    .parse_next(input)?;

    let mut fields = Vec::new();
    let mut variants = Vec::new();
    for item in items {
        match item {
            FieldOrVariant::Field(f) => fields.push(f),
            FieldOrVariant::Variant(v) => variants.push(v),
        }
    }

    // A custom type cannot mix typed fields and untyped variants.
    if !fields.is_empty() && !variants.is_empty() {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }

    // An empty body is a struct per the Varlink grammar; otherwise, untyped
    // members make an enum and typed members make a struct.
    if !variants.is_empty() {
        Ok(CustomType::from(CustomEnum::new_owned(
            name, variants, comments,
        )))
    } else {
        Ok(CustomType::from(CustomObject::new_owned(
            name, fields, comments,
        )))
    }
}

/// Parse zero or more comments that precede a definition. Whitespace before,
/// between, and after the comments is consumed; if no comment is found, the
/// input is left untouched.
fn parse_preceding_comments<'a>(
    input: &mut &'a [u8],
) -> ModalResult<Vec<Comment<'a>>, InputError<&'a [u8]>> {
    repeat(
        0..,
        delimited(whitespace_only, comment_def, whitespace_only),
    )
    .parse_next(input)
}

/// Parse a single `# ...` comment up to (but not including) the end of line.
/// Leading spaces and tabs after `#` are stripped from the captured content.
fn comment_def<'a>(input: &mut &'a [u8]) -> ModalResult<Comment<'a>, InputError<&'a [u8]>> {
    preceded(
        (
            literal("#"),
            take_while(0.., |c: u8| c == b' ' || c == b'\t'),
        ),
        take_while(0.., |c: u8| c != b'\n'),
    )
    .map(|content| Comment::new(bytes_to_str(content)))
    .parse_next(input)
}

/// One member of an interface body, used inside `interface_def` to collect
/// the three member kinds via a single `repeat`.
enum InterfaceMember<'a> {
    Custom(CustomType<'a>),
    Method(Method<'a>),
    Error(super::Error<'a>),
}

/// Parse an interface definition.
fn interface_def<'a>(input: &mut &'a [u8]) -> Result<Interface<'a>, Error> {
    let comments = parse_preceding_comments(input).map_err(to_parse_error)?;

    literal("interface")
        .parse_next(input)
        .map_err(to_parse_error)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace())
        .parse_next(input)
        .map_err(to_parse_error)?;
    let name_str = parse_interface_name(input).map_err(to_parse_error)?;
    let name = InterfaceName::try_from(name_str).map_err(|err| Error::Parse(err.to_string()))?;

    let members: Vec<InterfaceMember<'a>> = repeat(
        0..,
        preceded(
            whitespace_only,
            alt((
                type_def.map(InterfaceMember::Custom),
                method_def.map(InterfaceMember::Method),
                error_def.map(InterfaceMember::Error),
            )),
        ),
    )
    .parse_next(input)
    .map_err(to_parse_error)?;

    let mut methods = Vec::new();
    let mut custom_types = Vec::new();
    let mut errors = Vec::new();
    for m in members {
        match m {
            InterfaceMember::Custom(c) => custom_types.push(c),
            InterfaceMember::Method(m) => methods.push(m),
            InterfaceMember::Error(e) => errors.push(e),
        }
    }

    Ok(Interface::new_owned(
        name,
        methods,
        custom_types,
        errors,
        comments,
    ))
}

fn to_parse_error(err: ErrMode<InputError<&[u8]>>) -> Error {
    Error::Parse(err.to_string())
}

/// Parse an interface from a string.
pub(super) fn parse_interface(input: &str) -> Result<Interface<'_>, Error> {
    parse_from_str(input, interface_def)
}

/// Helper function to parse from string using byte-based parsers.
fn parse_from_str<'a, T, E: core::fmt::Display>(
    input: &'a str,
    parser: impl Fn(&mut &'a [u8]) -> Result<T, E>,
) -> Result<T, Error> {
    let input_bytes = input.trim().as_bytes();
    if input_bytes.is_empty() {
        return Err(Error::Empty);
    }

    let mut input_mut = input_bytes;
    match parser(&mut input_mut) {
        Ok(result) => {
            let _ = ws(&mut input_mut);
            if input_mut.is_empty() {
                Ok(result)
            } else {
                Err(Error::TrailingInput(
                    core::str::from_utf8(input_mut)
                        .map_or("<invalid UTF-8>", |s| s)
                        .to_string(),
                ))
            }
        }
        Err(err) => Err(Error::Parse(err.to_string())),
    }
}

#[cfg(test)]
mod tests;
