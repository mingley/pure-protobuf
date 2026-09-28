# Building Services with pbrs-grpc

This guide is the main entry point for building native gRPC services with `pbrs-grpc`. It is for Rust developers who want generated clients and servers over `pbrs` messages without Tonic. The bottom line: define protobuf services, generate native stubs, then use the focused guides for production TLS, streaming, interceptors, and operations.

`pbrs-grpc` is a standalone, pure-Rust gRPC client and server kernel built over [`pbrs`](../README.md). It has no C compiler requirement and runs directly on prior-knowledge HTTP/2. The gRPC framing, dispatch, transport, TLS, and codec modules forbid unsafe; two Linux-only OS helpers use scoped, documented unsafe for socket/user-timeout and CPU-affinity syscalls.

<a id="quickstart"></a>
## Quickstart

### Installation

Add `pbrs` and `pbrs-grpc` from crates.io to `Cargo.toml`:

```toml
[dependencies]
pbrs = "0.2"
pbrs-grpc = "0.1.0-alpha.2"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.2"
```

### Protocol definition (`proto/hello.proto`)

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

### Code generation (`build.rs`)

By default, `compile_protos` emits native `pbrs-grpc` client and server stubs:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])?;
    Ok(())
}
```

### Server implementation

Include the generated code, implement the `Greeter` trait, and start serving:

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
        let name = request.get_ref().name();
        let mut reply = HelloReply::new();
        reply.set_message(format!("Hello, {name}!"));
        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    GreeterServer::new(MyGreeter).serve("127.0.0.1:50051").await?;
    Ok(())
}
```

### Client execution

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = GreeterClient::connect("127.0.0.1:50051").await?;
    let mut req = HelloRequest::new();
    req.set_name("Ada");

    let reply = client.say_hello(Request::new(req)).await?;
    println!("Response: {}", reply.get_ref().message());
    Ok(())
}
```

<a id="the-four-call-shapes"></a>
<a id="reading-a-stream"></a>
<a id="writing-a-stream"></a>
<a id="client-streaming"></a>
## The Four Call Shapes

`pbrs-grpc` supports all four standard gRPC communication patterns.

| Shape | How to think about it |
|---|---|
| Unary | One request, one response. |
| Server streaming | The client reads a stream through `Streaming<T>`. |
| Client streaming | The client sends a stream through `StreamSender<T>`. |
| Bidirectional streaming | Both sides exchange messages concurrently on one HTTP/2 stream. |

For complete examples, channel setup, and cancellation behavior, see the
[RPC shapes guide](guides/rpc-shapes.md).

## Production Capabilities & How-to Guides

The sections below summarize what ships and point to the detailed guides.

<a id="tls"></a>
<a id="unix-domain-sockets"></a>
<a id="in-process-connections"></a>
### 1. Transport security and topologies

- **TLS and mutual TLS (mTLS)**: powered by `rustls` + Graviola. It enforces ALPN `h2`. Certificate verification is not optional. Inspect peer certificates through `Rpc::peer_identity`.
- **Unix Domain Sockets (UDS)**: low-latency local inter-process communication (IPC) with `serve_unix_unlink` and peer credentials through `Rpc::peer_cred`.
- **In-process channels (`from_io`)**: in-memory channels using `tokio::io::duplex`. In-process `from_io` connections have no transparent retry.
- **HTTP CONNECT proxy**: TCP dials consult `HTTPS_PROXY` / `NO_PROXY` and tunnel with CONNECT when the target is not bypassed. TLS still runs end-to-end above the tunnel.

See the [production service configuration guide](guides/production-service.md).

<a id="deadlines-and-cancellation"></a>
<a id="connect-timeout"></a>
<a id="wait-for-ready-and-lazy-connect"></a>
<a id="graceful-shutdown"></a>
<a id="connection-age-and-idle"></a>
<a id="serving-several-services"></a>
<a id="compression"></a>
### 2. Lifecycle, deadlines, and routing

- **Timeouts and deadlines**: propagate `grpc-timeout` headers and inspect the remaining budget at runtime.
- **Graceful shutdown**: drain active connections with `Server::serve_with_shutdown` and HTTP/2 `GOAWAY`.
- **Connection age and idle**: recycle connections with `max_connection_age` (plus or minus 10% jitter) and `max_connection_idle`.
- **Name resolution**: `Channel::connect` dials one direct authority (`host:port`). `Channel::connect_uri` opts into resolver targets: `dns:`, `passthrough:`, `ipv4:`, `ipv6:`, `unix:`, and `unix-abstract:`. DNS refresh requires explicit `DnsConfig` bounds.
- **Load balancing**: resolver-managed channels default to `pick_first`. Service-config `loadBalancingConfig` can select `round_robin`, `weighted_round_robin`, `ring_hash`, `least_request`, `random_subsetting_experimental`, `priority`, or `outlier_detection`.
- **Multi-service routing**: serve multiple services on one TCP/TLS listener with `Router`.
- **Compression**: negotiate message-level gzip compression.

See the [production service configuration guide](guides/production-service.md).

<a id="metadata"></a>
<a id="interceptors-and-middleware"></a>
### 3. Interceptors and metadata

- **Interceptors**: attach middleware on the client (`ClientInterceptor`), server inbound path (`Interceptor`), or server outbound path (`ResponseInterceptor`).
- **Metadata**: handle ASCII headers and binary `-bin` trailers.
- **Request overlays and extensions**: set per-request timeouts, user agents, compression, and typed Rust values through `Extensions`.

See the [interceptors and context guide](guides/interceptors.md).

<a id="health-checks"></a>
<a id="reflection"></a>
<a id="keepalive"></a>
<a id="tuning"></a>
<a id="errors-and-status-codes"></a>
<a id="testing"></a>
### 4. Operations, diagnostics, and errors

- **Health checking (`grpc.health.v1`)**: built-in `HealthReporter`, `Check`, `Watch`, and `Health::list`.
- **Server reflection (`grpc.reflection.v1`)**: dynamic service discovery for tools such as `grpcurl`.
- **Channelz (`grpc.channelz.v1`)**: opt into serving `ChannelzService::shared_global()` to expose bounded channel, server, subchannel, socket, and trace snapshots.
- **Binary logging**: attach `binlog::BinaryLogger` to a `Channel`, `Server`, or `Router`. Filters and `{h;m}` caps keep logging explicit and bounded.
- **OpenTelemetry**: optional `otel` Cargo feature. Install `otel::Metrics` and tracing observers explicitly; without the feature the module compiles out.
- **ORCA**: backends can publish per-call or out-of-band load reports, and `weighted_round_robin` consumes them when selected.
- **Keepalive and sockets**: HTTP/2 PINGs and TCP `SO_KEEPALIVE` with `TCP_NODELAY`.
- **Rich errors**: standard `Code` variants and packed `google.rpc.Status` with `ErrorDetails` on `grpc-status-details-bin`.
- **Authorization**: enforce gRFC A43 JSON policies with `Server::authorization_policy` or `Router::authorization_policy`; audit logging hooks are available through `authz`.

See the [operations and diagnostics guide](guides/operations.md).

<a id="writing-a-service-without-codegen"></a>
### 5. Code generation and migration

- **Custom stubs**: configure native kernel stubs (`emit_kernel_stubs`), Tonic stubs (`emit_tonic_stubs`), or messages only.
- **Prost and Tonic migration**: compare `Parse` / `Serialize` with `prost::Message`.
- **Manual services**: implement raw byte streaming through `Service::call` without codegen.

See the [code generation guide](guides/codegen.md) and [migration guide](guides/migration.md).

<a id="limits-and-the-threat-model"></a>
## Limits and the Threat Model

`pbrs-grpc` uses bounded defaults against common denial-of-service vectors.

| Limit | What it prevents |
|---|---|
| Rapid reset defense | Mitigates HTTP/2 rapid reset abuse (CVE-2023-44487) with `ServerConfig::max_pending_accept_reset_streams`. |
| CONTINUATION flood defense | Caps header lists with `max_header_list_size` (16 KiB default) before unbounded buffers allocate. |
| Message framing limits | Caps inbound messages at 4 MiB by default (`max_decoding_message_size`); oversize messages fail early as `Code::ResourceExhausted`. |
| Concurrency and streams | `ServerConfig::max_concurrent_rpcs` caps in-flight server tasks; overflow is rejected instead of queued without bound. |

## Retries and Resilience

`pbrs-grpc` keeps retry behavior explicit and bounded.

- **Transparent retry**: Transparent retry is at most once. It happens only when a connection drops before request headers or body transmission commits.
- **Service-config retries**: attach an A6 document with `Channel::service_config` for `retryPolicy` settings: backoff, throttling, per-attempt timeouts, and server pushback. Unary and server-streaming calls use the policy before response commitment. Unary calls also accept bounded `hedgingPolicy`.
- **No policy fallback**: without a policy, application retries stay at the call site and should be evaluated with `Code::is_retryable`.
- **In-process bypass**: in-process connections (`from_io`) have no transparent retry.

For full state transitions and commitment boundaries, read the
[retry contract](retry-contract.md).

<a id="what-is-not-here"></a>
## What is not here

`pbrs-grpc` deliberately leaves out features that would add control-plane
complexity or unbounded speculation.

- **No xDS control plane**: `xds:///` targets are rejected. Use an L4/L7 proxy or service mesh for xDS-managed routing.
- **No automatic resolver defaults**: plain `host:port` remains direct dialing. Resolver/LB behavior starts only when code calls `Channel::connect_uri` or attaches service config.
- **No always-on observability export**: channelz and binary logging ship, but you must mount the service or attach a logger. OpenTelemetry requires the `otel` feature plus an installed observer.
- **No unbounded speculation**: Hedging ships only as opt-in, `maxAttempts`-bounded unary `hedgingPolicy` (see [Retries and Resilience](#retries-and-resilience)); there is no speculative RPC outside that policy.

For a wider comparison with Tonic and gRPC-Go, see the
[framework comparison guide](guides/comparison.md).
