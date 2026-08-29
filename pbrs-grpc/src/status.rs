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
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The rarely-populated half of a [`Status`], boxed so `Result<T, Status>`
/// stays small on the hot path.
#[derive(Clone, Debug, Default)]
struct Detail {
    message: String,
    metadata: Metadata,
    details: Bytes,
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
    #[must_use]
    pub fn details(&self) -> &[u8] {
        self.detail.as_ref().map_or(&[], |d| d.details.as_ref())
    }

    /// Attach `details` as `grpc-status-details-bin`.
    ///
    /// The gRPC spec puts a serialized `google.rpc.Status` here: the same
    /// code and message as the ASCII trailers, plus a repeated
    /// `google.protobuf.Any` payload. This method does not parse or generate
    /// that message; it ships whatever bytes you give it. An empty slice
    /// omits the trailer.
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
}
