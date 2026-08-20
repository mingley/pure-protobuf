use std::borrow::Cow;
use std::fmt;

/// An error that happened during parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub(crate) message: Cow<'static, str>,
}

impl ParseError {
    pub fn new(message: &'static str) -> Self {
        Self {
            message: Cow::Borrowed(message),
        }
    }

    pub fn owned(message: String) -> Self {
        Self {
            message: Cow::Owned(message),
        }
    }
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
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
