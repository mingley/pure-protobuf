//! HTTP/2 gRPC kernel over **pbrs**.
//!
//! Independent of `tonic` and of `protobuf-tonic`. The protobuf crate
//! (`pbrs`) has no dependency on this crate. Use `protobuf-tonic` if you
//! want generated tonic stubs instead of this stack.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod codec;
pub mod gzip;
pub mod hello;
pub mod timeout;

mod client;
pub mod interop_cases;
mod metadata;
mod request;
mod server;
mod status;
mod stream;
mod testing;
mod wire;

pub use client::Channel;
pub use hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
pub use interop_cases::run_case;
pub use metadata::Metadata;
pub use request::{Call, CallHandle, Request, Response};
pub use server::{Http2Handler, Server};
pub use status::{Code, Status};
pub use stream::{InItem, Inbound, OutItem, StreamingSender};
pub use testing::{
    BoolValue, EchoStatus, Empty, InteropTestService, Payload, ResponseParameters, SimpleRequest,
    SimpleResponse, StreamingInputCallRequest, StreamingInputCallResponse,
    StreamingOutputCallRequest, StreamingOutputCallResponse, TestService, TestServiceClient,
    TestServiceServer,
};
