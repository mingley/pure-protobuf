//! gRPC binary logging (gRFC A16).
//!
//! Records RPCs as `grpc.binarylog.v1.GrpcLogEntry` wire bytes: headers,
//! messages, half-close, trailers, and cancellation, each stamped with a
//! call id and a per-call sequence number. [`BinaryLogger`] owns the
//! [`BinaryLogFilter`] (usually from `GRPC_BINARY_LOG_FILTER`), the
//! [`Sink`] records go to, and the call-id counter.
//!
//! Attach one to a [`Channel`](crate::Channel) or a
//! [`Server`](crate::Server) / [`Router`](crate::Router) with
//! `binary_logger`; calls whose method the filter excludes log nothing.
//! Message and header classes are logged only when the method's `{h;m}`
//! config names them, each entry capped at its `:N` byte limit with
//! `payload_truncated` set on overflow. Credential headers are omitted
//! and sensitive metadata values masked per the OB-03 default; see
//! [`BinaryLogger::allow_sensitive_values`] for the consent-gated opt-out.
//!
//! ```
//! use std::sync::Arc;
//! use pbrs_grpc::binlog::{BinaryLogFilter, BinaryLogger, Logger, VecSink};
//!
//! let sink = Arc::new(VecSink::new());
//! let binlog = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
//! let call = binlog.start_call("/demo.Echo/Ping", Logger::Client).expect("logged");
//! call.log_half_close();
//! assert_eq!(sink.len(), 1);
//! ```

mod entry;
mod filter;
mod logger;
mod sink;

pub use entry::{
    Address, AddressType, ClientHeader, EventType, GrpcLogEntry, LoggedMessage, Logger, Payload,
    ServerHeader, Trailer,
};
pub use filter::{BinaryLogFilter, Cap, FilterError, Rule};
pub use logger::{BinaryLogger, CallLogger};
pub use sink::{FileSink, FileSinkError, LogRecord, Sink, VecSink};
