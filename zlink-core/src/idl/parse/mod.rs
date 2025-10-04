//! Parsers for Varlink IDL using winnow.
//!
//! This module provides parsers for converting IDL strings into the corresponding
//! Rust types defined in the parent module. Uses byte-based parsing to avoid UTF-8 overhead.

use winnow::{
    ascii::multispace0,
    combinator::{alt, separated},
    error::{ErrMode, InputError, ParserError},
    token::{literal, take_while},
    ModalResult, Parser,
};

use super::{
    Comment, CustomEnum, CustomObject, CustomType, EnumVariant, Error, Field, Interface, List,
    Method, Parameter, Type, TypeRef,
};

use alloc::{format, vec::Vec};

/// Parse whitespace and comments according to Varlink grammar.
/// The `_` production in Varlink grammar: whitespace / comment / eol_r
fn ws<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    loop {
        let start_len = input.len();

        // Consume regular whitespace (spaces, tabs, etc.)
        multispace0::<_, InputError<&'a [u8]>>
            .parse_next(input)
            .ok();

        // Try to consume a comment: "#" [^\n\r\u{2028}\u{2029}]* eol_r
        if input.starts_with(b"#") {
            // Skip the '#'
            *input = &input[1..];

            // Consume everything until end of line
            while !input.is_empty() {
                match input[0] {
                    b'\n' | b'\r' => {
                        // Consume the end-of-line character(s)
                        if input.starts_with(b"\r\n") {
                            *input = &input[2..];
                        } else {
                            *input = &input[1..];
                        }
                        break;
                    }
                    _ => {
                        *input = &input[1..];
                    }
                }
            }
        }

        // If we didn't consume anything in this iteration, break
        if input.len() == start_len {
            break;
        }
    }
    Ok(())
}

/// Parse only whitespace (not comments) - used in interface parsing where comments are members.
fn whitespace_only<'a>(input: &mut &'a [u8]) -> ModalResult<(), InputError<&'a [u8]>> {
    multispace0::<_, InputError<&'a [u8]>>
        .parse_next(input)
        .ok();
    Ok(())
}

/// Convert bytes to str with input lifetime.
fn bytes_to_str(bytes: &[u8]) -> &str {
    // SAFETY: We only accept ASCII characters in our parsers
    core::str::from_utf8(bytes).unwrap()
}

/// Parse a field name: starts with letter, continues with alphanumeric and underscores.
fn field_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    let start = *input;
    let mut pos = 0;

    // First character must be alphabetic
    if pos >= input.len() || !input[pos].is_ascii_alphabetic() {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }
    pos += 1;

    // Continue with alphanumeric and underscores
    while pos < input.len() && (input[pos].is_ascii_alphanumeric() || input[pos] == b'_') {
        pos += 1;
    }

    let name_bytes = &start[0..pos];
    *input = &input[pos..];
    Ok(bytes_to_str(name_bytes))
}

/// Parse a type name: starts with uppercase letter, continues with alphanumeric.
fn type_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    let start = *input;
    if input.is_empty() || !input[0].is_ascii_uppercase() {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }

    let mut end = 1;
    while end < input.len() && input[end].is_ascii_alphanumeric() {
        end += 1;
    }

    let name_bytes = &start[0..end];
    *input = &input[end..];
    Ok(bytes_to_str(name_bytes))
}

/// Parse a primitive type.
fn primitive_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((
        literal("bool").map(|_| Type::Bool),
        literal("int").map(|_| Type::Int),
        literal("float").map(|_| Type::Float),
        literal("string").map(|_| Type::String),
        literal("object").map(|_| Type::ForeignObject),
    ))
    .parse_next(input)
}

/// Parse a field in a struct or parameter list.
fn field<'a>(input: &mut &'a [u8]) -> ModalResult<Field<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    let name = field_name(input)?;
    ws(input)?;
    literal(":").parse_next(input)?;
    ws(input)?;
    let ty = varlink_type(input)?;
    Ok(Field::new_owned(name, ty, comments))
}

/// Parse an inline struct type: (field1: type1, field2: type2).
fn struct_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    literal("(").parse_next(input)?;
    ws(input)?;
    let fields: Vec<Field<'a>> = separated(0.., field, (ws, literal(","), ws)).parse_next(input)?;
    ws(input)?;
    literal(")").parse_next(input)?;
    Ok(Type::Object(List::from(fields)))
}

/// Parse an inline enum type: (variant1, variant2, variant3).
fn enum_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    literal("(").parse_next(input)?;
    ws(input)?;
    let variant_names: Vec<&str> =
        separated(0.., field_name, (ws, literal(","), ws)).parse_next(input)?;
    ws(input)?;
    literal(")").parse_next(input)?;

    let variants: Vec<EnumVariant<'a>> = variant_names
        .into_iter()
        .map(|name| EnumVariant::new(name, &[]))
        .collect();
    Ok(Type::Enum(List::from(variants)))
}

/// Parse an inline type (struct or enum).
/// Determines if it's a struct by looking for ':' character.
fn inline_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    // Look ahead to see if this contains a colon (indicating struct)
    if let Some(pos) = input.iter().position(|&b| b == b')') {
        let content = &input[1..pos]; // Skip opening paren
        if content.contains(&b':') {
            struct_type(input)
        } else {
            enum_type(input)
        }
    } else {
        Err(ErrMode::Backtrack(ParserError::from_input(input)))
    }
}

/// Parse an element type (primitive, custom, or inline).
fn element_type<'a>(input: &mut &'a [u8]) -> ModalResult<Type<'a>, InputError<&'a [u8]>> {
    alt((primitive_type, type_name.map(Type::Custom), inline_type)).parse_next(input)
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

/// Parse an interface name: reverse domain notation like org.example.test.
fn interface_name<'a>(input: &mut &'a [u8]) -> ModalResult<&'a str, InputError<&'a [u8]>> {
    let start = *input;
    let mut pos = 0;

    // First segment: [A-Za-z]([-]*[A-Za-z0-9])*
    if pos >= input.len() || !input[pos].is_ascii_alphabetic() {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }
    pos += 1;

    while pos < input.len() && (input[pos].is_ascii_alphanumeric() || input[pos] == b'-') {
        pos += 1;
    }

    let mut found_dot = false;
    // Subsequent segments: .[A-Za-z0-9]([-]*[A-Za-z0-9])*
    while pos < input.len() && input[pos] == b'.' {
        found_dot = true;
        pos += 1; // skip dot

        // Must have at least one alphanumeric after dot
        if pos >= input.len() || !input[pos].is_ascii_alphanumeric() {
            break;
        }
        pos += 1;

        // Continue with alphanumeric and dashes
        while pos < input.len() && (input[pos].is_ascii_alphanumeric() || input[pos] == b'-') {
            pos += 1;
        }
    }

    // Check for at least one dot
    if !found_dot {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }

    let name_bytes = &start[0..pos];
    *input = &input[pos..];
    Ok(bytes_to_str(name_bytes))
}

/// Parse a parameter list: (param1: type1, param2: type2).
fn parameter_list<'a>(
    input: &mut &'a [u8],
) -> ModalResult<Vec<Parameter<'a>>, InputError<&'a [u8]>> {
    literal("(").parse_next(input)?;
    whitespace_only(input)?;

    let mut params = Vec::new();

    // Handle empty parameter list
    if literal::<_, _, InputError<&'a [u8]>>(")")
        .parse_next(input)
        .is_ok()
    {
        return Ok(params);
    }

    // Parse first parameter with any preceding comments
    loop {
        // Parse any preceding comments for this parameter
        let comments = parse_preceding_comments(input)?;

        // Parse the parameter itself (field name and type)
        let name = field_name(input)?;
        ws(input)?;
        literal(":").parse_next(input)?;
        ws(input)?;
        let ty = varlink_type(input)?;

        params.push(Parameter::new_owned(name, ty, comments));

        whitespace_only(input)?;

        // Check for comma (more parameters) or closing paren (end)
        if literal::<_, _, InputError<&'a [u8]>>(",")
            .parse_next(input)
            .is_ok()
        {
            whitespace_only(input)?;
            // Continue to next parameter
        } else if literal::<_, _, InputError<&'a [u8]>>(")")
            .parse_next(input)
            .is_ok()
        {
            // End of parameter list
            break;
        } else {
            return Err(ErrMode::Backtrack(ParserError::from_input(input)));
        }
    }

    Ok(params)
}

/// Parse a method definition: method Name(inputs) -> (outputs).
fn method_def<'a>(input: &mut &'a [u8]) -> ModalResult<Method<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("method").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = type_name(input)?;
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
fn error_def<'a>(input: &mut &'a [u8]) -> ModalResult<Error<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("error").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = type_name(input)?;
    ws(input)?;
    let params = parameter_list(input)?;

    Ok(Error::new_owned(name, params, comments))
}

/// Parse a type definition: type Name <definition>.
fn type_def<'a>(input: &mut &'a [u8]) -> ModalResult<CustomType<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("type").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = type_name(input)?;
    ws(input)?;
    literal("(").parse_next(input)?;
    whitespace_only(input)?;

    let mut fields = Vec::new();
    let mut variants: Vec<EnumVariant<'a>> = Vec::new();
    let mut has_typed_fields = false;
    let mut has_untyped_fields = false;

    // Handle empty field list
    if literal::<_, _, InputError<&'a [u8]>>(")")
        .parse_next(input)
        .is_ok()
    {
        return Ok(CustomType::from(CustomObject::new_owned(
            name, fields, comments,
        )));
    }

    // Parse fields with any preceding comments
    loop {
        // Parse any preceding comments for this field
        let field_comments = parse_preceding_comments(input)?;

        // Parse the field itself
        let field_name = field_name(input)?;
        whitespace_only(input)?;

        // Try to parse the colon and type
        if literal::<_, _, InputError<&'a [u8]>>(":")
            .parse_next(input)
            .is_ok()
        {
            whitespace_only(input)?;
            let ty = varlink_type(input)?;
            fields.push(Field::new_owned(field_name, ty, field_comments));
            has_typed_fields = true;
        } else {
            // This is an enum-like field without type - collect as variant with comments
            variants.push(EnumVariant::new_owned(field_name, field_comments));
            has_untyped_fields = true;
        }

        whitespace_only(input)?;

        // Check for comma (more fields) or closing paren (end)
        if literal::<_, _, InputError<&'a [u8]>>(",")
            .parse_next(input)
            .is_ok()
        {
            whitespace_only(input)?;
            // Continue to next field
        } else if literal::<_, _, InputError<&'a [u8]>>(")")
            .parse_next(input)
            .is_ok()
        {
            // End of field list
            break;
        } else {
            return Err(ErrMode::Backtrack(ParserError::from_input(input)));
        }
    }

    // Error if we have both typed and untyped fields (mixed custom type)
    if has_typed_fields && has_untyped_fields {
        return Err(ErrMode::Backtrack(ParserError::from_input(input)));
    }

    // Decide whether to create an enum or object based on whether we saw typed fields
    if has_typed_fields {
        Ok(CustomType::from(CustomObject::new_owned(
            name, fields, comments,
        )))
    } else {
        // All fields were untyped, so this is an enum
        Ok(CustomType::from(CustomEnum::new_owned(
            name, variants, comments,
        )))
    }
}

/// Parse a member definition (type, method, or error).
/// Helper function to parse any preceding comments.
fn parse_preceding_comments<'a>(
    input: &mut &'a [u8],
) -> ModalResult<Vec<Comment<'a>>, InputError<&'a [u8]>> {
    let mut comments = Vec::new();
    while !input.is_empty() {
        let checkpoint = *input;
        whitespace_only(input)?;

        if input.is_empty() {
            break;
        }

        if let Ok(comment) = comment_def(input) {
            comments.push(comment);
            whitespace_only(input)?;
        } else {
            // Not a comment, restore position
            *input = checkpoint;
            break;
        }
    }
    Ok(comments)
}

fn comment_def<'a>(input: &mut &'a [u8]) -> ModalResult<Comment<'a>, InputError<&'a [u8]>> {
    literal("#").parse_next(input)?;

    // Skip all leading whitespace after #
    while !input.is_empty() && (input[0] == b' ' || input[0] == b'\t') {
        *input = &input[1..];
    }

    // Take until newline or end of input - this is the actual comment content
    let line_content = take_while(0.., |c: u8| c != b'\n').parse_next(input)?;
    let comment_text = bytes_to_str(line_content);

    Ok(Comment::new(comment_text))
}

/// Parse an interface definition.
fn interface_def<'a>(input: &mut &'a [u8]) -> ModalResult<Interface<'a>, InputError<&'a [u8]>> {
    let comments = parse_preceding_comments(input)?;

    literal("interface").parse_next(input)?;
    take_while(1.., |c: u8| c.is_ascii_whitespace()).parse_next(input)?;
    let name = interface_name(input)?;
    whitespace_only(input)?;

    // Parse members separated by whitespace/newlines
    let mut methods = Vec::new();
    let mut custom_types = Vec::new();
    let mut errors = Vec::new();

    while !input.is_empty() {
        whitespace_only(input)?;

        if input.is_empty() {
            break;
        }

        enum ParsedMember<'a> {
            Custom(CustomType<'a>),
            Method(Method<'a>),
            Error(Error<'a>),
        }

        let result = alt((
            type_def.map(ParsedMember::Custom),
            method_def.map(ParsedMember::Method),
            error_def.map(ParsedMember::Error),
        ))
        .parse_next(input);

        match result {
            Ok(ParsedMember::Custom(custom_type)) => custom_types.push(custom_type),
            Ok(ParsedMember::Method(method)) => methods.push(method),
            Ok(ParsedMember::Error(error)) => errors.push(error),
            Err(_) => break,
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

/// Parse an interface from a string.
pub(super) fn parse_interface(input: &str) -> Result<Interface<'_>, crate::Error> {
    parse_from_str(input, interface_def)
}

/// Helper function to parse from string using byte-based parsers.
fn parse_from_str<'a, T>(
    input: &'a str,
    parser: impl Fn(&mut &'a [u8]) -> ModalResult<T, InputError<&'a [u8]>>,
) -> Result<T, crate::Error> {
    use alloc::string::ToString;

    let input_bytes = input.trim().as_bytes();
    if input_bytes.is_empty() {
        return Err(crate::Error::IdlParse("Input is empty".to_string()));
    }

    let mut input_mut = input_bytes;
    match parser(&mut input_mut) {
        Ok(result) => {
            let _ = ws(&mut input_mut);
            if input_mut.is_empty() {
                Ok(result)
            } else {
                Err(crate::Error::IdlParse(format!(
                    "Unexpected remaining input: {:?}",
                    core::str::from_utf8(input_mut).map_or("<invalid UTF-8>", |s| s)
                )))
            }
        }
        Err(err) => Err(crate::Error::IdlParse(format!("Parse error: {err}"))),
    }
}

#[cfg(test)]
mod tests;
