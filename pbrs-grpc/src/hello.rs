//! `helloworld.Greeter`: messages, client, and server, all generated.
//!
//! Everything in this module comes out of `protoc-gen-pbrs` from
//! `proto/hello.proto` with [`Stubs::Kernel`](pbrs::codegen::Stubs::Kernel).
//! The kernel dogfoods its own code generator, so this module is also the
//! reference for what your `build.rs` produces. Dial with
//! [`GreeterClient::connect`]; serve with [`GreeterServer::serve`].
//! Generated [`GreeterClient::intercept`] reads the same Outgoing overlays as
//! [`crate::Channel::intercept`]:
//! [`crate::Outgoing::user_agent_is_set`] is occupancy on this hello intercept path, so a later interceptor can prefix only when unset.
//! [`crate::Outgoing::wait_for_ready_is_set`] is occupancy on this hello intercept path, so a later interceptor can fill wait-for-ready only when unset.
//! [`crate::Outgoing::compress_is_set`] is occupancy on this hello intercept path, so a later interceptor can fill compress only when unset.
//! [`crate::Outgoing::clear_user_agent`] restores the channel user-agent after a hello intercept prefix.
//! [`crate::Outgoing::clear_wait_for_ready`] restores the channel wait-for-ready overlay after a hello intercept choice.
//! [`crate::Outgoing::clear_compress`] then [`crate::Outgoing::set_compress`] from [`crate::Outgoing::compresses_outbound`] reapplies channel gzip after a hello intercept choice.
//! [`crate::Outgoing::clear_timeout`] opts out of the channel timeout after a hello intercept choice.
//! [`crate::Outgoing::connected`] is the live-socket snapshot on this hello intercept path ([`crate::Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//! [`crate::Status::from_error_details`] is the typed bag after a hello intercept Err; a local reject never opens a stream.
//! Distinct from a hello handler Err: that is after the handler ran; this hello intercept Err is a local reject never opens a stream.
//! Distinct from a hello client on_response Err: that fails the Call after a successful receive; this hello intercept Err is a local reject never opens a stream.
//! Distinct from a hello server intercept Err: that is trailers without reading the body; this hello intercept Err is a local reject never opens a stream.
//! Distinct from [`crate::Channel::max_concurrent_rpcs`]: that takes a slot when the [`crate::Call`] is polled; a hello intercept already ran, so a local Err never consumes that budget.
//! Distinct from [`GreeterServer::intercept`]: that runs on the inbound RPC before the handler; this hello intercept runs on the outbound call before the stream opens.
//! Distinct from [`GreeterClient::on_response`]: that runs after a successful receive; this hello intercept runs on the outbound call before the stream opens.
//!
//! ```
//! # fn demo(channel: pbrs_grpc::Channel) -> pbrs_grpc::hello::GreeterClient {
//! pbrs_grpc::hello::GreeterClient::new(channel).intercept(|call: &mut pbrs_grpc::Outgoing<'_>| {
//!     let _ = (
//!         call.path(),
//!         call.service(),
//!         call.method(),
//!         call.authority(),
//!         call.scheme(),
//!         call.user_agent(),
//!         call.user_agent_is_set(),
//!         call.metadata(),
//!         call.timeout(),
//!         call.deadline(),
//!         call.rpc_timeout(),
//!         call.wait_for_ready(),
//!         call.wait_for_ready_is_set(),
//!         call.waits_for_ready(),
//!         call.compress(),
//!         call.compress_is_set(),
//!         call.compresses_outbound(),
//!         call.accepts_compressed(),
//!         call.gzip_level(),
//!         call.concurrent_rpc_limit(),
//!         call.stream_buffer_size(),
//!         call.send_buffer_size(),
//!         call.limits(),
//!         call.connected(),
//!         call.extensions(),
//!     );
//!     Ok(())
//! })
//! # }
//! ```
//! Generated [`GreeterClient::on_response`] reads the same ResponseParts overlays as
//! [`crate::Channel::on_response`]:
//! [`crate::ResponseParts::compress_is_set`] is occupancy after a hello client on_response, so a later interceptor can fill compress only when unset.
//! [`crate::ResponseParts::clear_compress`] drops a compress choice after a hello client on_response; a received reply has no server gzip overlay to restore.
//! [`crate::Status::from_error_details`] is the typed bag after a hello client on_response Err; a local reject fails the Call after a successful receive.
//! Distinct from a hello handler Err: that is after the handler ran; this hello client on_response Err fails the Call after a successful receive.
//! Distinct from a hello intercept Err: that is a local reject never opens a stream; this hello client on_response Err fails the Call after a successful receive.
//! Distinct from [`GreeterClient::intercept`]: that runs on the outbound call before the stream opens; this hello client on_response runs after a successful receive.
//!
//! ```
//! # fn demo(channel: pbrs_grpc::Channel) -> pbrs_grpc::hello::GreeterClient {
//! pbrs_grpc::hello::GreeterClient::new(channel).on_response(|parts: &mut pbrs_grpc::ResponseParts| {
//!     let _ = (
//!         parts.path(),
//!         parts.service(),
//!         parts.method(),
//!         parts.metadata(),
//!         parts.trailers(),
//!         parts.compress(),
//!         parts.compress_is_set(),
//!         parts.encoding(),
//!         parts.gzip_level(),
//!         parts.compresses_outbound(),
//!         parts.accepts_gzip(),
//!         parts.deadline(),
//!         parts.timeout(),
//!         parts.limits(),
//!         parts.peer_timeout(),
//!         parts.rpc_timeout(),
//!         parts.accepts_compressed(),
//!         parts.send_buffer_size(),
//!         parts.extensions(),
//!     );
//!     Ok(())
//! })
//! # }
//! ```
//! Generated [`GreeterServer::intercept`] reads the same Rpc overlays as
//! [`crate::Server::intercept`]:
//! [`crate::Status::from_error_details`] is the typed bag after a hello server intercept Err; those trailers reach the client without reading the body.
//! Distinct from a hello handler Err: that is after the handler ran; this hello server intercept Err is trailers without reading the body.
//! Distinct from a hello server on_response Err: that is trailers-only after handler Ok; this hello server intercept Err is trailers without reading the body.
//! Distinct from a hello intercept Err: that is a local reject never opens a stream; this hello server intercept Err is trailers without reading the body.
//! Distinct from [`GreeterClient::intercept`]: that runs on the outbound call before the stream opens; this hello server intercept runs on the inbound RPC before the handler.
//! Distinct from [`GreeterServer::on_response`]: that runs after the handler returns Ok; this hello server intercept runs on the inbound RPC before the handler.
//!
//! ```
//! # struct Svc;
//! # impl pbrs_grpc::hello::Greeter for Svc {}
//! # fn demo() -> pbrs_grpc::Server<pbrs_grpc::hello::GreeterServer<Svc>> {
//! pbrs_grpc::hello::GreeterServer::new(Svc).intercept(|rpc: &mut pbrs_grpc::Rpc| {
//!     let _ = (
//!         rpc.path(),
//!         rpc.service(),
//!         rpc.method(),
//!         rpc.metadata(),
//!         rpc.timeout(),
//!         rpc.peer_timeout(),
//!         rpc.rpc_timeout(),
//!         rpc.effective_timeout(),
//!         rpc.deadline(),
//!         rpc.accepts_gzip(),
//!         rpc.encoding(),
//!         rpc.compresses_outbound(),
//!         rpc.gzip_level(),
//!         rpc.accepts_compressed(),
//!         rpc.concurrent_rpc_limit(),
//!         rpc.send_buffer_size(),
//!         rpc.limits(),
//!         rpc.local_addr(),
//!         rpc.remote_addr(),
//!         rpc.peer_identity(),
//!         rpc.peer_cred(),
//!         rpc.authority(),
//!         rpc.scheme(),
//!         rpc.extensions(),
//!     );
//!     Ok(())
//! })
//! # }
//! ```
//! Generated [`GreeterServer::on_response`] reads the same ResponseParts overlays as
//! [`crate::Server::on_response`]:
//! [`crate::ResponseParts::compress_is_set`] is occupancy after a hello server on_response, so a later interceptor can fill compress only when unset.
//! [`crate::ResponseParts::clear_compress`] restores the server gzip overlay after a hello server on_response.
//! [`crate::Status::from_error_details`] is the typed bag after a hello server on_response Err; a local reject is trailers-only after handler Ok.
//! Distinct from a hello handler Err: that is after the handler ran; this hello server on_response Err is trailers-only after handler Ok.
//! Distinct from a hello server intercept Err: that is trailers without reading the body; this hello server on_response Err is trailers-only after handler Ok.
//! Distinct from [`GreeterServer::intercept`]: that runs on the inbound RPC before the handler; this hello server on_response runs after the handler returns Ok.
//!
//! ```
//! # struct Svc;
//! # impl pbrs_grpc::hello::Greeter for Svc {}
//! # fn demo() -> pbrs_grpc::Server<pbrs_grpc::hello::GreeterServer<Svc>> {
//! pbrs_grpc::hello::GreeterServer::new(Svc).on_response(|parts: &mut pbrs_grpc::ResponseParts| {
//!     let _ = (
//!         parts.path(),
//!         parts.service(),
//!         parts.method(),
//!         parts.metadata(),
//!         parts.trailers(),
//!         parts.compress(),
//!         parts.compress_is_set(),
//!         parts.encoding(),
//!         parts.gzip_level(),
//!         parts.compresses_outbound(),
//!         parts.accepts_gzip(),
//!         parts.deadline(),
//!         parts.timeout(),
//!         parts.limits(),
//!         parts.peer_timeout(),
//!         parts.rpc_timeout(),
//!         parts.accepts_compressed(),
//!         parts.send_buffer_size(),
//!         parts.extensions(),
//!     );
//!     Ok(())
//! })
//! # }
//! ```
//! Generated [`Greeter`] handler `Err` unpacks the same trailers as a generated trait method.
//! [`crate::Status::from_error_details`] is the typed bag after a hello handler Err; those trailers reach the client.
//! Distinct from a hello server intercept Err: that is trailers without reading the body; this hello handler Err is after the handler ran.
//! Distinct from a hello intercept Err: that is a local reject never opens a stream; this hello handler Err is after the handler ran.
//! Distinct from a hello server on_response Err: that is trailers-only after handler Ok; this hello handler Err is after the handler ran.
//! Distinct from a hello client on_response Err: that fails the Call after a successful receive; this hello handler Err is after the handler ran.
//! Distinct from a hello StreamSender fail: that is trailers after any messages already sent; this hello handler Err is after the handler ran.
//! Generated [`Greeter`] ServerHello / StreamHello [`crate::StreamSender::fail`] ships those trailers after a streamed DATA frame.
//! [`crate::Status::from_error_details`] is the typed bag after a hello StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//! Distinct from a hello handler Err: that is after the handler ran; this hello StreamSender fail is trailers after any messages already sent.
//! Distinct from a hello server intercept Err: that is trailers without reading the body; this hello StreamSender fail is trailers after any messages already sent.

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/hello.rs"));
