# protobuf-tonic

> ⚠️ **Pre-Release Notice**: `protobuf-tonic` is currently in **preview / pre-release (`0.1.0-alpha.1`)** and is **not yet production ready**.
>
> ### Known Gaps & Roadmap (TBD)
> - **OK-Path Custom Trailers**: Tonic 0.14 does not expose custom trailers on successful responses (`Response` lacks a `trailers()` accessor); custom trailing metadata is only delivered on error statuses.
> - **Tonic Ecosystem Trait Boundaries**: `pbrs` message types implement Google protobuf v4 application traits (`Parse` / `Serialize`), not `prost::Message`. Existing middleware or tower layers expecting `prost::Message` are incompatible.
> - **Version Support**: Strictly targets Tonic 0.14+. Older versions (0.12, 0.13) are unsupported.

A [tonic 0.14+](https://crates.io/crates/tonic) `Codec` adapter and code generator stubs over `pbrs` message types (`Parse` / `Serialize`).

`protobuf-tonic` allows you to build standard Tonic gRPC services and clients using `pbrs` pure-Rust protobuf messages instead of `prost`.

---

## Overview

- **Not `tonic-prost`**: These types implement the Google protobuf v4 application traits (`Parse` / `Serialize`), not `prost::Message`.
- **Decoupled Architecture**: The `pbrs` protobuf kernel remains completely free of any `tonic`, `hyper`, or `tower` dependencies.
- **Alternative gRPC Stacks**:
  - Use `protobuf-tonic` if you want to integrate with the standard `tonic` ecosystem (middleware, tower layers, etc.).
  - Use [`pbrs-grpc`](../pbrs-grpc) if you want a lightweight, pure-Rust HTTP/2 gRPC kernel without Tonic or Tower.
- **Tonic Version**: Supports Tonic 0.14+ (Tonic 0.12 and 0.13 are unsupported). MSRV is 1.88.

---

## Usage

### 1. Dependencies

```toml
[dependencies]
tonic = { version = "0.14", default-features = false, features = ["transport", "codegen"] }
pbrs = "0.1"
protobuf-tonic = { git = "https://github.com/mingley/pure-protobuf" }

[build-dependencies]
pbrs = "0.1"
```

> **Note on crates.io**: `protobuf-tonic` currently depends on `pbrs` by path/git. It will be published to crates.io following the publication of `pbrs`.

### 2. Code Generation (`build.rs`)

Generate Tonic service stubs using `pbrs`. Default `compile_protos` emits
native `pbrs-grpc` kernel stubs; tonic users must opt in. Generated messages
implement `Parse` / `Serialize`, not `prost::Message`.

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(true)
        .compile_protos(&["proto/hello.proto"], &["proto"])?;
    Ok(())
}
```

The generator emits `FooClient`, `FooServer`, and the `Foo` service trait using `ProtobufCodec` instead of Prost.

### 3. Implementing a Service

```rust
use tonic::{Request, Response, Status};
use helloworld::{Greeter, GreeterServer, HelloRequest, HelloReply};

struct MyGreeter;

#[tonic::async_trait]
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
    tonic::transport::Server::builder()
        .add_service(GreeterServer::new(MyGreeter))
        .serve(addr)
        .await?;
    Ok(())
}
```

### 4. Client Example

```rust
use tonic::Request;
use helloworld::{GreeterClient, HelloRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = GreeterClient::connect("http://127.0.0.1:50051").await?;

    let mut req = HelloRequest::new();
    req.set_name("World");

    let response = client.say_hello(Request::new(req)).await?;
    println!("Response: {}", response.into_inner().message());

    Ok(())
}
```

---

## Features Supported

- **All Call Shapes**: Unary, client-streaming, server-streaming, and bidirectional streaming.
- **Compression**: `send_compressed` and `accept_compressed` for Gzip.
- **Interceptors**: Request and response interception via Tonic's `with_interceptor`.
- **Message Size Limits**: Configurable via `max_decoding_message_size` and `max_encoding_message_size`.
- **Health & Reflection**: Works seamlessly with `tonic-health` and `tonic-reflection`.
- **Trailers & Status**: Full metadata support for HTTP/2 headers and status trailers.

---

## Performance vs Prost

Benchmarked in `tonic-bench` (see [`docs/benchmarks.md`](../docs/benchmarks.md)):
- **Unary `rpc_mixed`**: ~2× the throughput of Prost.
- **Large payloads (`name_4kib`)**: Outperforms Prost in combined encode/decode.
- **Sparse messages & dense tags**: Gated regression tests ensure decoding speed meets or exceeds targets.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../LICENSE-MIT))

at your option.
