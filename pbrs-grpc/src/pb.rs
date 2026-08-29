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

/// The standard `google.rpc` payloads a status may carry, plus any other
/// `Any` the peer sent.
///
/// Pack with [`crate::Status::from_error_details`]; unpack with
/// [`crate::Status::error_details`]. Unknown types stay in [`Self::unknown`]
/// so a custom detail is not dropped on a round-trip.
///
/// ```
/// use pbrs_grpc::pb::{ErrorDetails, ErrorInfo};
/// use pbrs_grpc::{Code, Status};
///
/// let mut info = ErrorInfo::new();
/// info.set_reason("API_DISABLED");
/// info.set_domain("example.com");
/// let details = ErrorDetails {
///     error_info: Some(info),
///     ..ErrorDetails::default()
/// };
/// let status = Status::from_error_details(Code::FailedPrecondition, "disabled", &details)?;
/// let info = status.error_details()?.error_info.ok_or_else(|| Status::internal("missing"))?;
/// assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
/// # Ok::<(), Status>(())
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ErrorDetails {
    /// [`ErrorInfo`], if the status named one.
    pub error_info: Option<ErrorInfo>,
    /// [`RetryInfo`].
    pub retry_info: Option<RetryInfo>,
    /// [`DebugInfo`].
    pub debug_info: Option<DebugInfo>,
    /// [`QuotaFailure`].
    pub quota_failure: Option<QuotaFailure>,
    /// [`PreconditionFailure`].
    pub precondition_failure: Option<PreconditionFailure>,
    /// [`BadRequest`].
    pub bad_request: Option<BadRequest>,
    /// [`RequestInfo`].
    pub request_info: Option<RequestInfo>,
    /// [`ResourceInfo`].
    pub resource_info: Option<ResourceInfo>,
    /// [`Help`].
    pub help: Option<Help>,
    /// [`LocalizedMessage`].
    pub localized_message: Option<LocalizedMessage>,
    /// Payloads that are not one of the standard types.
    pub unknown: Vec<Any>,
}

impl ErrorDetails {
    /// No detail messages.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Encode every populated field as `google.protobuf.Any`, standard
    /// types first, then [`Self::unknown`].
    pub fn to_anys(&self) -> Result<Vec<Any>, crate::Status> {
        let mut out = Vec::new();
        push_named(&mut out, self.error_info.as_ref())?;
        push_named(&mut out, self.retry_info.as_ref())?;
        push_named(&mut out, self.debug_info.as_ref())?;
        push_named(&mut out, self.quota_failure.as_ref())?;
        push_named(&mut out, self.precondition_failure.as_ref())?;
        push_named(&mut out, self.bad_request.as_ref())?;
        push_named(&mut out, self.request_info.as_ref())?;
        push_named(&mut out, self.resource_info.as_ref())?;
        push_named(&mut out, self.help.as_ref())?;
        push_named(&mut out, self.localized_message.as_ref())?;
        out.extend(self.unknown.iter().cloned());
        Ok(out)
    }

    /// Decode a `google.rpc.Status` details list. The first value of each
    /// standard type fills the matching field; anything else, including a
    /// second value of a known type, goes to [`Self::unknown`].
    pub fn from_rpc(rpc: &Status) -> Result<Self, crate::Status> {
        let mut out = Self::new();
        let details = rpc.details();
        for i in 0..details.len() {
            let Some(view) = details.get(i) else {
                continue;
            };
            if fill_standard(&mut out, &view)? {
                continue;
            }
            out.unknown.push(Any::clone(&view));
        }
        Ok(out)
    }
}

fn push_named<M: pbrs::Serialize + pbrs::MessageName>(
    out: &mut Vec<Any>,
    msg: Option<&M>,
) -> Result<(), crate::Status> {
    if let Some(msg) = msg {
        out.push(Any::pack(msg)?);
    }
    Ok(())
}

fn fill_standard(out: &mut ErrorDetails, any: &Any) -> Result<bool, crate::Status> {
    if out.error_info.is_none() && any.is::<ErrorInfo>() {
        out.error_info = Some(any.unpack()?);
        return Ok(true);
    }
    if out.retry_info.is_none() && any.is::<RetryInfo>() {
        out.retry_info = Some(any.unpack()?);
        return Ok(true);
    }
    if out.debug_info.is_none() && any.is::<DebugInfo>() {
        out.debug_info = Some(any.unpack()?);
        return Ok(true);
    }
    if out.quota_failure.is_none() && any.is::<QuotaFailure>() {
        out.quota_failure = Some(any.unpack()?);
        return Ok(true);
    }
    if out.precondition_failure.is_none() && any.is::<PreconditionFailure>() {
        out.precondition_failure = Some(any.unpack()?);
        return Ok(true);
    }
    if out.bad_request.is_none() && any.is::<BadRequest>() {
        out.bad_request = Some(any.unpack()?);
        return Ok(true);
    }
    if out.request_info.is_none() && any.is::<RequestInfo>() {
        out.request_info = Some(any.unpack()?);
        return Ok(true);
    }
    if out.resource_info.is_none() && any.is::<ResourceInfo>() {
        out.resource_info = Some(any.unpack()?);
        return Ok(true);
    }
    if out.help.is_none() && any.is::<Help>() {
        out.help = Some(any.unpack()?);
        return Ok(true);
    }
    if out.localized_message.is_none() && any.is::<LocalizedMessage>() {
        out.localized_message = Some(any.unpack()?);
        return Ok(true);
    }
    Ok(false)
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

    #[test]
    fn error_details_round_trips_known_and_unknown() {
        use super::ErrorDetails;
        use crate::HelloRequest;

        let mut info = ErrorInfo::new();
        info.set_reason("API_DISABLED");
        let mut extra = HelloRequest::new();
        extra.set_name("custom");
        let bag = ErrorDetails {
            error_info: Some(info),
            unknown: vec![Any::pack(&extra).expect("pack hello")],
            ..ErrorDetails::default()
        };
        let rpc = Status::with_details(
            Code::FailedPrecondition,
            "disabled",
            bag.to_anys().expect("anys"),
        );
        let got = ErrorDetails::from_rpc(&rpc).expect("decode");
        assert_eq!(
            got.error_info
                .as_ref()
                .expect("info")
                .reason()
                .to_str()
                .unwrap_or(""),
            "API_DISABLED"
        );
        assert_eq!(got.unknown.len(), 1);
        let hello = got.unknown[0].unpack::<HelloRequest>().expect("hello");
        assert_eq!(hello.name().to_str().unwrap_or(""), "custom");
    }
}
