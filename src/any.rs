//! Typed conversion for the shared well-known `Any` message.

use crate::{Name, ParseError, SerializeError, wkt};

impl wkt::Any {
    /// Serialize a named message with its configured type URL.
    ///
    /// Generated messages implement [`Name`] when code generation enables
    /// `enable_type_names`. The shared well-known messages already implement it.
    ///
    /// ```
    /// use pbrs::wkt::{Any, Timestamp};
    /// let mut timestamp = Timestamp::new();
    /// timestamp.set_seconds(42);
    /// let any = Any::from_msg(&timestamp)?;
    /// assert_eq!(any.to_msg::<Timestamp>()?, timestamp);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_msg<M: Name>(message: &M) -> Result<Self, SerializeError> {
        let value = message.serialize()?;
        let mut any = Self::new();
        any.set_type_url(M::type_url());
        any.set_value(value);
        Ok(any)
    }

    /// Parse a named message after checking its type URL.
    ///
    /// The full name after the last slash must match; the URL prefix may differ.
    /// Missing slashes, leading dots, invalid UTF-8, mismatched names and invalid
    /// payloads return a parse error, following Prost's type-URL matching rules.
    pub fn to_msg<M: Name>(&self) -> Result<M, ParseError> {
        let expected = M::type_url();
        let actual = self
            .type_url()
            .to_str()
            .map_err(|_| ParseError::new("invalid Any type URL"))?;
        if let (Some(expected), Some(actual)) = (type_name(&expected), type_name(actual)) {
            if expected == actual {
                return M::parse(self.value());
            }
        }
        Err(ParseError::new("unexpected Any type URL"))
    }
}

fn type_name(url: &str) -> Option<&str> {
    let (_, name) = url.rsplit_once('/')?;
    (!name.starts_with('.')).then_some(name)
}

macro_rules! well_known_names {
    ($($message:ident => $name:literal),+ $(,)?) => {
        $(impl Name for wkt::$message {
            const NAME: &'static str = $name;
            const PACKAGE: &'static str = "google.protobuf";

            fn type_url() -> String {
                concat!("type.googleapis.com/google.protobuf.", $name).to_owned()
            }
        })+
    };
}

well_known_names! {
    Any => "Any",
    BoolValue => "BoolValue",
    BytesValue => "BytesValue",
    DoubleValue => "DoubleValue",
    Duration => "Duration",
    Empty => "Empty",
    FieldMask => "FieldMask",
    FloatValue => "FloatValue",
    Int32Value => "Int32Value",
    Int64Value => "Int64Value",
    ListValue => "ListValue",
    PbValue => "Value",
    StringValue => "StringValue",
    Struct => "Struct",
    Timestamp => "Timestamp",
    UInt32Value => "UInt32Value",
    UInt64Value => "UInt64Value",
}
