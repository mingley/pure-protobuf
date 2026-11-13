# protobuf-tonic

[![Crates.io](https://img.shields.io/crates/v/protobuf-tonic.svg)](https://crates.io/crates/protobuf-tonic)
[![Documentation](https://docs.rs/protobuf-tonic/badge.svg)](https://docs.rs/protobuf-tonic)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/mingley/pure-protobuf/blob/main/LICENSE-MIT)

`protobuf-tonic` connects [`pbrs`](https://crates.io/crates/pbrs) messages to
Tonic 0.14 transport and middleware. It provides a `ProtobufCodec` adapter,
generated client and server stubs, and single-buffer wire conversion helpers.
Version `0.1.0-alpha.3` (paired with `pbrs` `0.2.1`) is a pre-release preview
and requires Rust 1.88. Older Tonic versions (0.12, 0.13) are unsupported.

Two boundaries matter when migrating:

- Generated messages implement Google protobuf v4 traits (`Parse` and
  `Serialize`), not `prost::Message`. Middleware or call sites that require
  `prost::Message` can bridge types with `prost_to_pbrs` / `pbrs_to_prost`.
- Successful Tonic 0.14 responses do not expose custom trailing metadata
  through a `Response::trailers()` accessor.

## How it differs from nearby crates

| Choice | Use it when | Important boundary |
|---|---|---|
| `protobuf-tonic` | You want Tonic 0.14+ transport and Tower layers with `pbrs` messages. | Messages implement `Parse` / `Serialize`, not `prost::Message`. |
| `tonic-prost` | You want the default Tonic + Prost stack. | Expects `prost::Message` types. |
| [`pbrs-grpc`](https://crates.io/crates/pbrs-grpc) | You want the native HTTP/2 gRPC transport, with optional Tower, Prost, and Tonic adapters. | Separate gRPC stack, not a Tonic codec. |

The `pbrs` core crate has no `tonic`, `hyper`, or `tower` dependency;
`protobuf-tonic` is the adapter layer connecting `pbrs` to Tonic 0.14+. MSRV is
1.88.

## Usage

### 1. Add dependencies

```toml
[dependencies]
tonic = { version = "0.14", default-features = false, features = ["transport", "codegen", "router"] }
pbrs = "0.2.1"
protobuf-tonic = "0.1.0-alpha.3"
http = "1"
tokio-stream = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.2.1"
```

From the older `0.1.0-alpha.2` release onward, `protobuf-tonic` builds from a
checked descriptor set without `protoc` (only the older `0.1.0-alpha.1` archive
required `protoc` to build the crate itself).

Compiling your own `.proto` files with `compile_protos` requires `protoc` on
`PATH`. For a cold application build without `protoc`, use
[`Config::compile_descriptor_set`](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/codegen.md#generating-from-a-checked-descriptor-set)
with a checked-in descriptor set.

### 2. Generate Tonic stubs (`build.rs`)

Default `compile_protos` emits native `pbrs-grpc` kernel stubs. Tonic users
must opt in with `emit_tonic_stubs(true)`. Generated messages implement `Parse`
and `Serialize`, not `prost::Message`.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(true)
        .compile_protos(&["proto/hello.proto"], &["proto"])?;
    Ok(())
}
```

The generator emits `FooClient`, `FooServer`, and the `Foo` service trait using
`ProtobufCodec` instead of Prost.

### 3. Implement a service

```rust
use tonic::transport::Server;
use tonic::{Request, Response, Status};

include!(concat!(env!("OUT_DIR"), "/hello.rs"));

struct MyGreeter;

impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(format!("Hello, {}!", request.get_ref().name()));
        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "127.0.0.1:50051".parse()?;
    Server::builder()
        .add_service(GreeterServer::new(MyGreeter))
        .serve(addr)
        .await?;
    Ok(())
}
```

### 4. Call the service

```rust
use tonic::Request;
use tonic::transport::Channel;

include!(concat!(env!("OUT_DIR"), "/hello.rs"));

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel::from_static("http://127.0.0.1:50051")
        .connect()
        .await?;
    let mut client = GreeterClient::new(channel);

    let mut req = HelloRequest::new();
    req.set_name("World");

    let response = client.say_hello(Request::new(req)).await?;
    println!("Response: {}", response.into_inner().message());

    Ok(())
}
```

### Bridging `prost::Message` and `pbrs` messages

At migration boundaries where surrounding middleware or services still use
`prost::Message`, convert wire-compatible messages with `prost_to_pbrs` and
`pbrs_to_prost`:

```rust
let pbrs_msg: pbrs_api::HelloRequest = protobuf_tonic::prost_to_pbrs(&prost_msg)?;
let prost_msg: prost_api::HelloRequest = protobuf_tonic::pbrs_to_prost(&pbrs_msg)?;
```

Each direction encodes once into an owned wire buffer and moves that buffer
into the target decoder without copying payload bytes again.

## Supported features

| Feature | Status |
|---|---|
| RPC shapes | Unary, client-streaming, server-streaming, and bidirectional streaming. |
| Compression | `send_compressed` and `accept_compressed` for Gzip. |
| Interceptors | Request interception through Tonic's `with_interceptor`; use Tower middleware for response handling. |
| Message size limits | `max_decoding_message_size` and `max_encoding_message_size`. |
| Prost wire conversion | `prost_to_pbrs` and `pbrs_to_prost` (`ConversionError`). |
| Health and reflection | Works with `tonic-health` and `tonic-reflection`. |
| Trailers and status | HTTP/2 headers and status trailers are supported. Tonic 0.14 still lacks an OK-path custom trailer accessor on `Response`. |

## Performance evidence

The adapter changes the message codec while keeping Tonic's HTTP/2 transport.
Compare your schemas with `tonic-bench`; the
[benchmark report](https://github.com/mingley/pure-protobuf/blob/main/docs/benchmarks.md)
lists measured workloads and results.

Tonic encodes each message into a contiguous buffer. For large shared `bytes`
fields, the native `pbrs-grpc` transport can avoid additional send-side copies;
the [zero-copy guide](https://github.com/mingley/pure-protobuf/blob/main/docs/zero-copy.md)
explains that separate path.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/mingley/pure-protobuf/blob/main/LICENSE-APACHE))
- MIT License ([LICENSE-MIT](https://github.com/mingley/pure-protobuf/blob/main/LICENSE-MIT))

at your option.
