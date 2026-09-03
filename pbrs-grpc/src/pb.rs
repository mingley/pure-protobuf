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
    /// Distinct from [`crate::Status::with_error_details`]: that packs `Any` values onto a status; this packs one message into an `Any`.
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
    /// Distinct from [`Self::pack`]: that uses `type.googleapis.com/<FULL_NAME>`; this takes an explicit type URL.
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
    /// Distinct from [`Self::unpack`]: that decodes the payload; this is a type-URL check.
    #[must_use]
    pub fn is<M: pbrs::MessageName>(&self) -> bool {
        type_name_of(type_url_str(self)) == M::FULL_NAME
    }

    /// Decode the payload as `M`, after checking the type URL.
    ///
    /// [`crate::Code::InvalidArgument`] if the type URL names a different
    /// message; [`crate::Code::Internal`] if the bytes are not a valid `M`.
    /// Distinct from [`Self::is`]: that is a type-URL check; this decodes the payload.
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
    /// Distinct from [`Self::try_to_std`]: that converts this protobuf to `std`; this builds the protobuf from `std`.
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
    /// Distinct from [`Self::from_std`]: that builds the protobuf from `std`; this converts this protobuf to `std`.
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
    /// Distinct from [`Self::with_metadata`]: that is a metadata pair, not reason and domain.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_error_info(ErrorInfo::with_reason("API_DISABLED", "example.com"));
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

    /// Inserts `key` → `value` into this payload's metadata map.
    ///
    /// Chain after [`Self::with_reason`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::error_info`].
    /// Distinct from [`Self::with_reason`]: that is reason and domain, not a metadata pair.
    /// Distinct from [`crate::Status::request_info`]: that is a typed request_id, not this metadata map.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let info = ErrorInfo::with_reason("API_DISABLED", "example.com")
    ///     .with_metadata("resource", "projects/123");
    /// let details = ErrorDetails::new().with_error_info(info);
    /// let status = Status::from_error_details(Code::FailedPrecondition, "disabled", &details)?;
    /// let got = status.error_info().expect("ErrorInfo");
    /// let resource = got
    ///     .metadata()
    ///     .get("resource")
    ///     .and_then(|s| s.to_str().ok().map(str::to_owned));
    /// assert_eq!(resource.as_deref(), Some("projects/123"));
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_metadata(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.metadata_mut().insert(key.as_ref(), value.as_ref());
        self
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

    /// Sets `reason` on this field violation.
    ///
    /// Chain after [`Self::with_field`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::bad_request`].
    /// Distinct from [`Self::with_field`]: that is a request field path, not the field-violation reason.
    /// Distinct from [`crate::pb::ErrorInfo::with_reason`]: that is reason and domain, not a field-violation reason.
    /// Distinct from [`crate::Status::invalid_argument`]: that is the ASCII code with no packed fields.
    ///
    /// ```
    /// use pbrs_grpc::pb::{BadRequest, ErrorDetails, FieldViolation};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let violation = FieldViolation::with_field("name", "required")
    ///     .with_reason("REQUIRED");
    /// let mut bad = BadRequest::new();
    /// bad.set_field_violations([violation]);
    /// let details = ErrorDetails::new().with_bad_request(bad);
    /// let status = Status::from_error_details(Code::InvalidArgument, "bad", &details)?;
    /// let got = status.bad_request().expect("BadRequest");
    /// assert_eq!(
    ///     got.field_violations()
    ///         .get(0)
    ///         .expect("field")
    ///         .reason()
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "REQUIRED"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.set_reason(reason.into());
        self
    }

    /// Sets `localized_message` on this field violation.
    ///
    /// Chain after [`Self::with_field`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::bad_request`].
    /// Distinct from [`Self::with_field`]: that is a request field path, not a field-violation localized message.
    /// Distinct from [`Self::with_reason`]: that is the field-violation reason, not a field-violation localized message.
    /// Distinct from [`LocalizedMessage::with_locale`]: that builds the locale payload; this attaches it to a field violation.
    ///
    /// ```
    /// use pbrs_grpc::pb::{BadRequest, ErrorDetails, FieldViolation, LocalizedMessage};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let violation = FieldViolation::with_field("name", "required")
    ///     .with_localized_message(LocalizedMessage::with_locale("fr-FR", "requis"));
    /// let mut bad = BadRequest::new();
    /// bad.set_field_violations([violation]);
    /// let details = ErrorDetails::new().with_bad_request(bad);
    /// let status = Status::from_error_details(Code::InvalidArgument, "bad", &details)?;
    /// let got = status.bad_request().expect("BadRequest");
    /// let field = got.field_violations().get(0).expect("field");
    /// let local = field.localized_message_opt().expect("locale");
    /// assert_eq!(local.locale().to_str().unwrap_or(""), "fr-FR");
    /// assert_eq!(local.message().to_str().unwrap_or(""), "requis");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_localized_message(mut self, localized_message: LocalizedMessage) -> Self {
        self.set_localized_message(localized_message);
        self
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
    /// let details = ErrorDetails::new()
    ///     .with_bad_request(BadRequest::with_field("name", "required"));
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

    /// Pushes another field violation onto this bad request.
    ///
    /// Chain after [`Self::with_field`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::bad_request`].
    /// Distinct from [`Self::with_field`]: that is the first field path, not an extra field violation.
    /// Distinct from [`FieldViolation::with_field`]: that builds one nested violation; this appends another onto BadRequest.
    /// Distinct from [`crate::Status::error_info`]: that is reason and domain, not an extra field violation.
    ///
    /// ```
    /// use pbrs_grpc::pb::{BadRequest, ErrorDetails};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new().with_bad_request(
    ///     BadRequest::with_field("name", "required").with_field_entry("email", "invalid"),
    /// );
    /// let status = Status::from_error_details(Code::InvalidArgument, "bad", &details)?;
    /// let bad = status.bad_request().expect("BadRequest");
    /// let field = bad.field_violations().get(1).expect("field");
    /// assert_eq!(field.field().to_str().unwrap_or(""), "email");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_field_entry(
        mut self,
        field: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.field_violations_mut()
            .push(FieldViolation::with_field(field, description));
        self
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

    /// Sets `api_service` on this quota violation.
    ///
    /// Chain after [`Self::with_subject`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::quota_failure`].
    /// Distinct from [`Self::with_subject`]: that is subject and description, not the API service name.
    /// Distinct from [`crate::Status::error_info`]: that is reason and domain, not a quota API service.
    /// Distinct from [`FieldViolation::with_field`]: that is a request field path, not a quota API service.
    ///
    /// ```
    /// use pbrs_grpc::pb::{quota_failure, ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let violation = quota_failure::Violation::with_subject("project:1", "tokens")
    ///     .with_api_service("compute.googleapis.com");
    /// let mut quota = QuotaFailure::new();
    /// quota.set_violations([violation]);
    /// let details = ErrorDetails::new().with_quota_failure(quota);
    /// let status = Status::from_error_details(Code::ResourceExhausted, "quota", &details)?;
    /// let got = status.quota_failure().expect("QuotaFailure");
    /// assert_eq!(
    ///     got.violations()
    ///         .get(0)
    ///         .expect("v")
    ///         .api_service()
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "compute.googleapis.com"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_api_service(mut self, api_service: impl Into<String>) -> Self {
        self.set_api_service(api_service.into());
        self
    }

    /// Sets `quota_metric` on this quota violation.
    ///
    /// Chain after [`Self::with_subject`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::quota_failure`].
    /// Distinct from [`Self::with_subject`]: that is subject and description, not the quota metric name.
    /// Distinct from [`Self::with_api_service`]: that is the API service name, not the quota metric name.
    /// Distinct from [`crate::Status::error_info`]: that is reason and domain, not a quota metric.
    ///
    /// ```
    /// use pbrs_grpc::pb::{quota_failure, ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let violation = quota_failure::Violation::with_subject("project:1", "tokens")
    ///     .with_quota_metric("compute.googleapis.com/cpus");
    /// let mut quota = QuotaFailure::new();
    /// quota.set_violations([violation]);
    /// let details = ErrorDetails::new().with_quota_failure(quota);
    /// let status = Status::from_error_details(Code::ResourceExhausted, "quota", &details)?;
    /// let got = status.quota_failure().expect("QuotaFailure");
    /// assert_eq!(
    ///     got.violations()
    ///         .get(0)
    ///         .expect("v")
    ///         .quota_metric()
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "compute.googleapis.com/cpus"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_quota_metric(mut self, quota_metric: impl Into<String>) -> Self {
        self.set_quota_metric(quota_metric.into());
        self
    }

    /// Sets `quota_id` on this quota violation.
    ///
    /// Chain after [`Self::with_subject`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::quota_failure`].
    /// Distinct from [`Self::with_subject`]: that is subject and description, not the quota id.
    /// Distinct from [`Self::with_quota_metric`]: that is the quota metric name, not the quota id.
    /// Distinct from [`crate::Status::error_info`]: that is reason and domain, not a quota id.
    ///
    /// ```
    /// use pbrs_grpc::pb::{quota_failure, ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let violation = quota_failure::Violation::with_subject("project:1", "tokens")
    ///     .with_quota_id("CPUS-PER-PROJECT");
    /// let mut quota = QuotaFailure::new();
    /// quota.set_violations([violation]);
    /// let details = ErrorDetails::new().with_quota_failure(quota);
    /// let status = Status::from_error_details(Code::ResourceExhausted, "quota", &details)?;
    /// let got = status.quota_failure().expect("QuotaFailure");
    /// assert_eq!(
    ///     got.violations()
    ///         .get(0)
    ///         .expect("v")
    ///         .quota_id()
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "CPUS-PER-PROJECT"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_quota_id(mut self, quota_id: impl Into<String>) -> Self {
        self.set_quota_id(quota_id.into());
        self
    }

    /// Inserts `key` → `value` into this violation's `quota_dimensions` map.
    ///
    /// Chain after [`Self::with_subject`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::quota_failure`].
    /// Distinct from [`Self::with_subject`]: that is subject and description, not a quota dimension pair.
    /// Distinct from [`Self::with_quota_id`]: that is the quota id, not a quota dimension pair.
    /// Distinct from [`crate::pb::ErrorInfo::with_metadata`]: that is ErrorInfo metadata, not quota dimensions.
    ///
    /// ```
    /// use pbrs_grpc::pb::{quota_failure, ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let violation = quota_failure::Violation::with_subject("project:1", "tokens")
    ///     .with_quota_dimension("region", "us-central1");
    /// let mut quota = QuotaFailure::new();
    /// quota.set_violations([violation]);
    /// let details = ErrorDetails::new().with_quota_failure(quota);
    /// let status = Status::from_error_details(Code::ResourceExhausted, "quota", &details)?;
    /// let got = status.quota_failure().expect("QuotaFailure");
    /// let region = got
    ///     .violations()
    ///     .get(0)
    ///     .expect("v")
    ///     .quota_dimensions()
    ///     .get("region")
    ///     .and_then(|s| s.to_str().ok().map(str::to_owned));
    /// assert_eq!(region.as_deref(), Some("us-central1"));
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_quota_dimension(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.quota_dimensions_mut()
            .insert(key.as_ref(), value.as_ref());
        self
    }

    /// Sets `quota_value` on this quota violation.
    ///
    /// Chain after [`Self::with_subject`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::quota_failure`].
    /// Distinct from [`Self::with_subject`]: that is subject and description, not the quota value.
    /// Distinct from [`Self::with_quota_dimension`]: that is a quota dimension pair, not the quota value.
    /// Distinct from [`crate::Status::retry_delay`]: that is a wait hint, not the quota value.
    ///
    /// ```
    /// use pbrs_grpc::pb::{quota_failure, ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let violation = quota_failure::Violation::with_subject("project:1", "tokens")
    ///     .with_quota_value(8);
    /// let mut quota = QuotaFailure::new();
    /// quota.set_violations([violation]);
    /// let details = ErrorDetails::new().with_quota_failure(quota);
    /// let status = Status::from_error_details(Code::ResourceExhausted, "quota", &details)?;
    /// let got = status.quota_failure().expect("QuotaFailure");
    /// assert_eq!(got.violations().get(0).expect("v").quota_value(), 8);
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_quota_value(mut self, quota_value: i64) -> Self {
        self.set_quota_value(quota_value);
        self
    }

    /// Sets `future_quota_value` on this quota violation.
    ///
    /// Chain after [`Self::with_subject`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::quota_failure`].
    /// Distinct from [`Self::with_subject`]: that is subject and description, not the future quota value.
    /// Distinct from [`Self::with_quota_value`]: that is the current quota value, not the future quota value.
    /// Distinct from [`crate::Status::retry_delay`]: that is a wait hint, not the future quota value.
    ///
    /// ```
    /// use pbrs_grpc::pb::{quota_failure, ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let violation = quota_failure::Violation::with_subject("project:1", "tokens")
    ///     .with_future_quota_value(16);
    /// let mut quota = QuotaFailure::new();
    /// quota.set_violations([violation]);
    /// let details = ErrorDetails::new().with_quota_failure(quota);
    /// let status = Status::from_error_details(Code::ResourceExhausted, "quota", &details)?;
    /// let got = status.quota_failure().expect("QuotaFailure");
    /// let v = got.violations().get(0).expect("v");
    /// assert!(v.has_future_quota_value());
    /// assert_eq!(v.future_quota_value(), 16);
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_future_quota_value(mut self, future_quota_value: i64) -> Self {
        self.set_future_quota_value(future_quota_value);
        self
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
    /// let details = ErrorDetails::new()
    ///     .with_quota_failure(QuotaFailure::with_violation("project:1", "tokens"));
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

    /// Pushes another quota violation onto this quota failure.
    ///
    /// Chain after [`Self::with_violation`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::quota_failure`].
    /// Distinct from [`Self::with_violation`]: that is the first quota subject, not an extra quota violation.
    /// Distinct from [`quota_failure::Violation::with_subject`]: that builds one nested violation; this appends another onto QuotaFailure.
    /// Distinct from [`crate::Status::bad_request`]: that is a field path, not an extra quota violation.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new().with_quota_failure(
    ///     QuotaFailure::with_violation("project:1", "tokens")
    ///         .with_violation_entry("client:9", "qps"),
    /// );
    /// let status = Status::from_error_details(Code::ResourceExhausted, "quota", &details)?;
    /// let quota = status.quota_failure().expect("QuotaFailure");
    /// let extra = quota.violations().get(1).expect("subject");
    /// assert_eq!(extra.subject().to_str().unwrap_or(""), "client:9");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_violation_entry(
        mut self,
        subject: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.violations_mut()
            .push(quota_failure::Violation::with_subject(subject, description));
        self
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
    /// let details = ErrorDetails::new().with_precondition_failure(
    ///     PreconditionFailure::with_violation("TOS", "google.com/cloud", "unsigned"),
    /// );
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

    /// Pushes another precondition violation onto this precondition failure.
    ///
    /// Chain after [`Self::with_violation`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::precondition_failure`].
    /// Distinct from [`Self::with_violation`]: that is the first precondition type, not an extra precondition violation.
    /// Distinct from [`precondition_failure::Violation::with_type`]: that builds one nested violation; this appends another onto PreconditionFailure.
    /// Distinct from [`crate::Status::quota_failure`]: that is a quota subject, not an extra precondition violation.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, PreconditionFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new().with_precondition_failure(
    ///     PreconditionFailure::with_violation("TOS", "google.com/cloud", "unsigned")
    ///         .with_violation_entry("googleapis.com/iam/resource", "user:9", "missing"),
    /// );
    /// let status = Status::from_error_details(Code::FailedPrecondition, "tos", &details)?;
    /// let pre = status.precondition_failure().expect("PreconditionFailure");
    /// let extra = pre.violations().get(1).expect("violation");
    /// assert_eq!(
    ///     extra.r#type().to_str().unwrap_or(""),
    ///     "googleapis.com/iam/resource"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_violation_entry(
        mut self,
        r#type: impl Into<String>,
        subject: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.violations_mut()
            .push(precondition_failure::Violation::with_type(
                r#type,
                subject,
                description,
            ));
        self
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
    /// let details = ErrorDetails::new()
    ///     .with_help(Help::with_link("quota docs", "https://example.com/quota"));
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

    /// Pushes another documentation link onto this help payload.
    ///
    /// Chain after [`Self::with_link`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::help`].
    /// Distinct from [`Self::with_link`]: that is the first docs URL, not an extra help link.
    /// Distinct from [`help::Link::with_url`]: that builds one nested link; this appends another onto Help.
    /// Distinct from [`crate::Status::localized_message`]: that is a locale, not an extra help link.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, Help};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_help(
    ///         Help::with_link("quota docs", "https://example.com/quota")
    ///             .with_link_entry("retry", "https://example.com/retry"),
    ///     );
    /// let status = Status::from_error_details(Code::Unavailable, "backend", &details)?;
    /// let help = status.help().expect("Help");
    /// let link = help.links().get(1).expect("link");
    /// assert_eq!(
    ///     link.url().to_str().unwrap_or(""),
    ///     "https://example.com/retry"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_link_entry(
        mut self,
        description: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        self.links_mut()
            .push(help::Link::with_url(description, url));
        self
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
    /// let details = ErrorDetails::new()
    ///     .with_localized_message(LocalizedMessage::with_locale("fr-FR", "introuvable"));
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

    /// Sets `description` on this resource info.
    ///
    /// Chain after [`Self::with_resource`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::resource_info`].
    /// Distinct from [`Self::with_resource`]: that is type, name, and owner, not a resource description.
    /// Distinct from [`crate::Status::message`]: that is the ASCII `grpc-message`, not a resource description.
    /// Distinct from [`crate::Status::debug_info`]: that is an operator stack, not a resource description.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ResourceInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     resource_info: Some(
    ///         ResourceInfo::with_resource(
    ///             "sqladmin.googleapis.com/Instance",
    ///             "projects/1/instances/a",
    ///             "project:1",
    ///         )
    ///         .with_description("Cloud SQL instance"),
    ///     ),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::NotFound, "gone", &details)?;
    /// let info = status.resource_info().expect("ResourceInfo");
    /// assert_eq!(
    ///     info.description().to_str().unwrap_or(""),
    ///     "Cloud SQL instance"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.set_description(description.into());
        self
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

    /// Pushes another stack `entry` onto this debug info.
    ///
    /// Chain after [`Self::with_stack`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::debug_info`].
    /// Distinct from [`Self::with_stack`]: that is the first frame and detail, not an extra stack frame.
    /// Distinct from [`crate::Status::localized_message`]: that is a locale, not an extra stack frame.
    /// Distinct from [`crate::Status::help`]: that is a docs URL, not an extra stack frame.
    ///
    /// ```
    /// use pbrs_grpc::pb::{DebugInfo, ErrorDetails};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     debug_info: Some(
    ///         DebugInfo::with_stack("handler.rs:9", "nil pointer").with_stack_entry("rpc.rs:4"),
    ///     ),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::Internal, "boom", &details)?;
    /// let debug = status.debug_info().expect("DebugInfo");
    /// assert_eq!(
    ///     debug
    ///         .stack_entries()
    ///         .get(1)
    ///         .expect("frame")
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "rpc.rs:4"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_stack_entry(mut self, entry: impl Into<String>) -> Self {
        self.stack_entries_mut().push(entry.into());
        self
    }
}

impl Status {
    /// A `google.rpc.Status` with `code`, `message`, and packed `details`.
    ///
    /// Distinct from [`crate::Status::with_details`]: that ships raw trailer bytes; this builds a packed `google.rpc.Status`.
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
/// let details = ErrorDetails::new()
///     .with_error_info(ErrorInfo::with_reason("API_DISABLED", "example.com"));
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
    /// Distinct from [`Self::from_rpc`]: that unpacks the `Any` list on a packed `google.rpc.Status`; this is an empty bag.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Plants packed [`ErrorInfo`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::error_info`].
    /// Distinct from [`ErrorInfo::with_reason`]: that is reason and domain, not planting ErrorInfo on the bag.
    /// Distinct from [`crate::Status::error_info`]: that unpacks packed ErrorInfo; this plants it on the bag.
    /// Distinct from [`Self::from_rpc`]: that unpacks the Any list; this plants one typed ErrorInfo.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_error_info(ErrorInfo::with_reason("RATE_LIMITED", "example.com"));
    /// let status = Status::from_error_details(Code::ResourceExhausted, "limited", &details)?;
    /// let info = status.error_info().expect("ErrorInfo");
    /// assert_eq!(info.reason().to_str().unwrap_or(""), "RATE_LIMITED");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_error_info(mut self, error_info: ErrorInfo) -> Self {
        self.error_info = Some(error_info);
        self
    }

    /// Plants packed [`RetryInfo`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::retry_delay`].
    /// Distinct from [`RetryInfo::with_retry_delay`]: that is a wait hint, not planting RetryInfo on the bag.
    /// Distinct from [`crate::Status::retry_delay`]: that unpacks the wait hint; this plants RetryInfo on the bag.
    /// Distinct from [`Self::with_error_info`]: that plants ErrorInfo, not RetryInfo.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, RetryInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new().with_retry_info(RetryInfo::with_retry_delay(
    ///     std::time::Duration::from_millis(250),
    /// ));
    /// let status = Status::from_error_details(Code::Unavailable, "backend", &details)?;
    /// assert_eq!(
    ///     status.retry_delay(),
    ///     Some(std::time::Duration::from_millis(250))
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_retry_info(mut self, retry_info: RetryInfo) -> Self {
        self.retry_info = Some(retry_info);
        self
    }

    /// Plants packed [`DebugInfo`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::debug_info`].
    /// Distinct from [`DebugInfo::with_stack`]: that is the first frame and detail, not planting DebugInfo on the bag.
    /// Distinct from [`crate::Status::debug_info`]: that unpacks packed DebugInfo; this plants it on the bag.
    /// Distinct from [`Self::with_retry_info`]: that plants RetryInfo, not DebugInfo.
    ///
    /// ```
    /// use pbrs_grpc::pb::{DebugInfo, ErrorDetails};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_debug_info(DebugInfo::with_stack("rpc.rs:4", "deadline"));
    /// let status = Status::from_error_details(Code::Internal, "boom", &details)?;
    /// let debug = status.debug_info().expect("DebugInfo");
    /// assert_eq!(debug.detail().to_str().unwrap_or(""), "deadline");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_debug_info(mut self, debug_info: DebugInfo) -> Self {
        self.debug_info = Some(debug_info);
        self
    }

    /// Plants packed [`QuotaFailure`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::quota_failure`].
    /// Distinct from [`QuotaFailure::with_violation`]: that is the first quota subject, not planting QuotaFailure on the bag.
    /// Distinct from [`crate::Status::quota_failure`]: that unpacks packed QuotaFailure; this plants it on the bag.
    /// Distinct from [`Self::with_debug_info`]: that plants DebugInfo, not QuotaFailure.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, QuotaFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_quota_failure(QuotaFailure::with_violation("client:3", "qps"));
    /// let status = Status::from_error_details(Code::ResourceExhausted, "over", &details)?;
    /// let quota = status.quota_failure().expect("QuotaFailure");
    /// let subject = quota.violations().get(0).expect("subject");
    /// assert_eq!(subject.subject().to_str().unwrap_or(""), "client:3");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_quota_failure(mut self, quota_failure: QuotaFailure) -> Self {
        self.quota_failure = Some(quota_failure);
        self
    }

    /// Plants packed [`PreconditionFailure`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::precondition_failure`].
    /// Distinct from [`PreconditionFailure::with_violation`]: that is the first precondition type, not planting PreconditionFailure on the bag.
    /// Distinct from [`crate::Status::precondition_failure`]: that unpacks packed PreconditionFailure; this plants it on the bag.
    /// Distinct from [`Self::with_quota_failure`]: that plants QuotaFailure, not PreconditionFailure.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, PreconditionFailure};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new().with_precondition_failure(
    ///     PreconditionFailure::with_violation("IAM", "user:3", "expired"),
    /// );
    /// let status = Status::from_error_details(Code::FailedPrecondition, "denied", &details)?;
    /// let pre = status.precondition_failure().expect("PreconditionFailure");
    /// let violation = pre.violations().get(0).expect("violation");
    /// assert_eq!(violation.r#type().to_str().unwrap_or(""), "IAM");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_precondition_failure(mut self, precondition_failure: PreconditionFailure) -> Self {
        self.precondition_failure = Some(precondition_failure);
        self
    }

    /// Plants packed [`BadRequest`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::bad_request`].
    /// Distinct from [`BadRequest::with_field`]: that is the first field path, not planting BadRequest on the bag.
    /// Distinct from [`crate::Status::bad_request`]: that unpacks packed BadRequest; this plants it on the bag.
    /// Distinct from [`Self::with_precondition_failure`]: that plants PreconditionFailure, not BadRequest.
    ///
    /// ```
    /// use pbrs_grpc::pb::{BadRequest, ErrorDetails};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_bad_request(BadRequest::with_field("sku", "unknown"));
    /// let status = Status::from_error_details(Code::InvalidArgument, "bad", &details)?;
    /// let bad = status.bad_request().expect("BadRequest");
    /// let field = bad.field_violations().get(0).expect("field");
    /// assert_eq!(field.field().to_str().unwrap_or(""), "sku");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_bad_request(mut self, bad_request: BadRequest) -> Self {
        self.bad_request = Some(bad_request);
        self
    }

    /// Plants packed [`RequestInfo`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::request_info`].
    /// Distinct from [`RequestInfo::with_request_id`]: that is request_id and serving_data, not planting RequestInfo on the bag.
    /// Distinct from [`crate::Status::request_info`]: that unpacks packed RequestInfo; this plants it on the bag.
    /// Distinct from [`Self::with_bad_request`]: that plants BadRequest, not RequestInfo.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, RequestInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_request_info(RequestInfo::with_request_id("req-9", "encrypted"));
    /// let status = Status::from_error_details(Code::Internal, "boom", &details)?;
    /// let info = status.request_info().expect("RequestInfo");
    /// assert_eq!(info.request_id().to_str().unwrap_or(""), "req-9");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_request_info(mut self, request_info: RequestInfo) -> Self {
        self.request_info = Some(request_info);
        self
    }

    /// Plants packed [`ResourceInfo`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::resource_info`].
    /// Distinct from [`ResourceInfo::with_resource`]: that is type, name, and owner, not planting ResourceInfo on the bag.
    /// Distinct from [`crate::Status::resource_info`]: that unpacks packed ResourceInfo; this plants it on the bag.
    /// Distinct from [`Self::with_request_info`]: that plants RequestInfo, not ResourceInfo.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ResourceInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new().with_resource_info(ResourceInfo::with_resource(
    ///     "sqladmin.googleapis.com/Instance",
    ///     "projects/1/instances/a",
    ///     "project:1",
    /// ));
    /// let status = Status::from_error_details(Code::NotFound, "gone", &details)?;
    /// let info = status.resource_info().expect("ResourceInfo");
    /// assert_eq!(
    ///     info.resource_name().to_str().unwrap_or(""),
    ///     "projects/1/instances/a"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_resource_info(mut self, resource_info: ResourceInfo) -> Self {
        self.resource_info = Some(resource_info);
        self
    }

    /// Plants packed [`Help`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::help`].
    /// Distinct from [`Help::with_link`]: that is the first docs URL, not planting Help on the bag.
    /// Distinct from [`crate::Status::help`]: that unpacks packed Help; this plants it on the bag.
    /// Distinct from [`Self::with_resource_info`]: that plants ResourceInfo, not Help.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, Help};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_help(Help::with_link("quota docs", "https://example.com/quota"));
    /// let status = Status::from_error_details(Code::Unavailable, "backend", &details)?;
    /// assert!(status.is_retryable());
    /// let help = status.help().expect("Help");
    /// let link = help.links().get(0).expect("link");
    /// assert_eq!(
    ///     link.url().to_str().unwrap_or(""),
    ///     "https://example.com/quota"
    /// );
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_help(mut self, help: Help) -> Self {
        self.help = Some(help);
        self
    }

    /// Plants packed [`LocalizedMessage`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack with [`crate::Status::localized_message`].
    /// Distinct from [`LocalizedMessage::with_locale`]: that is locale and message, not planting LocalizedMessage on the bag.
    /// Distinct from [`crate::Status::localized_message`]: that unpacks packed LocalizedMessage; this plants it on the bag.
    /// Distinct from [`Self::with_help`]: that plants Help, not LocalizedMessage.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, LocalizedMessage};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails::new()
    ///     .with_localized_message(LocalizedMessage::with_locale("fr-FR", "introuvable"));
    /// let status = Status::from_error_details(Code::NotFound, "not found", &details)?;
    /// let local = status.localized_message().expect("LocalizedMessage");
    /// assert_eq!(local.locale().to_str().unwrap_or(""), "fr-FR");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_localized_message(mut self, localized_message: LocalizedMessage) -> Self {
        self.localized_message = Some(localized_message);
        self
    }

    /// Plants one non-standard [`Any`] on this bag.
    ///
    /// Chain after [`Self::new`]. Packed onto a status with
    /// [`crate::Status::from_error_details`]; unpack from
    /// [`crate::Status::error_details`] `.unknown`. Standard types belong on
    /// the typed fields; a first packed ErrorInfo still re-homes there on decode.
    /// Distinct from [`Any::pack`]: that packs one message into an Any, not planting it on the bag.
    /// Distinct from [`crate::Status::error_details`]: that unpacks the bag; this plants one unknown Any.
    /// Distinct from [`Self::with_localized_message`]: that plants LocalizedMessage, not an unknown Any.
    ///
    /// ```
    /// use pbrs_grpc::pb::{Any, ErrorDetails};
    /// use pbrs_grpc::{Code, HelloRequest, Status};
    ///
    /// let mut extra = HelloRequest::new();
    /// extra.set_name("custom");
    /// let details = ErrorDetails::new().with_unknown(Any::pack(&extra)?);
    /// let status = Status::from_error_details(Code::FailedPrecondition, "disabled", &details)?;
    /// let bag = status.error_details()?;
    /// let hello = bag.unknown.first().expect("custom Any").unpack::<HelloRequest>()?;
    /// assert_eq!(hello.name().to_str().unwrap_or(""), "custom");
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn with_unknown(mut self, unknown: Any) -> Self {
        self.unknown.push(unknown);
        self
    }

    /// Encode every populated field as `google.protobuf.Any`, standard
    /// types first, then [`Self::unknown`].
    ///
    /// Distinct from [`crate::Status::from_error_details`]: that encodes the bag as a trailer; this returns the `Any` list.
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
    /// Distinct from [`crate::Status::error_details`]: that unpacks `grpc-status-details-bin` on a kernel Status; this unpacks the `Any` list on a packed `google.rpc.Status`.
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
    fn error_details_with_error_info_round_trips() {
        let details = ErrorDetails::new()
            .with_error_info(ErrorInfo::with_reason("RATE_LIMITED", "example.com"));
        let status =
            crate::Status::from_error_details(Code::ResourceExhausted, "limited", &details)
                .expect("encode");
        let got = status.error_info().expect("ErrorInfo");
        assert_eq!(got.reason().to_str().unwrap_or(""), "RATE_LIMITED");
        assert_eq!(got.domain().to_str().unwrap_or(""), "example.com");
        assert!(status.retry_delay().is_none());
        assert!(status.bad_request().is_none());
    }

    #[test]
    fn error_details_with_retry_info_round_trips() {
        let details = ErrorDetails::new().with_retry_info(RetryInfo::with_retry_delay(
            std::time::Duration::from_millis(250),
        ));
        let status = crate::Status::from_error_details(Code::Unavailable, "backend", &details)
            .expect("encode");
        assert_eq!(
            status.retry_delay(),
            Some(std::time::Duration::from_millis(250))
        );
        assert!(status.error_info().is_none());
        assert!(status.bad_request().is_none());
    }

    #[test]
    fn error_details_with_debug_info_round_trips() {
        let details =
            ErrorDetails::new().with_debug_info(DebugInfo::with_stack("rpc.rs:4", "deadline"));
        let status =
            crate::Status::from_error_details(Code::Internal, "boom", &details).expect("encode");
        let got = status.debug_info().expect("DebugInfo");
        assert_eq!(got.detail().to_str().unwrap_or(""), "deadline");
        assert_eq!(
            got.stack_entries()
                .get(0)
                .expect("frame")
                .to_str()
                .unwrap_or(""),
            "rpc.rs:4"
        );
        assert!(status.retry_delay().is_none());
        assert!(status.help().is_none());
    }

    #[test]
    fn error_details_with_quota_failure_round_trips() {
        let details =
            ErrorDetails::new().with_quota_failure(QuotaFailure::with_violation("client:3", "qps"));
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "over", &details)
            .expect("encode");
        assert!(!status.is_retryable());
        let got = status.quota_failure().expect("QuotaFailure");
        let subject = got.violations().get(0).expect("subject");
        assert_eq!(subject.subject().to_str().unwrap_or(""), "client:3");
        assert_eq!(subject.description().to_str().unwrap_or(""), "qps");
        assert!(status.debug_info().is_none());
        assert!(status.bad_request().is_none());
    }

    #[test]
    fn error_details_with_precondition_failure_round_trips() {
        let details = ErrorDetails::new().with_precondition_failure(
            PreconditionFailure::with_violation("IAM", "user:3", "expired"),
        );
        let status =
            crate::Status::from_error_details(Code::FailedPrecondition, "denied", &details)
                .expect("encode");
        assert!(!status.is_retryable());
        let got = status.precondition_failure().expect("PreconditionFailure");
        let violation = got.violations().get(0).expect("violation");
        assert_eq!(violation.r#type().to_str().unwrap_or(""), "IAM");
        assert_eq!(violation.subject().to_str().unwrap_or(""), "user:3");
        assert_eq!(violation.description().to_str().unwrap_or(""), "expired");
        assert!(status.quota_failure().is_none());
        assert!(status.help().is_none());
    }

    #[test]
    fn error_details_with_bad_request_round_trips() {
        let details =
            ErrorDetails::new().with_bad_request(BadRequest::with_field("sku", "unknown"));
        let status = crate::Status::from_error_details(Code::InvalidArgument, "bad", &details)
            .expect("encode");
        let got = status.bad_request().expect("BadRequest");
        let field = got.field_violations().get(0).expect("field");
        assert_eq!(field.field().to_str().unwrap_or(""), "sku");
        assert_eq!(field.description().to_str().unwrap_or(""), "unknown");
        assert!(status.precondition_failure().is_none());
        assert!(status.error_info().is_none());
    }

    #[test]
    fn error_details_with_request_info_round_trips() {
        let details = ErrorDetails::new()
            .with_request_info(RequestInfo::with_request_id("req-9", "encrypted"));
        let status =
            crate::Status::from_error_details(Code::Internal, "boom", &details).expect("encode");
        let got = status.request_info().expect("RequestInfo");
        assert_eq!(got.request_id().to_str().unwrap_or(""), "req-9");
        assert_eq!(got.serving_data().to_str().unwrap_or(""), "encrypted");
        assert!(status.bad_request().is_none());
        assert!(status.error_info().is_none());
    }

    #[test]
    fn error_details_with_resource_info_round_trips() {
        let details = ErrorDetails::new().with_resource_info(ResourceInfo::with_resource(
            "sqladmin.googleapis.com/Instance",
            "projects/1/instances/a",
            "project:1",
        ));
        let status =
            crate::Status::from_error_details(Code::NotFound, "gone", &details).expect("encode");
        let got = status.resource_info().expect("ResourceInfo");
        assert_eq!(
            got.resource_name().to_str().unwrap_or(""),
            "projects/1/instances/a"
        );
        assert!(status.request_info().is_none());
        assert!(status.quota_failure().is_none());
    }

    #[test]
    fn error_details_with_help_round_trips() {
        let details = ErrorDetails::new()
            .with_help(Help::with_link("quota docs", "https://example.com/quota"));
        let status = crate::Status::from_error_details(Code::Unavailable, "backend", &details)
            .expect("encode");
        assert!(status.is_retryable());
        let got = status.help().expect("Help");
        let link = got.links().get(0).expect("link");
        assert_eq!(
            link.url().to_str().unwrap_or(""),
            "https://example.com/quota"
        );
        assert!(status.resource_info().is_none());
        assert!(status.localized_message().is_none());
    }

    #[test]
    fn error_details_with_localized_message_round_trips() {
        let details = ErrorDetails::new()
            .with_localized_message(LocalizedMessage::with_locale("fr-FR", "introuvable"));
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
    fn error_details_with_unknown_round_trips() {
        let mut extra = crate::HelloRequest::new();
        extra.set_name("custom");
        let details = ErrorDetails::new().with_unknown(Any::pack(&extra).expect("pack hello"));
        let status =
            crate::Status::from_error_details(Code::FailedPrecondition, "disabled", &details)
                .expect("encode");
        let bag = status.error_details().expect("bag");
        assert_eq!(bag.unknown.len(), 1);
        let hello = bag
            .unknown
            .first()
            .expect("custom Any")
            .unpack::<crate::HelloRequest>()
            .expect("hello");
        assert_eq!(hello.name().to_str().unwrap_or(""), "custom");
        assert!(status.localized_message().is_none());
        assert!(status.error_info().is_none());
    }

    #[test]
    fn error_info_with_metadata_round_trips() {
        let info = ErrorInfo::with_reason("API_DISABLED", "example.com")
            .with_metadata("resource", "projects/123");
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
        let resource = got
            .metadata()
            .get("resource")
            .and_then(|s| s.to_str().ok().map(str::to_owned));
        assert_eq!(resource.as_deref(), Some("projects/123"));
        assert!(status.request_info().is_none());
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
    fn bad_request_with_field_entry_round_trips() {
        let bad = BadRequest::with_field("name", "required").with_field_entry("email", "invalid");
        assert_eq!(
            bad.field_violations()
                .get(0)
                .expect("first")
                .field()
                .to_str()
                .unwrap_or(""),
            "name"
        );
        assert_eq!(
            bad.field_violations()
                .get(1)
                .expect("second")
                .field()
                .to_str()
                .unwrap_or(""),
            "email"
        );
        let details = ErrorDetails {
            bad_request: Some(bad),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::InvalidArgument, "bad", &details)
            .expect("encode");
        let got = status.bad_request().expect("BadRequest");
        let field = got.field_violations().get(1).expect("field");
        assert_eq!(field.field().to_str().unwrap_or(""), "email");
        assert!(status.error_info().is_none());
        assert!(status.help().is_none());
    }

    #[test]
    fn field_violation_with_reason_round_trips() {
        let violation = FieldViolation::with_field("name", "required").with_reason("REQUIRED");
        assert_eq!(violation.field().to_str().unwrap_or(""), "name");
        assert_eq!(violation.description().to_str().unwrap_or(""), "required");
        assert_eq!(violation.reason().to_str().unwrap_or(""), "REQUIRED");
        let mut bad = BadRequest::new();
        bad.set_field_violations([violation]);
        let details = ErrorDetails {
            bad_request: Some(bad),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::InvalidArgument, "bad", &details)
            .expect("encode");
        let got = status.bad_request().expect("BadRequest");
        let field = got.field_violations().get(0).expect("field");
        assert_eq!(field.reason().to_str().unwrap_or(""), "REQUIRED");
        assert!(field.localized_message_opt().is_none());
        assert!(status.error_info().is_none());
    }

    #[test]
    fn field_violation_with_localized_message_round_trips() {
        let violation = FieldViolation::with_field("name", "required")
            .with_localized_message(LocalizedMessage::with_locale("fr-FR", "requis"));
        assert_eq!(violation.field().to_str().unwrap_or(""), "name");
        let local = violation.localized_message_opt().expect("locale");
        assert_eq!(local.locale().to_str().unwrap_or(""), "fr-FR");
        assert_eq!(local.message().to_str().unwrap_or(""), "requis");
        let mut bad = BadRequest::new();
        bad.set_field_violations([violation]);
        let details = ErrorDetails {
            bad_request: Some(bad),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::InvalidArgument, "bad", &details)
            .expect("encode");
        let got = status.bad_request().expect("BadRequest");
        let field = got.field_violations().get(0).expect("field");
        let local = field.localized_message_opt().expect("locale");
        assert_eq!(local.locale().to_str().unwrap_or(""), "fr-FR");
        assert_eq!(local.message().to_str().unwrap_or(""), "requis");
        assert!(status.localized_message().is_none());
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
    fn quota_failure_with_violation_entry_round_trips() {
        let packed = QuotaFailure::with_violation("project:1", "tokens")
            .with_violation_entry("client:9", "qps");
        assert_eq!(
            packed
                .violations()
                .get(0)
                .expect("first")
                .subject()
                .to_str()
                .unwrap_or(""),
            "project:1"
        );
        assert_eq!(
            packed
                .violations()
                .get(1)
                .expect("second")
                .subject()
                .to_str()
                .unwrap_or(""),
            "client:9"
        );
        let details = ErrorDetails {
            quota_failure: Some(packed),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "quota", &details)
            .expect("encode");
        assert!(!status.is_retryable());
        let got = status.quota_failure().expect("QuotaFailure");
        let extra = got.violations().get(1).expect("subject");
        assert_eq!(extra.subject().to_str().unwrap_or(""), "client:9");
        assert_eq!(extra.description().to_str().unwrap_or(""), "qps");
        assert!(status.bad_request().is_none());
        assert!(status.help().is_none());
    }

    #[test]
    fn quota_failure_with_api_service_round_trips() {
        let violation = quota_failure::Violation::with_subject("project:1", "tokens")
            .with_api_service("compute.googleapis.com");
        assert_eq!(violation.subject().to_str().unwrap_or(""), "project:1");
        assert_eq!(violation.description().to_str().unwrap_or(""), "tokens");
        assert_eq!(
            violation.api_service().to_str().unwrap_or(""),
            "compute.googleapis.com"
        );
        let mut quota = QuotaFailure::new();
        quota.set_violations([violation]);
        let details = ErrorDetails {
            quota_failure: Some(quota),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "quota", &details)
            .expect("encode");
        let got = status.quota_failure().expect("QuotaFailure");
        let subject = got.violations().get(0).expect("subject");
        assert_eq!(subject.subject().to_str().unwrap_or(""), "project:1");
        assert_eq!(
            subject.api_service().to_str().unwrap_or(""),
            "compute.googleapis.com"
        );
        assert!(status.error_info().is_none());
        assert!(status.bad_request().is_none());
    }

    #[test]
    fn quota_failure_with_quota_metric_round_trips() {
        let violation = quota_failure::Violation::with_subject("project:1", "tokens")
            .with_quota_metric("compute.googleapis.com/cpus");
        assert_eq!(violation.subject().to_str().unwrap_or(""), "project:1");
        assert_eq!(
            violation.quota_metric().to_str().unwrap_or(""),
            "compute.googleapis.com/cpus"
        );
        let mut quota = QuotaFailure::new();
        quota.set_violations([violation]);
        let details = ErrorDetails {
            quota_failure: Some(quota),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "quota", &details)
            .expect("encode");
        let got = status.quota_failure().expect("QuotaFailure");
        let subject = got.violations().get(0).expect("subject");
        assert_eq!(
            subject.quota_metric().to_str().unwrap_or(""),
            "compute.googleapis.com/cpus"
        );
        assert!(subject.api_service().to_str().unwrap_or("").is_empty());
        assert!(status.error_info().is_none());
    }

    #[test]
    fn quota_failure_with_quota_id_round_trips() {
        let violation = quota_failure::Violation::with_subject("project:1", "tokens")
            .with_quota_id("CPUS-PER-PROJECT");
        assert_eq!(violation.subject().to_str().unwrap_or(""), "project:1");
        assert_eq!(
            violation.quota_id().to_str().unwrap_or(""),
            "CPUS-PER-PROJECT"
        );
        let mut quota = QuotaFailure::new();
        quota.set_violations([violation]);
        let details = ErrorDetails {
            quota_failure: Some(quota),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "quota", &details)
            .expect("encode");
        let got = status.quota_failure().expect("QuotaFailure");
        let subject = got.violations().get(0).expect("subject");
        assert_eq!(
            subject.quota_id().to_str().unwrap_or(""),
            "CPUS-PER-PROJECT"
        );
        assert!(subject.quota_metric().to_str().unwrap_or("").is_empty());
        assert!(status.error_info().is_none());
    }

    #[test]
    fn quota_failure_with_quota_dimension_round_trips() {
        let violation = quota_failure::Violation::with_subject("project:1", "tokens")
            .with_quota_dimension("region", "us-central1");
        let region = violation
            .quota_dimensions()
            .get("region")
            .and_then(|s| s.to_str().ok().map(str::to_owned));
        assert_eq!(region.as_deref(), Some("us-central1"));
        let mut quota = QuotaFailure::new();
        quota.set_violations([violation]);
        let details = ErrorDetails {
            quota_failure: Some(quota),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "quota", &details)
            .expect("encode");
        let got = status.quota_failure().expect("QuotaFailure");
        let region = got
            .violations()
            .get(0)
            .expect("subject")
            .quota_dimensions()
            .get("region")
            .and_then(|s| s.to_str().ok().map(str::to_owned));
        assert_eq!(region.as_deref(), Some("us-central1"));
        assert!(status.error_info().is_none());
    }

    #[test]
    fn quota_failure_with_quota_value_round_trips() {
        let violation =
            quota_failure::Violation::with_subject("project:1", "tokens").with_quota_value(8);
        assert_eq!(violation.subject().to_str().unwrap_or(""), "project:1");
        assert_eq!(violation.quota_value(), 8);
        let mut quota = QuotaFailure::new();
        quota.set_violations([violation]);
        let details = ErrorDetails {
            quota_failure: Some(quota),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "quota", &details)
            .expect("encode");
        let got = status.quota_failure().expect("QuotaFailure");
        let subject = got.violations().get(0).expect("subject");
        assert_eq!(subject.quota_value(), 8);
        assert!(!subject.has_future_quota_value());
        assert!(status.retry_delay().is_none());
        assert!(status.error_info().is_none());
    }

    #[test]
    fn quota_failure_with_future_quota_value_round_trips() {
        let violation = quota_failure::Violation::with_subject("project:1", "tokens")
            .with_future_quota_value(16);
        assert_eq!(violation.subject().to_str().unwrap_or(""), "project:1");
        assert!(violation.has_future_quota_value());
        assert_eq!(violation.future_quota_value(), 16);
        assert_eq!(violation.quota_value(), 0);
        let mut quota = QuotaFailure::new();
        quota.set_violations([violation]);
        let details = ErrorDetails {
            quota_failure: Some(quota),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::ResourceExhausted, "quota", &details)
            .expect("encode");
        let got = status.quota_failure().expect("QuotaFailure");
        let subject = got.violations().get(0).expect("subject");
        assert!(subject.has_future_quota_value());
        assert_eq!(subject.future_quota_value(), 16);
        assert_eq!(subject.quota_value(), 0);
        assert!(status.retry_delay().is_none());
        assert!(status.error_info().is_none());
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
    fn precondition_failure_with_violation_entry_round_trips() {
        let packed = PreconditionFailure::with_violation("TOS", "google.com/cloud", "unsigned")
            .with_violation_entry("googleapis.com/iam/resource", "user:9", "missing");
        assert_eq!(
            packed
                .violations()
                .get(0)
                .expect("first")
                .r#type()
                .to_str()
                .unwrap_or(""),
            "TOS"
        );
        assert_eq!(
            packed
                .violations()
                .get(1)
                .expect("second")
                .r#type()
                .to_str()
                .unwrap_or(""),
            "googleapis.com/iam/resource"
        );
        let details = ErrorDetails {
            precondition_failure: Some(packed),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::FailedPrecondition, "tos", &details)
            .expect("encode");
        assert!(!status.is_retryable());
        let got = status.precondition_failure().expect("PreconditionFailure");
        let extra = got.violations().get(1).expect("violation");
        assert_eq!(
            extra.r#type().to_str().unwrap_or(""),
            "googleapis.com/iam/resource"
        );
        assert_eq!(extra.subject().to_str().unwrap_or(""), "user:9");
        assert_eq!(extra.description().to_str().unwrap_or(""), "missing");
        assert!(status.quota_failure().is_none());
        assert!(status.help().is_none());
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
    fn help_with_link_entry_round_trips() {
        let packed = Help::with_link("quota docs", "https://example.com/quota")
            .with_link_entry("retry", "https://example.com/retry");
        assert_eq!(
            packed
                .links()
                .get(0)
                .expect("first")
                .url()
                .to_str()
                .unwrap_or(""),
            "https://example.com/quota"
        );
        assert_eq!(
            packed
                .links()
                .get(1)
                .expect("second")
                .url()
                .to_str()
                .unwrap_or(""),
            "https://example.com/retry"
        );
        let details = ErrorDetails {
            help: Some(packed),
            ..ErrorDetails::default()
        };
        let status = crate::Status::from_error_details(Code::Unavailable, "backend", &details)
            .expect("encode");
        let got = status.help().expect("Help");
        let link = got.links().get(1).expect("link");
        assert_eq!(
            link.url().to_str().unwrap_or(""),
            "https://example.com/retry"
        );
        assert!(status.localized_message().is_none());
        assert!(status.precondition_failure().is_none());
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
    fn resource_info_with_description_round_trips() {
        let info = ResourceInfo::with_resource(
            "sqladmin.googleapis.com/Instance",
            "projects/1/instances/a",
            "project:1",
        )
        .with_description("Cloud SQL instance");
        assert_eq!(
            info.resource_name().to_str().unwrap_or(""),
            "projects/1/instances/a"
        );
        assert_eq!(
            info.description().to_str().unwrap_or(""),
            "Cloud SQL instance"
        );
        let details = ErrorDetails {
            resource_info: Some(info),
            ..ErrorDetails::default()
        };
        let status =
            crate::Status::from_error_details(Code::NotFound, "gone", &details).expect("encode");
        let got = status.resource_info().expect("ResourceInfo");
        assert_eq!(
            got.description().to_str().unwrap_or(""),
            "Cloud SQL instance"
        );
        assert!(status.debug_info().is_none());
        assert!(status.help().is_none());
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

    #[test]
    fn debug_info_with_stack_entry_round_trips() {
        let debug =
            DebugInfo::with_stack("handler.rs:9", "nil pointer").with_stack_entry("rpc.rs:4");
        assert_eq!(
            debug
                .stack_entries()
                .get(0)
                .expect("frame")
                .to_str()
                .unwrap_or(""),
            "handler.rs:9"
        );
        assert_eq!(
            debug
                .stack_entries()
                .get(1)
                .expect("frame")
                .to_str()
                .unwrap_or(""),
            "rpc.rs:4"
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
                .get(1)
                .expect("frame")
                .to_str()
                .unwrap_or(""),
            "rpc.rs:4"
        );
        assert!(status.localized_message().is_none());
        assert!(status.help().is_none());
    }
}
