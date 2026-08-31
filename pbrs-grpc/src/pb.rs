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
//!
//! Nested payloads live in modules named after the parent message, matching
//! the `.proto`: [`bad_request::FieldViolation`], [`quota_failure::Violation`],
//! [`precondition_failure::Violation`], [`help::Link`]. [`Duration`] is
//! `google.protobuf.Duration`; convert with [`Duration::from_std`] /
//! [`Duration::try_to_std`].

#![allow(missing_docs, reason = "messages come from the code generator")]

mod status_pb {
    include!(concat!(env!("OUT_DIR"), "/status.rs"));
}

mod details_pb {
    include!(concat!(env!("OUT_DIR"), "/error_details.rs"));
}

/// [`BadRequest`] nested types (`FieldViolation`).
pub use details_pb::bad_request;
/// [`Help`] nested types (`Link`).
pub use details_pb::help;
/// [`PreconditionFailure`] nested types (`Violation`).
pub use details_pb::precondition_failure;
/// [`QuotaFailure`] nested types (`Violation`).
pub use details_pb::quota_failure;
/// [`bad_request::FieldViolation`], also available at this module root.
pub use details_pb::FieldViolation;
/// [`help::Link`], also available at this module root.
pub use details_pb::Link;
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
    /// let info = ErrorInfo::with_reason("API_DISABLED", "example.com");
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

impl Duration {
    /// `google.protobuf.Duration` for a non-negative `std` duration.
    ///
    /// Seconds saturate at [`i64::MAX`]. Nanos always fit the protobuf range.
    #[must_use]
    pub fn from_std(delay: std::time::Duration) -> Self {
        let mut out = Self::new();
        out.set_seconds(i64::try_from(delay.as_secs()).unwrap_or(i64::MAX));
        out.set_nanos(i32::try_from(delay.subsec_nanos()).unwrap_or(0));
        out
    }

    /// Convert to [`std::time::Duration`].
    ///
    /// Negative seconds or nanos, or nanos ≥ 1s, are
    /// [`crate::Code::InvalidArgument`]. An overflow of `std`'s range is the
    /// same code rather than a panic.
    pub fn try_to_std(&self) -> Result<std::time::Duration, crate::Status> {
        let seconds = self.seconds();
        let nanos = self.nanos();
        if seconds < 0 || nanos < 0 {
            return Err(crate::Status::invalid_argument(
                "google.protobuf.Duration is negative",
            ));
        }
        let nanos = u32::try_from(nanos).unwrap_or(0);
        if nanos >= 1_000_000_000 {
            return Err(crate::Status::invalid_argument(
                "google.protobuf.Duration nanos out of range",
            ));
        }
        let seconds = u64::try_from(seconds).unwrap_or(u64::MAX);
        std::time::Duration::from_secs(seconds)
            .checked_add(std::time::Duration::from_nanos(u64::from(nanos)))
            .ok_or_else(|| {
                crate::Status::invalid_argument("google.protobuf.Duration overflows Duration")
            })
    }
}

impl ErrorInfo {
    /// `ErrorInfo` whose `reason` is `reason` and `domain` is `domain`.
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::error_info`].
    /// Distinct from [`crate::Status::retry_delay`]: that is a wait hint, not a cause.
    /// Distinct from [`crate::Status::bad_request`]: that is a field path, not reason and domain.
    /// Distinct from [`crate::Status::failed_precondition`], which is the ASCII code with no packed reason.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     error_info: Some(ErrorInfo::with_reason("API_DISABLED", "example.com")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::FailedPrecondition, "disabled", &details)?;
    /// let info = status.error_info().expect("ErrorInfo");
    /// assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
    /// assert_eq!(info.domain().to_str().unwrap_or(""), "example.com");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_reason(reason: impl Into<String>, domain: impl Into<String>) -> Self {
        let mut info = Self::new();
        info.set_reason(reason.into());
        info.set_domain(domain.into());
        info
    }
}

impl RetryInfo {
    /// `RetryInfo` whose `retry_delay` is `delay`.
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::retry_delay`]. Distinct from
    /// [`crate::Status::is_retryable`]: a delay is a wait hint, not
    /// permission to retry.
    #[must_use]
    pub fn with_retry_delay(delay: std::time::Duration) -> Self {
        let mut info = Self::new();
        info.set_retry_delay(Duration::from_std(delay));
        info
    }
}

impl FieldViolation {
    /// A violation of `field` with `description`.
    ///
    /// Packed as part of [`BadRequest::with_field`]. Distinct from
    /// [`crate::Status::error_info`]: that is reason and domain, not a field
    /// path.
    #[must_use]
    pub fn with_field(field: impl Into<String>, description: impl Into<String>) -> Self {
        let mut violation = Self::new();
        violation.set_field(field.into());
        violation.set_description(description.into());
        violation
    }
}

impl BadRequest {
    /// `BadRequest` with one [`FieldViolation`].
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::bad_request`]. Distinct from
    /// [`crate::Status::error_info`]: that is reason and domain, not field
    /// violations. Distinct from [`crate::Status::invalid_argument`], which
    /// is the ASCII code with no packed fields.
    ///
    /// ```
    /// use pbrs_grpc::pb::{BadRequest, ErrorDetails};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     bad_request: Some(BadRequest::with_field("name", "required")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::InvalidArgument, "bad", &details)?;
    /// let bad = status.bad_request().expect("BadRequest");
    /// assert_eq!(
    ///     bad.field_violations().get(0).expect("field").field().to_str().unwrap_or(""),
    ///     "name"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_field(field: impl Into<String>, description: impl Into<String>) -> Self {
        let mut bad = Self::new();
        bad.set_field_violations([FieldViolation::with_field(field, description)]);
        bad
    }
}

impl quota_failure::Violation {
    /// A quota `subject` with `description`.
    ///
    /// Packed as part of [`QuotaFailure::with_violation`]. Distinct from
    /// [`FieldViolation::with_field`]: that is a request field path, not a
    /// quota subject.
    #[must_use]
    pub fn with_subject(subject: impl Into<String>, description: impl Into<String>) -> Self {
        let mut violation = Self::new();
        violation.set_subject(subject.into());
        violation.set_description(description.into());
        violation
    }
}

impl QuotaFailure {
    /// `QuotaFailure` with one [`quota_failure::Violation`].
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::quota_failure`]. Distinct from
    /// [`crate::Status::is_retryable`]: [`crate::Code::ResourceExhausted`] is
    /// never A6-retryable. Distinct from [`crate::Status::retry_delay`]: a
    /// wait hint can sit next to quota.
    /// Distinct from [`crate::Status::bad_request`]: that is a field path, not a quota subject.
    /// Distinct from [`crate::Status::resource_exhausted`], which is the ASCII
    /// code with no packed quota.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     quota_failure: Some(QuotaFailure::with_violation("project:1", "tokens")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::ResourceExhausted, "quota", &details)?;
    /// assert!(!status.is_retryable());
    /// let quota = status.quota_failure().expect("QuotaFailure");
    /// assert_eq!(
    ///     quota.violations().get(0).expect("subject").subject().to_str().unwrap_or(""),
    ///     "project:1"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_violation(subject: impl Into<String>, description: impl Into<String>) -> Self {
        let mut quota = Self::new();
        quota.set_violations([quota_failure::Violation::with_subject(subject, description)]);
        quota
    }
}

impl precondition_failure::Violation {
    /// A precondition `type` and `subject` with `description`.
    ///
    /// Packed as part of [`PreconditionFailure::with_violation`]. Distinct from
    /// [`quota_failure::Violation::with_subject`]: that is a quota subject, not a
    /// precondition type. Distinct from [`FieldViolation::with_field`]: that is a
    /// request field path.
    #[must_use]
    pub fn with_type(
        r#type: impl Into<String>,
        subject: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let mut violation = Self::new();
        violation.set_type(r#type.into());
        violation.set_subject(subject.into());
        violation.set_description(description.into());
        violation
    }
}

impl PreconditionFailure {
    /// `PreconditionFailure` with one [`precondition_failure::Violation`].
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::precondition_failure`]. Distinct from
    /// [`crate::Status::is_retryable`]: [`crate::Code::FailedPrecondition`] is
    /// never A6-retryable. Distinct from [`crate::Status::retry_delay`]: a
    /// wait hint can sit next to a precondition.
    /// Distinct from [`crate::Status::quota_failure`]: that is a quota subject, not a precondition type.
    /// Distinct from [`crate::Status::bad_request`]: that is a field path, not a precondition type.
    /// Distinct from [`crate::Status::failed_precondition`], which is the ASCII
    /// code with no packed violations.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, PreconditionFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     precondition_failure: Some(PreconditionFailure::with_violation(
    ///         "TOS",
    ///         "google.com/cloud",
    ///         "unsigned",
    ///     )),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::FailedPrecondition, "tos", &details)?;
    /// assert!(!status.is_retryable());
    /// let pre = status.precondition_failure().expect("PreconditionFailure");
    /// assert_eq!(
    ///     pre.violations().get(0).expect("violation").r#type().to_str().unwrap_or(""),
    ///     "TOS"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_violation(
        r#type: impl Into<String>,
        subject: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let mut pre = Self::new();
        pre.set_violations([precondition_failure::Violation::with_type(
            r#type,
            subject,
            description,
        )]);
        pre
    }
}

impl help::Link {
    /// A documentation `description` and `url`.
    ///
    /// Packed as part of [`Help::with_link`]. Distinct from
    /// [`quota_failure::Violation::with_subject`]: that is a quota subject, not a
    /// docs URL. Distinct from [`precondition_failure::Violation::with_type`]:
    /// that is a precondition type, not a docs URL. Distinct from
    /// [`FieldViolation::with_field`]: that is a request field path.
    #[must_use]
    pub fn with_url(description: impl Into<String>, url: impl Into<String>) -> Self {
        let mut link = Self::new();
        link.set_description(description.into());
        link.set_url(url.into());
        link
    }
}

impl Help {
    /// `Help` with one [`help::Link`].
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::help`]. Distinct from
    /// [`crate::Status::is_retryable`]: documentation links can sit next to a retryable [`crate::Code::Unavailable`].
    /// Distinct from [`crate::Status::precondition_failure`]: that is a type and subject, not a docs URL.
    /// Distinct from [`crate::Status::quota_failure`]: that is a quota subject, not a docs URL.
    /// Distinct from [`crate::Status::bad_request`]: that is a field path, not a docs URL.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, Help};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     help: Some(Help::with_link("quota docs", "https://example.com/quota")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::Unavailable, "backend", &details)?;
    /// assert!(status.is_retryable());
    /// let help = status.help().expect("Help");
    /// assert_eq!(
    ///     help.links().get(0).expect("link").url().to_str().unwrap_or(""),
    ///     "https://example.com/quota"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_link(description: impl Into<String>, url: impl Into<String>) -> Self {
        let link = help::Link::with_url(description, url);
        let mut out = Self::new();
        out.set_links([link]);
        out
    }
}

impl LocalizedMessage {
    /// `LocalizedMessage` for `locale` with `message`.
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::localized_message`].
    /// Distinct from [`crate::Status::message`]: that is the ASCII `grpc-message`, not a locale.
    /// Distinct from [`crate::Status::help`]: that is a docs URL, not a locale.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, LocalizedMessage};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     localized_message: Some(LocalizedMessage::with_locale("fr-FR", "introuvable")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::NotFound, "not found", &details)?;
    /// assert_eq!(status.message(), "not found");
    /// let local = status.localized_message().expect("LocalizedMessage");
    /// assert_eq!(local.locale().to_str().unwrap_or(""), "fr-FR");
    /// assert_eq!(local.message().to_str().unwrap_or(""), "introuvable");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_locale(locale: impl Into<String>, message: impl Into<String>) -> Self {
        let mut local = Self::new();
        local.set_locale(locale.into());
        local.set_message(message.into());
        local
    }
}

impl RequestInfo {
    /// `RequestInfo` with `request_id` and `serving_data`.
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::request_info`].
    /// Distinct from [`crate::Status::error_info`]: that is a metadata map, not a typed request_id.
    /// Distinct from [`crate::Status::help`]: that is a docs URL, not a request_id.
    /// Distinct from [`crate::Status::localized_message`]: that is a locale, not a request_id.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, RequestInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     request_info: Some(RequestInfo::with_request_id("req-9", "encrypted")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::Internal, "boom", &details)?;
    /// let info = status.request_info().expect("RequestInfo");
    /// assert_eq!(info.request_id().to_str().unwrap_or(""), "req-9");
    /// assert_eq!(info.serving_data().to_str().unwrap_or(""), "encrypted");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_request_id(request_id: impl Into<String>, serving_data: impl Into<String>) -> Self {
        let mut info = Self::new();
        info.set_request_id(request_id.into());
        info.set_serving_data(serving_data.into());
        info
    }
}

impl ResourceInfo {
    /// `ResourceInfo` with `resource_type`, `resource_name`, and `owner`.
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::resource_info`].
    /// Distinct from [`crate::Status::quota_failure`]: that is a quota subject, not a resource identity.
    /// Distinct from [`crate::Status::request_info`]: that is a request_id, not a resource.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ResourceInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     resource_info: Some(ResourceInfo::with_resource(
    ///         "sqladmin.googleapis.com/Instance",
    ///         "projects/1/instances/a",
    ///         "project:1",
    ///     )),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::NotFound, "gone", &details)?;
    /// let info = status.resource_info().expect("ResourceInfo");
    /// assert_eq!(
    ///     info.resource_name().to_str().unwrap_or(""),
    ///     "projects/1/instances/a"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_resource(
        resource_type: impl Into<String>,
        resource_name: impl Into<String>,
        owner: impl Into<String>,
    ) -> Self {
        let mut info = Self::new();
        info.set_resource_type(resource_type.into());
        info.set_resource_name(resource_name.into());
        info.set_owner(owner.into());
        info
    }
}

impl DebugInfo {
    /// `DebugInfo` with one stack `entry` and `detail`.
    ///
    /// Packed onto a status with [`crate::Status::from_error_details`];
    /// unpack with [`crate::Status::debug_info`].
    /// Distinct from [`crate::Status::localized_message`]: that is a locale, not an operator stack.
    /// Distinct from [`crate::Status::help`]: that is a docs URL, not an operator stack.
    ///
    /// ```
    /// use pbrs_grpc::pb::{DebugInfo, ErrorDetails};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     debug_info: Some(DebugInfo::with_stack("handler.rs:9", "nil pointer")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::Internal, "boom", &details)?;
    /// let debug = status.debug_info().expect("DebugInfo");
    /// assert_eq!(debug.detail().to_str().unwrap_or(""), "nil pointer");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_stack(entry: impl Into<String>, detail: impl Into<String>) -> Self {
        let mut info = Self::new();
        info.stack_entries_mut().push(entry.into());
        info.set_detail(detail.into());
        info
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
    /// Distinct from [`crate::Status::from_rpc`]: that encodes a packed protobuf as the trailer; this unpacks the typed bag.
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
    use super::{
        bad_request, help, precondition_failure, quota_failure, Any, BadRequest, DebugInfo,
        Duration, ErrorDetails, ErrorInfo, FieldViolation, Help, LocalizedMessage,
        PreconditionFailure, QuotaFailure, RequestInfo, ResourceInfo, RetryInfo, Status,
        TYPE_URL_PREFIX,
    };
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
    fn error_info_with_reason_round_trips() {
        let info = ErrorInfo::with_reason("API_DISABLED", "example.com");
        assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
        assert_eq!(info.domain().to_str().unwrap_or(""), "example.com");
        let details = ErrorDetails {
            error_info: Some(info),
            ..ErrorDetails::default()
        };
        let status =
            crate::Status::from_error_details(Code::FailedPrecondition, "disabled", &details)
                .expect("encode");
        let got = status.error_info().expect("ErrorInfo");
        assert_eq!(got.reason().to_str().unwrap_or(""), "API_DISABLED");
        assert_eq!(got.domain().to_str().unwrap_or(""), "example.com");
        assert!(status.retry_delay().is_none());
        assert!(status.bad_request().is_none());
        assert!(crate::Status::failed_precondition("disabled")
            .error_info()
            .is_none());
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
        let hello = got
            .unknown
            .first()
            .expect("custom Any")
            .unpack::<HelloRequest>()
            .expect("hello");
        assert_eq!(hello.name().to_str().unwrap_or(""), "custom");
    }

    #[test]
    fn nested_detail_types_round_trip_through_error_details() {
        let mut field = bad_request::FieldViolation::new();
        field.set_field("name");
        field.set_description("required");
        let mut bad = BadRequest::new();
        bad.set_field_violations([field]);

        let mut quota_v = quota_failure::Violation::new();
        quota_v.set_subject("project:1");
        quota_v.set_description("tokens");
        let mut quota = QuotaFailure::new();
        quota.set_violations([quota_v]);

        let mut pre_v = precondition_failure::Violation::new();
        pre_v.set_subject("TOS");
        pre_v.set_description("unsigned");
        let mut pre = PreconditionFailure::new();
        pre.set_violations([pre_v]);

        let mut link = help::Link::new();
        link.set_description("docs");
        link.set_url("https://example.com/quota");
        let mut help_msg = Help::new();
        help_msg.set_links([link]);

        let retry = RetryInfo::with_retry_delay(std::time::Duration::from_millis(1500));
        assert_eq!(
            retry.retry_delay().try_to_std().expect("delay"),
            std::time::Duration::from_millis(1500)
        );

        let bag = ErrorDetails {
            bad_request: Some(bad),
            quota_failure: Some(quota),
            precondition_failure: Some(pre),
            help: Some(help_msg),
            retry_info: Some(retry),
            ..ErrorDetails::default()
        };
        let status =
            crate::Status::from_error_details(Code::InvalidArgument, "bad", &bag).expect("encode");
        let got = status.error_details().expect("decode");

        let field = got
            .bad_request
            .as_ref()
            .expect("bad_request")
            .field_violations()
            .get(0)
            .expect("field");
        assert_eq!(field.field().to_str().unwrap_or(""), "name");
        assert_eq!(field.description().to_str().unwrap_or(""), "required");

        let quota_v = got
            .quota_failure
            .as_ref()
            .expect("quota")
            .violations()
            .get(0)
            .expect("qv");
        assert_eq!(quota_v.subject().to_str().unwrap_or(""), "project:1");

        let pre_v = got
            .precondition_failure
            .as_ref()
            .expect("pre")
            .violations()
            .get(0)
            .expect("pv");
        assert_eq!(pre_v.subject().to_str().unwrap_or(""), "TOS");

        let link = got
            .help
            .as_ref()
            .expect("help")
            .links()
            .get(0)
            .expect("link");
        assert_eq!(
            link.url().to_str().unwrap_or(""),
            "https://example.com/quota"
        );

        assert_eq!(
            got.retry_info
                .as_ref()
                .expect("retry")
                .retry_delay()
                .try_to_std()
                .expect("delay"),
            std::time::Duration::from_millis(1500)
        );
    }

    #[test]
    fn protobuf_duration_rejects_negative() {
        let mut delay = Duration::new();
        delay.set_seconds(-1);
        let err = delay.try_to_std().expect_err("negative");
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn bad_request_with_field_round_trips() {
        let violation = FieldViolation::with_field("name", "required");
        assert_eq!(violation.field().to_str().unwrap_or(""), "name");
        assert_eq!(violation.description().to_str().unwrap_or(""), "required");
        let bad = BadRequest::with_field("email", "invalid");
        let details = ErrorDetails {
            bad_request: Some(bad),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::InvalidArgument, "bad", &details)
            .expect("encode");
        let got = status.bad_request().expect("BadRequest");
        let field = got.field_violations().get(0).expect("field");
        assert_eq!(field.field().to_str().unwrap_or(""), "email");
        assert_eq!(field.description().to_str().unwrap_or(""), "invalid");
    }

    #[test]
    fn quota_failure_with_violation_round_trips() {
        let violation = quota_failure::Violation::with_subject("project:1", "tokens");
        assert_eq!(violation.subject().to_str().unwrap_or(""), "project:1");
        assert_eq!(violation.description().to_str().unwrap_or(""), "tokens");
        let quota = QuotaFailure::with_violation("client:9", "qps");
        let details = ErrorDetails {
            quota_failure: Some(quota),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "quota", &details)
            .expect("encode");
        assert!(!status.is_retryable());
        let got = status.quota_failure().expect("QuotaFailure");
        let subject = got.violations().get(0).expect("subject");
        assert_eq!(subject.subject().to_str().unwrap_or(""), "client:9");
        assert_eq!(subject.description().to_str().unwrap_or(""), "qps");
        assert!(status.retry_delay().is_none());
        assert!(status.bad_request().is_none());
    }

    #[test]
    fn precondition_failure_with_violation_round_trips() {
        let violation =
            precondition_failure::Violation::with_type("TOS", "google.com/cloud", "unsigned");
        assert_eq!(violation.r#type().to_str().unwrap_or(""), "TOS");
        assert_eq!(
            violation.subject().to_str().unwrap_or(""),
            "google.com/cloud"
        );
        assert_eq!(violation.description().to_str().unwrap_or(""), "unsigned");
        let pre =
            PreconditionFailure::with_violation("googleapis.com/iam/resource", "user:9", "missing");
        let details = ErrorDetails {
            precondition_failure: Some(pre),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::FailedPrecondition, "tos", &details)
            .expect("encode");
        assert!(!status.is_retryable());
        let got = status.precondition_failure().expect("PreconditionFailure");
        let violation = got.violations().get(0).expect("violation");
        assert_eq!(
            violation.r#type().to_str().unwrap_or(""),
            "googleapis.com/iam/resource"
        );
        assert_eq!(violation.subject().to_str().unwrap_or(""), "user:9");
        assert_eq!(violation.description().to_str().unwrap_or(""), "missing");
        assert!(status.retry_delay().is_none());
        assert!(status.quota_failure().is_none());
        assert!(status.bad_request().is_none());
    }

    #[test]
    fn help_with_link_round_trips() {
        let link = help::Link::with_url("quota docs", "https://example.com/quota");
        assert_eq!(link.description().to_str().unwrap_or(""), "quota docs");
        assert_eq!(
            link.url().to_str().unwrap_or(""),
            "https://example.com/quota"
        );
        let packed = Help::with_link("retry", "https://example.com/retry");
        let details = ErrorDetails {
            help: Some(packed),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::Unavailable, "backend", &details)
            .expect("encode");
        assert!(status.is_retryable());
        let got = status.help().expect("Help");
        let link = got.links().get(0).expect("link");
        assert_eq!(link.description().to_str().unwrap_or(""), "retry");
        assert_eq!(
            link.url().to_str().unwrap_or(""),
            "https://example.com/retry"
        );
        assert!(status.retry_delay().is_none());
        assert!(status.precondition_failure().is_none());
        assert!(status.quota_failure().is_none());
    }

    #[test]
    fn localized_message_with_locale_round_trips() {
        let local = LocalizedMessage::with_locale("fr-FR", "introuvable");
        assert_eq!(local.locale().to_str().unwrap_or(""), "fr-FR");
        assert_eq!(local.message().to_str().unwrap_or(""), "introuvable");
        let details = ErrorDetails {
            localized_message: Some(local),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::NotFound, "not found", &details)
            .expect("encode");
        assert_eq!(status.message(), "not found");
        let got = status.localized_message().expect("LocalizedMessage");
        assert_eq!(got.locale().to_str().unwrap_or(""), "fr-FR");
        assert_eq!(got.message().to_str().unwrap_or(""), "introuvable");
        assert!(status.help().is_none());
        assert!(status.precondition_failure().is_none());
    }

    #[test]
    fn request_info_with_request_id_round_trips() {
        let info = RequestInfo::with_request_id("req-9", "encrypted");
        assert_eq!(info.request_id().to_str().unwrap_or(""), "req-9");
        assert_eq!(info.serving_data().to_str().unwrap_or(""), "encrypted");
        let details = ErrorDetails {
            request_info: Some(info),
            ..ErrorDetails::default()
        };
        let status =
            crate::Status::from_error_details(Code::Internal, "boom", &details).expect("encode");
        let got = status.request_info().expect("RequestInfo");
        assert_eq!(got.request_id().to_str().unwrap_or(""), "req-9");
        assert_eq!(got.serving_data().to_str().unwrap_or(""), "encrypted");
        assert!(status.error_info().is_none());
        assert!(status.help().is_none());
        assert!(status.localized_message().is_none());
    }

    #[test]
    fn resource_info_with_resource_round_trips() {
        let info = ResourceInfo::with_resource(
            "sqladmin.googleapis.com/Instance",
            "projects/1/instances/a",
            "project:1",
        );
        assert_eq!(
            info.resource_type().to_str().unwrap_or(""),
            "sqladmin.googleapis.com/Instance"
        );
        assert_eq!(
            info.resource_name().to_str().unwrap_or(""),
            "projects/1/instances/a"
        );
        assert_eq!(info.owner().to_str().unwrap_or(""), "project:1");
        let details = ErrorDetails {
            resource_info: Some(info),
            ..ErrorDetails::default()
        };
        let status =
            crate::Status::from_error_details(Code::NotFound, "gone", &details).expect("encode");
        let got = status.resource_info().expect("ResourceInfo");
        assert_eq!(
            got.resource_name().to_str().unwrap_or(""),
            "projects/1/instances/a"
        );
        assert!(status.quota_failure().is_none());
        assert!(status.request_info().is_none());
    }

    #[test]
    fn debug_info_with_stack_round_trips() {
        let debug = DebugInfo::with_stack("handler.rs:9", "nil pointer");
        assert_eq!(
            debug
                .stack_entries()
                .get(0)
                .expect("frame")
                .to_str()
                .unwrap_or(""),
            "handler.rs:9"
        );
        assert_eq!(debug.detail().to_str().unwrap_or(""), "nil pointer");
        let details = ErrorDetails {
            debug_info: Some(debug),
            ..ErrorDetails::default()
        };
        let status =
            crate::Status::from_error_details(Code::Internal, "boom", &details).expect("encode");
        let got = status.debug_info().expect("DebugInfo");
        assert_eq!(
            got.stack_entries()
                .get(0)
                .expect("frame")
                .to_str()
                .unwrap_or(""),
            "handler.rs:9"
        );
        assert_eq!(got.detail().to_str().unwrap_or(""), "nil pointer");
        assert!(status.localized_message().is_none());
        assert!(status.help().is_none());
    }
}
