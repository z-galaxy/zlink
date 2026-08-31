//! Validating names against the Varlink grammar.
//!
//! These run the actual IDL parsers, so a name is accepted here exactly when the parser would
//! accept it in an interface definition -- there is no second copy of the grammar to drift from it.

#[cfg(test)]
mod tests {
    use crate::{is_valid_field_name, is_valid_interface_name, is_valid_type_name};

    #[test]
    fn field_names() {
        let valid = ["a", "Z", "user_name", "user0", "a_0_b", "type"];
        for name in valid {
            assert!(is_valid_field_name(name), "`{name}` must be valid");
        }

        let invalid = [
            "",
            "_foo",
            "0foo",
            "user-name",
            "user name",
            "user.name",
            "user!",
            "üser",
            "user\u{e9}",
            "a_",
            "x__",
            "a__b",
        ];
        for name in invalid {
            assert!(!is_valid_field_name(name), "`{name}` must be invalid");
        }
    }

    #[test]
    fn type_names() {
        let valid = ["A", "Foo", "FileNotFound", "Foo0", "X"];
        for name in valid {
            assert!(is_valid_type_name(name), "`{name}` must be valid");
        }

        let invalid = [
            "", "foo", "_Foo", "0Foo", "Foo_bar", "Foo-bar", "Foo bar", "Foo.Bar", "Foo!", "Über",
        ];
        for name in invalid {
            assert!(!is_valid_type_name(name), "`{name}` must be invalid");
        }
    }

    /// Type names are the stricter rule: every one of them is also a valid field name.
    #[test]
    fn type_names_are_field_names() {
        for name in ["A", "Foo", "FileNotFound", "Foo0"] {
            assert!(is_valid_field_name(name), "`{name}` must be valid");
        }
    }

    #[test]
    fn interface_names() {
        let valid = ["org.example.Foo", "a.b", "org.varlink.service", "a1.b-2.c3"];
        for name in valid {
            assert!(is_valid_interface_name(name), "`{name}` must be valid");
        }

        let invalid = [
            "",
            "Foo",          // No dot.
            "org.",         // Trailing dot, empty segment.
            ".org",         // Leading dot.
            "org..example", // Empty segment.
            "org.example.", // Trailing dot.
            "1org.example", // First segment must start with a letter.
            "org.example.Foo bar",
            "org.example.Foö",
        ];
        for name in invalid {
            assert!(!is_valid_interface_name(name), "`{name}` must be invalid");
        }
    }
}
