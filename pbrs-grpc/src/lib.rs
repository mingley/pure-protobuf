//! An HTTP/2 gRPC client and server for [`pbrs`] and optional Prost messages.
//!
//! Supports unary and streaming calls, generated service stubs, TLS,
//! compression, and configurable connection, RPC, and message limits.
//! Optional features add Tower and tonic integration.
//!
//! Uses Tokio and an embedded `h2` backend. gzip and deflate use
//! `miniz_oxide`; TLS uses rustls with
//! [Graviola](https://crates.io/crates/graviola) by default.
//! Supply application-owned TLS configs through [`ServerTls::from_rustls`]
//! and [`ClientTls::from_rustls`].
//!
//! # Quickstart
//!
//! Given `proto/hello.proto`:
//!
//! ```proto
//! syntax = "proto3";
//! package helloworld;
//!
//! service Greeter {
//!   rpc SayHello (HelloRequest) returns (HelloReply);
//! }
//!
//! message HelloRequest { string name = 1; }
//! message HelloReply   { string message = 1; }
//! ```
//!
//! generate client and server stubs from `build.rs`:
//!
//! ```no_run
//! // build.rs
//! fn main() {
//!     pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])
//!         .expect("codegen");
//! }
//! ```
//!
//! then implement the generated trait and serve it:
//!
//! ```no_run
//! use pbrs_grpc::hello::{Greeter, GreeterServer, HelloReply, HelloRequest};
//! use pbrs_grpc::{Request, Response, Status};
//!
//! struct MyGreeter;
//!
//! impl Greeter for MyGreeter {
//!     async fn say_hello(
//!         &self,
//!         request: Request<HelloRequest>,
//!     ) -> Result<Response<HelloReply>, Status> {
//!         let mut reply = HelloReply::new();
//!         reply.set_message(format!("hello {}", request.get_ref().name()));
//!         Ok(Response::new(reply))
//!     }
//! }
//!
//! # async fn example() -> Result<(), Status> {
//! GreeterServer::new(MyGreeter)
//!     .serve("127.0.0.1:50051".parse().expect("addr"))
//!     .await
//! # }
//! ```
//!
//! Methods you omit on the generated trait answer `UNIMPLEMENTED`.
//!
//! The client side mirrors it:
//!
//! ```no_run
//! # async fn example() -> Result<(), pbrs_grpc::Status> {
//! use pbrs_grpc::hello::{GreeterClient, HelloRequest};
//! use pbrs_grpc::Request;
//!
//! let client = GreeterClient::connect("127.0.0.1:50051").await?;
//!
//! let mut req = HelloRequest::new();
//! req.set_name("world");
//! let reply = client.say_hello(Request::new(req)).await?;
//! println!("{}", reply.get_ref().message());
//! # Ok(())
//! # }
//! ```
//!
//! `examples/greeter` is a complete external crate with its own proto,
//! `build.rs`, health, and reflection services.
//!
//! See [`docs/grpc.md`] in the repository for the full guide, and
//! [`docs/benchmarks.md`] for measured numbers.
//!
//! [`docs/grpc.md`]: https://github.com/mingley/pure-protobuf/blob/main/docs/grpc.md
//! [`docs/benchmarks.md`]: https://github.com/mingley/pure-protobuf/blob/main/docs/benchmarks.md
//!
//! # Main APIs
//!
//! | Purpose | Types |
//! |---|---|
//! | Servers and routing | [`Server`], [`Router`], [`Service`], [`Rpc`], [`Incoming`] |
//! | Clients and calls | [`Channel`], [`ChannelConfig`], [`Call`], [`CallHandle`] |
//! | TLS and peer identity | [`Identity`], [`ServerTls`], [`ClientTls`], [`PeerIdentity`] |
//! | Request and response data | [`Request`], [`Response`], [`Metadata`], [`Extensions`] |
//! | Errors | [`Status`], [`Code`], [`ErrorDetails`], [`Any`] |
//! | Streaming | [`Streaming`], [`StreamSender`], [`Framed`] |
//! | Interceptors | [`Interceptor`], [`ClientInterceptor`], [`ResponseInterceptor`] |
//! | Limits | [`ServerConfig`], [`ChannelConfig`], [`MessageLimits`] |
//! | Built-in services | [`health`], [`reflection`], [`channelz`] |
//!
//! Client interceptors run before a stream opens; returning an error sends
//! nothing. Server interceptors run before the handler. Response interceptors
//! run after a successful handler result or received response. Attach hooks
//! with the corresponding `intercept` and `on_response` methods; repeated
//! attachments run in order.
//!
//! Request overrides can change timeout, compression, and wait-for-ready.
//! The `*_is_set` methods tell an interceptor whether a caller supplied an
//! override. Configuration getters expose the channel or server defaults.
//! [`Outgoing::connected`] is a snapshot when the hook runs, not a readiness
//! guarantee. See each method for its override and clearing rules.
//!
//! Rich errors preserve unknown detail types through [`ErrorDetails::unknown`].
//! Reflection also serves the v1alpha path for older clients.
//!
//! # Safety
//!
//! The protocol-facing modules keep peer input away from `unsafe`: gRPC
//! framing, dispatch, transport, TLS, compression, codec, resolver,
//! load-balancing, authorization, binary logging, service-config, metadata,
//! status, request, response, and stream code all carry
//! `#[forbid(unsafe_code)]`. The private imported HTTP/2 backend retains one
//! upstream HPACK UTF-8 view behind a documented constructor invariant and a
//! function-local exemption; the transport module's forbid remains intact.
//! Resource limits additionally bound peer-controlled work and memory.
//!
//! ## Threat model
//!
//! The peer is assumed hostile and able to send any bytes at any rate. Each
//! defence below is enforced before the memory it guards is committed.
//!
//! | Attack | Defence | Default |
//! |---|---|---|
//! | Huge declared message length | Refused from the 5-byte frame header, before the payload is buffered | 4 MiB ([`MessageLimits`]) |
//! | Decompression bomb | Bounded inflate that stops one byte past the cap; opt out of inbound gzip entirely | 4 MiB ([`gzip::decode_limited`]); opt-out [`ServerConfig::accept_compressed`] / [`ChannelConfig::accept_compressed`] |
//! | Metadata flood | HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE` | 16 KiB ([`ServerConfig::max_header_list_size`]) |
//! | Stream flood | HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`; extras wait, they are not `RESOURCE_EXHAUSTED` | 256 ([`ServerConfig::max_concurrent_streams`]) |
//! | Unbounded buffering | Per-connection window and send buffer | 16 MiB / 1 MiB |
//! | Slow reader amplification | Capacity is released only after a chunk is handed on, so a slow handler throttles the peer | always on |
//! | Deeply nested protobuf | Recursion limit in [`pbrs`] | always on |
//! | Truncated or malformed frames | Rejected as a protocol error, never treated as an empty message | always on |
//! | Reserved metadata injection | `grpc-*` and hop-by-hop headers are never read from or written to user metadata | always on |
//! | Cleartext interception | TLS 1.2/1.3, ALPN `h2` required; built-in constructors configure certificate verification, caller configs retain their trusted application policy | opt-in [`Server::serve_tls`] / [`Channel::connect_tls`] |
//! | Impersonation | WebPKI roots or a CA you pin; mTLS via [`ServerTls::mtls`]; verified client chain on [`Rpc::peer_identity`] | opt-in |
//! | Unauthenticated Unix peer | Connecting process uid/gid/pid on [`Rpc::peer_cred`] from `SO_PEERCRED` / `LOCAL_PEERCRED` | Unix accept loop |
//! | Long-lived connection hold | GOAWAY (server) or close (client) after age or idle; keepalive PINGs do not reset idle and do not postpone age | opt-in [`ServerConfig::max_connection_age`] / [`ServerConfig::max_connection_idle`] / [`ChannelConfig::max_connection_age`] / [`ChannelConfig::max_connection_idle`] |
//! | Slow handshake | Whole client dial, and each of the server TLS accept and HTTP/2 preface, is timed out | 20 s ([`ChannelConfig::connect_timeout`] / [`ServerConfig::handshake_timeout`]) |
//! | Accept storm | Drop excess TCP/Unix accepts before a handshake task is spawned | opt-in [`ServerConfig::max_concurrent_connections`] |
//! | Unbounded handler concurrency | Refuse further RPCs with `RESOURCE_EXHAUSTED` before the handler runs | opt-in [`ServerConfig::max_concurrent_rpcs`] |
//! | Unbounded client RPC concurrency | Refuse further RPCs with `RESOURCE_EXHAUSTED` before the stream opens | opt-in [`ChannelConfig::max_concurrent_rpcs`] |
//! | Handler that never returns | Cap the RPC even when the client omits `grpc-timeout` | opt-in [`ServerConfig::timeout`] |
//! | Silent TCP half-open | TCP `SO_KEEPALIVE` (not HTTP/2 PING) | opt-in [`ServerConfig::tcp_keepalive`] / [`ChannelConfig::tcp_keepalive`] |
//! | HTTP/2 rapid reset | Cap remotely-reset streams waiting in the accept queue | 20 ([`DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS`], override [`ServerConfig::max_pending_accept_reset_streams`]) |
//! | HTTP/2 protocol-error RST flood | Cap locally-reset streams caused by invalid frames | 1024 ([`DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS`], override [`ServerConfig::max_local_error_reset_streams`]) |
//! | HTTP/2 locally-reset stream memory | Remember stream IDs after we RST; oldest purged, not GOAWAY | 50 ([`DEFAULT_MAX_CONCURRENT_RESET_STREAMS`], override [`ServerConfig::max_concurrent_reset_streams`]) |
//! | HTTP/2 small-DATA flood | Cap framing overhead of tiny DATA frames | 25600 ([`DEFAULT_DATA_FRAME_BUDGET`], override [`ServerConfig::data_frame_budget`]) |
//! | HTTP/2 CONTINUATION flood | Cap CONTINUATION frames on an unfinished header block; that connection drops | always (`h2`, scaled from [`ServerConfig::max_header_list_size`]) |
//! | Unfinished HEADERS | Header block without `END_HEADERS` stalls that stream only; the accept loop still serves | always |
//! | Client RST after the request is read | Signal [`Request::cancelled`], then drop a still-pending handler; abort a stream drain waiting for the next message | always |
//! | Client cancel after a client-streaming half-close | RST while the unary response is pending (handle, drop, or deadline) | always |
//! | Client request-stream abort ([`StreamSender::fail`]) | RST CANCEL; the [`Call`] resolves with that status (client-streaming, or bidi before headers — not `UNAVAILABLE` from the reset); after bidi headers the received [`Streaming`] sees [`Code::Cancelled`], not that status | always |
//! | Client streaming deadline | RST the send half before headers (server-streaming and bidi) and after a half-close; after those headers RST the parked send half | always |
//! | Non-gRPC HTTP/2 (GET, grpc-web, JSON, `grpc+json`) | HTTP 405 / 415 with no `grpc-status`, before an RPC slot is taken | always |
//!
//! h2c (cleartext prior-knowledge HTTP/2) remains the default, because that is
//! what a loopback test and a mesh sidecar speak. Production that is not
//! behind a sidecar should call [`Server::serve_tls`] / [`Channel::connect_tls`].
//! Built-in TLS constructors configure peer verification. Caller-owned rustls
//! configs are trusted application inputs: custom verifier security and ticket
//! invalidation belong to the caller. Disabling peer verification is unsupported.
//!
//! Set finite connection, RPC, message, and application queue limits for the
//! deployment. HTTP/2 stream limits govern each connection;
//! [`ServerConfig::max_concurrent_rpcs`] limits active handlers across the server
//! and rejects excess calls with `RESOURCE_EXHAUSTED`.
//!
//! ## `unsafe`
//!
//! The hand-written gRPC framing, dispatch, transport, TLS, compression,
//! codec, resolver, load-balancing, authorization, binary-logging,
//! service-config, metadata, status, request, response, and stream modules
//! carry `#[forbid(unsafe_code)]`, which cannot be overridden from inside the
//! module.
//!
//! Two hand-written Linux-only OS helpers use scoped `unsafe`, each with a
//! local `SAFETY` comment:
//!
//! - `proxy` sets `TCP_USER_TIMEOUT` with a raw `setsockopt` because `socket2`
//!   exposes no stable safe wrapper for that option.
//! - `rt::per_core` calls `sched_setaffinity` / `sched_getcpu` to pin
//!   per-core runtime threads.
//!
//! The private imported HTTP/2 backend additionally retains upstream's
//! `BytesStr::as_str` UTF-8 view. Its constructors validate UTF-8 or copy a
//! Rust string, and its `Bytes` storage is immutable. That single access has
//! a function-local exemption; the native transport module remains forbidden
//! from using unsafe code.
//!
//! Modules that `include!` generated message code ([`hello`], [`testing`],
//! [`health`], [`reflection`], [`pb`], [`channelz`], [`orca`]) are also outside
//! those per-module forbids; `pbrs` gencode uses `unsafe` for zeroed-message
//! construction, and that is a `pbrs` property rather than a gRPC transport
//! one.
//!
//! Invalid peer input is handled through gRPC status or HTTP/2 protocol errors.
//! Workspace lints restrict panics, unchecked indexing, and lossy casts in
//! shipping code. Negative tests and fuzzing exercise these paths.
//!
//! # Tuning
//!
//! Configure connection count, HTTP/2 windows, and stream queues for the
//! workload:
//!
//! 1. **[`ChannelConfig::connections`]** — one connection is one `h2` driver
//!    task. A pool can distribute driver work across runtime workers.
//! 2. **Window sizes** — the 16 MiB default keeps a 4 MiB message from
//!    stalling on a `WINDOW_UPDATE` round trip. Lower it only under memory
//!    pressure: [`Server::initial_stream_window_size`] /
//!    [`ChannelConfig::initial_stream_window_size`].
//! 3. **Stream queue depth** — the buffer a streaming handler passes to
//!    [`Streaming::channel`], and [`Channel::stream_buffer`] /
//!    [`ChannelConfig::stream_buffer`] on the client. The wire layer writes
//!    whatever is queued as one batch, so deeper means fewer and larger writes
//!    at the cost of memory. Received streams are decoded inline and have no
//!    queue to size.
//!
//! Compression trades CPU for bandwidth. Enable it with
//! [`Request::set_compress`], [`ServerConfig::send_compressed`], or
//! [`ChannelConfig::send_compressed`]. The default gzip effort is 1; configure it
//! with `gzip_compression_level`. Inbound compression is accepted by default
//! and can be disabled with `accept_compressed`. [`Response::encoding`] reports
//! the peer's selected encoding (`None` for identity).
//!
//! HTTP/2 windows are fixed unless adaptive flow control is explicitly enabled.
//! Configure reset-stream retention and flood limits through [`ServerConfig`]
//! and [`ChannelConfig`]. Client handshake settings take effect at connection
//! creation; changing a live channel cannot renegotiate HTTP/2 SETTINGS.
//!
//! # Relationship to the rest of the workspace
//!
//! [`pbrs`] does not depend on this crate. The native transport runs without
//! tonic; the optional `tonic` feature mounts native generated services
//! in tonic. Use `protobuf-tonic` to keep a tonic service with pbrs messages.

#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::needless_doctest_main,
    reason = "the quickstart shows a real build.rs, which needs its main"
)]

// Generated stubs refer to this crate by name. Inside the crate itself that
// name would not resolve without this alias.
extern crate self as pbrs_grpc;

// `forbid` on each hand-written module cannot be relaxed from inside it, so
// the no-`unsafe` claim is machine-checked rather than a convention. Modules
// that `include!` generated message code are excluded from `forbid`.
#[forbid(unsafe_code)]
pub mod codec;
#[forbid(unsafe_code)]
pub mod compat;
#[forbid(unsafe_code)]
pub mod compression;
#[forbid(unsafe_code)]
pub mod copy_counts;
#[forbid(unsafe_code)]
pub mod gzip;
#[forbid(unsafe_code)]
pub mod interop_cases;
#[forbid(unsafe_code)]
pub mod telemetry;
#[forbid(unsafe_code)]
pub mod timeout;
#[cfg(feature = "grpc-web")]
pub mod web;

#[cfg(feature = "tonic")]
pub mod tonic_server;
#[cfg(feature = "tower")]
pub mod tower_client;
#[cfg(feature = "tower")]
pub mod tower_server;

pub mod channelz;
pub mod health;
pub mod hello;
pub mod orca;
#[cfg(feature = "otel")]
pub mod otel;
pub mod pb;
pub mod reflection;
pub mod testing;

#[forbid(unsafe_code)]
pub mod authz;
#[forbid(unsafe_code)]
mod bdp;
#[forbid(unsafe_code)]
pub mod binlog;
#[forbid(unsafe_code)]
mod client;

mod config;
#[forbid(unsafe_code)]
mod interceptor;
#[forbid(unsafe_code)]
mod keepalive;
#[forbid(unsafe_code)]
pub mod lb;
#[forbid(unsafe_code)]
mod limits;
#[forbid(unsafe_code)]
mod metadata;
mod proxy;
#[forbid(unsafe_code)]
mod request;
#[forbid(unsafe_code)]
pub mod resolver;
// `rt::per_core` pins threads with one Linux affinity syscall; the rest of
// the seam stays safe-only (see the forbid on `mod manual`).
mod rt;
#[forbid(unsafe_code)]
mod server;
#[forbid(unsafe_code)]
pub mod service_config;
#[forbid(unsafe_code)]
mod status;
#[forbid(unsafe_code)]
mod stream;
#[forbid(unsafe_code)]
mod tcp;
#[forbid(unsafe_code)]
mod tls;
#[forbid(unsafe_code)]
mod transport;
// Pinned upstream backend, with one existing validated UTF-8 access scoped
// inside the import. The transport seam retains its unsafe-code forbid.
#[rustfmt::skip]
mod h2_backend;
#[forbid(unsafe_code)]
mod wire;

/// Re-exports that `protoc-gen-pbrs` stubs name explicitly.
///
/// Generated code must not assume the surrounding crate depends on `tokio` by
/// that name, so it reaches for these instead. Not a stable API.
#[doc(hidden)]
#[forbid(unsafe_code)]
pub mod codegen_support {
    pub use tokio::io::{AsyncRead, AsyncWrite};
    pub use tokio::net::TcpListener;
    #[cfg(unix)]
    pub use tokio::net::UnixListener;
}

pub use client::{Channel, Endpoint, RetryStats, Target};
pub use codec::CodecMessage;
pub use compression::{Codec, CompressionAlgorithm};
pub use config::{
    ChannelConfig, DEFAULT_ADAPTIVE_WINDOW_INITIAL_SIZE, DEFAULT_ADAPTIVE_WINDOW_MAX_SIZE,
    DEFAULT_CONNECT_TIMEOUT, DEFAULT_DATA_FRAME_BUDGET, DEFAULT_GZIP_COMPRESSION_LEVEL,
    DEFAULT_HEADER_TABLE_SIZE, DEFAULT_KEEP_ALIVE_TIMEOUT, DEFAULT_MAX_CONCURRENT_RESET_STREAMS,
    DEFAULT_MAX_CONCURRENT_STREAMS, DEFAULT_MAX_CONNECTION_AGE_GRACE, DEFAULT_MAX_FRAME_SIZE,
    DEFAULT_MAX_HEADER_LIST_SIZE, DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS,
    DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS, DEFAULT_MAX_SEND_BUFFER_SIZE,
    DEFAULT_RESET_STREAM_DURATION, DEFAULT_STREAM_BUFFER, DEFAULT_WINDOW_SIZE, ServerConfig,
};
pub use copy_counts::{CopyCounts, copy_counts, reset_copy_counts};
#[cfg(feature = "copy-counts")]
pub use copy_counts::{SchedulerCounts, reset_scheduler_counts, scheduler_counts};
/// `futures_core::Stream`, so [`Streaming`] can be driven with `StreamExt`.
pub use futures_core::Stream;
/// `futures_core::future::FusedFuture`, so a finished [`Call`] is skipped by
/// combinators that honour termination.
pub use futures_core::future::FusedFuture;
/// `futures_core::stream::FusedStream`, so a finished [`Streaming`] is skipped by
/// combinators that honour termination.
pub use futures_core::stream::FusedStream;
/// Per-RPC typed bag: insert in an interceptor, read in the handler.
pub use http::Extensions;
pub use interceptor::{
    ClientInterceptor, Intercepted, Interceptor, ResponseInterceptor, ServiceExt,
};
pub use limits::{ByteBudgetTracker, BytePermit, DEFAULT_MAX_DECODING_MESSAGE_SIZE, MessageLimits};
pub use metadata::Metadata;
pub use pb::{Any, ErrorDetails};
#[cfg(unix)]
pub use request::UdsConnectInfo;
pub use request::{
    Call, CallHandle, Outgoing, Parts, Request, Response, ResponseParts, TcpConnectInfo,
    TlsConnectInfo,
};
pub use server::{
    ConnectionInfo, Incoming, IncomingAccept, PeerCred, Router, Rpc, Server, Service,
};
pub use service_config::{
    HedgingPolicy, LbPolicyConfig, MethodConfig, MethodName, RetryPolicy, RetryThrottler,
    RetryThrottling, RingHashConfig, ServiceConfig, WeightedRoundRobinConfig, pushback_delay,
    retry_backoff,
};
pub use status::{Code, ParseCodeError, Pushback, Status};
pub use stream::{Framed, StreamSender, Streaming};
pub use telemetry::{
    AttemptLabels, BoundedMetricObserver, CallLabels, CallRole, CancellationEvent,
    CancellationReason, LifecycleObserver, MAX_METRIC_LABEL_BYTES, MAX_METRIC_RPCS,
    MAX_METRIC_TARGETS, MetricAttemptClass, MetricCallLabels, MetricEvent, MetricLabelPolicy,
    MetricSink, OTHER_METRIC_LABEL, ObserverChain, OwnedCallLabels, ReconnectEvent, RejectionEvent,
    RejectionReason,
};
pub use tls::{
    ClientTls, Identity, PeerIdentity, ServerTls, TcpStats, TlsHandshakeInfo,
    post_quantum_key_exchange_available,
};

pub use hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
pub use interop_cases::run_case;
pub use testing::{
    BoolValue, EchoStatus, Empty, InteropTestService, Payload, ResponseParameters, SimpleRequest,
    SimpleResponse, SizedInteropTestService, StreamingInputCallRequest, StreamingInputCallResponse,
    StreamingOutputCallRequest, StreamingOutputCallResponse, TestService, TestServiceClient,
    TestServiceServer,
};
