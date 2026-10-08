//! Generated `helloworld.Greeter` messages, client, and server.
//!
//! These types are generated from `proto/hello.proto` with
//! [`Stubs::Kernel`](pbrs::codegen::Stubs::Kernel), using the same generator
//! as application services. Dial with [`GreeterClient::connect`] and serve
//! with [`GreeterServer::serve`].
//!
//! Client and server interceptors use the channel and server contracts.
//! See [`crate::Outgoing`] for request overrides, [`crate::ResponseParts`]
//! for response hooks, and [`crate::Status`] for error metadata.

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/hello.rs"));
