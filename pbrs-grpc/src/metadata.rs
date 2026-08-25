//! ASCII and `-bin` gRPC metadata.

use crate::status::Status;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use http::{HeaderMap, HeaderName, HeaderValue};

/// gRPC metadata (headers or trailers). Binary keys must end in `-bin`.
#[derive(Clone, Debug, Default)]
pub struct Metadata {
    ascii: Vec<(String, String)>,
    bin: Vec<(String, Vec<u8>)>,
}

impl Metadata {
    /// Empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.ascii.is_empty() && self.bin.is_empty()
    }

    /// Insert an ASCII value. Key must not end in `-bin`.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), Status> {
        let key = key.into();
        if key.ends_with("-bin") {
            return Err(Status::invalid_argument(
                "ascii metadata key must not end in -bin",
            ));
        }
        self.ascii.push((key, value.into()));
        Ok(())
    }

    /// Insert a binary value. Key must end in `-bin`.
    pub fn insert_bin(
        &mut self,
        key: impl Into<String>,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), Status> {
        let key = key.into();
        if !key.ends_with("-bin") {
            return Err(Status::invalid_argument(
                "binary metadata key must end in -bin",
            ));
        }
        self.bin.push((key, value.into()));
        Ok(())
    }

    /// ASCII value for `key`, if present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.ascii
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    /// Binary value for `key`, if present.
    #[must_use]
    pub fn get_bin(&self, key: &str) -> Option<&[u8]> {
        self.bin
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_slice())
    }

    pub(crate) fn write_to(&self, headers: &mut HeaderMap) -> Result<(), Status> {
        for (k, v) in &self.ascii {
            let name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| Status::internal(e.to_string()))?;
            let value = HeaderValue::from_str(v).map_err(|e| Status::internal(e.to_string()))?;
            headers.append(name, value);
        }
        for (k, v) in &self.bin {
            let name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|e| Status::internal(e.to_string()))?;
            let encoded = STANDARD_NO_PAD.encode(v);
            let value =
                HeaderValue::from_str(&encoded).map_err(|e| Status::internal(e.to_string()))?;
            headers.append(name, value);
        }
        Ok(())
    }

    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        let mut md = Self::new();
        for (name, value) in headers {
            let key = name.as_str();
            if is_reserved(key) {
                continue;
            }
            let Ok(raw) = value.to_str() else {
                continue;
            };
            if key.ends_with("-bin") {
                if let Ok(bytes) = STANDARD_NO_PAD.decode(raw) {
                    md.bin.push((key.to_string(), bytes));
                }
            } else {
                md.ascii.push((key.to_string(), raw.to_string()));
            }
        }
        md
    }
}

fn is_reserved(key: &str) -> bool {
    key.starts_with(':')
        || key.eq_ignore_ascii_case("content-type")
        || key.eq_ignore_ascii_case("te")
        || key.eq_ignore_ascii_case("grpc-status")
        || key.eq_ignore_ascii_case("grpc-message")
        || key.eq_ignore_ascii_case("grpc-timeout")
        || key.eq_ignore_ascii_case("grpc-encoding")
        || key.eq_ignore_ascii_case("grpc-accept-encoding")
}
