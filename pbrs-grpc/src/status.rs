//! gRPC status: [`Code`], `grpc-message`, trailing metadata, and
//! `grpc-status-details-bin`.

use crate::metadata::Metadata;
use bytes::Bytes;
use std::fmt;
use std::sync::{Arc, OnceLock};

/// A `grpc-status` code.
///
/// The numeric values are fixed by the gRPC specification and are what travels
/// on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum Code {
    /// Success.
    Ok = 0,
    /// The operation was cancelled, typically by the caller.
    Cancelled = 1,
    /// Unknown error, or a status with no recognised code.
    Unknown = 2,
    /// The caller specified an invalid argument.
    InvalidArgument = 3,
    /// The deadline expired before the operation completed.
    DeadlineExceeded = 4,
    /// A requested entity was not found.
    NotFound = 5,
    /// The entity a caller tried to create already exists.
    AlreadyExists = 6,
    /// The caller is authenticated but lacks permission.
    PermissionDenied = 7,
    /// A resource has been exhausted, such as a per-message size cap.
    ResourceExhausted = 8,
    /// The system is not in the state the operation requires.
    FailedPrecondition = 9,
    /// The operation was aborted, typically by a concurrency conflict.
    Aborted = 10,
    /// The operation was attempted past the valid range.
    OutOfRange = 11,
    /// The operation is not implemented or not supported.
    Unimplemented = 12,
    /// An internal invariant was broken.
    Internal = 13,
    /// The service is currently unavailable; retrying may succeed.
    Unavailable = 14,
    /// Unrecoverable data loss or corruption.
    DataLoss = 15,
    /// The caller could not be authenticated.
    Unauthenticated = 16,
}

impl Code {
    /// Interpret a wire value. Unrecognised codes become [`Code::Unknown`],
    /// as the specification requires.
    /// Distinct from [`Self::to_i32`]: that emits the wire i32; this interprets a wire i32.
    #[must_use]
    pub fn from_i32(n: i32) -> Self {
        match n {
            0 => Self::Ok,
            1 => Self::Cancelled,
            3 => Self::InvalidArgument,
            4 => Self::DeadlineExceeded,
            5 => Self::NotFound,
            6 => Self::AlreadyExists,
            7 => Self::PermissionDenied,
            8 => Self::ResourceExhausted,
            9 => Self::FailedPrecondition,
            10 => Self::Aborted,
            11 => Self::OutOfRange,
            12 => Self::Unimplemented,
            13 => Self::Internal,
            14 => Self::Unavailable,
            15 => Self::DataLoss,
            16 => Self::Unauthenticated,
            _ => Self::Unknown,
        }
    }

    /// The value used on the wire.
    #[must_use]
    pub fn to_i32(self) -> i32 {
        self as i32
    }

    /// The canonical `SCREAMING_SNAKE_CASE` spelling used across gRPC
    /// implementations and tooling.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Cancelled => "CANCELLED",
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Self::NotFound => "NOT_FOUND",
            Self::AlreadyExists => "ALREADY_EXISTS",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::FailedPrecondition => "FAILED_PRECONDITION",
            Self::Aborted => "ABORTED",
            Self::OutOfRange => "OUT_OF_RANGE",
            Self::Unimplemented => "UNIMPLEMENTED",
            Self::Internal => "INTERNAL",
            Self::Unavailable => "UNAVAILABLE",
            Self::DataLoss => "DATA_LOSS",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// One-line description from `google.rpc.Code`.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Ok => "The operation completed successfully",
            Self::Cancelled => "The operation was cancelled, typically by the caller",
            Self::Unknown => "Unknown error",
            Self::InvalidArgument => "The client specified an invalid argument",
            Self::DeadlineExceeded => "The deadline expired before the operation could complete",
            Self::NotFound => "Some requested entity was not found",
            Self::AlreadyExists => "The entity that a client attempted to create already exists",
            Self::PermissionDenied => {
                "The caller does not have permission to execute the specified operation"
            }
            Self::ResourceExhausted => "Some resource has been exhausted",
            Self::FailedPrecondition => {
                "The operation was rejected because the system is not in the required state"
            }
            Self::Aborted => "The operation was aborted, typically by a concurrency conflict",
            Self::OutOfRange => "The operation was attempted past the valid range",
            Self::Unimplemented => "The operation is not implemented or is not supported/enabled",
            Self::Internal => "Internal errors",
            Self::Unavailable => "The service is currently unavailable",
            Self::DataLoss => "Unrecoverable data loss or corruption",
            Self::Unauthenticated => "The request does not have valid authentication credentials",
        }
    }

    /// gRPC A6 default retryable set: [`Self::Unavailable`] only.
    ///
    /// Distinct from a packed [`crate::pb::RetryInfo`] delay, which is a
    /// server wait hint and does not enlarge this set. Distinct from
    /// transparent retry of HTTP/2 connection death (already one redial).
    /// [`Self::ResourceExhausted`] is not retryable: a
    /// [`crate::Channel::max_concurrent_rpcs`] refusal would loop, and a
    /// quota trailer should wait [`Status::retry_delay`] instead.
    #[must_use]
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The string was not a canonical gRPC code name or a code in `0..=16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseCodeError;

impl fmt::Display for ParseCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown grpc code")
    }
}

impl std::error::Error for ParseCodeError {}

impl std::str::FromStr for Code {
    type Err = ParseCodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "OK" => Ok(Self::Ok),
            "CANCELLED" => Ok(Self::Cancelled),
            "UNKNOWN" => Ok(Self::Unknown),
            "INVALID_ARGUMENT" => Ok(Self::InvalidArgument),
            "DEADLINE_EXCEEDED" => Ok(Self::DeadlineExceeded),
            "NOT_FOUND" => Ok(Self::NotFound),
            "ALREADY_EXISTS" => Ok(Self::AlreadyExists),
            "PERMISSION_DENIED" => Ok(Self::PermissionDenied),
            "RESOURCE_EXHAUSTED" => Ok(Self::ResourceExhausted),
            "FAILED_PRECONDITION" => Ok(Self::FailedPrecondition),
            "ABORTED" => Ok(Self::Aborted),
            "OUT_OF_RANGE" => Ok(Self::OutOfRange),
            "UNIMPLEMENTED" => Ok(Self::Unimplemented),
            "INTERNAL" => Ok(Self::Internal),
            "UNAVAILABLE" => Ok(Self::Unavailable),
            "DATA_LOSS" => Ok(Self::DataLoss),
            "UNAUTHENTICATED" => Ok(Self::Unauthenticated),
            _ => match s.parse::<i32>() {
                Ok(n) if (0..=16).contains(&n) => Ok(Self::from_i32(n)),
                _ => Err(ParseCodeError),
            },
        }
    }
}

/// The rarely-populated half of a [`Status`], boxed so `Result<T, Status>`
/// stays small on the hot path.
#[derive(Clone, Debug, Default)]
struct Detail {
    message: String,
    metadata: Metadata,
    details: Bytes,
    /// HTTP/2 connection died (GOAWAY, I/O, `REFUSED_STREAM`). Not a peer
    /// `UNAVAILABLE` trailer. Unary/server-streaming redial once.
    transport: bool,
    /// Local cause. Peer trailers leave this unset.
    source: Option<Arc<dyn std::error::Error + Send + Sync>>,
}

/// A gRPC status: a [`Code`], an optional message, optional trailing
/// metadata, and optional `grpc-status-details-bin`.
///
/// `Status` is the error type of every fallible operation in this crate, so it
/// is kept to two machine words. The message, metadata, and details live
/// behind a pointer that is only allocated when one of them is set, which
/// means the common `Ok` and bare-code cases allocate nothing.
///
/// It implements [`std::error::Error`]. Local I/O ([`std::io::Error`]),
/// a TLS handshake, and HTTP/2 connection death attach the original error
/// as [`std::error::Error::source`]. A peer trailer has no cause. Distinct
/// from [`Self::with_error_details`] (a packed `google.rpc.Status` on the
/// wire). [`Self::from_error`] wraps an arbitrary local error: an already-
/// [`Status`] (including in the source chain) is returned as-is.
/// [`Self::is_retryable`] is the gRPC A6 default ([`Code::Unavailable`]
/// only). Packed [`crate::pb::RetryInfo`] is [`Self::retry_delay`], a wait
/// hint, not a larger retryable set.
///
/// ```
/// use pbrs_grpc::{Code, Status};
///
/// let status = Status::not_found("no such row");
/// assert_eq!(status.code(), Code::NotFound);
/// assert_eq!(status.message(), "no such row");
/// assert_eq!(status.to_string(), "NOT_FOUND: no such row");
/// assert!(!status.is_retryable());
///
/// assert!(Code::Unavailable.is_retryable());
/// assert!(!Code::ResourceExhausted.is_retryable());
///
/// // Two words, whatever the payload.
/// assert!(std::mem::size_of::<Status>() <= 2 * std::mem::size_of::<usize>());
/// ```
///
/// Attaching metadata to an error puts it in the response trailers:
///
/// ```
/// use pbrs_grpc::{Code, Status};
///
/// let mut status = Status::resource_exhausted("quota exceeded");
/// status.metadata_mut().insert("x-retry-after", "30")?;
/// assert_eq!(status.metadata().get("x-retry-after"), Some("30"));
///
/// let rich = Status::with_details(Code::NotFound, "gone", vec![0x08, 0x05]);
/// assert_eq!(rich.details(), &[0x08, 0x05]);
/// # Ok::<(), Status>(())
/// ```
///
/// Structured details are a `google.rpc.Status` packed into that trailer.
/// [`Self::with_error_details`] builds one from [`crate::pb::Any`] values;
/// [`Self::rpc`] parses it back. [`Self::set_code`] / [`Self::set_message`]
/// rewrite a packed protobuf that still matches the ASCII trailers.
/// [`Self::set_rpc`] / [`Self::set_error_details`] / [`Self::set_from_error_details`] replace the protobuf
/// without dropping trailing metadata.
#[derive(Clone, Debug)]
pub struct Status {
    code: Code,
    detail: Option<Box<Detail>>,
}

/// Shared empty metadata, so [`Status::metadata`] can hand out a reference
/// without forcing an allocation on statuses that have none.
fn empty_metadata() -> &'static Metadata {
    static EMPTY: OnceLock<Metadata> = OnceLock::new();
    EMPTY.get_or_init(Metadata::new)
}

impl Status {
    /// A status with `code` and `message`, and no trailing metadata.
    /// Distinct from [`Self::from_code`]: that is code-only; this takes a code and message.
    #[must_use]
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        let message = message.into();
        let detail = if message.is_empty() {
            None
        } else {
            Some(Box::new(Detail {
                message,
                ..Detail::default()
            }))
        };
        Self { code, detail }
    }

    /// A status with just a code.
    /// Distinct from [`Self::new`]: that takes a code and message; this is code-only.
    #[must_use]
    pub fn from_code(code: Code) -> Self {
        Self { code, detail: None }
    }

    /// The status code.
    #[must_use]
    pub fn code(&self) -> Code {
        self.code
    }

    /// The `grpc-message` text, or `""`.
    #[must_use]
    pub fn message(&self) -> &str {
        self.detail.as_ref().map_or("", |d| d.message.as_str())
    }

    /// Replace the [`Code`]. Metadata is left alone. When
    /// `grpc-status-details-bin` holds a `google.rpc.Status` whose code
    /// matches this status, that protobuf is rewritten so the ASCII
    /// `grpc-status` and the packed code stay the same. Opaque detail
    /// bytes that are not a matching `google.rpc.Status` are left alone.
    /// Distinct from [`Self::with_code`]: that is the builder; this mutates in place.
    pub fn set_code(&mut self, code: Code) {
        if !self.details().is_empty() {
            if let Ok(mut rpc) = <crate::pb::Status as pbrs::Parse>::parse(self.details()) {
                if rpc.code() == self.code.to_i32() {
                    rpc.set_code(code.to_i32());
                    if let Ok(bytes) = pbrs::Serialize::serialize(&rpc) {
                        self.set_details(bytes);
                    }
                }
            }
        }
        self.code = code;
    }

    /// [`Self::set_code`] as a builder.
    /// Distinct from [`Self::set_code`]: that mutates in place; this is the builder.
    #[must_use]
    pub fn with_code(mut self, code: Code) -> Self {
        self.set_code(code);
        self
    }

    /// Replace the `grpc-message` text. Empty clears it. Metadata is left
    /// alone. When `grpc-status-details-bin` holds a `google.rpc.Status`
    /// whose message matches this status, that protobuf is rewritten so the
    /// ASCII trailer and the packed message stay the same. Opaque detail
    /// bytes that are not a matching `google.rpc.Status` are left alone.
    /// Distinct from [`Self::with_message`]: that is the builder; this mutates in place.
    pub fn set_message(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.details().is_empty() {
            if let Ok(mut rpc) = <crate::pb::Status as pbrs::Parse>::parse(self.details()) {
                let packed = rpc.message().to_str().unwrap_or("");
                if packed == self.message() {
                    rpc.set_message(message.clone());
                    if let Ok(bytes) = pbrs::Serialize::serialize(&rpc) {
                        self.set_details(bytes);
                    }
                }
            }
        }
        if message.is_empty() {
            if let Some(detail) = self.detail.as_mut() {
                detail.message.clear();
            }
            return;
        }
        self.detail.get_or_insert_with(Box::default).message = message;
    }

    /// [`Self::set_message`] as a builder.
    /// Distinct from [`Self::set_message`]: that mutates in place; this is the builder.
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.set_message(message);
        self
    }

    /// Trailing metadata carried with this status.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        match &self.detail {
            Some(detail) => &detail.metadata,
            None => empty_metadata(),
        }
    }

    /// Trailing metadata, allocating the detail block on first use.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.detail.get_or_insert_with(Box::default).metadata
    }

    /// Serialized `google.rpc.Status` (or any protobuf the peer understands)
    /// carried as `grpc-status-details-bin`. Empty when the trailer was absent.
    ///
    /// Prefer [`Self::rpc`] / [`Self::with_error_details`] when the payload
    /// is a `google.rpc.Status`. This returns the raw bytes so a proxy can
    /// forward a trailer it does not parse.
    ///
    /// Raw bytes still round-trip on every call shape, including over TLS,
    /// mTLS, Unix, and [`crate::Channel::from_io`]. They do not appear as a
    /// `grpc-status-details-bin` metadata key.
    /// Distinct from [`Self::rpc`]: that parses a packed `google.rpc.Status`; this returns raw trailer bytes.
    #[must_use]
    pub fn details(&self) -> &[u8] {
        self.detail.as_ref().map_or(&[], |d| d.details.as_ref())
    }

    /// Attach `details` as `grpc-status-details-bin`.
    ///
    /// The gRPC spec puts a serialized [`crate::pb::Status`] here: the same
    /// code and message as the ASCII trailers, plus a repeated
    /// [`crate::pb::Any`] payload. This method ships whatever bytes you give
    /// it. An empty slice omits the trailer. To build the protobuf, use
    /// [`Self::with_error_details`].
    ///
    /// A non-empty blob is `grpc-status-details-bin` on the wire for every
    /// call shape, including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
    /// [`Self::details`] returns those bytes; they do not appear as a metadata
    /// key.
    /// Distinct from [`Self::set_error_details`]: that packs `Any` values into a `google.rpc.Status`; this ships raw trailer bytes on an existing status.
    pub fn set_details(&mut self, details: impl Into<Bytes>) {
        let details = details.into();
        if details.is_empty() {
            if let Some(detail) = self.detail.as_mut() {
                detail.details = Bytes::new();
            }
            return;
        }
        self.detail.get_or_insert_with(Box::default).details = details;
    }

    /// [`Self::new`] plus [`Self::set_details`].
    ///
    /// Distinct from [`Self::with_error_details`]: that packs `Any` values into a `google.rpc.Status`; this ships raw trailer bytes a proxy can forward without parsing.
    #[must_use]
    pub fn with_details(code: Code, message: impl Into<String>, details: impl Into<Bytes>) -> Self {
        let mut status = Self::new(code, message);
        status.set_details(details);
        status
    }

    /// Encode `rpc` as `grpc-status-details-bin`.
    ///
    /// Distinct from [`Self::rpc`]: that parses the trailer; this encodes it.
    /// The kernel [`Status`] code and message come from `rpc`. The same
    /// protobuf is the trailer payload, which is what grpc-go, grpc-java,
    /// and tonic-types expect to find there. This mints a fresh status:
    /// trailing metadata is empty. To keep existing trailers, use
    /// [`Self::set_rpc`].
    pub fn from_rpc(rpc: &crate::pb::Status) -> Result<Self, Self> {
        let mut status = Self::from_code(Code::Ok);
        status.set_rpc(rpc)?;
        Ok(status)
    }

    /// Replace code, message, and `grpc-status-details-bin` from `rpc`.
    /// Trailing metadata is left alone.
    ///
    /// Prefer this over [`Self::from_rpc`] when the status already carries
    /// trailers such as `x-retry-after`.
    /// Distinct from [`Self::set_error_details`]: that packs `Any` values; this encodes a packed `google.rpc.Status`.
    pub fn set_rpc(&mut self, rpc: &crate::pb::Status) -> Result<(), Self> {
        let bytes = pbrs::Serialize::serialize(rpc)
            .map_err(|e| Self::internal(format!("serialize google.rpc.Status: {e}")))?;
        self.code = Code::from_i32(rpc.code());
        let message = rpc.message().to_str().unwrap_or("").to_owned();
        self.set_details(bytes);
        if message.is_empty() {
            if let Some(detail) = self.detail.as_mut() {
                detail.message.clear();
            }
        } else {
            self.detail.get_or_insert_with(Box::default).message = message;
        }
        Ok(())
    }

    /// [`Self::set_rpc`] as a builder.
    ///
    /// Distinct from [`Self::from_rpc`]: that mints a fresh status with empty trailers; this keeps existing trailers.
    pub fn with_rpc(mut self, rpc: &crate::pb::Status) -> Result<Self, Self> {
        self.set_rpc(rpc)?;
        Ok(self)
    }

    /// Parse `grpc-status-details-bin` as [`crate::pb::Status`].
    ///
    /// When the trailer is absent, this synthesizes a protobuf with this
    /// status's code and message and no `Any` payloads. Corrupt bytes are
    /// [`Code::Internal`].
    ///
    /// Receiving does not overwrite ASCII `grpc-status` / `grpc-message`
    /// from the protobuf. [`Self::code`] and [`Self::message`] are the
    /// trailers; this returns the packed message as-is when details are
    /// present. A peer can send a protobuf whose code or message disagrees
    /// with the ASCII half. [`Self::set_code`] / [`Self::set_message`] only
    /// rewrite the protobuf when it still matches.
    /// Distinct from [`Self::error_details`]: that is the typed bag, not this packed `google.rpc.Status`.
    /// Distinct from [`Self::from_rpc`]: that encodes the trailer; this parses it.
    /// Distinct from [`Self::details`]: that returns raw trailer bytes; this parses a packed `google.rpc.Status`.
    ///
    /// A handler or interceptor [`Err`] built with [`Self::with_error_details`]
    /// is this protobuf on the client for every call shape, including a
    /// client-interceptor `Err` that never opens a stream.
    pub fn rpc(&self) -> Result<crate::pb::Status, Self> {
        if self.details().is_empty() {
            return Ok(crate::pb::Status::with_details(
                self.code(),
                self.message().to_owned(),
                [],
            ));
        }
        pbrs::Parse::parse(self.details())
            .map_err(|_| Self::internal("grpc-status-details-bin is not a google.rpc.Status"))
    }

    /// Pack `details` into a `google.rpc.Status` and attach it as
    /// `grpc-status-details-bin`.
    ///
    /// Distinct from [`Self::from_error_details`]: that takes the typed bag, not packed `Any` values.
    ///
    /// ```
    /// use pbrs_grpc::pb::{Any, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let info = ErrorInfo::with_reason("API_DISABLED", "example.com");
    /// let status = Status::with_error_details(
    ///     Code::FailedPrecondition,
    ///     "api disabled",
    ///     [Any::pack(&info)?],
    /// )?;
    /// assert_eq!(status.code(), Code::FailedPrecondition);
    /// let rpc = status.rpc()?;
    /// let info = rpc
    ///     .details()
    ///     .get(0)
    ///     .ok_or_else(|| Status::internal("missing Any"))?
    ///     .unpack::<ErrorInfo>()?;
    /// assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
    /// # Ok::<(), Status>(())
    /// ```
    ///
    /// Ships as trailers on every call shape, including a client-interceptor
    /// `Err` that never opens a stream.
    pub fn with_error_details(
        code: Code,
        message: impl Into<String>,
        details: impl IntoIterator<Item = crate::pb::Any>,
    ) -> Result<Self, Self> {
        Self::from_rpc(&crate::pb::Status::with_details(code, message, details))
    }

    /// [`Self::with_error_details`] in place. Trailing metadata is left
    /// alone; [`Self::with_error_details`] mints a fresh status.
    /// Distinct from [`Self::set_from_error_details`]: that takes the typed bag, not packed `Any` values.
    ///
    /// ```
    /// use pbrs_grpc::pb::{Any, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let mut status = Status::not_found("gone");
    /// status.metadata_mut().insert("x-retry-after", "30")?;
    /// let info = ErrorInfo::with_reason("STOCKOUT", "example.com");
    /// status.set_error_details(
    ///     Code::ResourceExhausted,
    ///     "out of stock",
    ///     [Any::pack(&info)?],
    /// )?;
    /// assert_eq!(status.code(), Code::ResourceExhausted);
    /// assert_eq!(status.metadata().get("x-retry-after"), Some("30"));
    /// # Ok::<(), Status>(())
    /// ```
    pub fn set_error_details(
        &mut self,
        code: Code,
        message: impl Into<String>,
        details: impl IntoIterator<Item = crate::pb::Any>,
    ) -> Result<(), Self> {
        self.set_rpc(&crate::pb::Status::with_details(code, message, details))
    }

    /// Encode a typed [`crate::pb::ErrorDetails`] bag as
    /// `grpc-status-details-bin`.
    ///
    /// Distinct from [`Self::with_error_details`]: that packs `Any` values; this takes the typed bag.
    /// Distinct from [`Self::from_rpc`]: that encodes a packed `google.rpc.Status`; this encodes the typed bag.
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     error_info: Some(ErrorInfo::with_reason("API_DISABLED", "example.com")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(Code::FailedPrecondition, "typed-bag", &details)?;
    /// let info = status.error_details()?.error_info.expect("ErrorInfo");
    /// assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
    /// # Ok::<(), Status>(())
    /// ```
    pub fn from_error_details(
        code: Code,
        message: impl Into<String>,
        details: &crate::pb::ErrorDetails,
    ) -> Result<Self, Self> {
        Self::with_error_details(code, message, details.to_anys()?)
    }

    /// [`Self::from_error_details`] in place. Trailing metadata is left
    /// alone.
    /// Distinct from [`Self::set_error_details`]: that packs `Any` values; this takes the typed bag.
    pub fn set_from_error_details(
        &mut self,
        code: Code,
        message: impl Into<String>,
        details: &crate::pb::ErrorDetails,
    ) -> Result<(), Self> {
        self.set_error_details(code, message, details.to_anys()?)
    }

    /// Decode [`crate::pb::ErrorDetails`] from this status.
    ///
    /// Distinct from [`Self::rpc`]: that is the packed `google.rpc.Status`, not this typed bag.
    /// Absent or empty `grpc-status-details-bin` yields an empty bag, not an
    /// error. Corrupt bytes are [`Code::Internal`].
    pub fn error_details(&self) -> Result<crate::pb::ErrorDetails, Self> {
        crate::pb::ErrorDetails::from_rpc(&self.rpc()?)
    }

    /// Whether this status represents success.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.code == Code::Ok
    }

    /// Whether [`Self::code`] is gRPC A6-retryable ([`Code::Unavailable`]).
    ///
    /// Distinct from [`Self::retry_delay`]: a packed wait hint is not
    /// permission to retry. [`Code::ResourceExhausted`] from
    /// [`crate::Channel::max_concurrent_rpcs`] is this process, not a peer,
    /// and is not retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.code.is_retryable()
    }

    /// Packed `google.rpc.RetryInfo.retry_delay`, if this status carries one.
    ///
    /// Distinct from [`Self::is_retryable`]: a delay is a wait hint, not
    /// permission to retry. Peer trailers unpack `grpc-status-details-bin`;
    /// a local [`Self::with_cause`] has no packed details. Negative or
    /// unparseable protobuf durations are `None`, so a retry loop can treat
    /// absence as "no hint". A zero delay is `Some(Duration::ZERO)`, not
    /// `None`. Build the payload with [`crate::pb::RetryInfo::with_retry_delay`].
    #[must_use]
    pub fn retry_delay(&self) -> Option<std::time::Duration> {
        let retry = self.error_details().ok()?.retry_info?;
        retry.retry_delay().try_to_std().ok()
    }

    /// Packed `google.rpc.ErrorInfo`, if this status carries one.
    ///
    /// Distinct from [`Self::error_details`]: this is one typed message, not
    /// the bag. Distinct from [`Self::retry_delay`]: that is a wait hint.
    /// Peer trailers unpack `grpc-status-details-bin`; a local
    /// [`Self::with_cause`] has no packed details. Corrupt bytes are `None`.
    /// Build the payload with [`crate::pb::ErrorInfo::with_reason`].
    /// Fill a metadata pair with [`crate::pb::ErrorInfo::with_metadata`].
    ///
    /// ```
    /// use pbrs_grpc::pb::{ErrorDetails, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let details = ErrorDetails {
    ///     error_info: Some(ErrorInfo::with_reason("API_DISABLED", "example.com")),
    ///     ..ErrorDetails::default()
    /// };
    /// let status = Status::from_error_details(
    ///     Code::FailedPrecondition,
    ///     "disabled",
    ///     &details,
    /// )?;
    /// let info = status.error_info().expect("ErrorInfo");
    /// assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
    /// assert!(Status::not_found("row").error_info().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn error_info(&self) -> Option<crate::pb::ErrorInfo> {
        self.error_details().ok()?.error_info
    }

    /// Packed `google.rpc.BadRequest`, if this status carries one.
    ///
    /// Distinct from [`Self::error_info`]: that is reason and domain, not
    /// field violations. Distinct from [`Self::invalid_argument`], which is
    /// the ASCII code with no packed fields. Corrupt bytes are `None`.
    /// Build the payload with [`crate::pb::BadRequest::with_field`].
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
    ///     bad.field_violations()
    ///         .get(0)
    ///         .expect("field")
    ///         .field()
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "name"
    /// );
    /// assert!(Status::invalid_argument("name").bad_request().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn bad_request(&self) -> Option<crate::pb::BadRequest> {
        self.error_details().ok()?.bad_request
    }

    /// Packed `google.rpc.QuotaFailure`, if this status carries one.
    ///
    /// Distinct from [`Self::is_retryable`]: [`Code::ResourceExhausted`] is never
    /// A6-retryable.
    /// Distinct from [`Self::retry_delay`]: a wait hint can sit next to quota.
    /// Distinct from [`Self::bad_request`]: that is a field path, not a quota subject.
    /// Distinct from [`Self::resource_exhausted`], which is the ASCII code with no packed quota.
    /// Distinct from [`Self::error_details`]: this is one typed message, not the bag.
    /// Corrupt bytes are `None`. Build the payload with
    /// [`crate::pb::QuotaFailure::with_violation`].
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
    ///     quota.violations()
    ///         .get(0)
    ///         .expect("subject")
    ///         .subject()
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "project:1"
    /// );
    /// assert!(Status::resource_exhausted("tokens").quota_failure().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn quota_failure(&self) -> Option<crate::pb::QuotaFailure> {
        self.error_details().ok()?.quota_failure
    }

    /// Packed `google.rpc.PreconditionFailure`, if this status carries one.
    ///
    /// Distinct from [`Self::is_retryable`]: [`Code::FailedPrecondition`] is never
    /// A6-retryable.
    /// Distinct from [`Self::retry_delay`]: a wait hint can sit next to a precondition.
    /// Distinct from [`Self::quota_failure`]: that is a quota subject, not a precondition type.
    /// Distinct from [`Self::bad_request`]: that is a field path, not a precondition type.
    /// Distinct from [`Self::failed_precondition`], which is the ASCII code with no packed violations.
    /// Distinct from [`Self::error_details`]: this is one typed message, not the bag.
    /// Corrupt bytes are `None`. Build the payload with
    /// [`crate::pb::PreconditionFailure::with_violation`].
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
    ///     pre.violations()
    ///         .get(0)
    ///         .expect("violation")
    ///         .r#type()
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "TOS"
    /// );
    /// assert!(Status::failed_precondition("tos").precondition_failure().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn precondition_failure(&self) -> Option<crate::pb::PreconditionFailure> {
        self.error_details().ok()?.precondition_failure
    }

    /// Packed `google.rpc.Help`, if this status carries one.
    ///
    /// Distinct from [`Self::is_retryable`]: documentation links can sit next to a retryable [`Code::Unavailable`].
    /// Distinct from [`Self::precondition_failure`]: that is a type and subject, not a docs URL.
    /// Distinct from [`Self::quota_failure`]: that is a quota subject, not a docs URL.
    /// Distinct from [`Self::bad_request`]: that is a field path, not a docs URL.
    /// Distinct from [`Self::error_info`]: that is reason and domain, not a documentation link.
    /// Distinct from [`Self::error_details`]: this is one typed message, not the bag.
    /// Corrupt bytes are `None`. Build the payload with
    /// [`crate::pb::Help::with_link`].
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
    ///     help.links()
    ///         .get(0)
    ///         .expect("link")
    ///         .url()
    ///         .to_str()
    ///         .unwrap_or(""),
    ///     "https://example.com/quota"
    /// );
    /// assert!(Status::unavailable("backend").help().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn help(&self) -> Option<crate::pb::Help> {
        self.error_details().ok()?.help
    }

    /// Packed `google.rpc.LocalizedMessage`, if this status carries one.
    ///
    /// Distinct from [`Self::message`]: that is the ASCII `grpc-message`, not a locale.
    /// Distinct from [`Self::help`]: that is a docs URL, not a locale.
    /// Distinct from [`Self::error_details`]: this is one typed message, not the bag.
    /// Corrupt bytes are `None`. Build the payload with
    /// [`crate::pb::LocalizedMessage::with_locale`].
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
    /// assert!(Status::not_found("row").localized_message().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn localized_message(&self) -> Option<crate::pb::LocalizedMessage> {
        self.error_details().ok()?.localized_message
    }

    /// Packed `google.rpc.RequestInfo`, if this status carries one.
    ///
    /// Distinct from [`Self::error_info`]: that is a metadata map, not a typed request_id.
    /// Distinct from [`Self::help`]: that is a docs URL, not a request_id.
    /// Distinct from [`Self::localized_message`]: that is a locale, not a request_id.
    /// Distinct from [`Self::error_details`]: this is one typed message, not the bag.
    /// Corrupt bytes are `None`. Build the payload with
    /// [`crate::pb::RequestInfo::with_request_id`].
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
    /// assert!(Status::internal("boom").request_info().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn request_info(&self) -> Option<crate::pb::RequestInfo> {
        self.error_details().ok()?.request_info
    }

    /// Packed `google.rpc.ResourceInfo`, if this status carries one.
    ///
    /// Distinct from [`Self::quota_failure`]: that is a quota subject, not a resource identity.
    /// Distinct from [`Self::request_info`]: that is a request_id, not a resource.
    /// Distinct from [`Self::error_info`]: that is reason and domain, not a resource type and name.
    /// Distinct from [`Self::error_details`]: this is one typed message, not the bag.
    /// Corrupt bytes are `None`. Build the payload with
    /// [`crate::pb::ResourceInfo::with_resource`].
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
    ///     info.resource_type().to_str().unwrap_or(""),
    ///     "sqladmin.googleapis.com/Instance"
    /// );
    /// assert!(Status::not_found("gone").resource_info().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn resource_info(&self) -> Option<crate::pb::ResourceInfo> {
        self.error_details().ok()?.resource_info
    }

    /// Packed `google.rpc.DebugInfo`, if this status carries one.
    ///
    /// Distinct from [`Self::localized_message`]: that is a locale, not an operator stack.
    /// Distinct from [`Self::help`]: that is a docs URL, not an operator stack.
    /// Distinct from [`Self::error_details`]: this is one typed message, not the bag.
    /// Corrupt bytes are `None`. Build the payload with
    /// [`crate::pb::DebugInfo::with_stack`].
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
    /// assert_eq!(
    ///     debug.stack_entries().get(0).expect("frame").to_str().unwrap_or(""),
    ///     "handler.rs:9"
    /// );
    /// assert!(Status::internal("boom").debug_info().is_none());
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn debug_info(&self) -> Option<crate::pb::DebugInfo> {
        self.error_details().ok()?.debug_info
    }

    /// Wrap a local error as a [`Status`].
    ///
    /// If `err` is already a [`Status`], or one appears in
    /// [`std::error::Error::source`], that status is returned as-is (cause
    /// and packed details stay). A top-level [`std::io::Error`] uses the
    /// same mapping as `From` (timeouts [`Code::DeadlineExceeded`],
    /// connection failures [`Code::Unavailable`], ...). Anything else is
    /// [`Code::Unknown`] with `err`'s display as the message and `err` as
    /// [`std::error::Error::source`].
    ///
    /// Distinct from [`Self::with_error_details`]: this is local wrapping,
    /// not a packed `google.rpc.Status` on the wire. Distinct from
    /// [`Self::with_cause`]: that attaches a cause onto an existing status;
    /// this builds one. A peer trailer has no cause.
    ///
    /// ```
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let refused = Status::from_error(std::io::Error::new(
    ///     std::io::ErrorKind::ConnectionRefused,
    ///     "refused",
    /// ));
    /// assert_eq!(refused.code(), Code::Unavailable);
    /// assert!(refused.is_retryable());
    /// assert!(std::error::Error::source(&refused).is_some());
    ///
    /// let inner = Status::not_found("row");
    /// assert_eq!(Status::from_error(inner.clone()).code(), Code::NotFound);
    /// # Ok::<(), Status>(())
    /// ```
    #[must_use]
    pub fn from_error(err: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>) -> Self {
        let err = err.into();
        if let Some(status) = status_in_chain(err.as_ref()) {
            return status;
        }
        match err.downcast::<std::io::Error>() {
            Ok(io) => Self::from(*io),
            Err(err) => Self::unknown(err.to_string()).with_boxed_cause(err),
        }
    }

    /// [`Code::Cancelled`]: the caller gave up or reset the stream.
    #[must_use]
    pub fn cancelled() -> Self {
        Self::new(Code::Cancelled, "cancelled")
    }

    /// [`Code::DeadlineExceeded`]: `grpc-timeout` elapsed.
    #[must_use]
    pub fn deadline_exceeded() -> Self {
        Self::new(Code::DeadlineExceeded, "deadline exceeded")
    }

    /// [`Code::Unknown`].
    #[must_use]
    pub fn unknown(message: impl Into<String>) -> Self {
        Self::new(Code::Unknown, message)
    }

    /// [`Code::InvalidArgument`]: the request itself is wrong, so retrying it
    /// unchanged will fail again.
    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(Code::InvalidArgument, message)
    }

    /// [`Code::NotFound`].
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(Code::NotFound, message)
    }

    /// [`Code::AlreadyExists`].
    #[must_use]
    pub fn already_exists(message: impl Into<String>) -> Self {
        Self::new(Code::AlreadyExists, message)
    }

    /// [`Code::PermissionDenied`].
    #[must_use]
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(Code::PermissionDenied, message)
    }

    /// [`Code::ResourceExhausted`]: a size or rate cap was hit.
    #[must_use]
    pub fn resource_exhausted(message: impl Into<String>) -> Self {
        Self::new(Code::ResourceExhausted, message)
    }

    /// [`Code::FailedPrecondition`].
    #[must_use]
    pub fn failed_precondition(message: impl Into<String>) -> Self {
        Self::new(Code::FailedPrecondition, message)
    }

    /// [`Code::Aborted`].
    #[must_use]
    pub fn aborted(message: impl Into<String>) -> Self {
        Self::new(Code::Aborted, message)
    }

    /// [`Code::OutOfRange`].
    #[must_use]
    pub fn out_of_range(message: impl Into<String>) -> Self {
        Self::new(Code::OutOfRange, message)
    }

    /// [`Code::Unimplemented`]: the method or service is not hosted here.
    #[must_use]
    pub fn unimplemented(message: impl Into<String>) -> Self {
        Self::new(Code::Unimplemented, message)
    }

    /// [`Code::Internal`]: an invariant of this process was broken.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(Code::Internal, message)
    }

    /// [`Code::Unavailable`]: the peer or transport is not usable right now.
    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(Code::Unavailable, message)
    }

    /// [`Code::DataLoss`].
    #[must_use]
    pub fn data_loss(message: impl Into<String>) -> Self {
        Self::new(Code::DataLoss, message)
    }

    /// [`Code::Unauthenticated`].
    #[must_use]
    pub fn unauthenticated(message: impl Into<String>) -> Self {
        Self::new(Code::Unauthenticated, message)
    }

    /// Record `cause` as [`std::error::Error::source`].
    ///
    /// Local I/O ([`std::io::Error`]), a TLS handshake, and HTTP/2
    /// connection death already attach the original error. A peer trailer has
    /// no cause: [`std::error::Error::source`] is `None`. Distinct from
    /// [`Self::with_error_details`] (a packed `google.rpc.Status` on the wire).
    #[must_use]
    pub fn with_cause(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.detail.get_or_insert_with(Box::default).source = Some(Arc::new(cause));
        self
    }

    fn with_boxed_cause(
        mut self,
        cause: Box<dyn std::error::Error + Send + Sync + 'static>,
    ) -> Self {
        self.detail.get_or_insert_with(Box::default).source = Some(Arc::from(cause));
        self
    }

    /// Map an HTTP/2 error onto [`Code::Unavailable`].
    ///
    /// Connection death (`GOAWAY`, I/O, `REFUSED_STREAM`) is marked so a
    /// unary or server-streaming RPC can redial this call once, and so
    /// client-streaming / bidi can redial once before HEADERS. Stream
    /// resets and user errors stay plain `UNAVAILABLE` and are not retried.
    pub(crate) fn from_h2(err: impl Into<h2::Error>) -> Self {
        let err = err.into();
        let mut status = Self::unavailable(err.to_string());
        if h2_lost_connection(&err) {
            status.mark_transport();
        }
        status.with_cause(err)
    }

    /// Like [`Self::from_h2`], but non-connection failures stay
    /// [`Code::Internal`] so a flow-control `send_data` error is not
    /// reported as a dead peer.
    pub(crate) fn from_h2_send(err: impl Into<h2::Error>) -> Self {
        let err = err.into();
        if h2_lost_connection(&err) {
            let mut status = Self::unavailable(err.to_string());
            status.mark_transport();
            status.with_cause(err)
        } else {
            Self::internal(err.to_string()).with_cause(err)
        }
    }

    /// [`Code::Unavailable`] for a send stream that vanished under us.
    pub(crate) fn stream_closed() -> Self {
        let mut status = Self::unavailable("stream closed");
        status.mark_transport();
        status
    }

    fn mark_transport(&mut self) {
        self.detail.get_or_insert_with(Box::default).transport = true;
    }

    /// The HTTP/2 connection died; this is not a peer status trailer.
    #[must_use]
    pub(crate) fn is_transport(&self) -> bool {
        self.detail.as_ref().is_some_and(|d| d.transport)
    }
}

fn h2_lost_connection(err: &h2::Error) -> bool {
    err.is_io() || err.is_go_away() || err.reason() == Some(h2::Reason::REFUSED_STREAM)
}

fn status_in_chain(err: &(dyn std::error::Error + 'static)) -> Option<Status> {
    let mut cur = Some(err);
    while let Some(cause) = cur {
        if let Some(status) = cause.downcast_ref::<Status>() {
            return Some(status.clone());
        }
        cur = cause.source();
    }
    None
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = self.message();
        if message.is_empty() {
            write!(f, "{}", self.code)
        } else {
            write!(f, "{}: {message}", self.code)
        }
    }
}

impl std::error::Error for Status {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.detail
            .as_ref()?
            .source
            .as_deref()
            .map(|err| err as &(dyn std::error::Error + 'static))
    }
}

/// Map a local I/O failure onto a gRPC code.
///
/// Timeouts become [`Code::DeadlineExceeded`]. Connection failures become
/// [`Code::Unavailable`]. Everything else is [`Code::Unknown`], with the
/// original error text as the message. This is for *this process's* I/O,
/// not for a peer status.
impl From<std::io::Error> for Status {
    fn from(err: std::io::Error) -> Self {
        let code = match err.kind() {
            std::io::ErrorKind::TimedOut => Code::DeadlineExceeded,
            std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::AddrNotAvailable => Code::Unavailable,
            std::io::ErrorKind::InvalidData => Code::Internal,
            _ => Code::Unknown,
        };
        Self::new(code, err.to_string()).with_cause(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{Code, Status};

    #[test]
    fn status_is_two_words() {
        assert!(std::mem::size_of::<Status>() <= 2 * std::mem::size_of::<usize>());
    }

    #[test]
    fn bare_codes_carry_no_detail() {
        let status = Status::from_code(Code::Ok);
        assert!(status.is_ok());
        assert_eq!(status.message(), "");
        assert!(status.metadata().is_empty());
        assert!(status.details().is_empty());
        assert_eq!(status.to_string(), "OK");
    }

    #[test]
    fn metadata_is_allocated_on_demand() {
        let mut status = Status::from_code(Code::Aborted);
        assert!(status.metadata().is_empty());
        status.metadata_mut().insert("k", "v").expect("insert");
        assert_eq!(status.metadata().get("k"), Some("v"));
        assert_eq!(status.to_string(), "ABORTED");
    }

    #[test]
    fn wire_codes_round_trip() {
        for n in 0..=16 {
            let code = Code::from_i32(n);
            assert_eq!(code.to_i32(), n, "code {n} must round-trip");
        }
    }

    #[test]
    fn unrecognised_codes_become_unknown() {
        for n in [-1, 17, 99, i32::MAX, i32::MIN] {
            assert_eq!(Code::from_i32(n), Code::Unknown);
        }
    }

    /// Sixteen near-identical constructors are a copy-paste hazard: one
    /// returning the wrong code would be invisible until a caller matched on
    /// it. This pins every one.
    #[test]
    fn every_constructor_carries_its_own_code() {
        let cases: [(Status, Code); 15] = [
            (Status::cancelled(), Code::Cancelled),
            (Status::deadline_exceeded(), Code::DeadlineExceeded),
            (Status::unknown("m"), Code::Unknown),
            (Status::invalid_argument("m"), Code::InvalidArgument),
            (Status::not_found("m"), Code::NotFound),
            (Status::already_exists("m"), Code::AlreadyExists),
            (Status::permission_denied("m"), Code::PermissionDenied),
            (Status::resource_exhausted("m"), Code::ResourceExhausted),
            (Status::failed_precondition("m"), Code::FailedPrecondition),
            (Status::aborted("m"), Code::Aborted),
            (Status::out_of_range("m"), Code::OutOfRange),
            (Status::unimplemented("m"), Code::Unimplemented),
            (Status::internal("m"), Code::Internal),
            (Status::unavailable("m"), Code::Unavailable),
            (Status::data_loss("m"), Code::DataLoss),
        ];
        for (status, want) in cases {
            assert_eq!(status.code(), want, "{status}");
            assert!(!status.is_ok());
            assert!(!status.message().is_empty());
        }
        // Listed separately so the array above stays one code per line.
        assert_eq!(Status::unauthenticated("m").code(), Code::Unauthenticated);
    }

    #[test]
    fn display_matches_canonical_names() {
        assert_eq!(Status::not_found("gone").to_string(), "NOT_FOUND: gone");
        assert_eq!(
            Status::deadline_exceeded().to_string(),
            "DEADLINE_EXCEEDED: deadline exceeded"
        );
        assert_eq!(Code::ResourceExhausted.name(), "RESOURCE_EXHAUSTED");
        assert_eq!(
            Code::NotFound.description(),
            "Some requested entity was not found"
        );
    }

    #[test]
    fn codes_parse_from_name_and_number() {
        use std::str::FromStr;

        for n in 0..=16 {
            let code = Code::from_i32(n);
            assert_eq!(Code::from_str(code.name()), Ok(code), "{}", code.name());
            assert_eq!(Code::from_str(&n.to_string()), Ok(code), "{n}");
        }
        assert_eq!(Code::from_str("not_found"), Err(super::ParseCodeError));
        assert_eq!(Code::from_str("17"), Err(super::ParseCodeError));
        assert_eq!(Code::from_str(""), Err(super::ParseCodeError));
    }

    #[test]
    fn set_message_keeps_metadata_and_details() {
        let mut status = Status::with_details(Code::NotFound, "gone", vec![0x08, 0x05]);
        status
            .metadata_mut()
            .insert("x-retry-after", "30")
            .expect("md");
        status.set_message("still gone");
        assert_eq!(status.message(), "still gone");
        assert_eq!(status.details(), &[0x08, 0x05]);
        assert_eq!(status.metadata().get("x-retry-after"), Some("30"));
        let status = status.with_message("");
        assert_eq!(status.message(), "");
        assert_eq!(status.details(), &[0x08, 0x05]);
    }

    #[test]
    fn set_message_keeps_a_packed_google_rpc_status_in_sync() {
        use crate::pb::{Any, ErrorInfo};

        let mut info = ErrorInfo::new();
        info.set_reason("STOCKOUT");
        let mut status =
            Status::with_error_details(Code::NotFound, "gone", [Any::pack(&info).expect("pack")])
                .expect("encode");
        status.set_message("still gone");
        assert_eq!(status.message(), "still gone");
        let rpc = status.rpc().expect("parse");
        assert_eq!(rpc.message().to_str().unwrap_or(""), "still gone");
        assert_eq!(rpc.code(), Code::NotFound.to_i32());
        assert_eq!(rpc.details().len(), 1);
    }

    #[test]
    fn set_code_keeps_a_packed_google_rpc_status_in_sync() {
        use crate::pb::{Any, ErrorInfo};

        let mut info = ErrorInfo::new();
        info.set_reason("STOCKOUT");
        let mut status =
            Status::with_error_details(Code::NotFound, "gone", [Any::pack(&info).expect("pack")])
                .expect("encode");
        status
            .metadata_mut()
            .insert("x-retry-after", "30")
            .expect("md");
        status.set_code(Code::PermissionDenied);
        assert_eq!(status.code(), Code::PermissionDenied);
        assert_eq!(status.message(), "gone");
        assert_eq!(status.metadata().get("x-retry-after"), Some("30"));
        let rpc = status.rpc().expect("parse");
        assert_eq!(rpc.code(), Code::PermissionDenied.to_i32());
        assert_eq!(rpc.message().to_str().unwrap_or(""), "gone");
        assert_eq!(rpc.details().len(), 1);
        let status = status.with_code(Code::FailedPrecondition);
        assert_eq!(status.code(), Code::FailedPrecondition);
        assert_eq!(
            status.rpc().expect("parse").code(),
            Code::FailedPrecondition.to_i32()
        );
    }

    #[test]
    fn set_code_leaves_opaque_details_alone() {
        // Not a google.rpc.Status (unlike 0x08 0x05, which is code 5).
        let mut status = Status::with_details(Code::NotFound, "gone", vec![0xff]);
        status.set_code(Code::PermissionDenied);
        assert_eq!(status.code(), Code::PermissionDenied);
        assert_eq!(status.details(), &[0xff]);
    }

    #[test]
    fn set_code_leaves_a_mismatched_packed_code_alone() {
        let packed =
            Status::with_error_details(Code::NotFound, "gone", Vec::<crate::pb::Any>::new())
                .expect("encode");
        let mut status = Status::permission_denied("no");
        status.set_details(packed.details().to_vec());
        status.set_code(Code::Unavailable);
        assert_eq!(status.code(), Code::Unavailable);
        assert_eq!(status.rpc().expect("parse").code(), Code::NotFound.to_i32());
    }

    #[test]
    fn set_rpc_keeps_trailing_metadata() {
        use crate::pb::{Any, ErrorInfo};

        let mut status = Status::not_found("gone");
        status
            .metadata_mut()
            .insert("x-retry-after", "30")
            .expect("md");
        let mut info = ErrorInfo::new();
        info.set_reason("STOCKOUT");
        status
            .set_error_details(
                Code::ResourceExhausted,
                "out of stock",
                [Any::pack(&info).expect("pack")],
            )
            .expect("encode");
        assert_eq!(status.code(), Code::ResourceExhausted);
        assert_eq!(status.message(), "out of stock");
        assert_eq!(status.metadata().get("x-retry-after"), Some("30"));
        let rpc = status.rpc().expect("parse");
        assert_eq!(rpc.code(), Code::ResourceExhausted.to_i32());
        assert_eq!(rpc.details().len(), 1);

        let minted = Status::from_rpc(&rpc).expect("mint");
        assert!(minted.metadata().is_empty());
        let mut with_md = Status::cancelled();
        with_md
            .metadata_mut()
            .insert("x-retry-after", "30")
            .expect("md");
        let kept = with_md.with_rpc(&rpc).expect("keep");
        assert_eq!(kept.metadata().get("x-retry-after"), Some("30"));
        assert_eq!(kept.code(), Code::ResourceExhausted);
        assert_eq!(kept.message(), "out of stock");
    }

    #[test]
    fn set_from_error_details_keeps_trailing_metadata() {
        use crate::pb::{ErrorDetails, ErrorInfo};

        let mut status = Status::not_found("gone");
        status
            .metadata_mut()
            .insert("x-retry-after", "30")
            .expect("md");
        let mut info = ErrorInfo::new();
        info.set_reason("STOCKOUT");
        let details = ErrorDetails {
            error_info: Some(info),
            ..ErrorDetails::default()
        };
        status
            .set_from_error_details(Code::ResourceExhausted, "out of stock", &details)
            .expect("encode");
        assert_eq!(status.metadata().get("x-retry-after"), Some("30"));
        assert_eq!(
            status
                .error_details()
                .expect("decode")
                .error_info
                .as_ref()
                .expect("info")
                .reason()
                .to_str()
                .unwrap_or(""),
            "STOCKOUT"
        );
    }

    #[test]
    fn details_are_independent_of_metadata() {
        let mut status = Status::with_details(Code::NotFound, "gone", vec![0x08, 0x05]);
        assert_eq!(status.details(), &[0x08, 0x05]);
        status
            .metadata_mut()
            .insert("x-retry-after", "30")
            .expect("md");
        assert_eq!(status.details(), &[0x08, 0x05]);
        assert_eq!(status.metadata().get("x-retry-after"), Some("30"));
        status.set_details(Vec::<u8>::new());
        assert!(status.details().is_empty());
        assert_eq!(status.metadata().get("x-retry-after"), Some("30"));
    }

    #[test]
    fn typed_details_round_trip_through_status() {
        use crate::pb::{Any, ErrorInfo};

        let mut info = ErrorInfo::new();
        info.set_reason("STOCKOUT");
        info.set_domain("spanner.googleapis.com");
        let status = Status::with_error_details(
            Code::ResourceExhausted,
            "out of stock",
            [Any::pack(&info).expect("pack")],
        )
        .expect("encode");
        assert_eq!(status.code(), Code::ResourceExhausted);
        assert_eq!(status.message(), "out of stock");
        assert!(!status.details().is_empty());

        let rpc = status.rpc().expect("parse");
        assert_eq!(rpc.code(), Code::ResourceExhausted.to_i32());
        assert_eq!(rpc.message().to_str().unwrap_or(""), "out of stock");
        assert_eq!(rpc.details().len(), 1);
        let got = rpc
            .details()
            .get(0)
            .expect("one")
            .unpack::<ErrorInfo>()
            .expect("unpack");
        assert_eq!(got.reason().to_str().unwrap_or(""), "STOCKOUT");
        assert_eq!(
            got.domain().to_str().unwrap_or(""),
            "spanner.googleapis.com"
        );
    }

    #[test]
    fn rpc_synthesizes_a_protobuf_when_the_trailer_is_absent() {
        let status = Status::not_found("no such row");
        let rpc = status.rpc().expect("synth");
        assert_eq!(rpc.code(), Code::NotFound.to_i32());
        assert_eq!(rpc.message().to_str().unwrap_or(""), "no such row");
        assert!(rpc.details().is_empty());
    }

    #[test]
    fn error_details_bag_round_trips_through_status() {
        use crate::pb::{ErrorDetails, ErrorInfo};

        let mut info = ErrorInfo::new();
        info.set_reason("STOCKOUT");
        let details = ErrorDetails {
            error_info: Some(info),
            ..ErrorDetails::default()
        };
        let status = Status::from_error_details(Code::ResourceExhausted, "out of stock", &details)
            .expect("encode");
        let got = status.error_details().expect("decode");
        assert_eq!(
            got.error_info
                .as_ref()
                .expect("info")
                .reason()
                .to_str()
                .unwrap_or(""),
            "STOCKOUT"
        );
    }

    #[test]
    fn io_timeout_is_deadline_exceeded() {
        let err = std::io::Error::new(std::io::ErrorKind::TimedOut, "slow");
        let status = Status::from(err);
        assert_eq!(status.code(), Code::DeadlineExceeded);
        assert!(status.message().contains("slow"));
        let cause = std::error::Error::source(&status).expect("io cause");
        assert_eq!(
            cause.downcast_ref::<std::io::Error>().expect("io").kind(),
            std::io::ErrorKind::TimedOut
        );
        let cloned = status.clone();
        assert!(std::error::Error::source(&cloned).is_some());
    }

    #[test]
    fn io_connection_refused_is_unavailable() {
        let err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let status = Status::from(err);
        assert_eq!(status.code(), Code::Unavailable);
        assert!(!status.is_transport());
        let cause = std::error::Error::source(&status).expect("io cause");
        assert_eq!(
            cause.downcast_ref::<std::io::Error>().expect("io").kind(),
            std::io::ErrorKind::ConnectionRefused
        );
    }

    #[test]
    fn refused_stream_is_transport_lost() {
        let status = Status::from_h2(h2::Reason::REFUSED_STREAM);
        assert_eq!(status.code(), Code::Unavailable);
        assert!(status.is_transport());
        assert!(std::error::Error::source(&status).is_some());
    }

    #[test]
    fn peer_trailer_has_no_cause() {
        let status = Status::not_found("no such row");
        assert!(std::error::Error::source(&status).is_none());
    }

    #[test]
    fn peer_unavailable_message_is_not_transport_lost() {
        let status = Status::unavailable("too many concurrent RPCs");
        assert!(!status.is_transport());
        assert!(std::error::Error::source(&status).is_none());
    }

    #[test]
    fn with_cause_is_error_source() {
        let cause = std::io::Error::new(std::io::ErrorKind::Other, "disk");
        let status = Status::internal("write failed").with_cause(cause);
        let src = std::error::Error::source(&status).expect("cause");
        assert!(src.to_string().contains("disk"), "{src}");
    }

    #[test]
    fn a6_retryable_is_unavailable_only() {
        assert!(Code::Unavailable.is_retryable());
        assert!(Status::unavailable("gone").is_retryable());
        for code in [
            Code::Ok,
            Code::Cancelled,
            Code::Unknown,
            Code::InvalidArgument,
            Code::DeadlineExceeded,
            Code::NotFound,
            Code::AlreadyExists,
            Code::PermissionDenied,
            Code::ResourceExhausted,
            Code::FailedPrecondition,
            Code::Aborted,
            Code::OutOfRange,
            Code::Unimplemented,
            Code::Internal,
            Code::DataLoss,
            Code::Unauthenticated,
        ] {
            assert!(
                !code.is_retryable(),
                "{code} must not be A6-retryable by default"
            );
            assert!(!Status::from_code(code).is_retryable());
        }
        let cap = Status::resource_exhausted("too many concurrent RPCs");
        assert!(!cap.is_retryable());
        assert!(cap.retry_delay().is_none());
    }

    #[test]
    fn retry_delay_reads_packed_retry_info() {
        let delay = std::time::Duration::from_millis(1500);
        let details = crate::pb::ErrorDetails {
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(delay)),
            ..crate::pb::ErrorDetails::default()
        };
        let status =
            Status::from_error_details(Code::Unavailable, "backoff", &details).expect("encode");
        assert!(status.is_retryable());
        assert_eq!(status.retry_delay(), Some(delay));

        let quota =
            Status::from_error_details(Code::ResourceExhausted, "quota", &details).expect("encode");
        assert!(!quota.is_retryable());
        assert_eq!(quota.retry_delay(), Some(delay));

        let zero = crate::pb::ErrorDetails {
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(
                std::time::Duration::ZERO,
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let zero_status =
            Status::from_error_details(Code::Unavailable, "now", &zero).expect("encode");
        assert_eq!(zero_status.retry_delay(), Some(std::time::Duration::ZERO));
        assert!(Status::unavailable("gone").retry_delay().is_none());
        assert!(Status::unavailable("gone")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .retry_delay()
            .is_none());
    }

    #[test]
    fn error_info_reads_packed_error_info() {
        let mut info = crate::pb::ErrorInfo::new();
        info.set_reason("API_DISABLED");
        info.set_domain("example.com");
        let details = crate::pb::ErrorDetails {
            error_info: Some(info.clone()),
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(
                std::time::Duration::from_millis(10),
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let status = Status::from_error_details(Code::FailedPrecondition, "disabled", &details)
            .expect("encode");
        let got = status.error_info().expect("ErrorInfo");
        assert_eq!(got.reason().to_str().unwrap_or(""), "API_DISABLED");
        assert_eq!(got.domain().to_str().unwrap_or(""), "example.com");
        assert_eq!(
            status.retry_delay(),
            Some(std::time::Duration::from_millis(10))
        );

        let retry_only = crate::pb::ErrorDetails {
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(
                std::time::Duration::from_millis(10),
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let retry_status =
            Status::from_error_details(Code::Unavailable, "backoff", &retry_only).expect("encode");
        assert!(retry_status.error_info().is_none());
        assert!(retry_status.retry_delay().is_some());

        assert!(Status::not_found("row").error_info().is_none());
        assert!(Status::not_found("row")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .error_info()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .error_info()
        .is_none());
    }

    #[test]
    fn bad_request_reads_packed_field_violations() {
        let details = crate::pb::ErrorDetails {
            bad_request: Some(crate::pb::BadRequest::with_field("name", "required")),
            error_info: {
                let mut info = crate::pb::ErrorInfo::new();
                info.set_reason("API_DISABLED");
                Some(info)
            },
            ..crate::pb::ErrorDetails::default()
        };
        let status =
            Status::from_error_details(Code::InvalidArgument, "bad", &details).expect("encode");
        let bad = status.bad_request().expect("BadRequest");
        let field = bad.field_violations().get(0).expect("field");
        assert_eq!(field.field().to_str().unwrap_or(""), "name");
        assert_eq!(field.description().to_str().unwrap_or(""), "required");
        assert!(status.error_info().is_some());

        let info_only = crate::pb::ErrorDetails {
            error_info: status.error_info(),
            ..crate::pb::ErrorDetails::default()
        };
        let info_status =
            Status::from_error_details(Code::FailedPrecondition, "disabled", &info_only)
                .expect("encode");
        assert!(info_status.bad_request().is_none());
        assert!(info_status.error_info().is_some());

        assert!(Status::invalid_argument("name").bad_request().is_none());
        assert!(Status::invalid_argument("name")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .bad_request()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .bad_request()
        .is_none());
    }

    #[test]
    fn quota_failure_reads_packed_quota_subjects() {
        let delay = std::time::Duration::from_millis(25);
        let details = crate::pb::ErrorDetails {
            quota_failure: Some(crate::pb::QuotaFailure::with_violation(
                "project:1",
                "tokens",
            )),
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(delay)),
            bad_request: Some(crate::pb::BadRequest::with_field("name", "required")),
            ..crate::pb::ErrorDetails::default()
        };
        let status =
            Status::from_error_details(Code::ResourceExhausted, "quota", &details).expect("encode");
        assert!(!status.is_retryable());
        assert_eq!(status.retry_delay(), Some(delay));
        let quota = status.quota_failure().expect("QuotaFailure");
        let subject = quota.violations().get(0).expect("subject");
        assert_eq!(subject.subject().to_str().unwrap_or(""), "project:1");
        assert_eq!(subject.description().to_str().unwrap_or(""), "tokens");
        assert!(status.bad_request().is_some());

        let retry_only = crate::pb::ErrorDetails {
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(delay)),
            ..crate::pb::ErrorDetails::default()
        };
        let retry_status =
            Status::from_error_details(Code::ResourceExhausted, "quota", &retry_only)
                .expect("encode");
        assert!(retry_status.quota_failure().is_none());
        assert_eq!(retry_status.retry_delay(), Some(delay));
        assert!(!retry_status.is_retryable());

        let bad_only = crate::pb::ErrorDetails {
            bad_request: Some(crate::pb::BadRequest::with_field("name", "required")),
            ..crate::pb::ErrorDetails::default()
        };
        let bad_status =
            Status::from_error_details(Code::InvalidArgument, "bad", &bad_only).expect("encode");
        assert!(bad_status.quota_failure().is_none());
        assert!(bad_status.bad_request().is_some());

        assert!(Status::resource_exhausted("tokens")
            .quota_failure()
            .is_none());
        assert!(Status::resource_exhausted("tokens")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .quota_failure()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .quota_failure()
        .is_none());
    }

    #[test]
    fn precondition_failure_reads_packed_precondition_types() {
        let delay = std::time::Duration::from_millis(25);
        let details = crate::pb::ErrorDetails {
            precondition_failure: Some(crate::pb::PreconditionFailure::with_violation(
                "TOS",
                "google.com/cloud",
                "unsigned",
            )),
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(delay)),
            quota_failure: Some(crate::pb::QuotaFailure::with_violation(
                "project:1",
                "tokens",
            )),
            bad_request: Some(crate::pb::BadRequest::with_field("name", "required")),
            ..crate::pb::ErrorDetails::default()
        };
        let status =
            Status::from_error_details(Code::FailedPrecondition, "tos", &details).expect("encode");
        assert!(!status.is_retryable());
        assert_eq!(status.retry_delay(), Some(delay));
        let pre = status.precondition_failure().expect("PreconditionFailure");
        let violation = pre.violations().get(0).expect("violation");
        assert_eq!(violation.r#type().to_str().unwrap_or(""), "TOS");
        assert_eq!(
            violation.subject().to_str().unwrap_or(""),
            "google.com/cloud"
        );
        assert_eq!(violation.description().to_str().unwrap_or(""), "unsigned");
        assert!(status.quota_failure().is_some());
        assert!(status.bad_request().is_some());

        let retry_only = crate::pb::ErrorDetails {
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(delay)),
            ..crate::pb::ErrorDetails::default()
        };
        let retry_status = Status::from_error_details(Code::FailedPrecondition, "tos", &retry_only)
            .expect("encode");
        assert!(retry_status.precondition_failure().is_none());
        assert_eq!(retry_status.retry_delay(), Some(delay));
        assert!(!retry_status.is_retryable());

        let quota_only = crate::pb::ErrorDetails {
            quota_failure: Some(crate::pb::QuotaFailure::with_violation(
                "project:1",
                "tokens",
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let quota_status =
            Status::from_error_details(Code::ResourceExhausted, "quota", &quota_only)
                .expect("encode");
        assert!(quota_status.precondition_failure().is_none());
        assert!(quota_status.quota_failure().is_some());

        let bad_only = crate::pb::ErrorDetails {
            bad_request: Some(crate::pb::BadRequest::with_field("name", "required")),
            ..crate::pb::ErrorDetails::default()
        };
        let bad_status =
            Status::from_error_details(Code::InvalidArgument, "bad", &bad_only).expect("encode");
        assert!(bad_status.precondition_failure().is_none());
        assert!(bad_status.bad_request().is_some());

        assert!(Status::failed_precondition("tos")
            .precondition_failure()
            .is_none());
        assert!(Status::failed_precondition("tos")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .precondition_failure()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .precondition_failure()
        .is_none());
    }

    #[test]
    fn help_reads_packed_documentation_links() {
        let delay = std::time::Duration::from_millis(25);
        let details = crate::pb::ErrorDetails {
            help: Some(crate::pb::Help::with_link(
                "quota docs",
                "https://example.com/quota",
            )),
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(delay)),
            precondition_failure: Some(crate::pb::PreconditionFailure::with_violation(
                "TOS",
                "google.com/cloud",
                "unsigned",
            )),
            quota_failure: Some(crate::pb::QuotaFailure::with_violation(
                "project:1",
                "tokens",
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let status =
            Status::from_error_details(Code::Unavailable, "backend", &details).expect("encode");
        assert!(status.is_retryable());
        assert_eq!(status.retry_delay(), Some(delay));
        let help = status.help().expect("Help");
        let link = help.links().get(0).expect("link");
        assert_eq!(link.description().to_str().unwrap_or(""), "quota docs");
        assert_eq!(
            link.url().to_str().unwrap_or(""),
            "https://example.com/quota"
        );
        assert!(status.precondition_failure().is_some());
        assert!(status.quota_failure().is_some());

        let retry_only = crate::pb::ErrorDetails {
            retry_info: Some(crate::pb::RetryInfo::with_retry_delay(delay)),
            ..crate::pb::ErrorDetails::default()
        };
        let retry_status =
            Status::from_error_details(Code::Unavailable, "backend", &retry_only).expect("encode");
        assert!(retry_status.help().is_none());
        assert_eq!(retry_status.retry_delay(), Some(delay));
        assert!(retry_status.is_retryable());

        let pre_only = crate::pb::ErrorDetails {
            precondition_failure: Some(crate::pb::PreconditionFailure::with_violation(
                "TOS",
                "google.com/cloud",
                "unsigned",
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let pre_status =
            Status::from_error_details(Code::FailedPrecondition, "tos", &pre_only).expect("encode");
        assert!(pre_status.help().is_none());
        assert!(pre_status.precondition_failure().is_some());
        assert!(!pre_status.is_retryable());

        assert!(Status::unavailable("backend").help().is_none());
        assert!(Status::unavailable("backend")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .help()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .help()
        .is_none());
    }

    #[test]
    fn localized_message_reads_packed_locale() {
        let details = crate::pb::ErrorDetails {
            localized_message: Some(crate::pb::LocalizedMessage::with_locale(
                "fr-FR",
                "introuvable",
            )),
            help: Some(crate::pb::Help::with_link(
                "docs",
                "https://example.com/not-found",
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let status =
            Status::from_error_details(Code::NotFound, "not found", &details).expect("encode");
        assert_eq!(status.message(), "not found");
        let local = status.localized_message().expect("LocalizedMessage");
        assert_eq!(local.locale().to_str().unwrap_or(""), "fr-FR");
        assert_eq!(local.message().to_str().unwrap_or(""), "introuvable");
        assert!(status.help().is_some());

        let help_only = crate::pb::ErrorDetails {
            help: Some(crate::pb::Help::with_link(
                "docs",
                "https://example.com/not-found",
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let help_status =
            Status::from_error_details(Code::NotFound, "not found", &help_only).expect("encode");
        assert!(help_status.localized_message().is_none());
        assert!(help_status.help().is_some());
        assert_eq!(help_status.message(), "not found");

        assert!(Status::not_found("row").localized_message().is_none());
        assert!(Status::not_found("row")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .localized_message()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .localized_message()
        .is_none());
    }

    #[test]
    fn request_info_reads_packed_request_id() {
        let mut error = crate::pb::ErrorInfo::new();
        error.set_reason("BACKEND");
        error.set_domain("example.com");
        let details = crate::pb::ErrorDetails {
            request_info: Some(crate::pb::RequestInfo::with_request_id(
                "req-9",
                "encrypted",
            )),
            error_info: Some(error),
            help: Some(crate::pb::Help::with_link(
                "docs",
                "https://example.com/boom",
            )),
            localized_message: Some(crate::pb::LocalizedMessage::with_locale("fr-FR", "boom")),
            ..crate::pb::ErrorDetails::default()
        };
        let status = Status::from_error_details(Code::Internal, "boom", &details).expect("encode");
        let info = status.request_info().expect("RequestInfo");
        assert_eq!(info.request_id().to_str().unwrap_or(""), "req-9");
        assert_eq!(info.serving_data().to_str().unwrap_or(""), "encrypted");
        assert!(status.error_info().is_some());
        assert!(status.help().is_some());
        assert!(status.localized_message().is_some());

        let error_only = crate::pb::ErrorDetails {
            error_info: Some({
                let mut info = crate::pb::ErrorInfo::new();
                info.set_reason("BACKEND");
                info
            }),
            ..crate::pb::ErrorDetails::default()
        };
        let error_status =
            Status::from_error_details(Code::Internal, "boom", &error_only).expect("encode");
        assert!(error_status.request_info().is_none());
        assert!(error_status.error_info().is_some());

        assert!(Status::internal("boom").request_info().is_none());
        assert!(Status::internal("boom")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .request_info()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .request_info()
        .is_none());
    }

    #[test]
    fn resource_info_reads_packed_resource_identity() {
        let details = crate::pb::ErrorDetails {
            resource_info: Some(crate::pb::ResourceInfo::with_resource(
                "sqladmin.googleapis.com/Instance",
                "projects/1/instances/a",
                "project:1",
            )),
            quota_failure: Some(crate::pb::QuotaFailure::with_violation(
                "project:1",
                "instances",
            )),
            request_info: Some(crate::pb::RequestInfo::with_request_id("req-9", "")),
            ..crate::pb::ErrorDetails::default()
        };
        let status = Status::from_error_details(Code::NotFound, "gone", &details).expect("encode");
        let info = status.resource_info().expect("ResourceInfo");
        assert_eq!(
            info.resource_type().to_str().unwrap_or(""),
            "sqladmin.googleapis.com/Instance"
        );
        assert_eq!(
            info.resource_name().to_str().unwrap_or(""),
            "projects/1/instances/a"
        );
        assert_eq!(info.owner().to_str().unwrap_or(""), "project:1");
        assert!(status.quota_failure().is_some());
        assert!(status.request_info().is_some());

        let quota_only = crate::pb::ErrorDetails {
            quota_failure: Some(crate::pb::QuotaFailure::with_violation(
                "project:1",
                "instances",
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let quota_status =
            Status::from_error_details(Code::ResourceExhausted, "quota", &quota_only)
                .expect("encode");
        assert!(quota_status.resource_info().is_none());
        assert!(quota_status.quota_failure().is_some());

        assert!(Status::not_found("gone").resource_info().is_none());
        assert!(Status::not_found("gone")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .resource_info()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .resource_info()
        .is_none());
    }

    #[test]
    fn debug_info_reads_packed_operator_stack() {
        let details = crate::pb::ErrorDetails {
            debug_info: Some(crate::pb::DebugInfo::with_stack(
                "handler.rs:9",
                "nil pointer",
            )),
            localized_message: Some(crate::pb::LocalizedMessage::with_locale("fr-FR", "boom")),
            help: Some(crate::pb::Help::with_link(
                "docs",
                "https://example.com/boom",
            )),
            ..crate::pb::ErrorDetails::default()
        };
        let status = Status::from_error_details(Code::Internal, "boom", &details).expect("encode");
        let debug = status.debug_info().expect("DebugInfo");
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
        assert!(status.localized_message().is_some());
        assert!(status.help().is_some());

        let local_only = crate::pb::ErrorDetails {
            localized_message: Some(crate::pb::LocalizedMessage::with_locale("fr-FR", "boom")),
            ..crate::pb::ErrorDetails::default()
        };
        let local_status =
            Status::from_error_details(Code::Internal, "boom", &local_only).expect("encode");
        assert!(local_status.debug_info().is_none());
        assert!(local_status.localized_message().is_some());

        assert!(Status::internal("boom").debug_info().is_none());
        assert!(Status::internal("boom")
            .with_cause(std::io::Error::new(std::io::ErrorKind::Other, "local"))
            .debug_info()
            .is_none());
        assert!(Status::with_details(
            Code::Internal,
            "junk",
            bytes::Bytes::from_static(b"not-protobuf")
        )
        .debug_info()
        .is_none());
    }

    #[test]
    fn from_error_wraps_local_errors() {
        let refused = Status::from_error(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "refused",
        ));
        assert_eq!(refused.code(), Code::Unavailable);
        assert!(refused.is_retryable());
        let cause = std::error::Error::source(&refused).expect("io cause");
        assert_eq!(
            cause.downcast_ref::<std::io::Error>().expect("io").kind(),
            std::io::ErrorKind::ConnectionRefused
        );

        let inner = Status::not_found("row");
        let same = Status::from_error(inner.clone());
        assert_eq!(same.code(), Code::NotFound);
        assert_eq!(same.message(), "row");
        assert!(!same.is_retryable());

        #[derive(Debug)]
        struct Wrapped(Status);
        impl std::fmt::Display for Wrapped {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "wrapped: {}", self.0)
            }
        }
        impl std::error::Error for Wrapped {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let chained = Status::from_error(Wrapped(Status::permission_denied("no")));
        assert_eq!(chained.code(), Code::PermissionDenied);
        assert_eq!(chained.message(), "no");

        #[derive(Debug)]
        struct Boom;
        impl std::fmt::Display for Boom {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("boom")
            }
        }
        impl std::error::Error for Boom {}
        let unknown = Status::from_error(Boom);
        assert_eq!(unknown.code(), Code::Unknown);
        assert!(unknown.message().contains("boom"));
        assert!(std::error::Error::source(&unknown).is_some());
        assert!(!unknown.is_retryable());
        assert!(unknown.retry_delay().is_none());
    }
}
