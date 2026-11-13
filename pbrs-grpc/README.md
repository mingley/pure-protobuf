# pbrs-grpc

[![Crates.io](https://img.shields.io/crates/v/pbrs-grpc.svg)](https://crates.io/crates/pbrs-grpc)
[![Documentation](https://docs.rs/pbrs-grpc/badge.svg)](https://docs.rs/pbrs-grpc)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/mingley/pure-protobuf/blob/main/LICENSE-MIT)

`pbrs-grpc` is a native HTTP/2 gRPC client and server for Rust. It supports all
four RPC shapes, generated [`pbrs`](https://crates.io/crates/pbrs) messages,
TLS/mTLS, gRPC-Web over HTTP/2, and optional Prost, Tower, and Tonic
integration. The transport uses an embedded `h2` backend and Tokio.

Version `0.1.0-alpha.3` (paired with `pbrs` `0.2.1`) is a pre-release preview.
See [implementation status](https://github.com/mingley/pure-protobuf/blob/main/docs/status.md)
for tested features, the [scoreboard](https://github.com/mingley/pure-protobuf/blob/main/docs/scoreboard.md)
for measurements, and the [support policy](https://github.com/mingley/pure-protobuf/blob/main/docs/support-policy.md)
for qualification boundaries. Configure connection, RPC, message, and byte
limits for your workload.

For a runnable introduction, start with
[the greeter example](https://github.com/mingley/pure-protobuf/blob/main/examples/greeter/README.md).
The [gRPC guide](https://github.com/mingley/pure-protobuf/blob/main/docs/grpc.md)
links the full tutorials and protocol contracts.

## What it provides

- Unary, client-streaming, server-streaming, and bidirectional calls.
- Generated client and server stubs using `pbrs` `Parse` and `Serialize` traits
  (plus Tonic-shaped compat stubs via `Config::tonic_compat(true)`).
- TLS/mTLS, gzip/deflate/optional zstd compression, multi-service `Router`,
  `Endpoint` builder, and fail-fast resource limits.
- Optional adapters for `prost::Message` codecs, Tower client/server middleware,
  Tonic services, and HTTP/2 gRPC-Web (`application/grpc-web` and
  `application/grpc-web-text`).

### TLS target portability

On `x86_64` and `aarch64`, built-in TLS constructors (`ClientTls::webpki`,
`ClientTls::ca`, `ServerTls::new`, `ServerTls::mtls`) use `rustls` with
Graviola and WebPKI verification. On other architectures (including `i686`
Linux, where the older published `0.1.0-alpha.2` release failed to build),
built-in TLS constructors return `UNIMPLEMENTED`; supply a trusted rustls
configuration and crypto provider via `ClientTls::from_rustls` or
`ServerTls::from_rustls`. TLS failures never fall back to plaintext.

### Buffer retention bounds

Compressed streams reuse large serialization and output backing buffers up to
4 MiB per allocation and 16 MiB process-wide, bounded independently of decoded
message storage; encoding failure or panic drops reusable storage while
preserving completed frames. See the
[resource budget rules](https://github.com/mingley/pure-protobuf/blob/main/docs/resource-budgets.md#2-buffer-ownership),
the [October 27 decode/map record](https://github.com/mingley/pure-protobuf/blob/main/docs/evidence/decode-and-map-speed-20261027/README.md),
and the [October 28 transport record](https://github.com/mingley/pure-protobuf/blob/main/docs/evidence/optimizations-20261028/README.md).

## Installation

Add `pbrs` and `pbrs-grpc` to `Cargo.toml`:

```toml
[dependencies]
pbrs = "0.2.1"
pbrs-grpc = "0.1.0-alpha.3"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.2.1"
```

From `0.1.0-alpha.2` onward, `pbrs-grpc` builds its internal service descriptors
without `protoc` (only the older `0.1.0-alpha.1` archive required `protoc` to
build the crate itself).

Compiling your own `.proto` files with `compile_protos` requires `protoc` on
`PATH`. To build application stubs without `protoc`, check in a compiled
descriptor set and call
[`Config::compile_descriptor_set`](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/codegen.md#generating-from-a-checked-descriptor-set).

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
use pbrs_grpc::{Request, Response, Status};

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
    GreeterServer::new(MyGreeter).serve("127.0.0.1:50051".parse()?).await?;
    Ok(())
}
```

### 4. Call the service

```rust
use pbrs_grpc::Request;

include!(concat!(env!("OUT_DIR"), "/hello.rs"));

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

## Optional features

Default features keep the crate MSRV at **Rust 1.85** with native gzip/deflate
compression and TLS. Enable optional features in `Cargo.toml` as needed:

```toml
pbrs-grpc = { version = "0.1.0-alpha.3", features = ["grpc-web", "tower"] }
```

| Feature | MSRV | Use it for |
|---|---|---|
| `prost` | 1.85 | Serving and calling `prost::Message` types on the native transport (`pbrs_grpc::codec::prost`). |
| `tower` | 1.85 | `Router::into_tower_service()` for axum/Hyper co-hosting and `Channel::tower_*()` client services. |
| `tonic` | 1.88 | Mounting native services in Tonic or Tonic services/clients on the native transport. |
| `tonic-gzip` | 1.88 | Tonic's gzip codec alongside `tonic`. Native gzip needs no feature flag. |
| `grpc-web` | 1.85 | Unary and server-streaming HTTP/2 gRPC-Web (`grpc-web` and base64 `grpc-web-text`) plus opt-in CORS. |
| `otel` | 1.85 | OpenTelemetry metrics and trace propagation with explicitly installed observers. |
| `native-roots` | 1.85 | Operating-system root certificate trust (`ClientTls::native_roots`). |
| `zstd` | 1.87 | Zstandard (`grpc-encoding: zstd`) via `ruzstd` 0.9 (`Fastest` mode). |
| `copy-counts` | 1.85 | User-space copy and scheduler counters for benchmarks. |

*(Note: the older published `0.1.0-alpha.2` preview included only `otel` and
`copy-counts`.)*

### Tower and gRPC-Web notes

With `tower`, `Router::into_tower_service()` exposes native services (including
health and reflection) as `tower::Service<http::Request<B>>`, and
`Channel::tower_unary()`, `tower_server_streaming()`, `tower_client_streaming()`,
and `tower_bidi()` expose client calls to Tower timeout, concurrency,
load-shed, and tracing layers while keeping the default `Channel` unbuffered.
See [the gRPC guide](https://github.com/mingley/pure-protobuf/blob/main/docs/grpc.md#message-codecs-and-tower-integration)
and [the axum co-host example](https://github.com/mingley/pure-protobuf/blob/main/examples/axum-cohost/README.md).

With `grpc-web`, native preflight denies all origins by default. Configure
allowed origins with `ServerConfig::grpc_web_allow_origin` or
`grpc_web_allow_any_origin`, and opt into automatic response CORS headers on
Web responses (including early errors and deadlines) with
`grpc_web_cors_response_headers(true)`. Application-provided
`access-control-allow-origin` and `access-control-expose-headers` metadata take
precedence; leave automatic response headers off if an upstream proxy already
adds CORS headers or if you need credentialed (`Access-Control-Allow-Credentials`)
CORS policies. For browser HTTP/1.1, see the
[Tower/Hyper recipe](https://github.com/mingley/pure-protobuf/blob/main/docs/decisions/http1-grpc-web.md).

## Capabilities

| Area | What is available |
|---|---|
| RPC shapes | Unary, server-streaming, client-streaming, and bidirectional streaming. See the [RPC shapes guide](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/rpc-shapes.md). |
| gRPC-Web | Optional `grpc-web` feature for HTTP/2 unary and server-streaming gRPC-Web (`application/grpc-web` and `application/grpc-web-text`) with explicit CORS controls. |
| Message codec | `CodecMessage` abstracts native messages. `pbrs` messages encode directly into frames, parse from `Bytes`, and share large `bytes` segments; optional `prost` adds `prost::Message` codecs. |
| TLS and mTLS | Built-in constructors use `rustls` + Graviola and WebPKI/CA/CRL/SPIFFE verification on x86-64 and AArch64. Caller-owned rustls configs/providers are supported via `ServerTls::from_rustls` / `ClientTls::from_rustls` with enforced ALPN `h2`. See the [caller-config contract](https://github.com/mingley/pure-protobuf/blob/main/docs/decisions/caller-rustls-config.md) and [production service guide](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/production-service.md). |
| Routing & connect info | `Router` composes multiple services on one port. Incoming requests carry Tonic-shaped `TcpConnectInfo`, `UdsConnectInfo`, and `TlsConnectInfo` extensions alongside direct `Rpc`/`Request` peer getters. |
| Metadata | Typed ASCII and `-bin` metadata plus read-only raw wire inspection (`Metadata::get_raw`, `Metadata::get_all_raw`); reserved protocol headers stay hidden. |
| Local IPC | Unix Domain Sockets (`serve_unix`, `connect_unix`) with peer credentials (`SO_PEERCRED` / `LOCAL_PEERCRED`) and in-memory duplex channels (`Channel::from_io`). |
| HTTP/2 defenses | Mitigations for rapid reset (CVE-2023-44487), CONTINUATION floods, small-DATA floods, and oversize frames; optional adaptive flow-control windows (`adaptive_window`). |
| Name resolution & `Endpoint` | Direct `host:port`, Tonic-style `Endpoint` (`origin`, `executor`, `connect_lazy`, `balance_list`, `balance_channel`), and opt-in `Channel::connect_uri` for `dns:`, `passthrough:`, `ipv4:`, `ipv6:`, `unix:`, and `unix-abstract:`. |
| Load balancing | Default `pick_first`; opt-in `loadBalancingConfig` supports `pick_first`, `round_robin`, `weighted_round_robin`, `ring_hash`, `least_request`, `random_subsetting_experimental`, `priority`, and `outlier_detection`. |
| Retries and hedging | At-most-once transparent retry on proven non-execution; opt-in service-config `retryPolicy` and unary `hedgingPolicy` via `Channel::service_config` or resolver config. Hedging has response-commitment and pushback limitations; see the [retry contract](https://github.com/mingley/pure-protobuf/blob/main/docs/retry-contract.md). |
| HTTP CONNECT proxy | `HTTPS_PROXY` / `NO_PROXY` environment support with CONNECT tunneling, optional Basic auth, and end-to-end TLS. |
| Interceptors | Client, server, and response interceptor pipelines with per-RPC `Extensions`. See the [interceptors guide](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/interceptors.md). |
| Operations | Built-in health checking (`grpc.health.v1`), server reflection (`grpc.reflection.v1` and `v1alpha`), channelz (`grpc.channelz.v1`), binary logging, ORCA load reports, and optional OpenTelemetry observers. See the [operations guide](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/operations.md). |
| Authorization | gRFC A43 JSON authorization policies for `Server` and `Router`, plus A59 audit logging hooks. |
| Errors | Packed `google.rpc.Status` error details on `grpc-status-details-bin` (`ErrorDetails`). |
| Large payloads | `bytes` fields of 4 KiB or more parse without copying; fields of 32 KiB or more set from `bytes::Bytes` send without copying. See [large payloads / zero-copy](https://github.com/mingley/pure-protobuf/blob/main/docs/zero-copy.md). |

## Configuration and behavior

`pbrs-grpc` provides explicit resource limits and fail-fast admission controls.
Connection and RPC counts are uncapped by default; configured transport limits
do not bound application allocations or total process memory. See the
[resource model](https://github.com/mingley/pure-protobuf/blob/main/docs/resource-budgets.md)
and [framework comparison](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/comparison.md).

| Domain | `pbrs-grpc` invariant | Difference from common alternatives |
|---|---|---|
| Addressing | `host:port`, `Endpoint` URI, or opt-in resolver URI | Tonic uses `http://` or `https://` URIs. gRPC-Go also supports xDS resolver schemes; `pbrs-grpc` does not. |
| Concurrency | `ServerConfig::max_concurrent_rpcs` enforces a cap | Overflow fails fast with `RESOURCE_EXHAUSTED` instead of unbounded queuing. |
| Keepalive | HTTP/2 PINGs and TCP OS keepalives are configured separately | Idle PINGs default on once an interval is set; `ChannelConfig::keep_alive_while_idle(false)` limits them to active RPCs. |
| Retries | Sequential transparent retry is at most once per attempt; service-config retries and hedges are opt-in with bounded attempt counts | Application retries can use `Code::is_retryable`. Hedging has known commitment and pushback limitations; see the [retry contract](https://github.com/mingley/pure-protobuf/blob/main/docs/retry-contract.md). |

xDS targets and async OAuth2/JWT credential providers are planned work.
HTTP CONNECT proxy selection uses `HTTPS_PROXY` / `NO_PROXY` environment
variables on each dial. The
[support policy](https://github.com/mingley/pure-protobuf/blob/main/docs/support-policy.md)
records qualification boundaries.

## More documentation

- [Primary gRPC guide](https://github.com/mingley/pure-protobuf/blob/main/docs/grpc.md) — end-to-end service guide.
- [Implementing RPC call shapes](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/rpc-shapes.md) — unary and streaming walkthroughs.
- [Production service and TLS](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/production-service.md) — certificates, mTLS, timeouts, and drain.
- [Interceptors and overlays](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/interceptors.md) — context propagation and middleware.
- [Operations and diagnostics](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/operations.md) — health checking, reflection, and tuning.
- [Code generation](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/codegen.md) — `protoc` integration, stub options, and build scripts.
- [Framework comparison](https://github.com/mingley/pure-protobuf/blob/main/docs/guides/comparison.md) — detailed comparison with Tonic and gRPC-Go.
- [Large payloads / zero-copy](https://github.com/mingley/pure-protobuf/blob/main/docs/zero-copy.md) — where large `bytes` payloads are and are not copied.
- [Architecture and design](https://github.com/mingley/pure-protobuf/blob/main/docs/architecture.md) — kernel internals and HTTP/2 framing architecture.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/mingley/pure-protobuf/blob/main/LICENSE-APACHE))
- MIT License ([LICENSE-MIT](https://github.com/mingley/pure-protobuf/blob/main/LICENSE-MIT))

at your option.
