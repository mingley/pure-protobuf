# pbrs-grpc

[![Crates.io](https://img.shields.io/crates/v/pbrs-grpc.svg)](https://crates.io/crates/pbrs-grpc)
[![Documentation](https://docs.rs/pbrs-grpc/badge.svg)](https://docs.rs/pbrs-grpc)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../LICENSE-MIT)

`pbrs-grpc` is the native gRPC client and server crate for [`pbrs`](../README.md). Use it when you want pure-Rust protobuf messages and a direct HTTP/2 gRPC stack without Tonic, Tower, Hyper, or a C/C++ build toolchain. The crate is still preview software at `0.1.0-alpha.2`.

> ⚠️ **Pre-Release Notice**: `pbrs-grpc` is currently in **preview / pre-release (`0.1.0-alpha.2`)** and is undergoing active production qualification.
>
> ### Scope & Boundaries
> - **Name resolution and load balancing**: plain `Channel::connect` still dials one `host:port`; resolver URIs opt in with `Channel::connect_uri`. Built-in schemes are `dns:`, `passthrough:`, `ipv4:`, `ipv6:`, `unix:`, and `unix-abstract:`. xDS targets are not supported.
> - **Retries and hedging**: transparent retry stays at most once before a call commits. JSON service config is opt-in through `Channel::service_config` or resolver-delivered DNS TXT and supplies bounded `retryPolicy`, `hedgingPolicy`, throttling, and pushback.
> - **HTTP CONNECT proxy**: TCP dials consult `HTTPS_PROXY`/`NO_PROXY` on every dial and tunnel with HTTP CONNECT. There is no per-channel proxy configuration surface yet.

## What it provides

- **Pure Rust**: no C or C++ compiler is required in the build tree.
- **Mostly safe Rust kernel**: gRPC framing, dispatch, transport, TLS, codec, resolver, load-balancer, authz, binlog, and service-config modules forbid unsafe. Two Linux-only OS helpers use scoped `SAFETY`-documented unsafe for `TCP_USER_TIMEOUT` and per-core CPU pinning.
- **Independent transport**: runs directly on prior-knowledge HTTP/2 (`h2`), `rustls`, and Graviola.
- **Native pbrs messages**: generated stubs use the `pbrs` `Parse` and `Serialize` traits.

## Optional features

The default feature set stays small. Enable `tower` only when you want Tower
integration:

```toml
pbrs-grpc = { version = "0.1.0-alpha.2", features = ["tower"] }
```

With that feature, `Router::into_tower_service()` exposes native services as
`tower::Service<http::Request<B>>` for axum/hyper co-hosting; health and
reflection use the same router path. `Channel::tower_unary()` exposes one
unary method as a tower service so caller-selected layers such as timeout,
concurrency limit, load shed, and tracing wrap the client without adding a
buffer to the default `Channel` path. See `examples/axum-cohost`.

Enable `zstd` only when you need `grpc-encoding: zstd`:

```toml
pbrs-grpc = { version = "0.1.0-alpha.2", features = ["zstd"] }
```

The default feature set keeps the crate MSRV at Rust 1.85. The `zstd` feature
uses `ruzstd` 0.9 and therefore has MSRV 1.87; all requested compression levels
map to `ruzstd`'s implemented `Fastest` mode (roughly zstd level 1).

## Installation

Add `pbrs` and `pbrs-grpc` to `Cargo.toml`:

```toml
[dependencies]
pbrs = "0.2"
pbrs-grpc = "0.1.0-alpha.2"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.2"
```

From `0.1.0-alpha.2` on, `pbrs-grpc` builds from checked descriptor sets
without `protoc`. The older `0.1.0-alpha.1` archive still needs `protoc` for
its own build.

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
| Message codec | The `CodecMessage` trait abstracts native messages. pbrs messages use the default fast path: direct encode into frames, `Bytes` parsing, and shared large `bytes` segments. |
| TLS and mTLS | `rustls` + Graviola with enforced ALPN `h2`; verified client identities are available through `Rpc::peer_identity`. See the [production service guide](../docs/guides/production-service.md). |
| Routing | `Router` composes multiple services on one TCP/TLS port. With the optional `tower` feature it can also be mounted as a tower service next to REST routes. |
| Local IPC | Unix Domain Sockets (`serve_unix`, `connect_unix`) and in-memory duplex channels (`Channel::from_io`). |
| HTTP/2 defenses | Mitigations for rapid reset (CVE-2023-44487), CONTINUATION floods, and oversize frames. |
| Name resolution | Default direct `host:port`; opt-in `Channel::connect_uri` for `dns:`, `passthrough:`, `ipv4:`, `ipv6:`, `unix:`, and `unix-abstract:` targets. DNS refresh needs explicit `DnsConfig` bounds. |
| Load balancing | Default `pick_first`; opt-in `loadBalancingConfig` supports `pick_first`, `round_robin`, `weighted_round_robin`, `ring_hash`, `least_request`, `random_subsetting_experimental`, `priority`, and `outlier_detection`. |
| Retries and hedging | Transparent retry is automatic and at most once before commitment. Service-config `retryPolicy` and unary `hedgingPolicy` are opt-in through `Channel::service_config` or resolver service config. |
| HTTP CONNECT proxy | `HTTPS_PROXY` / `NO_PROXY` env support with CONNECT tunneling, optional Basic auth, and TLS end-to-end through the tunnel. |
| Interceptors | Client and server interceptor pipelines with request context extensions. See the [interceptors guide](../docs/guides/interceptors.md). |
| Operations | Built-in gRPC health checking (`grpc.health.v1`), server reflection (`grpc.reflection.v1`), channelz (`grpc.channelz.v1`), binary logging, ORCA load reports, and optional OpenTelemetry observers. See the [operations guide](../docs/guides/operations.md). |
| Authorization | gRFC A43 JSON authorization policies for `Server` and `Router`, plus A59 audit logging hooks. |
| Errors | Packed `google.rpc.Status` error details on `grpc-status-details-bin`. |
| Large payloads | `bytes` fields of 4 KiB or more are parsed without copying; fields of 32 KiB or more set from `bytes::Bytes` are sent without copying. See [large payloads / zero-copy](../docs/zero-copy.md). |

## Design invariants and comparisons

`pbrs-grpc` focuses on predictable execution, high throughput, and strict bounded memory consumption. For a deeper comparison with Tonic and gRPC-Go, see the [framework comparison guide](../docs/guides/comparison.md).

| Domain | `pbrs-grpc` invariant | Difference from common alternatives |
|---|---|---|
| Addressing | `host:port` string or opt-in resolver URI | Tonic uses `http://` or `https://` URIs. gRPC-Go also supports xDS resolver schemes; `pbrs-grpc` does not. |
| Concurrency | `ServerConfig::max_concurrent_rpcs` enforces a cap | Overflow fails fast with `RESOURCE_EXHAUSTED` instead of unbounded queuing. |
| Keepalive | HTTP/2 PINGs and TCP OS keepalives are configured separately | Idle PINGs are enabled once an interval is set. |
| Retries | Transparent retry is at most once; service-config retry/hedging is opt-in and bounded | Application retries still use `Code::is_retryable` when no service-config policy applies. |

## More documentation

- [Primary gRPC guide](../docs/grpc.md) — end-to-end service guide.
- [Implementing RPC call shapes](../docs/guides/rpc-shapes.md) — unary and streaming walkthroughs.
- [Production service and TLS](../docs/guides/production-service.md) — certificates, mTLS, timeouts, and drain.
- [Interceptors and overlays](../docs/guides/interceptors.md) — context propagation and middleware.
- [Operations and diagnostics](../docs/guides/operations.md) — health checking, reflection, and tuning.
- [Code generation](../docs/guides/codegen.md) — `protoc` integration, stub options, and build scripts.
- [Framework comparison](../docs/guides/comparison.md) — detailed comparison with Tonic and gRPC-Go.
- [Large payloads / zero-copy](../docs/zero-copy.md) — where large `bytes` payloads are and are not copied.
- [Architecture and design](../docs/architecture.md) — kernel internals and HTTP/2 framing architecture.
