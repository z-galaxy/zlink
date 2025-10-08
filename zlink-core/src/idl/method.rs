//! Method definitions for Varlink IDL.

use core::fmt;

use alloc::vec::Vec;

use super::{Comment, List, Parameter};

/// A method definition in Varlink IDL.
#[derive(Debug, Clone, Eq)]
pub struct Method<'a> {
    /// The name of the method.
    name: &'a str,
    /// Input parameters for the method.
    inputs: List<'a, Parameter<'a>>,
    /// Output parameters for the method.
    outputs: List<'a, Parameter<'a>>,
    /// Comments associated with this method.
    comments: List<'a, Comment<'a>>,
}

impl<'a> Method<'a> {
    /// Creates a new method with the given name, borrowed parameters, and comments.
    pub const fn new(
        name: &'a str,
        inputs: &'a [&'a Parameter<'a>],
        outputs: &'a [&'a Parameter<'a>],
        comments: &'a [&'a Comment<'a>],
    ) -> Self {
        Self {
            name,
            inputs: List::Borrowed(inputs),
            outputs: List::Borrowed(outputs),
            comments: List::Borrowed(comments),
        }
    }

    /// Creates a new method with the given name, owned parameters, and comments.
    pub fn new_owned(
        name: &'a str,
        inputs: Vec<Parameter<'a>>,
        outputs: Vec<Parameter<'a>>,
        comments: Vec<Comment<'a>>,
    ) -> Self {
        Self {
            name,
            inputs: List::from(inputs),
            outputs: List::from(outputs),
            comments: List::from(comments),
        }
    }

    /// Returns the name of the method.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns an iterator over the input parameters.
    pub fn inputs(&self) -> impl Iterator<Item = &Parameter<'a>> {
        self.inputs.iter()
    }

    /// Returns an iterator over the output parameters.
    pub fn outputs(&self) -> impl Iterator<Item = &Parameter<'a>> {
        self.outputs.iter()
    }

    /// Returns true if the method has no input parameters.
    pub fn has_no_inputs(&self) -> bool {
        self.inputs.is_empty()
    }

    /// Returns true if the method has no output parameters.
    pub fn has_no_outputs(&self) -> bool {
        self.outputs.is_empty()
    }

    /// Returns the comments associated with this method.
    pub fn comments(&self) -> impl Iterator<Item = &Comment<'a>> {
        self.comments.iter()
    }
}

impl<'a> fmt::Display for Method<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Comments first
        for comment in self.comments.iter() {
            writeln!(f, "{comment}")?;
        }
        write!(f, "method {}(", self.name)?;
        let mut first = true;
        for param in self.inputs.iter() {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            write!(f, "{param}")?;
        }
        write!(f, ")")?;

        write!(f, " -> (")?;
        let mut first = true;
        for param in self.outputs.iter() {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            write!(f, "{param}")?;
        }
        write!(f, ")")?;

        Ok(())
    }
}

impl PartialEq for Method<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.inputs == other.inputs && self.outputs == other.outputs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idl::{Parameter, Type};

    #[test]
    fn method_creation() {
        let input = Parameter::new("id", &Type::Int, &[]);
        let output = Parameter::new("name", &Type::String, &[]);
        let inputs = [&input];
        let outputs = [&output];

        let method = Method::new("GetName", &inputs, &outputs, &[]);
        assert_eq!(method.name(), "GetName");
        assert_eq!(method.inputs().count(), 1);
        assert_eq!(method.outputs().count(), 1);
        assert!(!method.has_no_inputs());
        assert!(!method.has_no_outputs());

        // Check the parameters individually - order and values.
        let inputs_vec: Vec<_> = method.inputs().collect();
        assert_eq!(inputs_vec[0].name(), "id");
        assert_eq!(inputs_vec[0].ty(), &Type::Int);

        let outputs_vec: Vec<_> = method.outputs().collect();
        assert_eq!(outputs_vec[0].name(), "name");
        assert_eq!(outputs_vec[0].ty(), &Type::String);
    }

    #[test]
    fn method_no_params() {
        let method = Method::new("Ping", &[], &[], &[]);
        assert_eq!(method.name(), "Ping");
        assert!(method.has_no_inputs());
        assert!(method.has_no_outputs());
    }

    #[test]
    fn method_display_with_no_outputs() {
        use core::fmt::Write;

        let method = Method::new("Ping", &[], &[], &[]);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", method).unwrap();
        assert_eq!(displayed, "method Ping() -> ()");

        let name_param = Parameter::new("name", &Type::String, &[]);
        let id_param = Parameter::new("id", &Type::String, &[]);
        let params = [&name_param, &id_param];
        let method = Method::new("Register", &params, &[], &[]);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", method).unwrap();
        assert_eq!(displayed, "method Register(name: string, id: string) -> ()");
    }

    #[test]
    fn display_with_comments() {
        use crate::idl::Comment;
        use core::fmt::Write;

        let comment1 = Comment::new("Get user information");
        let comment2 = Comment::new("Returns user details by ID");
        let comments = [&comment1, &comment2];

        let input = Parameter::new("id", &Type::Int, &[]);
        let output = Parameter::new("user", &Type::Custom("User"), &[]);
        let inputs = [&input];
        let outputs = [&output];

        let method = Method::new("GetUser", &inputs, &outputs, &comments);
        let mut displayed = String::new();
        write!(&mut displayed, "{}", method).unwrap();
        assert_eq!(displayed, "# Get user information\n# Returns user details by ID\nmethod GetUser(id: int) -> (user: User)");
    }
}
