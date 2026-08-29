//! `google.rpc.Status`, `google.protobuf.Any`, and the standard error-detail
//! messages.
//!
//! Generated from the official protos. [`Any`] knows how to pack and unpack
//! a typed message; [`crate::Status`] encodes a [`Status`] into
//! `grpc-status-details-bin`.
//!
//! [`Status`] here is `google.rpc.Status`, not [`crate::Status`]. The kernel
//! type is the one on the wire as `grpc-status` / `grpc-message`; this type
//! is the protobuf that rides in the reserved trailer.

#![allow(missing_docs, reason = "messages come from the code generator")]

mod status_pb {
    include!(concat!(env!("OUT_DIR"), "/status.rs"));
}

mod details_pb {
    include!(concat!(env!("OUT_DIR"), "/error_details.rs"));
}

pub use details_pb::{
    BadRequest, DebugInfo, Duration, ErrorInfo, Help, LocalizedMessage, PreconditionFailure,
    QuotaFailure, RequestInfo, ResourceInfo, RetryInfo,
};
pub use status_pb::{Any, Status};

/// Prefix used by [`Any::pack`]. The type name after the last `/` is what
/// [`Any::is`] and [`Any::unpack`] compare against, so a different prefix
/// still unpacks.
pub const TYPE_URL_PREFIX: &str = "type.googleapis.com/";

fn type_name_of(type_url: &str) -> &str {
    type_url.rsplit('/').next().unwrap_or(type_url)
}

fn type_url_str(any: &Any) -> &str {
    any.type_url().to_str().unwrap_or("")
}

impl Any {
    /// Pack `msg` as `type.googleapis.com/<FULL_NAME>`.
    ///
    /// ```
    /// use pbrs_grpc::pb::{Any, ErrorInfo};
    ///
    /// let mut info = ErrorInfo::new();
    /// info.set_reason("API_DISABLED");
    /// info.set_domain("example.com");
    /// let any = Any::pack(&info)?;
    /// assert!(any.is::<ErrorInfo>());
    /// let got = any.unpack::<ErrorInfo>()?;
    /// assert_eq!(got.reason().to_str().unwrap_or(""), "API_DISABLED");
    /// # Ok::<(), pbrs_grpc::Status>(())
    /// ```
    pub fn pack<M: pbrs::Serialize + pbrs::MessageName>(msg: &M) -> Result<Self, crate::Status> {
        Self::pack_with(format!("{TYPE_URL_PREFIX}{}", M::FULL_NAME), msg)
    }

    /// Pack `msg` with an explicit type URL.
    ///
    /// Use this when talking to a peer that does not use the
    /// `type.googleapis.com/` prefix. [`Self::unpack`] still matches on the
    /// type name after the last `/`.
    pub fn pack_with<M: pbrs::Serialize>(
        type_url: impl Into<String>,
        msg: &M,
    ) -> Result<Self, crate::Status> {
        let bytes = msg
            .serialize()
            .map_err(|e| crate::Status::internal(format!("serialize: {e}")))?;
        let mut any = Self::new();
        any.set_type_url(type_url.into());
        any.set_value(bytes);
        Ok(any)
    }

    /// Whether this `Any` names `M`.
    ///
    /// Compares the protobuf full name, so
    /// `type.googleapis.com/google.rpc.ErrorInfo` and
    /// `example.com/google.rpc.ErrorInfo` both match [`ErrorInfo`].
    #[must_use]
    pub fn is<M: pbrs::MessageName>(&self) -> bool {
        type_name_of(type_url_str(self)) == M::FULL_NAME
    }

    /// Decode the payload as `M`, after checking the type URL.
    ///
    /// [`crate::Code::InvalidArgument`] if the type URL names a different
    /// message; [`crate::Code::Internal`] if the bytes are not a valid `M`.
    pub fn unpack<M: pbrs::Parse + Default + pbrs::MessageName>(&self) -> Result<M, crate::Status> {
        if !self.is::<M>() {
            return Err(crate::Status::invalid_argument(format!(
                "Any type {} is not {}",
                type_url_str(self),
                M::FULL_NAME
            )));
        }
        M::parse(self.value()).map_err(|_| {
            crate::Status::internal(format!("Any payload is not a valid {}", M::FULL_NAME))
        })
    }
}

impl Status {
    /// A `google.rpc.Status` with `code`, `message`, and packed `details`.
    pub fn with_details(
        code: crate::Code,
        message: impl Into<String>,
        details: impl IntoIterator<Item = Any>,
    ) -> Self {
        let mut rpc = Self::new();
        rpc.set_code(code.to_i32());
        rpc.set_message(message.into());
        for any in details {
            rpc.details_mut().push(any);
        }
        rpc
    }
}

#[cfg(test)]
mod tests {
    use super::{Any, ErrorInfo, Status, TYPE_URL_PREFIX};
    use crate::Code;

    #[test]
    fn pack_unpack_round_trips_error_info() {
        let mut info = ErrorInfo::new();
        info.set_reason("API_DISABLED");
        info.set_domain("googleapis.com");
        info.metadata_mut().insert("resource", "projects/123");
        let any = Any::pack(&info).expect("pack");
        assert!(any.is::<ErrorInfo>());
        assert!(any
            .type_url()
            .to_str()
            .unwrap_or("")
            .starts_with(TYPE_URL_PREFIX));
        let got = any.unpack::<ErrorInfo>().expect("unpack");
        assert_eq!(got.reason().to_str().unwrap_or(""), "API_DISABLED");
        let resource = got
            .metadata()
            .get("resource")
            .and_then(|s| s.to_str().ok().map(str::to_owned));
        assert_eq!(resource.as_deref(), Some("projects/123"));
    }

    #[test]
    fn unpack_rejects_the_wrong_type() {
        let mut info = ErrorInfo::new();
        info.set_reason("X");
        let any = Any::pack(&info).expect("pack");
        let err = any.unpack::<super::DebugInfo>().expect_err("wrong type");
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn custom_prefix_still_unpacks() {
        let mut info = ErrorInfo::new();
        info.set_reason("STOCKOUT");
        let any =
            Any::pack_with(format!("example.com/{}", ErrorInfo::FULL_NAME), &info).expect("pack");
        assert!(any.is::<ErrorInfo>());
        assert_eq!(
            any.unpack::<ErrorInfo>()
                .expect("unpack")
                .reason()
                .to_str()
                .unwrap_or(""),
            "STOCKOUT"
        );
    }

    #[test]
    fn google_rpc_status_carries_packed_details() {
        let mut info = ErrorInfo::new();
        info.set_reason("QUOTA");
        let rpc = Status::with_details(
            Code::ResourceExhausted,
            "quota",
            [Any::pack(&info).expect("pack")],
        );
        assert_eq!(rpc.code(), Code::ResourceExhausted.to_i32());
        assert_eq!(rpc.details().len(), 1);
        let got = rpc
            .details()
            .get(0)
            .expect("one")
            .unpack::<ErrorInfo>()
            .expect("unpack");
        assert_eq!(got.reason().to_str().unwrap_or(""), "QUOTA");
    }
}
