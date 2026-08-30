//! `helloworld.Greeter`: messages, client, and server, all generated.
//!
//! Everything in this module comes out of `protoc-gen-pbrs` from
//! `proto/hello.proto` with [`Stubs::Kernel`](pbrs::codegen::Stubs::Kernel).
//! The kernel dogfoods its own code generator, so this module is also the
//! reference for what your `build.rs` produces. Dial with
//! [`GreeterClient::connect`]; serve with [`GreeterServer::serve`].
//! Generated [`GreeterClient::intercept`] reads the same Outgoing overlays as
//! [`crate::Channel::intercept`]:
//!
//! ```
//! # fn demo(channel: pbrs_grpc::Channel) -> pbrs_grpc::hello::GreeterClient {
//! pbrs_grpc::hello::GreeterClient::new(channel).intercept(|call: &mut pbrs_grpc::Outgoing<'_>| {
//!     let _ = (
//!         call.rpc_timeout(),
//!         call.waits_for_ready(),
//!         call.compresses_outbound(),
//!         call.accepts_compressed(),
//!         call.gzip_level(),
//!         call.concurrent_rpc_limit(),
//!         call.stream_buffer_size(),
//!         call.send_buffer_size(),
//!         call.limits(),
//!         call.connected(),
//!     );
//!     Ok(())
//! })
//! # }
//! ```
//! Generated [`GreeterServer::intercept`] reads the same Rpc overlays as
//! [`crate::Server::intercept`]:
//!
//! ```
//! # struct Svc;
//! # impl pbrs_grpc::hello::Greeter for Svc {}
//! # fn demo() -> pbrs_grpc::Server<pbrs_grpc::hello::GreeterServer<Svc>> {
//! pbrs_grpc::hello::GreeterServer::new(Svc).intercept(|rpc: &mut pbrs_grpc::Rpc| {
//!     let _ = (
//!         rpc.path(),
//!         rpc.peer_timeout(),
//!         rpc.rpc_timeout(),
//!         rpc.effective_timeout(),
//!         rpc.deadline(),
//!         rpc.gzip_level(),
//!         rpc.accepts_compressed(),
//!         rpc.concurrent_rpc_limit(),
//!         rpc.send_buffer_size(),
//!         rpc.limits(),
//!     );
//!     Ok(())
//! })
//! # }
//! ```

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/hello.rs"));
