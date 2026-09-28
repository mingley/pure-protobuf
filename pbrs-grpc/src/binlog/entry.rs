//! `grpc.binarylog.v1.GrpcLogEntry` model and protobuf wire encoding.
//!
//! The schema is fixed by
//! `third_party/grpc/third_party/grpc-proto/grpc/binlog/v1/binarylog.proto`;
//! this module hand-encodes it so the logger has no codegen dependency.
//! Field numbers and enum values below mirror that file exactly, and
//! `pbrs-grpc/tests/binlog.rs` decodes our bytes with `protoc --decode`
//! against the vendored proto.

use std::time::{Duration, SystemTime};

/// Which event a [`GrpcLogEntry`] records. Discriminants are the
/// `EventType` enum values from `binarylog.proto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventType {
    /// Never emitted; the proto default.
    Unknown = 0,
    /// Header sent from client to server.
    ClientHeader = 1,
    /// Header sent from server to client.
    ServerHeader = 2,
    /// Message sent from client to server.
    ClientMessage = 3,
    /// Message sent from server to client.
    ServerMessage = 4,
    /// The client is done sending.
    ClientHalfClose = 5,
    /// Trailer ending the RPC.
    ServerTrailer = 6,
    /// The RPC was cancelled.
    Cancel = 7,
}

/// Which side emitted a [`GrpcLogEntry`]. Discriminants are the `Logger`
/// enum values from `binarylog.proto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logger {
    /// Never emitted; the proto default.
    Unknown = 0,
    /// Logged by the channel.
    Client = 1,
    /// Logged by the server.
    Server = 2,
}

/// The payload of a [`GrpcLogEntry`]: the `oneof payload` from
/// `binarylog.proto`. Half-close and cancel carry no payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    /// `client_header` (field 6).
    ClientHeader(ClientHeader),
    /// `server_header` (field 7).
    ServerHeader(ServerHeader),
    /// `message` (field 8).
    Message(LoggedMessage),
    /// `trailer` (field 9).
    Trailer(Trailer),
}

/// One `grpc.binarylog.v1.GrpcLogEntry`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcLogEntry {
    /// When the event happened.
    pub timestamp: SystemTime,
    /// Call this entry belongs to. Never zero.
    pub call_id: u64,
    /// 1-based sequence within the call.
    pub sequence_id_within_call: u64,
    /// What happened.
    pub event: EventType,
    /// Who logged it.
    pub logger: Logger,
    /// Event payload, or `None` for half-close and cancel.
    pub payload: Option<Payload>,
    /// `true` when a size cap cut the payload short.
    pub payload_truncated: bool,
    /// Peer address, on the first incoming event only.
    pub peer: Option<Address>,
}

/// `ClientHeader`: application metadata, method, authority, timeout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientHeader {
    /// Application metadata entries, `(key, raw value bytes)`.
    pub metadata: Vec<(String, Vec<u8>)>,
    /// Full method path, `/<service>/<method>`.
    pub method_name: String,
    /// Server identity, usually `<host>` or `<host>:<port>`.
    pub authority: String,
    /// The RPC timeout, when one is set.
    pub timeout: Option<Duration>,
}

/// `ServerHeader`: application metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerHeader {
    /// Application metadata entries, `(key, raw value bytes)`.
    pub metadata: Vec<(String, Vec<u8>)>,
}

/// `Message`: one framed message payload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoggedMessage {
    /// Full on-wire payload length, even when `data` is truncated.
    pub length: u32,
    /// Payload bytes, possibly truncated or empty.
    pub data: Vec<u8>,
}

/// `Trailer`: application metadata plus the status.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trailer {
    /// Application metadata entries, `(key, raw value bytes)`.
    pub metadata: Vec<(String, Vec<u8>)>,
    /// gRPC status code.
    pub status_code: u32,
    /// Status message before transport encoding.
    pub status_message: String,
    /// Raw `grpc-status-details-bin` bytes, when present.
    pub status_details: Vec<u8>,
}

/// `Address`: peer address information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    /// Address family.
    pub addr_type: AddressType,
    /// Dotted IPv4, canonical IPv6 (no scope), or Unix path.
    pub address: String,
    /// IP port, for IPv4/IPv6 only.
    pub ip_port: u32,
}

/// `Address.Type` from `binarylog.proto`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressType {
    /// Never emitted; the proto default.
    Unknown = 0,
    /// Dotted `1.2.3.4`.
    Ipv4 = 1,
    /// Canonical IPv6 (RFC 5952 §4), no scope.
    Ipv6 = 2,
    /// Unix-domain socket path.
    Unix = 3,
}

impl GrpcLogEntry {
    /// Encode this entry as `grpc.binarylog.v1.GrpcLogEntry` wire bytes.
    ///
    /// Encoding follows proto3 canonical form: zero scalars, empty strings,
    /// and unset messages are omitted; every set field uses its
    /// `binarylog.proto` field number.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        encode_timestamp(&mut out, 1, self.timestamp);
        varint_field(&mut out, 2, self.call_id);
        varint_field(&mut out, 3, self.sequence_id_within_call);
        varint_field(&mut out, 4, self.event as u64);
        varint_field(&mut out, 5, self.logger as u64);
        match &self.payload {
            None => {}
            Some(Payload::ClientHeader(h)) => {
                delimited_field(&mut out, 6, |out| {
                    encode_metadata(out, &h.metadata);
                    string_field(out, 2, &h.method_name);
                    string_field(out, 3, &h.authority);
                    if let Some(timeout) = h.timeout {
                        encode_duration(out, 4, timeout);
                    }
                });
            }
            Some(Payload::ServerHeader(h)) => {
                delimited_field(&mut out, 7, |out| {
                    encode_metadata(out, &h.metadata);
                });
            }
            Some(Payload::Message(m)) => {
                delimited_field(&mut out, 8, |out| {
                    varint_field(out, 1, u64::from(m.length));
                    bytes_field(out, 2, &m.data);
                });
            }
            Some(Payload::Trailer(t)) => {
                delimited_field(&mut out, 9, |out| {
                    encode_metadata(out, &t.metadata);
                    varint_field(out, 2, u64::from(t.status_code));
                    string_field(out, 3, &t.status_message);
                    bytes_field(out, 4, &t.status_details);
                });
            }
        }
        if self.payload_truncated {
            varint_field(&mut out, 10, 1);
        }
        if let Some(peer) = &self.peer {
            delimited_field(&mut out, 11, |out| {
                varint_field(out, 1, peer.addr_type as u64);
                string_field(out, 2, &peer.address);
                varint_field(out, 3, u64::from(peer.ip_port));
            });
        }
        out
    }
}

/// Encode one `Metadata` message's `entry` list (field 1, repeated).
fn encode_metadata(out: &mut Vec<u8>, metadata: &[(String, Vec<u8>)]) {
    if metadata.is_empty() {
        return;
    }
    delimited_field(out, 1, |out| {
        for (key, value) in metadata {
            delimited_field(out, 1, |out| {
                string_field(out, 1, key);
                bytes_field(out, 2, value);
            });
        }
    });
}

/// Encode a `google.protobuf.Timestamp` field.
fn encode_timestamp(out: &mut Vec<u8>, field: u32, timestamp: SystemTime) {
    let (seconds, nanos) = match timestamp.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(elapsed) => (
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
            i32::try_from(elapsed.subsec_nanos()).unwrap_or(i32::MAX),
        ),
        Err(before) => {
            let elapsed = before.duration();
            let mut seconds = -i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX);
            let mut nanos = -i32::try_from(elapsed.subsec_nanos()).unwrap_or(i32::MAX);
            if nanos < 0 {
                seconds -= 1;
                nanos += 1_000_000_000;
            }
            (seconds, nanos)
        }
    };
    delimited_field(out, field, |out| {
        if seconds != 0 {
            varint_field(out, 1, u64::from_ne_bytes(seconds.to_ne_bytes()));
        }
        if nanos != 0 {
            varint_field(out, 2, u64::from_ne_bytes(i64::from(nanos).to_ne_bytes()));
        }
    });
}

/// Encode a `google.protobuf.Duration` field.
fn encode_duration(out: &mut Vec<u8>, field: u32, duration: Duration) {
    delimited_field(out, field, |out| {
        let seconds = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
        let nanos = i32::try_from(duration.subsec_nanos()).unwrap_or(i32::MAX);
        if seconds != 0 {
            varint_field(out, 1, u64::from_ne_bytes(seconds.to_ne_bytes()));
        }
        if nanos != 0 {
            varint_field(out, 2, u64::from_ne_bytes(i64::from(nanos).to_ne_bytes()));
        }
    });
}

/// Append a varint field unless the value is the proto3 default (zero).
fn varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
    if value == 0 {
        return;
    }
    tag(out, field, 0);
    varint(out, value);
}

/// Append a string field unless empty.
fn string_field(out: &mut Vec<u8>, field: u32, value: &str) {
    if value.is_empty() {
        return;
    }
    tag(out, field, 2);
    varint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

/// Append a bytes field unless empty.
fn bytes_field(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    if value.is_empty() {
        return;
    }
    tag(out, field, 2);
    varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

/// Append a length-delimited field whose contents `encode` writes.
fn delimited_field(out: &mut Vec<u8>, field: u32, encode: impl FnOnce(&mut Vec<u8>)) {
    let mut nested = Vec::new();
    encode(&mut nested);
    tag(out, field, 2);
    varint(out, nested.len() as u64);
    out.extend_from_slice(&nested);
}

/// Append a field tag.
fn tag(out: &mut Vec<u8>, field: u32, wire_type: u8) {
    varint(out, u64::from(field) * 8 + u64::from(wire_type));
}

/// Append an unsigned varint.
fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}
