//! Enum type definition for Varlink IDL.

use core::fmt;

use alloc::vec::Vec;

use super::{EnumVariant, List};

/// An enum type definition in Varlink IDL (enum-like with named variants).
#[derive(Debug, Clone, Eq)]
pub struct CustomEnum<'a> {
    /// The name of the enum type.
    name: &'a str,
    /// The variants of the enum type.
    variants: List<'a, EnumVariant<'a>>,
    /// The comments associated with this enum type.
    comments: List<'a, super::Comment<'a>>,
}

impl<'a> CustomEnum<'a> {
    /// Creates a new enum type with the given name, borrowed variants, and comments.
    pub const fn new(
        name: &'a str,
        variants: &'a [&'a EnumVariant<'a>],
        comments: &'a [&'a super::Comment<'a>],
    ) -> Self {
        Self {
            name,
            variants: List::Borrowed(variants),
            comments: List::Borrowed(comments),
        }
    }

    /// Creates a new enum type with the given name, owned variants, and comments.
    pub fn new_owned(
        name: &'a str,
        variants: Vec<EnumVariant<'a>>,
        comments: Vec<super::Comment<'a>>,
    ) -> Self {
        Self {
            name,
            variants: List::from(variants),
            comments: List::from(comments),
        }
    }

    /// Returns the name of the enum type.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns an iterator over the variants of the enum type.
    pub fn variants(&self) -> impl Iterator<Item = &EnumVariant<'a>> {
        self.variants.iter()
    }

    /// Returns an iterator over the comments associated with this enum type.
    pub fn comments(&self) -> impl Iterator<Item = &super::Comment<'a>> {
        self.comments.iter()
    }
}

impl<'a> fmt::Display for CustomEnum<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Comments first
        for comment in self.comments.iter() {
            writeln!(f, "{comment}")?;
        }

        // Check if any variant has comments to determine formatting
        let has_variant_comments = self.variants.iter().any(|v| v.has_comments());

        if has_variant_comments {
            // Multi-line format when any variant has comments
            writeln!(f, "type {} (", self.name)?;
            for variant in self.variants.iter() {
                // Write comments first
                for comment in variant.comments() {
                    writeln!(f, "\t{}", comment)?;
                }
                // Then write the variant name
                writeln!(f, "\t{}", variant.name())?;
            }
            write!(f, ")")
        } else {
            // Single-line format when no variants have comments
            write!(f, "type {} (", self.name)?;
            let mut first = true;
            for variant in self.variants.iter() {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                write!(f, "{}", variant)?;
            }
            write!(f, ")")
        }
    }
}

impl<'a> PartialEq for CustomEnum<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.variants == other.variants
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;
    use crate::idl::{Comment, EnumVariant};
    use core::fmt::Write;

    #[test]
    fn display_with_comments() {
        let comment1 = Comment::new("Status enumeration");
        let comment2 = Comment::new("Represents current state");
        let comments = [&comment1, &comment2];

        let var1 = EnumVariant::new("active", &[]);
        let var2 = EnumVariant::new("inactive", &[]);
        let var3 = EnumVariant::new("pending", &[]);
        let variants = [&var1, &var2, &var3];

        let custom_enum = CustomEnum::new("Status", &variants, &comments);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", custom_enum).unwrap();
        assert_eq!(
            displayed,
            "# Status enumeration\n# Represents current state\ntype Status (active, inactive, pending)"
        );
    }

    #[test]
    fn display_with_variant_comments() {
        let var_comment = Comment::new("The active state");
        let var1 = EnumVariant::new_owned("active", vec![var_comment]);
        let var2 = EnumVariant::new_owned("inactive", vec![]);
        let custom_enum = CustomEnum::new_owned("Status", vec![var1, var2], vec![]);

        let mut displayed = String::new();
        write!(&mut displayed, "{}", custom_enum).unwrap();
        assert_eq!(
            displayed,
            "type Status (\n\t# The active state\n\tactive\n\tinactive\n)"
        );
    }

    #[test_log::test]
    fn comprehensive_enum_with_per_variant_comments() {
        // Test enum-level comments plus per-variant comments
        let enum_comment = Comment::new("Status enumeration with detailed docs");

        let active_comment = Comment::new("System is operational");
        let inactive_comment = Comment::new("System is stopped");
        let pending_comment = Comment::new("System is starting up");

        let var1 = EnumVariant::new_owned("active", vec![active_comment]);
        let var2 = EnumVariant::new_owned("inactive", vec![inactive_comment]);
        let var3 = EnumVariant::new_owned("pending", vec![pending_comment]);

        let custom_enum =
            CustomEnum::new_owned("SystemStatus", vec![var1, var2, var3], vec![enum_comment]);

        // Test that we can access all the information
        assert_eq!(custom_enum.name(), "SystemStatus");
        assert_eq!(custom_enum.variants().count(), 3);
        assert_eq!(custom_enum.comments().count(), 1);

        // Test display output includes both enum and variant comments
        let mut displayed = String::new();
        write!(&mut displayed, "{}", custom_enum).unwrap();

        // Should contain enum comment
        assert!(displayed.contains("Status enumeration with detailed docs"));
        // Should contain variant comments on separate lines
        assert!(displayed.contains("# System is operational\n\tactive"));
        assert!(displayed.contains("# System is stopped\n\tinactive"));
        assert!(displayed.contains("# System is starting up\n\tpending"));

        debug!("✓ Comprehensive enum display: {}", displayed);
    }

    #[test]
    fn formatting_with_and_without_comments() {
        // Test single-line format when no variants have comments
        let var1 = EnumVariant::new("red", &[]);
        let var2 = EnumVariant::new("green", &[]);
        let var3 = EnumVariant::new("blue", &[]);
        let variants_no_comments = [&var1, &var2, &var3];

        let enum_no_comments = CustomEnum::new("Color", &variants_no_comments, &[]);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", enum_no_comments).unwrap();
        assert_eq!(displayed, "type Color (red, green, blue)");

        // Test multi-line format when any variant has comments
        let comment = Comment::new("Primary color");
        let comment_refs = [&comment];
        let var_with_comment = EnumVariant::new("red", &comment_refs);
        let var_without_comment1 = EnumVariant::new("green", &[]);
        let var_without_comment2 = EnumVariant::new("blue", &[]);
        let variants_with_comments = [
            &var_with_comment,
            &var_without_comment1,
            &var_without_comment2,
        ];

        let enum_with_comments = CustomEnum::new("Color", &variants_with_comments, &[]);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", enum_with_comments).unwrap();
        assert_eq!(
            displayed,
            "type Color (\n\t# Primary color\n\tred\n\tgreen\n\tblue\n)"
        );
    }
}
