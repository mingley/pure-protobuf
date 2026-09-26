# Building Services with pbrs-grpc

`pbrs-grpc` is a standalone, pure-Rust gRPC client and server kernel built over [`pbrs`](../README.md). It provides an independent, memory-safe alternative to `tonic` with no C compiler requirements, no `unsafe` code in the kernel (`#![forbid(unsafe_code)]`), and direct execution on prior-knowledge HTTP/2.

---

<a id="quickstart"></a>
## Quickstart

### Installation
Add `pbrs` and `pbrs-grpc` from crates.io to your `Cargo.toml`:

```toml
[dependencies]
pbrs = "0.1"
pbrs-grpc = "0.1.0-alpha.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.1"
```

### Protocol Definition (`proto/hello.proto`)
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

### Code Generation (`build.rs`)
By default, `compile_protos` emits native `pbrs-grpc` client and server stubs:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])?;
    Ok(())
}
```

### Server Implementation
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

### Client Execution
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

---

<a id="the-four-call-shapes"></a>
<a id="reading-a-stream"></a>
<a id="writing-a-stream"></a>
<a id="client-streaming"></a>
## The Four Call Shapes

`pbrs-grpc` natively supports all four standard gRPC communication patterns:

1. **Unary Calls**: Single request, single response.
2. **Server Streaming**: Client reads a continuous stream via `Streaming<T>`.
3. **Client Streaming**: Client streams messages to server via `StreamSender<T>`.
4. **Bidirectional Streaming**: Concurrent full-duplex streaming over one HTTP/2 stream.

For detailed walkthroughs, channel setup, and cancellation patterns, see the dedicated [Implementing RPC Call Shapes Guide](guides/rpc-shapes.md).

---

## Production Capabilities & How-to Guides

<a id="tls"></a>
<a id="unix-domain-sockets"></a>
<a id="in-process-connections"></a>
### 1. Transport Security and Topologies
- **TLS and Mutual TLS (mTLS)**: Powered by `rustls` + `Graviola`. Enforces ALPN `h2`; Certificate verification is not optional. Inspect peer certificates via `Rpc::peer_identity`.
- **Unix Domain Sockets (UDS)**: Low-latency local IPC with `serve_unix_unlink` and peer credentials via `Rpc::peer_cred`.
- **In-Process Channels (`from_io`)**: High-performance in-memory channels using `tokio::io::duplex`. Note that in-process `from_io` connections have no transparent retry.

&rarr; See [Production Service Configuration Guide](guides/production-service.md).

<a id="deadlines-and-cancellation"></a>
<a id="connect-timeout"></a>
<a id="wait-for-ready-and-lazy-connect"></a>
<a id="graceful-shutdown"></a>
<a id="connection-age-and-idle"></a>
<a id="serving-several-services"></a>
<a id="compression"></a>
### 2. Lifecycle, Deadlines & Routing
- **Timeouts & Deadlines**: Propagate `grpc-timeout` headers and inspect remaining budget at runtime.
- **Graceful Shutdown**: Drain active connections safely with `Server::serve_with_shutdown` and HTTP/2 `GOAWAY`.
- **Connection Age & Idle**: Enforce connection recycling with `max_connection_age` (±10% jitter) and `max_connection_idle`.
- **Multi-Service Routing**: Multiplex multiple services onto one TCP/TLS listener using `Router`.
- **Compression**: Negotiate message-level gzip compression transparently.

&rarr; See [Production Service Configuration Guide](guides/production-service.md).

<a id="metadata"></a>
<a id="interceptors-and-middleware"></a>
### 3. Interceptors and Metadata
- **Interceptors**: Attach middleware at client (`ClientInterceptor`), server inbound (`Interceptor`), or server outbound (`ResponseInterceptor`).
- **Metadata**: Seamlessly handle ASCII headers and binary `-bin` trailers.
- **Request Overlays & Extensions**: Customize timeouts, user-agent, or compression per-request, and pass typed Rust values via `Extensions`.

&rarr; See [Interceptors and Context Guide](guides/interceptors.md).

<a id="health-checks"></a>
<a id="reflection"></a>
<a id="keepalive"></a>
<a id="tuning"></a>
<a id="errors-and-status-codes"></a>
<a id="testing"></a>
### 4. Operations, Diagnostics & Errors
- **Health Checking (`grpc.health.v1`)**: Built-in support for `HealthReporter`, `Check`, `Watch`, and `Health::list`.
- **Server Reflection (`grpc.reflection.v1`)**: Dynamic service discovery for tools like `grpcurl`.
- **Keepalive & Sockets**: HTTP/2 PINGs and TCP OS-level `SO_KEEPALIVE` with `TCP_NODELAY`.
- **Rich Error Model**: Standard `Code` variants and packed `google.rpc.Status` with `ErrorDetails` on `grpc-status-details-bin`.

&rarr; See [Operations and Diagnostics Guide](guides/operations.md).

<a id="writing-a-service-without-codegen"></a>
### 5. Code Generation & Migration
- **Custom Stubs**: Configure native kernel stubs (`emit_kernel_stubs`), Tonic stubs (`emit_tonic_stubs`), or messages only.
- **Prost & Tonic Migration**: Trait model comparison (`Parse`/`Serialize` vs `prost::Message`).
- **Manual Services**: Implement raw byte streaming via `Service::call` without codegen.

&rarr; See [Code Generation Guide](guides/codegen.md) and [Migration Guide](guides/migration.md).

---

<a id="limits-and-the-threat-model"></a>
## Limits and the Threat Model

`pbrs-grpc` is engineered with rigorous defensive defaults against denial-of-service vectors:

- **Rapid Reset Defense (CVE-2023-44487)**: Mitigates stream cancellation abuse with rapid reset protection via `ServerConfig::max_pending_accept_reset_streams`.
- **CONTINUATION Flood Defense**: Enforces strict header list caps against CONTINUATION frame floods via `max_header_list_size` (16 KiB default), terminating frame floods before unbounded buffers allocate.
- **Message Framing Limits**: Inbound messages default to a 4 MiB limit (`max_decoding_message_size`); exceeding messages fail early as `Code::ResourceExhausted`.
- **Concurrency & Streams**: `ServerConfig::max_concurrent_rpcs` caps in-flight server tasks; connection slots reject overflow rather than allocating unbounded memory queues.

---

## Retries and Resilience

`pbrs-grpc` implements a strict, predictable retry policy:
- **Transparent Retries**: Transparent retry is at most once, and occurs automatically only if a connection drops before request headers or body transmission commits.
- **Service-Config Retries**: Attach an A6 document with `Channel::service_config` for unary `retryPolicy` (backoff, throttling, per-attempt timeouts, server pushback) and `hedgingPolicy`. Without a policy, application-level retries remain at the call site evaluated against `Code::is_retryable`.
- **In-Process Bypasses**: In-process connections (`from_io`) have no transparent retry.

For complete state transitions and commitment boundaries, consult the [Retry Contract](retry-contract.md).

---

<a id="what-is-not-here"></a>
## What is not here

To maintain zero C dependencies, minimal overhead, and deterministic execution, `pbrs-grpc` deliberately omits non-essential features:
- **No xDS or Client-Side Load Balancing**: Services dial direct authorities (`host:port`); use an L4/L7 proxy (e.g. Envoy) for dynamic routing.
- **No Channelz or Binary Logging**: Metrics and observability are exposed via standard Rust tracing and middleware extensions.
- **No Hedging or Speculative RPCs**: Omitted from kernel to prevent latency spikes and unexpected traffic multiplication.

For a comprehensive comparison against Tonic and gRPC-Go, refer to the [Framework Comparison Guide](guides/comparison.md).
