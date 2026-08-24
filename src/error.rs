use std::fmt;

/// An error that happened during parsing.
///
/// Official rust_out tests match this as a unit struct (`matches_pattern!(&ParseError)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParseError;

impl ParseError {
    pub fn new(_message: &'static str) -> Self {
        Self
    }

    pub fn owned(_message: String) -> Self {
        Self
    }
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("parse error")
    }
}

/// An error that happened during serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializeError {
    pub(crate) message: &'static str,
}

impl SerializeError {
    pub fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl std::error::Error for SerializeError {}

impl fmt::Display for SerializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}
