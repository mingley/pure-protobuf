# pbrs-grpc

[![Crates.io](https://img.shields.io/crates/v/pbrs-grpc.svg)](https://crates.io/crates/pbrs-grpc)
[![Documentation](https://docs.rs/pbrs-grpc/badge.svg)](https://docs.rs/pbrs-grpc)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../LICENSE-MIT)

`pbrs-grpc` is the native gRPC client and server crate for [`pbrs`](../README.md). Use it when you want pure-Rust protobuf messages and a direct HTTP/2 gRPC stack without Tonic, Tower, Hyper, or a C/C++ build toolchain. The crate is still preview software at `0.1.0-alpha.1`.

> ⚠️ **Pre-Release Notice**: `pbrs-grpc` is currently in **preview / pre-release (`0.1.0-alpha.1`)** and is undergoing active production qualification.
>
> ### Scope & Boundaries
> - **Client-Side Name Resolution & Load Balancing**: Dials single authorities directly (`host:port`); no dynamic DNS or xDS control plane.
> - **Application-Level Retries & Hedging**: Connection loss transparently redials at most once before stream transmission; call-site retries use `Code::is_retryable`.
> - **HTTP CONNECT Proxy**: Dials TCP directly; HTTP proxy traversal is not supported.

## What it provides

- **Pure Rust**: no C or C++ compiler is required in the build tree.
- **No unsafe in the gRPC kernel**: the crate uses `#![forbid(unsafe_code)]`.
- **Independent transport**: runs directly on prior-knowledge HTTP/2 (`h2`), `rustls`, and Graviola.
- **Native pbrs messages**: generated stubs use the `pbrs` `Parse` and `Serialize` traits.

## Installation

Add `pbrs` and `pbrs-grpc` to `Cargo.toml`:

```toml
[dependencies]
pbrs = "0.1"
pbrs-grpc = "0.1.0-alpha.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.1"
```

This checkout builds `pbrs-grpc` from checked descriptor sets without `protoc`.
The published `0.1.0-alpha.1` archive still needs `protoc` for its own build
until a new version ships.

The quickstart `compile_protos` step below still requires `protoc` for your own
`.proto` files. To build application stubs without `protoc`, check in a
descriptor set and use
[`Config::compile_descriptor_set`](../docs/guides/codegen.md#generating-from-a-checked-descriptor-set).

## Quickstart

### 1. Define a service (`proto/hello.proto`)

```protobuf
syntax = "proto3";
package hello;

service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
}

message HelloRequest {
  string name = 1;
}

message HelloReply {
  string message = 1;
}
```

### 2. Generate native stubs (`build.rs`)

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])?;
    Ok(())
}
```

### 3. Implement the server

```rust
use pbrs_grpc::{Request, Response, Server, Status};

include!(concat!(env!("OUT_DIR"), "/hello.rs"));

#[derive(Clone)]
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
    GreeterServer::new(MyGreeter).serve("127.0.0.1:50051").await?;
    Ok(())
}
```

### 4. Call the service

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = GreeterClient::connect("127.0.0.1:50051").await?;
    let mut req = HelloRequest::new();
    req.set_name("Ada");

    let reply = client.say_hello(Request::new(req)).await?;
    println!("Server replied: {}", reply.get_ref().message());
    Ok(())
}
```

## Capabilities

| Area | What is available |
|---|---|
| RPC shapes | Unary, server-streaming, client-streaming, and bidirectional streaming. See the [RPC shapes guide](../docs/guides/rpc-shapes.md). |
| TLS and mTLS | `rustls` + Graviola with enforced ALPN `h2`; verified client identities are available through `Rpc::peer_identity`. See the [production service guide](../docs/guides/production-service.md). |
| Routing | `Router` composes multiple services on one TCP/TLS port. |
| Local IPC | Unix Domain Sockets (`serve_unix`, `connect_unix`) and in-memory duplex channels (`Channel::from_io`). |
| HTTP/2 defenses | Mitigations for rapid reset (CVE-2023-44487), CONTINUATION floods, and oversize frames. |
| Interceptors | Client and server interceptor pipelines with request context extensions. See the [interceptors guide](../docs/guides/interceptors.md). |
| Operations | Built-in gRPC health checking (`grpc.health.v1`) and server reflection (`grpc.reflection.v1`). See the [operations guide](../docs/guides/operations.md). |
| Errors | Packed `google.rpc.Status` error details on `grpc-status-details-bin`. |

## Design invariants and comparisons

`pbrs-grpc` focuses on predictable execution, high throughput, and strict bounded memory consumption. For a deeper comparison with Tonic and gRPC-Go, see the [framework comparison guide](../docs/guides/comparison.md).

| Domain | `pbrs-grpc` invariant | Difference from common alternatives |
|---|---|---|
| Addressing | `host:port` string or `Target` | Tonic uses `http://` or `https://` URIs. gRPC-Go uses resolver schemes. |
| Concurrency | `ServerConfig::max_concurrent_rpcs` enforces a cap | Overflow fails fast with `RESOURCE_EXHAUSTED` instead of unbounded queuing. |
| Keepalive | HTTP/2 PINGs and TCP OS keepalives are configured separately | Idle PINGs are enabled once an interval is set. |
| Retries | Transparent retry is at most once on fresh connections | There is no complex service-config engine for all cases; application retries use `Code::is_retryable`. |

## More documentation

- [Primary gRPC guide](../docs/grpc.md) — end-to-end service guide.
- [Implementing RPC call shapes](../docs/guides/rpc-shapes.md) — unary and streaming walkthroughs.
- [Production service and TLS](../docs/guides/production-service.md) — certificates, mTLS, timeouts, and drain.
- [Interceptors and overlays](../docs/guides/interceptors.md) — context propagation and middleware.
- [Operations and diagnostics](../docs/guides/operations.md) — health checking, reflection, and tuning.
- [Code generation](../docs/guides/codegen.md) — `protoc` integration, stub options, and build scripts.
- [Framework comparison](../docs/guides/comparison.md) — detailed comparison with Tonic and gRPC-Go.
- [Architecture and design](../docs/architecture.md) — kernel internals and HTTP/2 framing architecture.
