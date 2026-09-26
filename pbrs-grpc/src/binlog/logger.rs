//! Loggers: method filtering, size caps, masking, and event emission.
//!
//! [`BinaryLogger`] owns the filter, the sink, and the call-id counter.
//! [`BinaryLogger::start_call`] resolves one RPC against the filter and
//! hands back a [`CallLogger`] that numbers that call's entries. Taps on
//! the call paths hold the `CallLogger` by value (`None` when the method
//! is not logged, so disabled logging is one branch).
//!
//! # Metadata policy
//!
//! Entries starting with `grpc-` never reach the log: [`Metadata`] rejects
//! them on insert, so `grpc-trace-bin` — which A16 would otherwise always
//! log — is never present at the taps. Transport keys (`user-agent`,
//! `content-encoding`, `lb-token`) are omitted on both sides, and
//! credential headers are omitted too (`authorization` everywhere,
//! `proxy-authorization` on the client). Everything else that
//! [`Metadata::is_sensitive`] (or a custom [`Metadata::mark_sensitive`]
//! key) flags is emitted with its value replaced by `[REDACTED]`, per the
//! OB-03 default; [`BinaryLogger::allow_sensitive_values`] opts out with
//! explicit consent.
//!
//! [`Metadata`]: crate::metadata::Metadata
#![allow(
    clippy::disallowed_types,
    reason = "one short std Mutex guards a set-once peer address; never held across await"
)]

use super::entry::{
    Address, AddressType, ClientHeader, EventType, GrpcLogEntry, LoggedMessage, Logger, Payload,
    ServerHeader, Trailer,
};
use super::filter::{BinaryLogFilter, Cap, FilterError, Rule};
use super::sink::{LogRecord, Sink};
use crate::metadata::Metadata;
use crate::status::Status;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Marker replacing a masked metadata value.
const REDACTED: &[u8] = b"[REDACTED]";

/// Owns binary-log configuration and hands out per-call loggers.
#[derive(Debug)]
pub struct BinaryLogger {
    filter: BinaryLogFilter,
    sink: Arc<dyn Sink>,
    next_call_id: AtomicU64,
    mask_sensitive: bool,
}

impl BinaryLogger {
    /// Log per `filter`, emitting to `sink`. Call ids start at 1.
    ///
    /// Sensitive metadata values are masked by default; see
    /// [`Self::allow_sensitive_values`].
    #[must_use]
    pub fn new(filter: BinaryLogFilter, sink: Arc<dyn Sink>) -> Self {
        Self {
            filter,
            sink,
            next_call_id: AtomicU64::new(1),
            mask_sensitive: true,
        }
    }

    /// Build from `GRPC_BINARY_LOG_FILTER`, emitting to `sink`.
    ///
    /// Returns `None` when the variable is unset or blank (logging stays
    /// off) and an error when it is malformed — the caller must refuse to
    /// start logging, per A16.
    pub fn from_env(sink: Arc<dyn Sink>) -> Result<Option<Self>, FilterError> {
        let value = std::env::var("GRPC_BINARY_LOG_FILTER").unwrap_or_default();
        if value.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(Self::new(BinaryLogFilter::parse(&value)?, sink)))
    }

    /// Log sensitive metadata values raw instead of masking them.
    ///
    /// This is the explicit-consent escape hatch for the OB-03 default:
    /// `authorization` stays omitted (A16), but cookies, `-bin` values,
    /// tokens, and custom sensitive keys are recorded as sent. Only use it
    /// on non-production traffic or a sink with matching access controls.
    #[must_use]
    pub fn allow_sensitive_values(mut self) -> Self {
        self.mask_sensitive = false;
        self
    }

    /// Start logging one call to `path` (`/<service>/<method>`).
    ///
    /// Returns `None` when the filter excludes the method, in which case
    /// the caller logs nothing for the call.
    #[must_use]
    pub fn start_call(&self, path: &str, logger: Logger) -> Option<CallLogger> {
        let rule = self.filter.resolve(path)?;
        let call_id = self.next_call_id.fetch_add(1, Ordering::Relaxed);
        Some(CallLogger {
            inner: Arc::new(CallState {
                sink: self.sink.clone(),
                rule,
                logger,
                call_id,
                sequence: AtomicU64::new(1),
                peer: std::sync::Mutex::new(None),
                peer_logged: AtomicBool::new(false),
                mask_sensitive: self.mask_sensitive,
            }),
        })
    }
}

/// Logs one call's entries. Cheap to clone; sequence ids stay ordered.
#[derive(Clone, Debug)]
pub struct CallLogger {
    inner: Arc<CallState>,
}

#[derive(Debug)]
struct CallState {
    sink: Arc<dyn Sink>,
    rule: Rule,
    logger: Logger,
    call_id: u64,
    sequence: AtomicU64,
    peer: std::sync::Mutex<Option<Address>>,
    peer_logged: AtomicBool,
    mask_sensitive: bool,
}

impl CallLogger {
    /// Record the peer address for the first incoming event.
    ///
    /// A server logger attaches it to the client header; a client logger
    /// to the server header, or to the trailer when the response is
    /// trailers-only. Later events never repeat it.
    pub fn set_peer(&self, peer: SocketAddr) {
        let address = match peer {
            SocketAddr::V4(addr) => Address {
                addr_type: AddressType::Ipv4,
                address: addr.ip().to_string(),
                ip_port: u32::from(addr.port()),
            },
            SocketAddr::V6(addr) => Address {
                addr_type: AddressType::Ipv6,
                address: addr.ip().to_string(),
                ip_port: u32::from(addr.port()),
            },
        };
        *self.inner.peer.lock().unwrap_or_else(|e| e.into_inner()) = Some(address);
    }

    /// Log a header sent from client to server.
    ///
    /// Emitted only when the rule logs headers. A server logger attaches
    /// the peer set by [`Self::set_peer`].
    pub fn log_client_header(
        &self,
        md: &Metadata,
        method: &str,
        authority: &str,
        timeout: Option<Duration>,
    ) {
        let Some(cap) = self.inner.rule.headers else {
            return;
        };
        let (metadata, truncated) = self.metadata_entries(md, cap);
        let peer = self.take_peer(matches!(self.inner.logger, Logger::Server));
        self.emit(
            EventType::ClientHeader,
            Some(Payload::ClientHeader(ClientHeader {
                metadata,
                method_name: method.to_owned(),
                authority: authority.to_owned(),
                timeout,
            })),
            truncated,
            peer,
        );
    }

    /// Log a header sent from server to client.
    ///
    /// Emitted only when the rule logs headers. A client logger attaches
    /// the peer set by [`Self::set_peer`].
    pub fn log_server_header(&self, md: &Metadata) {
        let Some(cap) = self.inner.rule.headers else {
            return;
        };
        let (metadata, truncated) = self.metadata_entries(md, cap);
        let peer = self.take_peer(matches!(self.inner.logger, Logger::Client));
        self.emit(
            EventType::ServerHeader,
            Some(Payload::ServerHeader(ServerHeader { metadata })),
            truncated,
            peer,
        );
    }

    /// Log message bytes read from the peer: a client message on a server
    /// logger, a server message on a client logger.
    ///
    /// Emitted only when the rule logs messages. `payload` is the exact
    /// frame payload as transmitted (still compressed when the
    /// Compressed-Flag was set).
    pub fn log_read(&self, payload: &[u8]) {
        let event = match self.inner.logger {
            Logger::Server => EventType::ClientMessage,
            _ => EventType::ServerMessage,
        };
        self.log_message(event, payload);
    }

    /// Log one outbound frame's bytes: a server message on a server
    /// logger, a client message on a client logger.
    ///
    /// Emitted only when the rule logs messages. `frame` is one full
    /// gRPC frame (the 5-byte prefix plus payload); the prefix is
    /// stripped and the transmitted payload logged.
    pub fn log_written(&self, frame: &[u8]) {
        let event = match self.inner.logger {
            Logger::Server => EventType::ServerMessage,
            _ => EventType::ClientMessage,
        };
        let payload = frame.get(5..).unwrap_or(frame);
        self.log_message(event, payload);
    }

    /// Log the client half-close signal.
    pub fn log_half_close(&self) {
        self.emit(EventType::ClientHalfClose, None, false, None);
    }

    /// Log the trailer ending the RPC.
    ///
    /// Emitted only when the rule logs headers. A client logger attaches
    /// a not-yet-logged peer here, which is the trailers-only case.
    pub fn log_trailer(&self, md: &Metadata, status: &Status) {
        let Some(cap) = self.inner.rule.headers else {
            return;
        };
        let (metadata, truncated) = self.metadata_entries(md, cap);
        let peer = self.take_peer(matches!(self.inner.logger, Logger::Client));
        self.emit(
            EventType::ServerTrailer,
            Some(Payload::Trailer(Trailer {
                metadata,
                status_code: status.code() as u32,
                status_message: status.message().to_owned(),
                status_details: status.details().to_vec(),
            })),
            truncated,
            peer,
        );
    }

    /// Log cancellation: requested by the app on a client logger,
    /// detected on a server logger.
    pub fn log_cancel(&self) {
        self.emit(EventType::Cancel, None, false, None);
    }

    /// Which side this logger records.
    #[must_use]
    pub fn role(&self) -> Logger {
        self.inner.logger
    }

    /// This call's id. Never zero.
    #[must_use]
    pub fn call_id(&self) -> u64 {
        self.inner.call_id
    }

    fn log_message(&self, event: EventType, payload: &[u8]) {
        let Some(cap) = self.inner.rule.messages else {
            return;
        };
        let limit = match cap {
            Cap::Full => usize::MAX,
            Cap::Bytes(n) => usize::try_from(n).unwrap_or(usize::MAX),
        };
        let cut = limit.min(payload.len());
        let data = payload.get(..cut).unwrap_or_default().to_vec();
        let truncated = cut < payload.len();
        self.emit(
            event,
            Some(Payload::Message(LoggedMessage {
                length: u32::try_from(payload.len()).unwrap_or(u32::MAX),
                data,
            })),
            truncated,
            None,
        );
    }

    /// Convert application metadata to log entries, omitting transport
    /// and credential keys, masking sensitive values, and capping each
    /// value at `cap`. Keys are never cut, so entries stay identifiable;
    /// only values count toward the cap. Returns the entries and whether
    /// any value was cut.
    fn metadata_entries(&self, md: &Metadata, cap: Cap) -> (Vec<(String, Vec<u8>)>, bool) {
        let limit = match cap {
            Cap::Full => usize::MAX,
            Cap::Bytes(n) => usize::try_from(n).unwrap_or(usize::MAX),
        };
        let client = matches!(self.inner.logger, Logger::Client);
        let mut truncated = false;
        let mut entries = Vec::new();
        let mut push = |key: &str, value: &[u8]| {
            if omit_key(key, client) {
                return;
            }
            let masked = self.inner.mask_sensitive && md.is_sensitive(key);
            let value: &[u8] = if masked { REDACTED } else { value };
            let full = value.len();
            let cut = limit.min(full);
            let value = value.get(..cut).unwrap_or_default();
            truncated |= cut < full;
            entries.push((key.to_owned(), value.to_vec()));
        };
        for (key, value) in md.iter() {
            push(key, value.as_bytes());
        }
        for (key, value) in md.iter_bin() {
            push(key, &value);
        }
        (entries, truncated)
    }

    /// Take the pending peer when this event may carry it.
    fn take_peer(&self, eligible: bool) -> Option<Address> {
        if !eligible || self.inner.peer_logged.swap(true, Ordering::Relaxed) {
            return None;
        }
        self.inner
            .peer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    fn emit(
        &self,
        event: EventType,
        payload: Option<Payload>,
        payload_truncated: bool,
        peer: Option<Address>,
    ) {
        let entry = GrpcLogEntry {
            timestamp: SystemTime::now(),
            call_id: self.inner.call_id,
            sequence_id_within_call: self.inner.sequence.fetch_add(1, Ordering::Relaxed),
            event,
            logger: self.inner.logger,
            payload,
            payload_truncated,
            peer,
        };
        let bytes = entry.encode();
        self.inner.sink.emit(&LogRecord { entry, bytes });
    }
}

/// Keys the log never records. `grpc-*` and pseudo-headers cannot appear:
/// [`Metadata`] rejects them on insert.
fn omit_key(key: &str, client: bool) -> bool {
    key.eq_ignore_ascii_case("user-agent")
        || key.eq_ignore_ascii_case("content-encoding")
        || key.eq_ignore_ascii_case("lb-token")
        || key.eq_ignore_ascii_case("authorization")
        || (client && key.eq_ignore_ascii_case("proxy-authorization"))
}
