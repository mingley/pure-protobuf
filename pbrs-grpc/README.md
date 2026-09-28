# pbrs-grpc

[![Crates.io](https://img.shields.io/crates/v/pbrs-grpc.svg)](https://crates.io/crates/pbrs-grpc)
[![Documentation](https://docs.rs/pbrs-grpc/badge.svg)](https://docs.rs/pbrs-grpc)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../LICENSE-MIT)

> ⚠️ **Pre-Release Notice**: `pbrs-grpc` is currently in **preview / pre-release (`0.1.0-alpha.1`)** and is undergoing active production qualification.
>
> ### Scope & Boundaries
> - **Client-Side Name Resolution & Load Balancing**: Dials single authorities directly (`host:port`); no dynamic DNS or xDS control plane.
> - **Application-Level Retries & Hedging**: Connection loss transparently redials at most once before stream transmission; call-site retries use `Code::is_retryable`.
> - **HTTP CONNECT Proxy**: Dials TCP directly; HTTP proxy traversal is not supported.

A pure-Rust gRPC client and server kernel over [pbrs](../README.md).
- **Zero C Dependencies**: No C or C++ compiler required in the build tree.
- **Forbid Unsafe**: The gRPC kernel code contains zero `unsafe` blocks (`#![forbid(unsafe_code)]`).
- **Independent Stack**: Built directly on prior-knowledge HTTP/2 (`h2`), `rustls`, and `Graviola` (no `tonic`, `tower`, or `hyper` dependencies).

---

## Installation

Add `pbrs` and `pbrs-grpc` to your `Cargo.toml`:

```toml
[dependencies]
pbrs = "0.1"
pbrs-grpc = "0.1.0-alpha.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.1"
```

In this checkout, `pbrs-grpc` builds from checked descriptor sets without
`protoc`. The published `0.1.0-alpha.1` archive still needs `protoc` for its
own build until a new version ships. The quickstart `compile_protos` step below still
requires `protoc` for your own `.proto`. To build application stubs without
it, check in a descriptor set and use
[`Config::compile_descriptor_set`](../docs/guides/codegen.md#generating-from-a-checked-descriptor-set).

---

## Quickstart

### 1. Define Protobuf Service (`proto/hello.proto`)
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

### 2. Configure `build.rs`
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])?;
    Ok(())
}
```

### 3. Implement Server
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

### 4. Dial from Client
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

---

## Core Features & Capabilities

- **All Four Call Shapes**: Complete support for Unary, Server-Streaming, Client-Streaming, and Bidirectional Streaming. See [RPC Shapes Guide](../docs/guides/rpc-shapes.md).
- **Production TLS & mTLS**: Powered by `rustls` + `Graviola` with enforced ALPN `h2`. Inspect verified client identities via `Rpc::peer_identity`. See [Production Service Guide](../docs/guides/production-service.md).
- **Multi-Service Routing**: Compose multiple services onto one TCP/TLS port using `Router`.
- **Local IPC**: Unix Domain Sockets (`serve_unix`, `connect_unix`) and in-memory duplex channels (`Channel::from_io`).
- **Defensive HTTP/2 Limits**: Built-in mitigations for rapid reset (CVE-2023-44487), CONTINUATION floods, and oversize frames.
- **Interceptors & Metadata**: Client and server interceptor pipelines with request context extensions. See [Interceptors Guide](../docs/guides/interceptors.md).
- **Operational Diagnostics**: Built-in gRPC Health Checking (`grpc.health.v1`) and Server Reflection (`grpc.reflection.v1`). See [Operations Guide](../docs/guides/operations.md).
- **Rich Error Model**: Full support for packed `google.rpc.Status` error details on `grpc-status-details-bin`.

---

## Design Invariants and Comparisons

`pbrs-grpc` optimizes for predictable execution, high throughput, and strict bounded memory consumption. For detailed technical comparisons against Tonic and gRPC-Go, refer to the [Framework Comparison Guide](../docs/guides/comparison.md).

| Domain | Key Invariant in `pbrs-grpc` | Comparison with Alternatives |
|---|---|---|
| **Addressing** | `host:port` string or `Target` | Tonic requires `http://` / `https://` URIs; gRPC-Go uses resolver schemes. |
| **Concurrency** | Enforced via `ServerConfig::max_concurrent_rpcs` | Fast failure with `RESOURCE_EXHAUSTED` instead of unbounded queuing layers. |
| **Keepalive** | Separate HTTP/2 PINGs and TCP OS keepalives | Explicit configuration; idle PINGs enabled once interval is set. |
| **Retries** | At-most-once transparent retry on fresh connections | No complex service-config engine; application retries use `Code::is_retryable`. |

---

## Documentation Links

- [Primary gRPC Guide](../docs/grpc.md) — Comprehensive landing guide.
- [Implementing RPC Call Shapes](../docs/guides/rpc-shapes.md) — Walkthroughs for unary and streaming patterns.
- [Production Service & TLS](../docs/guides/production-service.md) — Certificates, mTLS, timeouts, and drain.
- [Interceptors & Overlays](../docs/guides/interceptors.md) — Context propagation and middleware.
- [Operations & Diagnostics](../docs/guides/operations.md) — Health checking, reflection, and tuning.
- [Code Generation](../docs/guides/codegen.md) — Protoc integration, stub options, and build scripts.
- [Framework Comparison](../docs/guides/comparison.md) — In-depth comparison with Tonic and gRPC-Go.
- [Architecture & Design](../docs/architecture.md) — Kernel internals and HTTP/2 framing architecture.
