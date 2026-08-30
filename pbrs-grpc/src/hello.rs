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

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/hello.rs"));
