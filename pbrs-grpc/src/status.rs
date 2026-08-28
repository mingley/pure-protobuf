//! gRPC status codes (`grpc-status` / `grpc-message`).

use crate::metadata::Metadata;
use std::fmt;

/// Numeric `grpc-status` code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Code {
    /// Not an error.
    Ok = 0,
    /// Caller cancelled the RPC.
    Cancelled = 1,
    /// Unknown / missing status.
    Unknown = 2,
    /// Client specified an invalid argument.
    InvalidArgument = 3,
    /// Deadline expired before the RPC completed.
    DeadlineExceeded = 4,
    /// Some requested entity was not found.
    NotFound = 5,
    /// Already exists.
    AlreadyExists = 6,
    /// Permission denied.
    PermissionDenied = 7,
    /// Resource exhausted.
    ResourceExhausted = 8,
    /// Failed precondition.
    FailedPrecondition = 9,
    /// Aborted.
    Aborted = 10,
    /// Out of range.
    OutOfRange = 11,
    /// Not implemented.
    Unimplemented = 12,
    /// Internal error.
    Internal = 13,
    /// Unavailable.
    Unavailable = 14,
    /// Unrecoverable data loss.
    DataLoss = 15,
    /// Unauthenticated.
    Unauthenticated = 16,
}

impl Code {
    /// Parse a `grpc-status` integer. Unknown values become [`Code::Unknown`].
    #[must_use]
    pub fn from_i32(n: i32) -> Self {
        match n {
            0 => Self::Ok,
            1 => Self::Cancelled,
            2 => Self::Unknown,
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

    /// Integer used on the wire.
    #[must_use]
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}

/// gRPC status (`grpc-status` plus optional message and trailing metadata).
#[derive(Clone, Debug)]
pub struct Status {
    code: Code,
    message: String,
    metadata: Metadata,
}

impl Status {
    /// Construct a status with no trailing metadata.
    #[must_use]
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            metadata: Metadata::new(),
        }
    }

    /// Status code.
    #[must_use]
    pub fn code(&self) -> Code {
        self.code
    }

    /// Status message (`grpc-message`).
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Trailing metadata attached to this status.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Mutable trailing metadata.
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }

    /// [`Code::Cancelled`].
    #[must_use]
    pub fn cancelled() -> Self {
        Self::new(Code::Cancelled, "cancelled")
    }

    /// [`Code::DeadlineExceeded`].
    #[must_use]
    pub fn deadline_exceeded() -> Self {
        Self::new(Code::DeadlineExceeded, "deadline exceeded")
    }

    /// [`Code::Internal`].
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(Code::Internal, message)
    }

    /// [`Code::Unavailable`].
    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(Code::Unavailable, message)
    }

    /// [`Code::Unimplemented`].
    #[must_use]
    pub fn unimplemented(message: impl Into<String>) -> Self {
        Self::new(Code::Unimplemented, message)
    }

    /// [`Code::NotFound`].
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(Code::NotFound, message)
    }

    /// [`Code::Unknown`].
    #[must_use]
    pub fn unknown(message: impl Into<String>) -> Self {
        Self::new(Code::Unknown, message)
    }

    /// [`Code::InvalidArgument`].
    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(Code::InvalidArgument, message)
    }

    /// [`Code::ResourceExhausted`].
    #[must_use]
    pub fn resource_exhausted(message: impl Into<String>) -> Self {
        Self::new(Code::ResourceExhausted, message)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for Status {}
