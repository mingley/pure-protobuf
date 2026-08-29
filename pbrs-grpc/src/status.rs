//! gRPC status: [`Code`], `grpc-message`, trailing metadata, and
//! `grpc-status-details-bin`.

use crate::metadata::Metadata;
use bytes::Bytes;
use std::fmt;
use std::sync::OnceLock;

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
}

/// A gRPC status: a [`Code`], an optional message, optional trailing
/// metadata, and optional `grpc-status-details-bin`.
///
/// `Status` is the error type of every fallible operation in this crate, so it
/// is kept to two machine words. The message, metadata, and details live
/// behind a pointer that is only allocated when one of them is set, which
/// means the common `Ok` and bare-code cases allocate nothing.
///
/// ```
/// use pbrs_grpc::{Code, Status};
///
/// let status = Status::not_found("no such row");
/// assert_eq!(status.code(), Code::NotFound);
/// assert_eq!(status.message(), "no such row");
/// assert_eq!(status.to_string(), "NOT_FOUND: no such row");
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
/// [`Self::set_rpc`] / [`Self::set_error_details`] replace the protobuf
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
    #[must_use]
    pub fn with_details(code: Code, message: impl Into<String>, details: impl Into<Bytes>) -> Self {
        let mut status = Self::new(code, message);
        status.set_details(details);
        status
    }

    /// Encode `rpc` as `grpc-status-details-bin`.
    ///
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
    /// ```
    /// use pbrs_grpc::pb::{Any, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let mut info = ErrorInfo::new();
    /// info.set_reason("API_DISABLED");
    /// info.set_domain("example.com");
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
    pub fn with_error_details(
        code: Code,
        message: impl Into<String>,
        details: impl IntoIterator<Item = crate::pb::Any>,
    ) -> Result<Self, Self> {
        Self::from_rpc(&crate::pb::Status::with_details(code, message, details))
    }

    /// [`Self::with_error_details`] in place. Trailing metadata is left
    /// alone; [`Self::with_error_details`] mints a fresh status.
    ///
    /// ```
    /// use pbrs_grpc::pb::{Any, ErrorInfo};
    /// use pbrs_grpc::{Code, Status};
    ///
    /// let mut status = Status::not_found("gone");
    /// status.metadata_mut().insert("x-retry-after", "30")?;
    /// let mut info = ErrorInfo::new();
    /// info.set_reason("STOCKOUT");
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
    pub fn from_error_details(
        code: Code,
        message: impl Into<String>,
        details: &crate::pb::ErrorDetails,
    ) -> Result<Self, Self> {
        Self::with_error_details(code, message, details.to_anys()?)
    }

    /// [`Self::from_error_details`] in place. Trailing metadata is left
    /// alone.
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

    /// Map an HTTP/2 error onto [`Code::Unavailable`].
    ///
    /// Connection death (`GOAWAY`, I/O, `REFUSED_STREAM`) is marked so a
    /// unary or server-streaming RPC can redial this call once. Stream
    /// resets and user errors stay plain `UNAVAILABLE` and are not retried.
    pub(crate) fn from_h2(err: impl Into<h2::Error>) -> Self {
        let err = err.into();
        let mut status = Self::unavailable(err.to_string());
        if h2_lost_connection(&err) {
            status.mark_transport();
        }
        status
    }

    /// Like [`Self::from_h2`], but non-connection failures stay
    /// [`Code::Internal`] so a flow-control `send_data` error is not
    /// reported as a dead peer.
    pub(crate) fn from_h2_send(err: impl Into<h2::Error>) -> Self {
        let err = err.into();
        if h2_lost_connection(&err) {
            let mut status = Self::unavailable(err.to_string());
            status.mark_transport();
            status
        } else {
            Self::internal(err.to_string())
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

impl std::error::Error for Status {}

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
        Self::new(code, err.to_string())
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
    }

    #[test]
    fn io_connection_refused_is_unavailable() {
        let err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let status = Status::from(err);
        assert_eq!(status.code(), Code::Unavailable);
        assert!(!status.is_transport());
    }

    #[test]
    fn refused_stream_is_transport_lost() {
        let status = Status::from_h2(h2::Reason::REFUSED_STREAM);
        assert_eq!(status.code(), Code::Unavailable);
        assert!(status.is_transport());
    }

    #[test]
    fn peer_unavailable_message_is_not_transport_lost() {
        let status = Status::unavailable("too many concurrent RPCs");
        assert!(!status.is_transport());
    }
}
