//! A pure-Rust gRPC kernel over [`pbrs`].
//!
//! `pbrs-grpc` speaks gRPC over HTTP/2 without `unsafe` and without `tonic`.
//! It is a *kernel*: the protocol, the framing, the dispatch, and the safety
//! limits, with nothing layered on top that you did not ask for.
//!
//! No C or C++ is compiled into the build. Nothing in the dependency graph
//! pulls in `cc`, `bindgen`, `pkg-config`, `aws-lc-rs`, `ring`, or a vendored
//! zlib. gzip goes through `miniz_oxide`. TLS goes through rustls with the
//! [Graviola](https://crates.io/crates/graviola) provider, which builds with
//! `rustc` only. The FFI crates present are `libc` and `socket2` (a safe
//! wrapper around socket syscalls). Tokio already used both; this crate takes
//! a direct `socket2` dependency so TCP keepalive can be set. Neither compiles
//! C.
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
//! A complete crate that depends on this kernel from the outside — own proto,
//! `build.rs`, health, and reflection — lives at `examples/greeter` in the
//! repository.
//!
//! See [`docs/grpc.md`] in the repository for the full guide, and
//! [`docs/benchmarks.md`] for measured numbers.
//!
//! [`docs/grpc.md`]: https://github.com/mingley/pure-protobuf/blob/main/docs/grpc.md
//! [`docs/benchmarks.md`]: https://github.com/mingley/pure-protobuf/blob/main/docs/benchmarks.md
//!
//! # Map of the crate
//!
//! | Concern | Types |
//! |---|---|
//! | Serving | [`Service`], [`Rpc`], [`Server`], [`Router`], [`Incoming`], [`IncomingAccept`], [`ConnectionInfo`], [`ServerConfig`], [`PeerCred`] |
//! | Calling | [`Channel`], [`ChannelConfig`], [`Target`], [`Call`], [`CallHandle`], [`FusedFuture`] |
//! | TLS | [`Identity`], [`ServerTls`], [`ClientTls`], [`PeerIdentity`] |
//! | Health | [`health`] |
//! | Reflection | [`reflection`] |
//! | Interceptors | [`Interceptor`], [`ResponseInterceptor`], [`Intercepted`], [`ClientInterceptor`], [`Outgoing`], [`Extensions`] |
//! | Envelopes | [`Request`], [`Parts`], [`Response`], [`ResponseParts`], [`Metadata`], [`Status`], [`Code`], [`Code::from_i32`], [`Code::to_i32`], [`Code::name`], [`Code::description`], [`Code::is_retryable`], [`ParseCodeError`], [`Any`] |
//! | Rich errors | [`pb`], [`Any::pack`], [`Any::pack_with`], [`Any::is`], [`Any::unpack`], [`ErrorDetails`], [`ErrorDetails::new`], [`ErrorDetails::to_anys`], [`ErrorDetails::from_rpc`], [`Status::new`], [`Status::from_code`], [`Status::code`], [`Status::message`], [`Status::set_code`], [`Status::with_code`], [`Status::set_message`], [`Status::with_message`], [`Status::with_error_details`], [`Status::from_error_details`], [`Status::with_details`], [`pb::Status::with_details`], [`Status::details`], [`Status::rpc`], [`Status::error_details`], [`Status::from_rpc`], [`Status::set_rpc`], [`Status::set_details`], [`Status::set_error_details`], [`Status::set_from_error_details`], [`Status::with_rpc`], [`Status::from_error`], [`Status::with_cause`], [`Status::is_ok`], [`Status::is_retryable`], [`Status::retry_delay`], [`pb::Duration::from_std`], [`pb::Duration::try_to_std`], [`pb::RetryInfo::with_retry_delay`], [`Status::error_info`], [`pb::ErrorInfo::with_reason`], [`pb::ErrorInfo::with_metadata`], [`Status::bad_request`], [`pb::BadRequest::with_field`], [`pb::FieldViolation::with_field`], [`pb::bad_request`], [`Status::quota_failure`], [`pb::QuotaFailure::with_violation`], [`pb::quota_failure::Violation::with_subject`], [`pb::quota_failure`], [`Status::precondition_failure`], [`pb::PreconditionFailure::with_violation`], [`pb::precondition_failure::Violation::with_type`], [`pb::precondition_failure`], [`Status::help`], [`pb::Help::with_link`], [`pb::help::Link::with_url`], [`pb::help`], [`Status::localized_message`], [`pb::LocalizedMessage::with_locale`], [`Status::request_info`], [`pb::RequestInfo::with_request_id`], [`Status::resource_info`], [`pb::ResourceInfo::with_resource`], [`Status::debug_info`], [`pb::DebugInfo::with_stack`], [`Status::metadata`], [`Status::metadata_mut`] |
//! | Streaming | [`Streaming`], [`StreamSender`], [`Framed`], [`Stream`], [`FusedStream`] |
//! | Limits | [`MessageLimits`] |
//! | Wire format | [`codec`], [`gzip`], [`timeout`] |
//!
//! Unknown types stay in [`ErrorDetails::unknown`] so a custom detail is not dropped on a round-trip.
//!
//! [`Outgoing::user_agent_is_set`] is occupancy on this crate-map interceptor path, so a later interceptor can prefix only when unset.
//!
//! [`Outgoing::wait_for_ready_is_set`] is occupancy on this crate-map interceptor path, so a later interceptor can fill wait-for-ready only when unset.
//!
//! [`Outgoing::compress_is_set`] is occupancy on this crate-map interceptor path, so a later interceptor can fill compress only when unset.
//!
//! [`Outgoing::clear_user_agent`] restores the channel user-agent after a crate-map interceptor prefix.
//!
//! [`Outgoing::clear_wait_for_ready`] restores the channel wait-for-ready overlay after a crate-map interceptor choice.
//!
//! [`Outgoing::clear_compress`] then [`Outgoing::set_compress`] from [`Outgoing::compresses_outbound`] reapplies channel gzip after a crate-map interceptor choice.
//!
//! [`Outgoing::clear_timeout`] opts out of the channel timeout after a crate-map interceptor choice.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from [`ClientInterceptor`]: that runs on the outbound call before the stream opens; this crate-map [`Interceptor`] runs on the inbound RPC before the handler.
//!
//! Distinct from [`Interceptor`]: that runs on the inbound RPC before the handler; this crate-map [`ClientInterceptor`] runs on the outbound call before the stream opens.
//!
//! Distinct from [`ResponseInterceptor`]: that runs after the handler returns Ok or after a successful receive; this crate-map [`Interceptor`] runs on the inbound RPC before the handler.
//!
//! Distinct from [`Interceptor`]: that runs on the inbound RPC before the handler; this crate-map [`ResponseInterceptor`] runs after the handler returns Ok or after a successful receive.
//!
//! Distinct from [`ClientInterceptor`]: that runs on the outbound call before the stream opens; this crate-map [`ResponseInterceptor`] runs after the handler returns Ok or after a successful receive.
//!
//! Distinct from [`ResponseInterceptor`]: that runs after the handler returns Ok or after a successful receive; this crate-map [`ClientInterceptor`] runs on the outbound call before the stream opens.
//!
//! Distinct from [`Interceptor`]: that is the inbound hook; this crate-map [`Intercepted`] is the wrapper that runs it before the handler.
//!
//! Distinct from [`ClientInterceptor`]: that is the outbound hook; this crate-map [`Intercepted`] is the wrapper that runs an inbound [`Interceptor`] before the handler.
//!
//! Distinct from [`ResponseInterceptor`]: that is the after-Ok hook; this crate-map [`Intercepted`] runs before the handler and may hold that hook for after Ok.
//!
//! Distinct from [`Server::intercept`]: that runs on the inbound RPC before the handler; this crate-map Channel intercept runs on the outbound call before the stream opens.
//!
//! Distinct from [`Channel::on_response`]: that runs after a successful receive; this crate-map Channel intercept runs on the outbound call before the stream opens.
//!
//! Distinct from [`Channel::intercept`]: that runs on the outbound call before the stream opens; this crate-map server intercept runs on the inbound RPC before the handler.
//!
//! Distinct from [`Server::on_response`]: that runs after the handler returns Ok; this crate-map server intercept runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map server intercept Err; those trailers reach the client without reading the body.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Health interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from a crate-map Health handler Err: that is after the handler ran; this crate-map Health interceptor Err is trailers without reading the body.
//!
//! Distinct from a crate-map Health client interceptor: that runs on the outbound call before the stream opens; this crate-map Health interceptor runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Health handler Err; those trailers reach the client.
//!
//! Distinct from a crate-map Health interceptor Err: that is trailers without reading the body; this crate-map Health handler Err is after the handler ran.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map Health client interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Health client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map Health client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from a crate-map Health interceptor: that runs on the inbound RPC before the handler; this crate-map Health client interceptor runs on the outbound call before the stream opens.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Health StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map reflection interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from a crate-map reflection client interceptor: that runs on the outbound call before the stream opens; this crate-map reflection interceptor runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map reflection handler Err; those trailers reach the client.
//!
//! Distinct from a crate-map reflection interceptor Err: that is trailers without reading the body; this crate-map reflection handler Err is after the handler ran.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map reflection client interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map reflection client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map reflection client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from a crate-map reflection interceptor: that runs on the inbound RPC before the handler; this crate-map reflection client interceptor runs on the outbound call before the stream opens.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map reflection StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Store interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from a crate-map Store client interceptor: that runs on the outbound call before the stream opens; this crate-map Store interceptor runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Store handler Err; those trailers reach the client.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map Store client interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Store client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map Store client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from a crate-map Store interceptor: that runs on the inbound RPC before the handler; this crate-map Store client interceptor runs on the outbound call before the stream opens.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Store StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map TestService interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from a crate-map TestService client interceptor: that runs on the outbound call before the stream opens; this crate-map TestService interceptor runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map TestService handler Err; those trailers reach the client.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map TestService client interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map TestService client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map TestService client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from a crate-map TestService interceptor: that runs on the inbound RPC before the handler; this crate-map TestService client interceptor runs on the outbound call before the stream opens.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map TestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Reverser interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from a crate-map Reverser client interceptor: that runs on the outbound call before the stream opens; this crate-map Reverser interceptor runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Reverser handler Err; those trailers reach the client.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map Reverser client interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Reverser client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map Reverser client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from a crate-map Reverser interceptor: that runs on the inbound RPC before the handler; this crate-map Reverser client interceptor runs on the outbound call before the stream opens.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Reverser StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map hello interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from a crate-map hello client interceptor: that runs on the outbound call before the stream opens; this crate-map hello interceptor runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map hello handler Err; those trailers reach the client.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map hello client interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map hello client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map hello client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from a crate-map hello interceptor: that runs on the inbound RPC before the handler; this crate-map hello client interceptor runs on the outbound call before the stream opens.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map hello StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map UnimplementedService interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from a crate-map UnimplementedService client interceptor: that runs on the outbound call before the stream opens; this crate-map UnimplementedService interceptor runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map UnimplementedService handler Err; those trailers reach the client.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map UnimplementedService client interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map UnimplementedService client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map UnimplementedService client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from a crate-map UnimplementedService interceptor: that runs on the inbound RPC before the handler; this crate-map UnimplementedService client interceptor runs on the outbound call before the stream opens.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map InteropTestService interceptor Err; those trailers reach the client without reading the body.
//!
//! Distinct from a crate-map InteropTestService client interceptor: that runs on the outbound call before the stream opens; this crate-map InteropTestService interceptor runs on the inbound RPC before the handler.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map InteropTestService handler Err; those trailers reach the client.
//!
//! [`Outgoing::connected`] is the live-socket snapshot on this crate-map InteropTestService client interceptor path ([`Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map InteropTestService client interceptor Err; a local reject never opens a stream.
//!
//! Distinct from [`Channel::max_concurrent_rpcs`]: that takes a slot when the [`Call`] is polled; this crate-map InteropTestService client interceptor already ran, so a local Err never consumes that budget.
//!
//! Distinct from a crate-map InteropTestService interceptor: that runs on the inbound RPC before the handler; this crate-map InteropTestService client interceptor runs on the outbound call before the stream opens.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map InteropTestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//!
//! [`ResponseParts::compress_is_set`] is occupancy on this crate-map on_response path, so a later interceptor can fill compress only when unset.
//!
//! [`ResponseParts::clear_compress`] restores the server gzip overlay after Server on_response on this crate-map on_response path.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map server on_response Err; a local reject is trailers-only after handler Ok.
//!
//! Distinct from [`Server::intercept`]: that runs on the inbound RPC before the handler; this crate-map server on_response runs after the handler returns Ok.
//!
//! [`ResponseParts::clear_compress`] drops a compress choice after Channel on_response on this crate-map on_response path; a received reply has no server gzip overlay to restore.
//!
//! [`Status::from_error_details`] is the typed bag after this crate-map Channel on_response Err; a local reject fails the Call after a successful receive.
//!
//! Distinct from [`Channel::intercept`]: that runs on the outbound call before the stream opens; this crate-map Channel on_response runs after a successful receive.
//!
//! Compiling intercept / on_response overlay dumps live on [`hello`] (`GreeterClient` / `GreeterServer`).
//! Compiling ConnectionInfo peer dumps live on [`Incoming`].
//!
//! # Safety
//!
//! The crate forbids `unsafe` in every hand-written module. What remains is
//! resource safety against a peer that is trying to hurt you.
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
//! | Cleartext interception | TLS 1.2/1.3, ALPN `h2` required, certificate verification is not optional | opt-in [`Server::serve_tls`] / [`Channel::connect_tls`] |
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
//! There is no constructor that skips certificate verification.
//!
//! `tests/hostile.rs` drives raw HTTP/2 at the server to check the table above,
//! including a rapid-reset flood that exceeds
//! [`ServerConfig::max_pending_accept_reset_streams`]: that connection drops as
//! `ENHANCE_YOUR_CALM` and the accept loop still serves a well-behaved client.
//! The flood is h2c-only. A well-behaved client never fills that queue.
//! Distinct from a protocol-error RST flood: invalid frames force RSTs *we*
//! send, capped by [`ServerConfig::max_local_error_reset_streams`] (default
//! [`DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS`]). Exceeding that is also
//! `ENHANCE_YOUR_CALM`; the accept loop still serves a well-behaved client.
//! h2's `None` disable is not exposed.
//! Distinct from locally-reset stream-ID memory
//! ([`ServerConfig::max_concurrent_reset_streams`], default
//! [`DEFAULT_MAX_CONCURRENT_RESET_STREAMS`]): exceeding that evicts the
//! oldest remembered ID; it is not `ENHANCE_YOUR_CALM`. Frames on a
//! purged ID are a connection `PROTOCOL_ERROR`.
//! [`ChannelConfig::max_concurrent_reset_streams`] is the client handshake cap.
//! Distinct from [`ServerConfig::reset_stream_duration`] (default
//! [`DEFAULT_RESET_STREAM_DURATION`]): that is how long an ID is remembered,
//! not how many. After that duration the ID is forgotten, not
//! `ENHANCE_YOUR_CALM`. [`ChannelConfig::reset_stream_duration`] is the
//! client handshake duration.
//! A raw peer that sends more CONTINUATION frames than the header-list cap
//! allows also drops that connection (`ENHANCE_YOUR_CALM`); an unfinished
//! HEADERS frame (no `END_HEADERS`) does not take the accept loop down.
//! Distinct from one complete oversize HEADERS frame
//! (`SETTINGS_MAX_HEADER_LIST_SIZE`). Those floods are h2c-only.
//! A raw peer that sends too many tiny DATA frames drops that connection
//! (`ENHANCE_YOUR_CALM` / `too_many_data_frames`); the accept loop still
//! serves a well-behaved client. Distinct from the connection window
//! (flow-control bytes). h2 Auto (half the window) is not exposed.
//! [`ChannelConfig::data_frame_budget`] is the client handshake cap.
//! [`ChannelConfig::max_pending_accept_reset_streams`] is the client accept
//! queue, not the server cap. Property tests in the wire module cover what
//! fixed cases cannot: frames survive arbitrary chunk boundaries, arbitrary
//! bytes yield a `Status` rather than a panic, and a compressed frame never
//! inflates past the cap.
//!
//! ## `unsafe`
//!
//! Every hand-written module in this crate carries `#[forbid(unsafe_code)]`,
//! which cannot be overridden from inside the module. The exceptions are the
//! modules that `include!` generated message code ([`hello`], [`testing`],
//! [`health`], [`reflection`], [`pb`]); `pbrs` gencode uses `unsafe` for
//! zeroed-message construction, and that is a `pbrs` property rather than a
//! gRPC one. No gRPC framing, dispatch, or transport code in this crate
//! contains `unsafe`.
//!
//! ## Panics
//!
//! No public API panics on peer input. `unwrap`, `expect`, `panic!`,
//! indexing, and lossy numeric casts are denied at the lint level for the
//! whole workspace, so bad input becomes a [`Status`], not an abort.
//!
//! # Tuning
//!
//! Defaults are chosen for correctness and safety first, then throughput.
//! Three knobs matter:
//!
//! 1. **[`ChannelConfig::connections`]** — one connection is one `h2` driver
//!    task, so concurrent small RPCs serialize behind one core. Pooling is the
//!    single biggest win for client-side throughput.
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
//! Compression is not free: [`Request::set_compress`] and
//! [`ServerConfig::send_compressed`] / [`ChannelConfig::send_compressed`]
//! trade CPU for bandwidth, and at LAN latencies identity framing usually
//! wins. A peer that did not advertise gzip is never sent a compressed frame.
//! [`ServerConfig::gzip_compression_level`] /
//! [`ChannelConfig::gzip_compression_level`] is deflate effort (default 1).
//! Distinct from `send_compressed`, which is on or off.
//! [`Outgoing::gzip_level`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::compresses_outbound`].
//! [`Rpc::gzip_level`] is that overlay in a server interceptor.
//! Distinct from [`Rpc::compresses_outbound`].
//! [`Channel::gzip_level`] reads the deflate overlay without colliding with [`Channel::gzip_compression_level`].
//! Same overlay as [`Outgoing::gzip_level`].
//! [`Server::gzip_level`] reads the deflate overlay without colliding with [`Server::gzip_compression_level`].
//! Same overlay as [`Rpc::gzip_level`].
//! [`Outgoing::compresses_outbound`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::compress`].
//! [`Rpc::compresses_outbound`] is that overlay in a server interceptor.
//! [`Channel::compresses_outbound`] reads the outbound gzip overlay without colliding with [`Channel::send_compressed`].
//! Same overlay as [`Outgoing::compresses_outbound`].
//! [`Server::compresses_outbound`] reads the outbound gzip overlay without colliding with [`Server::send_compressed`].
//! Same overlay as [`Rpc::compresses_outbound`].
//! [`Outgoing::accepts_compressed`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::gzip_level`].
//! [`Rpc::accepts_compressed`] is that overlay in a server interceptor.
//! Distinct from [`Rpc::accepts_gzip`].
//! [`Channel::accepts_compressed`] reads the inbound gzip overlay without colliding with [`Channel::accept_compressed`].
//! Same overlay as [`Outgoing::accepts_compressed`].
//! [`Server::accepts_compressed`] reads the inbound gzip overlay without colliding with [`Server::accept_compressed`].
//! Same overlay as [`Rpc::accepts_compressed`].
//! [`Outgoing::concurrent_rpc_limit`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::waits_for_ready`].
//! [`Rpc::concurrent_rpc_limit`] is that overlay in a server interceptor.
//! [`Channel::concurrent_rpc_limit`] reads the RPC-cap overlay without colliding with [`Channel::max_concurrent_rpcs`].
//! Same overlay as [`Outgoing::concurrent_rpc_limit`].
//! [`Server::concurrent_rpc_limit`] reads the RPC-cap overlay without colliding with [`Server::max_concurrent_rpcs`].
//! Same overlay as [`Rpc::concurrent_rpc_limit`].
//! [`Outgoing::waits_for_ready`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::connected`].
//! [`Channel::waits_for_ready`] reads the wait-for-ready overlay without colliding with [`Channel::wait_for_ready`].
//! Same overlay as [`Outgoing::waits_for_ready`].
//! [`Outgoing::connected`] is the live-socket snapshot in a client interceptor.
//! Distinct from [`Outgoing::waits_for_ready`].
//! [`Channel::connected`] is that same snapshot without an interceptor.
//! [`GreeterClient::connected`] is the live-socket snapshot on a generated client.
//! Distinct from [`GreeterClient::waits_for_ready`].
//! Same snapshot as [`Channel::connected`].
//! [`Outgoing::rpc_timeout`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::timeout`].
//! [`Rpc::rpc_timeout`] is that overlay in a server interceptor.
//! [`Channel::rpc_timeout`] reads the deadline overlay without colliding with [`Channel::timeout`].
//! Same overlay as [`Outgoing::rpc_timeout`].
//! [`Server::rpc_timeout`] reads the deadline overlay without colliding with [`Server::timeout`].
//! Same overlay as [`Rpc::rpc_timeout`].
//! [`Outgoing::stream_buffer_size`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::limits`].
//! [`Channel::stream_buffer_size`] reads the stream-queue overlay without colliding with [`Channel::stream_buffer`].
//! Same overlay as [`Outgoing::stream_buffer_size`].
//! [`Outgoing::limits`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::send_buffer_size`].
//! [`Rpc::limits`] is that overlay in a server interceptor.
//! [`Channel::limits`] reads the message-cap overlay without colliding with [`Channel::message_limits`].
//! Same overlay as [`Outgoing::limits`].
//! [`Server::limits`] reads the message-cap overlay without colliding with [`Server::message_limits`].
//! Same overlay as [`Rpc::limits`].
//! [`Channel::send_buffer_size`] reads the HTTP/2 send buffer overlay without colliding with [`Channel::max_send_buffer_size`].
//! Same overlay as [`Outgoing::send_buffer_size`].
//! [`Server::send_buffer_size`] reads the HTTP/2 send buffer overlay without colliding with [`Server::max_send_buffer_size`].
//! Same overlay as [`Rpc::send_buffer_size`].
//! [`Outgoing::send_buffer_size`] is that overlay in a client interceptor.
//! Distinct from [`Outgoing::stream_buffer_size`].
//! [`Rpc::send_buffer_size`] is that overlay in a server interceptor.
//! Distinct from [`Outgoing::send_buffer_size`].
//! [`Response::send_buffer_size`] is that overlay in a response interceptor.
//! Distinct from [`Request::send_buffer_size`].
//! Distinct from [`Rpc::send_buffer_size`].
//! [`Response::path`] is kernel-stamped after `Ok` / after receive.
//! Distinct from [`Request::path`].
//! Distinct from [`Outgoing::path`].
//! [`Response::gzip_level`] is that overlay in a response interceptor.
//! Distinct from [`Response::compress`].
//! Distinct from [`Rpc::gzip_level`].
//! [`Response::compresses_outbound`] is that overlay in a response interceptor.
//! Distinct from [`Response::compress`], which is the per-RPC flag.
//! Distinct from [`Rpc::compresses_outbound`].
//! [`Response::accepts_gzip`] is the peer advertisement in a response interceptor.
//! Distinct from [`Response::encoding`], which is received `grpc-encoding`.
//! Distinct from [`Rpc::accepts_gzip`].
//! [`Response::deadline`] is kernel-stamped after `Ok`, when writing.
//! Distinct from [`Request::deadline`].
//! Distinct from [`Rpc::deadline`].
//! [`Response::timeout`] is the duration stamped at dispatch, in a response interceptor.
//! Distinct from [`Response::deadline`].
//! Distinct from [`Rpc::timeout`].
//! [`Response::limits`] is the encode cap in a response interceptor.
//! Distinct from [`Request::limits`].
//! Distinct from [`Rpc::limits`].
//! [`Response::peer_timeout`] is the client's `grpc-timeout` in a response interceptor.
//! Distinct from [`Response::timeout`].
//! Distinct from [`Rpc::peer_timeout`].
//! [`Response::rpc_timeout`] is the server overlay in a response interceptor.
//! Distinct from [`Response::peer_timeout`].
//! Distinct from [`Rpc::rpc_timeout`].
//! [`Response::accepts_compressed`] is the inbound overlay in a response interceptor.
//! Distinct from [`Response::accepts_gzip`].
//! Distinct from [`Rpc::accepts_compressed`].
//! [`ServerConfig::header_table_size`] / [`ChannelConfig::header_table_size`]
//! is HTTP/2 `SETTINGS_HEADER_TABLE_SIZE` (HPACK dynamic table, default 4096).
//! Distinct from `max_header_list_size`, which caps uncompressed header-block
//! bytes. Handshake-only on the client.
//! [`ServerConfig::data_frame_budget`] / [`ChannelConfig::data_frame_budget`]
//! is the small-DATA framing budget (default 25600). Distinct from the
//! connection window (flow-control bytes). h2 Auto (half the window) is not
//! exposed.
//! [`ServerConfig::max_concurrent_reset_streams`] /
//! [`ChannelConfig::max_concurrent_reset_streams`]
//! is remembered locally-reset stream IDs (default 50). Distinct from
//! pending-reset and protocol-error RST (those GOAWAY). Handshake-only
//! on the client. Exceeding this evicts the oldest ID, not
//! `ENHANCE_YOUR_CALM`.
//! [`ServerConfig::reset_stream_duration`] /
//! [`ChannelConfig::reset_stream_duration`]
//! is how long those IDs are remembered (default 1 s). Distinct from the
//! count cap. Handshake-only on the client. After that duration the ID is
//! forgotten, not `ENHANCE_YOUR_CALM`.
//! Inbound gzip is on by default; [`ServerConfig::accept_compressed`]`(false)`
//! / [`ChannelConfig::accept_compressed`]`(false)` refuses it.
//! A received reply surfaces the peer's `grpc-encoding` on [`Response::encoding`]
//! (`None` for identity).
//!
//! # Relationship to the rest of the workspace
//!
//! [`pbrs`] does not depend on this crate, and this crate does not depend on
//! `tonic` or `protobuf-tonic`. Use `protobuf-tonic` if you need to keep an
//! existing `tonic` service and only want pbrs message types.

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
pub mod gzip;
#[forbid(unsafe_code)]
pub mod interop_cases;
#[forbid(unsafe_code)]
pub mod timeout;

pub mod health;
pub mod hello;
pub mod pb;
pub mod reflection;
pub mod testing;

#[forbid(unsafe_code)]
mod client;
#[forbid(unsafe_code)]
mod config;
#[forbid(unsafe_code)]
mod interceptor;
#[forbid(unsafe_code)]
mod keepalive;
#[forbid(unsafe_code)]
mod limits;
#[forbid(unsafe_code)]
mod metadata;
#[forbid(unsafe_code)]
mod request;
#[forbid(unsafe_code)]
mod server;
#[forbid(unsafe_code)]
mod status;
#[forbid(unsafe_code)]
mod stream;
#[forbid(unsafe_code)]
mod tcp;
#[forbid(unsafe_code)]
mod tls;
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

pub use client::{Channel, Target};
pub use config::{
    ChannelConfig, ServerConfig, DEFAULT_CONNECT_TIMEOUT, DEFAULT_DATA_FRAME_BUDGET,
    DEFAULT_GZIP_COMPRESSION_LEVEL, DEFAULT_HEADER_TABLE_SIZE, DEFAULT_KEEP_ALIVE_TIMEOUT,
    DEFAULT_MAX_CONCURRENT_RESET_STREAMS, DEFAULT_MAX_CONCURRENT_STREAMS,
    DEFAULT_MAX_CONNECTION_AGE_GRACE, DEFAULT_MAX_FRAME_SIZE, DEFAULT_MAX_HEADER_LIST_SIZE,
    DEFAULT_MAX_LOCAL_ERROR_RESET_STREAMS, DEFAULT_MAX_PENDING_ACCEPT_RESET_STREAMS,
    DEFAULT_MAX_SEND_BUFFER_SIZE, DEFAULT_RESET_STREAM_DURATION, DEFAULT_STREAM_BUFFER,
    DEFAULT_WINDOW_SIZE,
};
/// `futures_core::future::FusedFuture`, so a finished [`Call`] is skipped by
/// combinators that honour termination.
pub use futures_core::future::FusedFuture;
/// `futures_core::stream::FusedStream`, so a finished [`Streaming`] is skipped by
/// combinators that honour termination.
pub use futures_core::stream::FusedStream;
/// `futures_core::Stream`, so [`Streaming`] can be driven with `StreamExt`.
pub use futures_core::Stream;
/// Per-RPC typed bag: insert in an interceptor, read in the handler.
pub use http::Extensions;
pub use interceptor::{
    ClientInterceptor, Intercepted, Interceptor, ResponseInterceptor, ServiceExt,
};
pub use limits::{MessageLimits, DEFAULT_MAX_DECODING_MESSAGE_SIZE};
pub use metadata::Metadata;
pub use pb::{Any, ErrorDetails};
pub use request::{Call, CallHandle, Outgoing, Parts, Request, Response, ResponseParts};
pub use server::{
    ConnectionInfo, Incoming, IncomingAccept, PeerCred, Router, Rpc, Server, Service,
};
pub use status::{Code, ParseCodeError, Status};
pub use stream::{Framed, StreamSender, Streaming};
pub use tls::{ClientTls, Identity, PeerIdentity, ServerTls};

pub use hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
pub use interop_cases::run_case;
pub use testing::{
    BoolValue, EchoStatus, Empty, InteropTestService, Payload, ResponseParameters, SimpleRequest,
    SimpleResponse, StreamingInputCallRequest, StreamingInputCallResponse,
    StreamingOutputCallRequest, StreamingOutputCallResponse, TestService, TestServiceClient,
    TestServiceServer,
};
