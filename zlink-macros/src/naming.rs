use crate::utils::skip_unknown_meta;
use std::fmt::Debug;
use syn::{Attribute, Error, Ident, LitStr};
use zlink_names::{
    FieldName, FromPattern, OwnedFieldName, OwnedInterfaceName, OwnedTypeName, TypeName,
};

/// Where a resolved name came from, which decides what a rejection points at and suggests.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NameSource<'a> {
    /// An explicit `#[zlink(rename = "...")]`.
    Rename(&'a LitStr),
    /// A Rust ident, possibly with a case convention applied to it.
    Ident(&'a Ident),
}

/// Reject `name` if Varlink cannot express it under `grammar`, describing it as `what`.
///
/// Only ever called on a *resolved* name: unrawing and `rename`/`rename_all` decide what a thing is
/// called, and this decides whether that answer is sayable. Checking any earlier would reject
/// perfectly good Rust, e.g. `r#type`, which resolves to the valid `type`.
pub(crate) fn validate<'a, T>(name: &'a str, what: &str, source: NameSource<'_>) -> Result<T, Error>
where
    T: TryFrom<&'a str>,
    T: FromPattern,
    <T as TryFrom<&'a str>>::Error: Debug,
{
    let validation_result = T::try_from(name);
    if validation_result.is_err() {
        // Pointing at the rename literal is precise enough on its own; suggesting a rename to
        // someone who just wrote one would only be noise.
        let hint = match source {
            NameSource::Rename(_) => "",
            NameSource::Ident(_) => ", so name it explicitly with `#[zlink(rename = \"...\")]`",
        };
        let msg = format!(
            "`{name}` is not a valid Varlink {what}: it must match `{}`{hint}",
            T::from_pattern(),
        );

        return Err(match source {
            NameSource::Rename(lit) => Error::new_spanned(lit, msg),
            NameSource::Ident(ident) => Error::new_spanned(ident, msg),
        });
    }

    Ok(validation_result.unwrap())
}

/// Reject an interface name Varlink cannot express, reporting against `lit`'s span.
///
/// Kept apart from [`validate`]: an interface name is always a literal the user wrote out (never a
/// resolved ident), and its grammar is neither the field nor the type rule but reverse-domain
/// notation.
pub(crate) fn validate_interface(lit: &LitStr) -> Result<OwnedInterfaceName, Error> {
    let name = lit.value();
    let valid_name = OwnedInterfaceName::try_from(name.clone());
    if let Err(_err) = valid_name {
        return Err(Error::new_spanned(
            lit,
            format!(
                "`{name}` is not a valid Varlink interface name: it must be in reverse-domain \
             notation, e.g. `org.example.Foo`"
            ),
        ));
    }

    Ok(valid_name.unwrap())
}

/// The case convention requested by `#[zlink(rename_all = "...")]`.
///
/// The variants mirror serde's rename rules so the semantics are familiar. Note that the field and
/// variant rules differ, because the source convention differs: fields are `snake_case`, variants
/// are `PascalCase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenameAll {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameAll {
    /// Apply the convention to a struct field name, whose source convention is `snake_case`.
    pub(crate) fn apply_to_field(self, field: &str) -> String {
        match self {
            Self::Lower | Self::Snake => field.to_owned(),
            Self::Upper | Self::ScreamingSnake => field.to_ascii_uppercase(),
            Self::Pascal => {
                let mut pascal = String::new();
                let mut capitalize = true;
                for ch in field.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        pascal.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        pascal.push(ch);
                    }
                }
                pascal
            }
            Self::Camel => {
                let pascal = Self::Pascal.apply_to_field(field);
                match pascal.get(..1) {
                    Some(first) => first.to_ascii_lowercase() + &pascal[1..],
                    None => pascal,
                }
            }
            Self::Kebab => field.replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake.apply_to_field(field).replace('_', "-"),
        }
    }

    /// Apply the convention to an enum variant name, whose source convention is `PascalCase`.
    pub(crate) fn apply_to_variant(self, variant: &str) -> String {
        match self {
            Self::Pascal => variant.to_owned(),
            Self::Lower => variant.to_ascii_lowercase(),
            Self::Upper => variant.to_ascii_uppercase(),
            Self::Camel => match variant.get(..1) {
                Some(first) => first.to_ascii_lowercase() + &variant[1..],
                None => variant.to_owned(),
            },
            Self::Snake => {
                let mut snake = String::new();
                for (i, ch) in variant.char_indices() {
                    if i > 0 && ch.is_uppercase() {
                        snake.push('_');
                    }
                    snake.push(ch.to_ascii_lowercase());
                }
                snake
            }
            Self::ScreamingSnake => Self::Snake.apply_to_variant(variant).to_ascii_uppercase(),
            Self::Kebab => Self::Snake.apply_to_variant(variant).replace('_', "-"),
            Self::ScreamingKebab => Self::ScreamingSnake
                .apply_to_variant(variant)
                .replace('_', "-"),
        }
    }

    pub(crate) fn parse(lit: &LitStr) -> Result<Self, Error> {
        let rule = match lit.value().as_str() {
            "lowercase" => Self::Lower,
            "UPPERCASE" => Self::Upper,
            "PascalCase" => Self::Pascal,
            "camelCase" => Self::Camel,
            "snake_case" => Self::Snake,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::Kebab,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            unknown => {
                return Err(Error::new_spanned(
                    lit,
                    format!(
                        "`{unknown}` is not a known case convention, expected one of: {}",
                        VALID_RENAME_ALL.join(", "),
                    ),
                ));
            }
        };

        Ok(rule)
    }
}

/// The name a struct field should carry in the IDL and on the wire.
///
/// `#[zlink(rename)]` wins over an inherited `rename_all`, which wins over the Rust ident.
pub(crate) fn field_name(
    attrs: &[Attribute],
    ident: &Ident,
    rename_all: Option<RenameAll>,
) -> Result<OwnedFieldName, Error> {
    resolve(
        attrs,
        ident,
        rename_all,
        RenameAll::apply_to_field,
        "field name",
    )
}

/// The name an enum variant should carry in the IDL and on the wire.
///
/// `#[zlink(rename)]` wins over an inherited `rename_all`, which wins over the Rust ident.
#[cfg(feature = "introspection")]
pub(crate) fn enum_variant_name(
    attrs: &[Attribute],
    ident: &Ident,
    rename_all: Option<RenameAll>,
) -> Result<OwnedFieldName, Error> {
    resolve(
        attrs,
        ident,
        rename_all,
        RenameAll::apply_to_variant,
        "enum variant name",
    )
}

/// The name an error variant should carry in the IDL and on the wire.
///
/// `#[zlink(rename)]` wins over an inherited `rename_all`, which wins over the Rust ident.
///
/// Kept apart from [`enum_variant_name`] because the two obey different grammars: an error is named
/// like a type, an enum variant like a field. Sharing one function would leave the choice of rule
/// to whichever caller happened to reach for it.
pub(crate) fn error_name(
    attrs: &[Attribute],
    ident: &Ident,
    rename_all: Option<RenameAll>,
) -> Result<OwnedTypeName, Error> {
    resolve(
        attrs,
        ident,
        rename_all,
        RenameAll::apply_to_variant,
        "error name",
    )
}

/// Reject a container-level `#[zlink(rename)]` with `msg`, for derives where it means nothing.
///
/// Silently ignoring it would let users ship IDL that does not say what they wrote.
pub(crate) fn reject_container_rename(attrs: &[Attribute], msg: &str) -> Result<(), Error> {
    match parse_rename(attrs)? {
        Some(lit) => Err(Error::new_spanned(lit, msg)),
        None => Ok(()),
    }
}

/// The container's `#[zlink(rename_all = "...")]`, if any, checked against a grammar picked by
/// whichever terminal method is called.
///
/// Only two grammars ever apply -- field/variant names and type names -- so rather than a generic
/// parameter callers would have to turbofish (it never appears in the return type, so it can never
/// be inferred), the grammar is hardcoded by which terminal method is called.
pub(crate) struct RenameAllParser<'a> {
    attrs: &'a [Attribute],
}

impl<'a> RenameAllParser<'a> {
    pub(crate) fn new(attrs: &'a [Attribute]) -> Self {
        Self { attrs }
    }

    /// Checked against the field/variant grammar (`snake_case`-sourced, lowercase-first).
    pub(crate) fn try_for_field_name(self) -> Result<Option<RenameAll>, Error> {
        self.parse(
            |rule| FieldName::try_from(rule.apply_to_variant("Word").as_str()).is_ok(),
            FieldName::from_pattern(),
        )
    }

    /// Checked against the type grammar (`PascalCase`-sourced, uppercase-first).
    pub(crate) fn try_for_type_name(self) -> Result<Option<RenameAll>, Error> {
        self.parse(
            |rule| TypeName::try_from(rule.apply_to_variant("Word").as_str()).is_ok(),
            TypeName::from_pattern(),
        )
    }

    /// A convention that could never produce a name valid under `pattern` -- `snake_case` where a
    /// type name is wanted, say -- is rejected here, against the attribute's own span, rather than
    /// left for the per-name check to reject one produced name at a time.
    ///
    /// Probing a single-word sample rather than re-encoding that reasoning keeps the parser the one
    /// authority on the grammar: a word carries the first-character and character-set rules without
    /// the separators that only longer names introduce, so if a convention cannot make even this
    /// fit, no name ever will.
    fn parse(
        self,
        can_produce: impl Fn(RenameAll) -> bool,
        pattern: &'static str,
    ) -> Result<Option<RenameAll>, Error> {
        let Some(lit) = parse_zlink_lit_str(self.attrs, "rename_all")? else {
            return Ok(None);
        };

        let rule = RenameAll::parse(&lit)?;
        // Whether this convention could ever produce a name valid for the used parser.
        //
        // Some conventions can satisfy a grammar for no input at all: `snake_case`, say, always
        // lowercases the first character, which the type-name rule (uppercase-first) rejects for
        // every possible name. Offering such a pairing is a footgun, so it is refused up front
        // rather than left for the per-name check to reject one produced name at a time.
        //
        // Probing a single-word sample rather than re-encoding that reasoning keeps the parser the
        // one authority on the grammar: a word carries the first-character and
        // character-set rules without the separators that only longer names introduce, so
        // if a convention cannot make even this fit, no name ever will.
        if !can_produce(rule) {
            return Err(Error::new_spanned(
                &lit,
                format!(
                    "`{}` can never produce a name matching the Varlink grammar `{pattern}`, so it \
                     cannot apply here. Drop `rename_all`, use a convention that fits (e.g. \
                     `UPPERCASE` or `PascalCase`), or name items individually with \
                     `#[zlink(rename = \"...\")]`",
                    lit.value(),
                ),
            ));
        }

        Ok(Some(rule))
    }
}

/// The item's `#[zlink(rename = "...")]`, if any.
pub(crate) fn parse_rename(attrs: &[Attribute]) -> Result<Option<LitStr>, Error> {
    parse_zlink_lit_str(attrs, "rename")
}

/// The unraw'd name of an ident: its leading `r#`, if any, stripped off.
///
/// `r#` (e.g. `r#type`) is Rust syntax that lets a keyword be used as an identifier; it is never
/// part of the name itself, so it must not leak into the IDL or onto the wire.
pub(crate) fn unraw(ident: &Ident) -> String {
    let name = ident.to_string();

    match name.strip_prefix("r#") {
        Some(stripped) => stripped.to_owned(),
        None => name,
    }
}

fn resolve<'a, T, F>(
    attrs: &[Attribute],
    ident: &Ident,
    rename_all: Option<RenameAll>,
    apply: F,
    what: &'a str,
) -> Result<T, Error>
where
    T: for<'x> TryFrom<&'x str>,
    T: FromPattern,
    F: FnOnce(RenameAll, &str) -> String,
    for<'x> <T as TryFrom<&'x str>>::Error: Debug,
{
    // An explicit rename is a name the user wrote out, so it is taken verbatim -- mangling it would
    // defeat the point of the attribute -- but it still has to be a name Varlink can say.
    if let Some(lit) = parse_rename(attrs)? {
        let name = lit.value();
        return validate(&name, what, NameSource::Rename(&lit));
    }

    let unrawed = unraw(ident);
    let name = match rename_all {
        Some(rule) => apply(rule, &unrawed),
        None => unrawed,
    };
    validate(&name, what, NameSource::Ident(ident))
}

/// The string value of `#[zlink(<key> = "...")]`, if present.
///
/// Parse errors are reported rather than swallowed, which is what the rename attributes need in
/// order to reject bad input.
fn parse_zlink_lit_str(attrs: &[Attribute], key: &str) -> Result<Option<LitStr>, Error> {
    let mut result = None;

    for attr in attrs {
        if !attr.path().is_ident("zlink") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident(key) {
                let lit: LitStr = meta.value()?.parse()?;
                if result.is_some() {
                    return Err(meta.error(format!("duplicate `{key}` attribute")));
                }
                result = Some(lit);
            } else {
                skip_unknown_meta(&meta)?;
            }

            Ok(())
        })?;
    }

    Ok(result)
}

const VALID_RENAME_ALL: &[&str] = &[
    "lowercase",
    "UPPERCASE",
    "PascalCase",
    "camelCase",
    "snake_case",
    "SCREAMING_SNAKE_CASE",
    "kebab-case",
    "SCREAMING-KEBAB-CASE",
];

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn field_conventions() {
        let cases = [
            (RenameAll::Lower, "user_name"),
            (RenameAll::Upper, "USER_NAME"),
            (RenameAll::Pascal, "UserName"),
            (RenameAll::Camel, "userName"),
            (RenameAll::Snake, "user_name"),
            (RenameAll::ScreamingSnake, "USER_NAME"),
            (RenameAll::Kebab, "user-name"),
            (RenameAll::ScreamingKebab, "USER-NAME"),
        ];

        for (rule, expected) in cases {
            assert_eq!(rule.apply_to_field("user_name"), expected, "rule: {rule:?}");
        }
    }

    #[test]
    fn variant_conventions() {
        let cases = [
            (RenameAll::Lower, "username"),
            (RenameAll::Upper, "USERNAME"),
            (RenameAll::Pascal, "UserName"),
            (RenameAll::Camel, "userName"),
            (RenameAll::Snake, "user_name"),
            (RenameAll::ScreamingSnake, "USER_NAME"),
            (RenameAll::Kebab, "user-name"),
            (RenameAll::ScreamingKebab, "USER-NAME"),
        ];

        for (rule, expected) in cases {
            assert_eq!(
                rule.apply_to_variant("UserName"),
                expected,
                "rule: {rule:?}"
            );
        }
    }

    #[test]
    fn rename_beats_rename_all() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[zlink(rename = "ID")])];
        let ident: Ident = parse_quote!(user_id);
        let name = field_name(&attrs, &ident, Some(RenameAll::Camel)).unwrap();

        assert_eq!(name.as_ref().as_str(), "ID");
    }

    #[test]
    fn rename_all_applies_without_rename() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(user_id);
        let name = field_name(&attrs, &ident, Some(RenameAll::Camel)).unwrap();

        assert_eq!(name.as_ref().as_str(), "userId");
    }

    #[test]
    fn ident_used_without_any_attr() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(user_id);

        assert_eq!(
            field_name(&attrs, &ident, None).unwrap().as_ref().as_str(),
            "user_id"
        );
    }

    #[test]
    fn unraw_strips_the_prefix() {
        let ident: Ident = parse_quote!(r#type);

        assert_eq!(unraw(&ident), "type");
    }

    #[test]
    fn unraw_leaves_a_normal_ident_unchanged() {
        let ident: Ident = parse_quote!(user_id);

        assert_eq!(unraw(&ident), "user_id");
    }

    #[test]
    fn rename_all_parsed_alongside_other_keys() {
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[zlink(crate = "crate", rename_all = "camelCase")])];

        assert_eq!(
            RenameAllParser::new(&attrs).try_for_field_name().unwrap(),
            Some(RenameAll::Camel)
        );
    }

    #[test]
    fn unknown_rename_all_value_rejected() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[zlink(rename_all = "bogus")])];
        let err = RenameAllParser::new(&attrs)
            .try_for_field_name()
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("bogus"),
            "message should name the bad value: {err}"
        );
        assert!(
            err.contains("camelCase"),
            "message should list valid values: {err}"
        );
    }

    #[test]
    fn container_rename_rejected_with_message() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[zlink(rename = "Foo")])];
        let err = reject_container_rename(&attrs, "nope")
            .unwrap_err()
            .to_string();

        assert_eq!(err, "nope");
    }

    /// A raw-ident param unraws to a valid field name and must survive validation: this pins the
    /// order (unraw, then validate). Validating `r#type` verbatim would reject legitimate Rust.
    #[test]
    fn raw_ident_field_resolves_then_validates() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(r#type);

        assert_eq!(
            field_name(&attrs, &ident, None).unwrap().as_ref().as_str(),
            "type"
        );
    }

    /// The error-name counterpart: `r#Fn` unraws to the valid error name `Fn`.
    #[test]
    fn raw_ident_error_variant_resolves_then_validates() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(r#Fn);

        assert_eq!(
            error_name(&attrs, &ident, None).unwrap().as_ref().as_str(),
            "Fn"
        );
    }

    /// Enum variants follow field rules, which admit a lowercase first letter.
    #[cfg(feature = "introspection")]
    #[test]
    fn enum_variant_lowercase_is_accepted() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(Active);

        assert_eq!(
            enum_variant_name(&attrs, &ident, Some(RenameAll::Lower))
                .unwrap()
                .as_ref()
                .as_str(),
            "active"
        );
    }

    /// The very same lowercase name is rejected as an error name, which follows type rules.
    #[test]
    fn error_name_lowercase_is_rejected() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(Active);
        let err = error_name(&attrs, &ident, Some(RenameAll::Lower))
            .unwrap_err()
            .to_string();

        assert!(err.contains("`active`"), "must name the bad name: {err}");
        assert!(err.contains("error name"), "must name the context: {err}");
    }

    /// A bad `rename` is reported against its own value, not the ident it overrode, and offers no
    /// "use rename" hint to someone who already wrote one. This is how the error lands on the
    /// literal's span.
    #[test]
    fn invalid_rename_reports_the_literal_not_the_ident() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[zlink(rename = "not valid!")])];
        let ident: Ident = parse_quote!(good_field);
        let err = field_name(&attrs, &ident, None).unwrap_err().to_string();

        assert!(err.contains("`not valid!`"), "must quote the rename: {err}");
        assert!(
            !err.contains("good_field"),
            "must not blame the ident: {err}"
        );
        assert!(
            !err.contains("`#[zlink(rename"),
            "no hint on a rename: {err}"
        );
    }

    /// A bad ident, by contrast, is pointed at the `rename` escape hatch.
    #[test]
    fn invalid_ident_suggests_rename() {
        let attrs: Vec<Attribute> = vec![];
        let ident: Ident = parse_quote!(_foo);
        let err = field_name(&attrs, &ident, None).unwrap_err().to_string();

        assert!(err.contains("`_foo`"), "must name the bad ident: {err}");
        assert!(err.contains("rename"), "must offer the escape hatch: {err}");
    }

    #[test]
    fn interface_names_validated() {
        let good: LitStr = parse_quote!("org.example.Foo");
        assert!(validate_interface(&good).is_ok());

        // A `_` is not allowed in an interface segment (unlike a field name).
        let bad: LitStr = parse_quote!("org.example.bad_name");
        let err = validate_interface(&bad).unwrap_err().to_string();

        assert!(
            err.contains("`org.example.bad_name`"),
            "must quote it: {err}"
        );
        assert!(
            err.contains("interface name"),
            "must name the context: {err}"
        );
    }

    /// A convention that can sometimes yield a valid type name -- `SCREAMING_SNAKE_CASE` does for a
    /// single-word variant -- is accepted; the per-name check catches the names that do not fit.
    #[test]
    fn rename_all_that_can_sometimes_fit_the_type_grammar_is_accepted() {
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[zlink(rename_all = "SCREAMING_SNAKE_CASE")])];

        assert_eq!(
            RenameAllParser::new(&attrs).try_for_type_name().unwrap(),
            Some(RenameAll::ScreamingSnake)
        );
    }

    /// A convention that can never yield a valid type name -- `snake_case` always lowercases the
    /// first letter -- is refused up front, against the attribute's own span.
    #[test]
    fn rename_all_that_never_fits_the_type_grammar_is_rejected() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[zlink(rename_all = "snake_case")])];
        let err = RenameAllParser::new(&attrs)
            .try_for_type_name()
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("`snake_case`"),
            "must name the convention: {err}"
        );
        assert!(
            err.contains("[A-Z][A-Za-z0-9]*"),
            "must name the grammar it cannot meet: {err}"
        );
    }

    /// The field grammar admits a lowercase first letter, so every convention can fit it -- even
    /// the ones the type grammar refuses.
    #[test]
    fn every_rename_all_fits_the_field_grammar() {
        for value in VALID_RENAME_ALL {
            let attr: Attribute = parse_quote!(#[zlink(rename_all = #value)]);
            assert!(
                RenameAllParser::new(&[attr])
                    .try_for_field_name()
                    .unwrap()
                    .is_some(),
                "`{value}` should be accepted for a field name"
            );
        }
    }
}
