//! Channelz introspection: A14 data model + `grpc.channelz.v1` service.
//!
//! A process-global [`Registry`] tracks channels, subchannels,
//! servers, and sockets behind 64-bit IDs; the generated
//! `grpc.channelz.v1.Channelz` service answers the standard
//! `GetTopChannels`/`GetServers`/`GetServer`/`GetServerSockets`/
//! `GetChannel`/`GetSubchannel`/`GetSocket` RPCs so grpcdebug and
//! other tooling can inspect this process. Every entity carries an
//! A3 [`Trace`]: a bounded ring of timestamped events plus a
//! total count, so memory stays flat under churn.
//!
//! The registry removes an entity when its owner drops and bounds each trace
//! to [`DEFAULT_MAX_TRACE_EVENTS`] descriptions. A per-thread recording cache
//! can retain up to two socket entries after unregistration until eviction or
//! thread exit. In-flight streams may also retain an entry until accounting ends.

#![allow(missing_docs, reason = "messages come from the code generator")]
#![allow(
    clippy::disallowed_types,
    reason = "sync-only std locks; held for map clones and flag flips, never across await"
)]

// The generated stubs (messages, client, server traits) come from the
// vendored grpc-proto channelz.proto via build.rs, like health.
include!(concat!(env!("OUT_DIR"), "/channelz.rs"));

mod model;
mod service;
mod trace;

pub(crate) use model::{ChannelEntry, SocketEntry, socket_entry};
pub use model::{
    ChannelHandle, ChannelId, Connectivity, EndpointAddr, EntityCounts, Registry, ServerHandle,
    ServerId, SocketHandle, SocketId, SocketParent, SocketSecurity, SubchannelHandle, SubchannelId,
};
pub use service::{ChannelzService, DEFAULT_PAGE_SIZE};
pub use trace::{
    DEFAULT_MAX_TRACE_EVENTS, MAX_DESCRIPTION_LEN, Trace, TraceChild, TraceEvent, TraceSeverity,
    TraceSnapshot,
};
