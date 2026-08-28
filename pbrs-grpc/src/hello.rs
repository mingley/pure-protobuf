//! `helloworld.Greeter`: messages, client, and server, all generated.
//!
//! Everything in this module comes out of `protoc-gen-pbrs` from
//! `proto/hello.proto` with [`Stubs::Kernel`](pbrs::codegen::Stubs::Kernel).
//! The kernel dogfoods its own code generator, so this module is also the
//! reference for what your `build.rs` produces.

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/hello.rs"));
