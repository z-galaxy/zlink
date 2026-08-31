/// A type that can be validated or parsed using a fixed regular expression.
pub trait FromPattern {
    /// The regular expression matching valid values of this type.
    fn from_pattern() -> &'static str;
}
