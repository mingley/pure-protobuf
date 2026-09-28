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
//! Memory contract (the GF-03 accept gate): the registry holds one
//! entry per *live* entity — owners hold RAII guards and dropping a
//! guard unregisters — plus at most [`DEFAULT_MAX_TRACE_EVENTS`]
//! truncated descriptions per trace. Closed channels, evicted
//! subchannels, shut-down servers, and dead sockets leave nothing
//! behind; see `tests/channelz.rs` for the churn-bound proof.

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

pub use model::{
    ChannelHandle, ChannelId, Connectivity, EndpointAddr, EntityCounts, Registry, ServerHandle,
    ServerId, SocketHandle, SocketId, SocketParent, SocketSecurity, SubchannelHandle, SubchannelId,
};
pub use service::{ChannelzService, DEFAULT_PAGE_SIZE};
pub use trace::{
    DEFAULT_MAX_TRACE_EVENTS, MAX_DESCRIPTION_LEN, Trace, TraceChild, TraceEvent, TraceSeverity,
    TraceSnapshot,
};
