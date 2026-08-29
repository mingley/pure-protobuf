# Building gRPC services with pbrs-grpc

`pbrs-grpc` is a gRPC kernel: HTTP/2 framing, dispatch, and resource limits
over [pbrs](../README.md) messages, with no `unsafe` in the kernel, no C or C++
compiled into the build, and no dependency on tonic. This is the working guide.
For the API reference, run `cargo doc -p pbrs-grpc --open`; for measured
numbers, see [benchmarks](benchmarks.md).

- [Quickstart](#quickstart)
- [The four call shapes](#the-four-call-shapes)
- [Metadata](#metadata)
- [Errors and status codes](#errors-and-status-codes)
- [Deadlines and cancellation](#deadlines-and-cancellation)
- [Wait-for-ready and lazy connect](#wait-for-ready-and-lazy-connect)
- [Serving several services](#serving-several-services)
- [TLS](#tls)
- [Unix domain sockets](#unix-domain-sockets)
- [Health checks](#health-checks)
- [Graceful shutdown](#graceful-shutdown)
- [Connection age and idle](#connection-age-and-idle)
- [Compression](#compression)
- [Limits and the threat model](#limits-and-the-threat-model)
- [Tuning](#tuning)
- [Interceptors and middleware](#interceptors-and-middleware)
- [Testing](#testing)
- [Writing a service without codegen](#writing-a-service-without-codegen)
- [What is not here](#what-is-not-here)

## Quickstart

Three files. Start with the proto:

```proto
// proto/hello.proto
syntax = "proto3";
package helloworld;

service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
}

message HelloRequest { string name = 1; }
message HelloReply   { string message = 1; }
```

Add the dependencies and a build script:

```toml
# Cargo.toml
[dependencies]
pbrs = "0.1"
pbrs-grpc = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.1"
```

```rust
// build.rs
fn main() {
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(&["proto/hello.proto"], &["proto"])
        .expect("codegen");
}
```

`protoc` must be on `PATH`. `emit_kernel_stubs(true)` is what produces
`pbrs-grpc` stubs; the default is tonic stubs, and the two are mutually
exclusive because they claim the same `FooClient` / `FooServer` names.

Then implement the generated trait:

```rust
use pbrs_grpc::{Request, Response, Status};

mod pb {
    #![allow(missing_docs)]
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}

use pb::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};

struct MyGreeter;

impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {}", request.get_ref().name()));
        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Status> {
    GreeterServer::new(MyGreeter)
        .serve("127.0.0.1:50051".parse().expect("addr"))
        .await
}
```

And call it:

```rust
let channel = pbrs_grpc::Channel::connect("127.0.0.1:50051").await?;
let client = GreeterClient::new(channel);

let mut req = HelloRequest::new();
req.set_name("world");
let reply = client.say_hello(Request::new(req)).await?;
println!("{}", reply.get_ref().message());
```

`Channel::connect` takes anything that converts into a
[`Target`](https://docs.rs/pbrs-grpc): a `SocketAddr`, or a `host:port`
string that goes through DNS. The resulting `Channel` is meant to be cloned
and held for the life of the process: if a connection dies, the next RPC
redials that slot, so a server restart on the same address does not require
a new client. `Channel::connect_lazy` skips the initial dial so the client
can exist before the server; pair it with `Request::set_wait_for_ready`.

A complete worked example living in the repository is
[`pbrs-grpc-hello`](../pbrs-grpc/src/bin/pbrs-grpc-hello.rs), which exercises
all four call shapes over loopback.

## The four call shapes

What the generated trait and client look like depends on which sides of the
RPC stream.

| `.proto` | Handler receives | Handler returns | Client method returns |
|---|---|---|---|
| `rpc M (Req) returns (Resp)` | `Request<Req>` | `Response<Resp>` | `Call<Response<Resp>>` |
| `rpc M (stream Req) returns (Resp)` | `Request<Streaming<Req>>` | `Response<Resp>` | `(StreamSender<Req>, Call<Response<Resp>>)` |
| `rpc M (Req) returns (stream Resp)` | `Request<Req>` | `Response<Streaming<Resp>>` | `Call<Response<Streaming<Resp>>>` |
| `rpc M (stream Req) returns (stream Resp)` | `Request<Streaming<Req>>` | `Response<Streaming<Resp>>` | `(StreamSender<Req>, Call<Response<Streaming<Resp>>>)` |

There are only two streaming types, and they are the two halves of one channel:
`Streaming<T>` is what you read, `StreamSender<T>` is what you write. The same
pair is used on both sides of an RPC, so a client-streaming request and a
server-streaming response are the same shape seen from opposite ends.

### Reading a stream

```rust
async fn client_hello(
    &self,
    request: Request<Streaming<HelloRequest>>,
) -> Result<Response<HelloReply>, Status> {
    let mut stream = request.into_inner();
    let mut names = Vec::new();
    while let Some(req) = stream.message().await? {
        names.push(req.name().to_string());
    }
    let mut reply = HelloReply::new();
    reply.set_message(names.join(", "));
    Ok(Response::new(reply))
}
```

Received streams are decoded on the task that calls `message()`. There is no
pump task and no queue in between, which means backpressure is exact: stop
reading and you stop releasing HTTP/2 capacity, so the peer stalls at the
window rather than filling a buffer you own.

A handler is free to ignore its request stream entirely and answer straight
away; the RPC terminates normally.

### Writing a stream

```rust
async fn server_hello(
    &self,
    request: Request<HelloRequest>,
) -> Result<Response<Streaming<HelloReply>>, Status> {
    let name = request.get_ref().name().to_string();
    let (tx, stream) = Streaming::channel(16);
    tokio::spawn(async move {
        for i in 1..=3 {
            let mut reply = HelloReply::new();
            reply.set_message(format!("hello {name} #{i}"));
            // `Err` means the client went away.
            if tx.send(reply).await.is_err() {
                break;
            }
        }
    });
    Ok(Response::new(stream))
}
```

Dropping the sender half-closes the stream cleanly. To end it with an error
instead, use `tx.fail(status).await`, which puts the status in the trailers.

`Streaming::channel(n)` sets how many messages sit between your producer and
the wire. The wire layer takes whatever is ready in one go and writes it as a
single batch, so a deeper channel means fewer, larger writes.

### Client streaming

```rust
let (tx, call) = client.client_hello(Request::new(()));
for name in ["ada", "grace"] {
    let mut req = HelloRequest::new();
    req.set_name(name);
    tx.send(req).await?;
}
tx.close();                       // half-close; `drop(tx)` is equivalent
let reply = call.await?;
```

For bidirectional streaming, await the `Call` to get the response stream, then
interleave freely:

```rust
let (tx, call) = client.stream_hello(Request::new(()));
let mut inbound = call.await?.into_inner();
tx.send(request("ping")).await?;
let pong = inbound.message().await?;
```

## Metadata

`Metadata` is HTTP/2 headers or trailers minus the ones the protocol owns.
Keys ending in `-bin` carry arbitrary bytes and travel base64-encoded;
everything else carries ASCII. Mixing them up is an error rather than silent
corruption:

```rust
let mut req = Request::new(payload);
req.metadata_mut().insert("x-tenant", "acme")?;
req.metadata_mut().insert_bin("x-trace-bin", trace_id)?;

assert!(req.metadata_mut().insert("oops-bin", "not base64").is_err());
```

On the server, request metadata is on the `Request`:

```rust
let tenant = request.metadata().get("x-tenant").unwrap_or("default");
let peer = request.remote_addr();
```

Reading it costs nothing until you read it: `Metadata` wraps the received
header map rather than copying every entry into owned strings.

A `Response` has two independent sets. `metadata_mut()` is initial headers,
sent before the first message. `trailers_mut()` is trailing metadata, sent
with `grpc-status`:

```rust
let mut resp = Response::new(reply);
resp.metadata_mut().insert("x-cache", "miss")?;
resp.trailers_mut().insert("x-rows-scanned", "1742")?;
```

To attach metadata to an *error*, put it on the `Status`; error responses have
no separate trailers:

```rust
let mut status = Status::resource_exhausted("quota exceeded");
status.metadata_mut().insert("x-retry-after", "30")?;
return Err(status);
```

Rich errors travel as `grpc-status-details-bin`. Attach a serialized
`google.rpc.Status` (or any protobuf the peer understands); it is reserved
wire state, not user metadata:

```rust
let mut status = Status::failed_precondition("quota");
status.set_details(encoded_google_rpc_status);
return Err(status);
```

On the client, `status.details()` is the decoded bytes. The key is invisible
through `Metadata`, so forwarding received metadata cannot inject it.

Reserved keys (`grpc-status`, `grpc-status-details-bin`, `grpc-timeout`,
`content-type`, HTTP/2 pseudo-headers, ...) are invisible through `Metadata`
and are never written out, so forwarding received metadata cannot corrupt
the protocol framing.

## Errors and status codes

Return `Err(Status)`. All sixteen gRPC codes have constructors, and `Code`
knows its canonical name:

```rust
Err(Status::not_found(format!("row {id}")))
```

```rust
match client.say_hello(request).await {
    Ok(reply) => { /* ... */ }
    Err(status) if status.code() == Code::Unavailable => retry().await,
    Err(status) => eprintln!("{status}"),   // "NOT_FOUND: row 7"
}
```

`Status` is two machine words. Its message, metadata, and
`grpc-status-details-bin` live behind a pointer that is only allocated when
one of them is set, so `Result<T, Status>` stays cheap on paths where nothing
goes wrong.

Codes the kernel produces on your behalf:

| Code | When |
|---|---|
| `Unimplemented` | unknown service or method; unsupported `grpc-encoding` |
| `InvalidArgument` | `content-type` is not `application/grpc` |
| `ResourceExhausted` | a message exceeds an inbound or outbound cap |
| `DeadlineExceeded` | `grpc-timeout` elapsed |
| `Cancelled` | the peer reset the stream, or the caller cancelled |
| `Unavailable` | the connection could not be established or was lost |
| `Internal` | a malformed frame, or a protobuf parse failure |

## Deadlines and cancellation

A deadline set on a request travels as `grpc-timeout` and is enforced on both
ends: the server wraps the handler in a timeout, and the client stops waiting
and resets the stream.

```rust
let mut req = Request::new(payload);
req.set_timeout(Duration::from_millis(250));
match client.say_hello(req).await {
    Err(status) if status.code() == Code::DeadlineExceeded => { /* ... */ }
    other => { /* ... */ }
}
```

On the server, `request.timeout()` reports what the caller asked for, which is
what you want when deciding whether to start expensive work.

To cancel from elsewhere, take a handle before awaiting:

```rust
let call = client.say_hello(Request::new(payload));
let handle = call.handle();
tokio::spawn(async move {
    shutdown.notified().await;
    handle.cancel();
});
let result = call.await;   // Err(Cancelled) if the handle fired
```

Cancelling resets the HTTP/2 stream, so the server stops working on it rather
than finishing into a void.

## Wait-for-ready and lazy connect

A channel that is not yet connected fails an RPC immediately with
`UNAVAILABLE`. That is gRPC fail-fast, and it is the default. Set
`wait_for_ready` when the client is allowed to start before its server, or
when a restart should queue instead of bouncing.

```rust
let channel = Channel::connect_lazy(addr)?;
let client = GreeterClient::new(channel);

let mut req = Request::new(payload);
req.set_wait_for_ready(true);
req.set_timeout(Duration::from_secs(5));
let reply = client.say_hello(req).await?;
```

`connect_lazy` does not dial. Invalid authority still fails at construction.
A closed port, a name that does not resolve, or a refused TLS handshake
surfaces on the first RPC, which retries with backoff until the deadline or
a cancel if `wait_for_ready` is set. Without a deadline, a peer that never
comes up waits until cancellation.

The same flag applies after a live connection dies: fail-fast redials once
and returns `UNAVAILABLE` if nothing is listening; wait-for-ready keeps
trying.

## Serving several services

One service uses `Server`, which has no per-RPC dynamic dispatch:

```rust
Server::new(GreeterServer::new(MyGreeter)).serve(addr).await?;
```

Several use `Router`, which looks up the service half of the request path:

```rust
Router::new()
    .add_service(GreeterServer::new(MyGreeter))
    .add_service(EchoServer::new(MyEcho))
    .serve(addr)
    .await?;
```

Generated servers can start the chain themselves, which keeps their
configuration:

```rust
GreeterServer::new(MyGreeter)
    .config(ServerConfig::new().max_concurrent_streams(1024))
    .add_service(EchoServer::new(MyEcho))
    .serve(addr)
    .await?;
```

A request for a service that is not mounted, or a method the service does not
have, gets `UNIMPLEMENTED`. Routing costs one hash lookup plus one boxed
future per RPC; `Server` avoids both.

## TLS

h2c (cleartext prior-knowledge HTTP/2) is the default, because that is what a
loopback test and a mesh sidecar speak. Production that is not behind a
sidecar should serve and dial TLS.

The kernel uses rustls with the Graviola crypto provider. Graviola builds with
`rustc` only — no C compiler, no `aws-lc-rs`, no `ring`. Certificate
verification is not optional; there is no insecure constructor. ALPN is `h2`,
and a peer that does not negotiate it is dropped.

```rust
use pbrs_grpc::{Channel, ClientTls, Identity, ServerTls};

let identity = Identity::from_pem(cert_pem, key_pem)?;
GreeterServer::new(MyGreeter)
    .serve_tls(addr, ServerTls::new(identity)?)
    .await?;

// Dial 127.0.0.1, verify the certificate as localhost.
let tls = ClientTls::ca("localhost", ca_pem)?;
let channel = Channel::connect_tls("127.0.0.1:443", tls).await?;
```

`ClientTls::webpki("api.example.com")` trusts Mozilla's CA set. For private
PKI and tests, pin a CA with `ClientTls::ca`. Mutual TLS is
`ServerTls::mtls(identity, client_ca_pem)` plus `ClientTls::ca_mtls` (or
`webpki_mtls`) with a client `Identity`.

Graviola currently targets x86_64 and aarch64, and wants a CPU with AES-NI /
NEON. That is every machine this crate is likely to run a gRPC service on;
older or more exotic targets stay on h2c.

HTTP/2 PING keepalive is off by default. Turn it on when a NAT or load
balancer will drop idle connections:

```rust
ChannelConfig::new().keep_alive_interval(Duration::from_secs(30))
```

The same setter exists on `ServerConfig`. A PING that is not acknowledged
within 20 s (configurable) drops the connection. The next RPC redials that
slot; if the peer is still gone, the call fails with `UNAVAILABLE` (or
`DEADLINE_EXCEEDED` if the request deadline elapses while connecting) instead
of hanging on a dead socket.

A `Channel` also redials after a peer `GOAWAY` or a TCP reset, so restarting
the server on the same address does not require constructing a new client.
Healthy connections waiting only on `SETTINGS_MAX_CONCURRENT_STREAMS` are
left alone.

## Unix domain sockets

Loopback without TCP: a filesystem socket. The protocol is the same h2c as
`127.0.0.1`. TLS is TCP-only. `request.remote_addr()` is `None`; there is no
`std::net::SocketAddr` for a Unix peer.

```rust
GreeterServer::new(MyGreeter).serve_unix("/tmp/greeter.sock").await?;
let channel = Channel::connect_unix("/tmp/greeter.sock").await?;
```

`connect_unix_lazy` and `Request::set_wait_for_ready` work the same as on
TCP. The path is a filesystem path, not a `unix://` URI. Bind fails if the
path already exists; this crate does not unlink a stale socket.

`:authority` on Unix RPCs is `localhost`.

## Health checks

`grpc.health.v1.Health` ships in-tree as an ordinary service. Mount it next to
yours and drive it from a reporter:

```rust
let (health, reporter) = pbrs_grpc::health::service();
reporter.set_serving("helloworld.Greeter");

Router::new()
    .add_service(health)
    .add_service(GreeterServer::new(MyGreeter))
    .serve(addr)
    .await?;
```

`Check` on the empty name is the process; `Check` on a name you have not
set returns `NOT_FOUND`. `Watch` streams status changes, and an unknown name
yields `SERVICE_UNKNOWN` rather than an error, per the health protocol.

## Reflection

`grpc.reflection.v1.ServerReflection` ships in-tree so `grpcurl` and friends
can list and describe what you mounted. Register each generated
`FILE_DESCRIPTOR_SET` and add the service to the same `Router`:

```rust
let reflection = pbrs_grpc::reflection::Builder::new()
    .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)?;
Router::new()
    .add_service(reflection.build()?)
    .add_service(GreeterServer::new(MyGreeter))
    .serve(addr)
    .await?;
```

`list_services` reports every service in those sets. `file_containing_symbol`
and `file_by_filename` return the serialized `FileDescriptorProto` plus
whatever transitive imports were in the set. `file_containing_extension` and
`all_extension_numbers_of_type` answer from the same sets; a missing extension
is a `NOT_FOUND` on the stream, and extension-number listing is best-effort
(empty when the type has none). A missing symbol is a `NOT_FOUND` on the
stream (`ErrorResponse`), not a broken RPC.

## Graceful shutdown

`serve_with_shutdown` stops accepting, sends `GOAWAY` on every live
connection, and waits for in-flight RPCs to finish before returning:

```rust
let (tx, rx) = tokio::sync::oneshot::channel();
tokio::spawn(async move {
    tokio::signal::ctrl_c().await.ok();
    tx.send(()).ok();
});

GreeterServer::new(MyGreeter)
    .serve_with_shutdown(listener, async { rx.await.ok(); })
    .await?;
```

An RPC already running when the signal arrives completes and its response is
delivered. New connections are refused as soon as the signal fires. Process-wide
drain waits for those RPCs with no force-close; to cap how long a *peer* can
hold a socket, see [Connection age and idle](#connection-age-and-idle).

## Connection age and idle

A connection lives until the peer goes away unless you cap it. Age is measured
from accept. Idle is measured from the last RPC — keepalive PINGs do not count,
so a peer that only answers PINGs still looks idle.

```rust
ServerConfig::new()
    .max_connection_age(Duration::from_secs(30 * 60))
    .max_connection_idle(Duration::from_secs(5 * 60))
    .max_connection_age_grace(Duration::from_secs(10))
```

When either fires the kernel sends `GOAWAY`, waits the grace period (default
10 s) for in-flight RPCs, then drops the socket. Age is jittered by ±10% so a
process with many connections does not reconnect in lockstep. The next RPC on a
`Channel` redials that slot.

## Compression

gzip, via `grpc-encoding` and the per-message Compressed-Flag. It is per
message rather than per connection, so you can compress the payloads that
benefit and leave the rest alone:

```rust
let mut req = Request::new(big_payload);
req.set_compress(true);
```

```rust
let mut resp = Response::new(big_reply);
resp.set_compress(true);
```

On a stream, choose per message with `send` or `send_compressed`.
`request.compressed()` reports whether what arrived was compressed.

Compression is not free. At LAN latencies, identity framing usually wins:
gzipping a 300 KiB message costs more CPU time than the saved bytes cost in
transit. Measure before turning it on.

A peer asking for an encoding the kernel does not implement gets
`UNIMPLEMENTED` with `grpc-accept-encoding: identity,gzip` attached, so it
knows what to retry with.

## Limits and the threat model

The peer is assumed hostile. Every limit is enforced *before* the memory it
guards is committed.

| Attack | Defence | Default |
|---|---|---|
| Huge declared message length | Refused from the 5-byte frame header, before any payload is buffered | 4 MiB |
| Decompression bomb | Bounded inflate that stops one byte past the cap | 4 MiB |
| Metadata flood | HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE` | 16 KiB |
| Stream flood | HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS` | 256 |
| Unbounded buffering | Per-connection window and send buffer | 16 MiB / 1 MiB |
| Slow-reader amplification | Capacity is released only after a chunk is handed on | always |
| Deeply nested protobuf | Recursion limit in `pbrs` | always |
| Truncated or malformed frames | Protocol error, never treated as an empty message | always |
| Reserved metadata injection | `grpc-status`, `grpc-status-details-bin`, and friends are never read from or written to user metadata | always |
| Long-lived connection hold | `GOAWAY` after age or idle, then force-close; PINGs do not reset idle | opt-in |

The inbound cap is 4 MiB, matching gRPC's cross-language default. The outbound
cap is unlimited, because a peer does not control what your own service
produces.

```rust
GreeterServer::new(MyGreeter)
    .max_decoding_message_size(64 * 1024)
    .max_encoding_message_size(1024 * 1024)
```

Lifting the inbound cap entirely is possible and is only appropriate when
every peer is trusted:

```rust
ServerConfig::new().message_limits(MessageLimits::unlimited())
```

with the consequence that a single frame header can then ask for as much
memory as `u32::MAX` allows.

Two layers of tests enforce this.

`tests/hostile.rs` speaks raw HTTP/2 so it can send bytes no real client would
— a length prefix claiming 4 GiB, a 64 MiB gzip bomb small enough on the wire
to pass the frame check, reserved compressed-flag values, truncated frames,
malformed paths, garbage protobuf — and requires that every case answers with a
status and leaves the server serving.

Property tests in the wire module cover what fixed cases cannot, using a
deterministic xorshift generator so a failure reproduces from its seed:

- Frames survive arbitrary chunk boundaries. HTTP/2 can split a message
  anywhere, and the zero-copy fast path is the part that could get this wrong,
  so 2000 random framings are checked against arbitrary splits.
- Arbitrary bytes in arbitrary chunks yield frames or a `Status`, never a panic,
  and never a frame longer than the cap. 4000 cases.
- A compressed frame never inflates past the cap, and when it fits it
  round-trips exactly. Mixed compressible and incompressible payloads, so both
  the high-ratio and near-1:1 cases are exercised.

### Dependencies

`base64`, `bytes`, `flate2` (pinned to its `rust_backend`, i.e. `miniz_oxide`),
`h2`, `http`, `pbrs`, `tokio`, `rustls` (no default features), `rustls-graviola`,
`rustls-pemfile`, `tokio-rustls` (no default features), and `webpki-roots`.
Nothing in the graph pulls in `cc`, `bindgen`, `pkg-config`, `aws-lc-rs`,
`ring`, or a vendored zlib, so nothing compiles C or C++. The one FFI crate
is `libc`, which `tokio` uses for syscalls and which every Rust program links
through `std` regardless.

### `unsafe`

Every hand-written module in `pbrs-grpc` carries `#[forbid(unsafe_code)]`,
which cannot be relaxed from inside the module, so the claim is
machine-checked rather than a convention. The exceptions are the two modules
that `include!` generated message code: `pbrs` gencode uses `unsafe` for
zeroed-message construction. No gRPC framing, dispatch, or transport code
contains `unsafe`.

### Panics

`unwrap`, `expect`, `panic!`, indexing, and lossy numeric casts are denied at
the lint level for the whole workspace. Bad input becomes a `Status`, not an
abort.

## Tuning

Defaults are chosen for safety first, then throughput. Three knobs actually
matter.

**Connections.** One connection is one `h2` driver task, so concurrent small
RPCs serialize behind a single core's framing work. This is the largest
client-side lever:

```rust
Channel::connect_with(target, ChannelConfig::new().connections(4)).await?
```

**Window sizes.** The 16 MiB default keeps a 4 MiB message from stalling on a
`WINDOW_UPDATE` round trip, which is where large-payload throughput usually
goes. Lower it only under memory pressure:

```rust
ServerConfig::new().initial_stream_window_size(1024 * 1024)
```

**Stream queue depth.** How many messages sit between a producer and the wire.
The wire layer writes whatever is queued as one batch, so deeper means fewer and
larger writes, at the cost of memory. On the server this is the buffer the
handler chooses:

```rust
let (tx, stream) = Streaming::channel(64);
```

On the client it is configuration, because the client's outbound queue belongs
to the channel:

```rust
ChannelConfig::new().stream_buffer(64)
```

Received streams are decoded inline on the reading task, so they have no queue
to size in either direction.

Everything else — `max_frame_size`, `max_concurrent_streams`,
`max_send_buffer_size`, `max_header_list_size` — is available on both
`ServerConfig` and `ChannelConfig` and is more likely to be a safety decision
than a performance one.

## Interceptors and middleware

Auth, tracing, and tenant checks run before the handler. Closures implement
`Interceptor`, so most interceptors are one function:

```rust
fn require_token(rpc: &Rpc) -> Result<(), Status> {
    if rpc.metadata().get("authorization") != Some("Bearer secret") {
        return Err(Status::unauthenticated("bad or missing token"));
    }
    Ok(())
}

GreeterServer::new(MyGreeter)
    .intercept(require_token)
    .serve(addr)
    .await?;
```

`Router::intercept` runs before every mounted service. Calling it twice stacks:
the first interceptor runs first. Per-service wrapping is `Intercepted::new`
or `ServiceExt::intercept` when you do not want the generated server's
`.serve()` chain.

On the client, `Channel::intercept` (and the generated `FooClient::intercept`)
mutates outbound metadata before the stream opens:

```rust
let client = GreeterClient::new(channel).intercept(|md| {
    md.insert("authorization", "Bearer secret")?;
    Ok(())
});
```

A wrapping `Service` is still valid when the interceptor needs state the
closure form does not hold easily — `Rpc::reject` is the same turn-away path
either way, and `NAME` is inherited so the wrapper mounts where the inner
service would. There is no `tower` layer; use `protobuf-tonic` if you need
tonic's middleware stack.

For work that belongs to one method rather than the whole service, do it in the
handler; you have the metadata, the deadline, and the peer address there.

## Testing

Serve on an ephemeral port and connect a real client. The transport is fast
enough that in-process loopback is a reasonable default for service tests:

```rust
#[tokio::test]
async fn greets() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let server = tokio::spawn(async move {
        GreeterServer::new(MyGreeter).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(Channel::connect(addr).await.expect("connect"));
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let reply = client.say_hello(Request::new(req)).await.expect("rpc");
    assert_eq!(reply.get_ref().message(), "hello ada");

    server.abort();
}
```

`Request`, `Response`, `Status`, `Streaming`, and `Rpc` all implement `Debug`,
so `expect_err` and assertion failures print something useful.

For interop against other implementations, the crate ships the official
`grpc.testing.TestService` and its test cases. `scripts/grpc-interop.sh` runs
them three ways, and CI runs the script:

| Pass | Cases | Catches |
|---|---:|---|
| kernel client → kernel server | 18 | the two halves disagreeing with each other |
| kernel client → Go server | 14 | the client not speaking real gRPC |
| Go client → kernel server | 14 | the server not speaking real gRPC |

The four cases missing from the cross-language passes are the compression ones.
They are built on `SimpleRequest.expect_compressed` and `response_compressed`,
and grpc-go implements neither: its interop server reads neither field, and its
interop client rejects the case names. They run in the self-interop pass, which
is the only pass with an implementation on both ends that honours them.

To drive it by hand:

```bash
cargo run -p pbrs-grpc --bin pbrs-grpc-interop-server -- --port 10000
cargo run -p pbrs-grpc --bin pbrs-grpc-interop-client -- \
    --server_host 127.0.0.1 --server_port 10000 --test_case=large_unary
```

Either side can be replaced with `google.golang.org/grpc/interop/{client,server}`
run with `-use_tls=false`.

The interop client also has a `--bench` mode, which is what
`scripts/grpc-server-bench.sh` uses to compare the kernel's server against
grpc-go's with the client held constant. See [benchmarks](benchmarks.md).

Wiring these into CI immediately found a bug worth naming, because it is easy to
reproduce in any gRPC implementation: a deadline has to reach the *reads*, not
just the call setup. A server that answers with headers and then goes quiet
would otherwise hang the reader forever, and a peer that resets the stream at
the shared deadline would surface as `UNAVAILABLE` rather than
`DEADLINE_EXCEEDED`. Both are fixed, and `timeout_on_sleeping_server` now
passes against grpc-go.

## Writing a service without codegen

Codegen is a convenience, not a requirement. `Service` plus a `match` on the
method name is the whole contract, and every dispatch shape is public:

```rust
use pbrs_grpc::{Request, Response, Rpc, Service, Status};

struct Echo;

impl Service for Echo {
    const NAME: &'static str = "demo.Echo";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Ping" => {
                rpc.unary(|req: Request<HelloRequest>| async move {
                    let mut reply = HelloReply::new();
                    reply.set_message(req.get_ref().name());
                    Ok::<_, Status>(Response::new(reply))
                })
                .await;
            }
            "Subscribe" => {
                rpc.server_streaming(|_req: Request<HelloRequest>| async move {
                    let (tx, stream) = Streaming::channel(8);
                    tokio::spawn(async move { /* produce */ });
                    Ok::<_, Status>(Response::new(stream))
                })
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}
```

Consume the `Rpc` with exactly one of `unary`, `client_streaming`,
`server_streaming`, `bidi_streaming`, `reject`, or `unimplemented`. Each one
owns the whole response: headers, message frames, and `grpc-status` trailers.

On the client side, `Channel` takes a path directly, so no generated client is
required either:

```rust
let reply: HelloReply = channel
    .unary("/demo.Echo/Ping", Request::new(req))
    .await?
    .into_inner();
```

This is not a second-class path. The in-tree `TestService` implementation and
the hostile-peer tests both use it.

## What is not here

Deliberate omissions, with what to do instead.

| Missing | Instead |
|---|---|
| Load balancing and service discovery | `ChannelConfig::connections` pools to one authority. For more, resolve addresses yourself and hold a `Channel` per backend. |
| Retries and hedging | Retry at the call site; `Code::Unavailable` and `Code::DeadlineExceeded` are the retryable ones. |
| Reflection | `grpc.reflection.v1` ships in-tree. Register each generated `FILE_DESCRIPTOR_SET` and mount it. |
| `tower` integration | Use `protobuf-tonic`, which keeps tonic and only swaps in pbrs message types. |
| Encodings other than gzip | Not implemented. Unsupported requests are refused with `UNIMPLEMENTED` rather than mis-decoded. |

`pbrs` does not depend on this crate, and this crate does not depend on tonic
or `protobuf-tonic`. Use one, the other, or neither.
