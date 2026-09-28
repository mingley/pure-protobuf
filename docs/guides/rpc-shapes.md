# Implementing the Four gRPC Call Shapes

This guide provides concrete, task-oriented walkthroughs for all four gRPC communication patterns supported by `pbrs-grpc`:

1. **Unary RPC**: Single request, single response.
2. **Server-Streaming RPC**: Single request, stream of responses.
3. **Client-Streaming RPC**: Stream of requests, single response.
4. **Bidirectional (Bidi) Streaming RPC**: Concurrent streams of requests and responses.

All patterns match the runnable reference implementation in `examples/greeter/`.
Each recipe below ends with the exact command that runs it. Every recipe
asserts reply content, the final status, and clean shutdown instead of
printing optimistic success.

Run every recipe and the full binary path with:

```bash
cargo test -p pbrs-grpc-example-greeter
cargo run -p pbrs-grpc-example-greeter
```

The binary prints `hello world` only after `run()` asserts all four
shapes and drains the server; any content, status, or shutdown failure
exits nonzero instead.

---

## 1. Proto Definition

Consider the standard service definition covering all four shapes (`proto/hello.proto`):

```protobuf
syntax = "proto3";
package helloworld;

message HelloRequest {
  string name = 1;
}

message HelloReply {
  string message = 1;
}

service Greeter {
  // 1. Unary
  rpc SayHello (HelloRequest) returns (HelloReply);

  // 2. Client-Streaming (Upload)
  rpc ClientHello (stream HelloRequest) returns (HelloReply);

  // 3. Server-Streaming (Download)
  rpc ServerHello (HelloRequest) returns (stream HelloReply);

  // 4. Bidirectional Streaming
  rpc StreamHello (stream HelloRequest) returns (stream HelloReply);
}
```

Compile this in `build.rs` using native stub generation:

```rust
// build.rs
fn main() {
    pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"]).expect("compile_protos");
}
```

Include the generated stubs in your application module:

```rust
mod proto {
    include!(concat!(env!("OUT_DIR"), "/hello.rs"));
}
use proto::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
```

---

## 2. Unary RPC

Unary calls follow a simple request-reply pattern.

### Server Implementation
```rust
use pbrs_grpc::{Request, Response, Status};

impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request.get_ref().name().to_str().map_err(|_| {
            Status::invalid_argument("name must be valid UTF-8")
        })?;
        if name.is_empty() {
            return Err(Status::invalid_argument("name must not be empty"));
        }

        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {name}"));
        Ok(Response::new(reply))
    }
}
```

### Client Call
```rust
let client = GreeterClient::connect(addr).await?;
let mut req = HelloRequest::new();
req.set_name("ada");

// The awaited call resolves only on the final OK status; assert content.
let reply = client.say_hello(Request::new(req)).await?;
assert_eq!(
    reply.get_ref().message().to_str().unwrap_or_default(),
    "hello ada"
);

// Failures surface as typed status, never as silent success.
let mut bad = HelloRequest::new();
bad.set_name("");
let err = client.say_hello(Request::new(bad)).await.unwrap_err();
assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);
```

### Run this recipe
```bash
cargo test -p pbrs-grpc-example-greeter --lib recipe_unary_asserts_content_status_shutdown
```

Expect `test result: ok. 1 passed; 0 failed`. The test asserts the
`hello ada` content, the `InvalidArgument` final status on bad input,
and clean `shutdown()` drain.

---

<a id="reading-a-stream"></a>
## 3. Server-Streaming RPC

In server-streaming, the client sends a single request and reads a sequence of messages until the server closes the stream.

### Server Implementation
```rust
use pbrs_grpc::{Request, Response, Status, Streaming};

impl Greeter for MyGreeter {
    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = request.get_ref().name().to_str().map_err(|_| {
            Status::invalid_argument("name must be valid UTF-8")
        })?.to_owned();
        if name.is_empty() {
            return Err(Status::invalid_argument("name must not be empty"));
        }

        // Bounded channel with backpressure (queue depth 4)
        let (tx, stream) = Streaming::channel(4);

        tokio::spawn(async move {
            for i in 1..=3 {
                let mut reply = HelloReply::new();
                reply.set_message(format!("hello {name} #{i}"));
                if tx.send(reply).await.is_err() {
                    break; // Client dropped stream or cancelled
                }
            }
            // `tx` drops here, signaling EOF (clean half-close) to the client
        });

        Ok(Response::new(stream))
    }
}
```

### Client Call
```rust
let mut req = HelloRequest::new();
req.set_name("edsger");

let mut stream = client
    .server_hello(Request::new(req))
    .await?
    .into_inner();

// Read to EOF: the loop ends only on `Ok(None)`, which is the OK final
// status; a failed RPC surfaces as `Err(status)` from `message()`.
let mut chunks = Vec::new();
while let Some(reply) = stream.message().await? {
    chunks.push(reply.message().to_str().unwrap_or_default().to_owned());
}
assert_eq!(
    chunks,
    ["hello edsger #1", "hello edsger #2", "hello edsger #3"]
);
```

### Run this recipe
```bash
cargo test -p pbrs-grpc-example-greeter --lib recipe_server_streaming_asserts_content_status_shutdown
```

Expect `test result: ok. 1 passed; 0 failed`. The test asserts the
three-chunk content, the `InvalidArgument` final status on bad input,
and clean `shutdown()` drain.

---

<a id="writing-a-stream"></a>
<a id="client-streaming"></a>
## 4. Client-Streaming RPC

In client-streaming, the client sends multiple requests into an outbound bounded stream, half-closes the stream, and receives a single consolidated reply from the server.

### Server Implementation
```rust
use pbrs_grpc::{Request, Response, Status, Streaming};

impl Greeter for MyGreeter {
    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut stream = request.into_inner();
        let mut names = Vec::new();

        while let Some(req) = stream.message().await? {
            let name = req.name().to_str().map_err(|_| {
                Status::invalid_argument("streamed name must be valid UTF-8")
            })?;
            if !name.is_empty() {
                names.push(name.to_owned());
            }
        }

        if names.is_empty() {
            return Err(Status::invalid_argument("stream must contain at least one name"));
        }

        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {}", names.join(", ")));
        Ok(Response::new(reply))
    }
}
```

### Client Call
```rust
let (tx, call) = client.client_hello(Request::new(()));

for name in ["grace", "alan"] {
    let mut req = HelloRequest::new();
    req.set_name(name);
    // Sends apply backpressure when the outbound queue is full
    tx.send(req).await.map_err(|e| {
        Status::unavailable(format!("send error: {e}"))
    })?;
}

// Half-close: informs the server no more requests will be sent
tx.close();

// The awaited call resolves only on the final status; assert content.
let response = call.await?;
assert_eq!(
    response.get_ref().message().to_str().unwrap_or_default(),
    "hello grace, alan"
);
```

### Run this recipe
```bash
cargo test -p pbrs-grpc-example-greeter --lib recipe_client_streaming_asserts_content_status_shutdown
```

Expect `test result: ok. 1 passed; 0 failed`. The test asserts the
consolidated content, the `InvalidArgument` final status on an empty
upload, and clean `shutdown()` drain.

---

## 5. Bidirectional Streaming RPC

In bidirectional streaming, client and server independently send and receive messages over a single full-duplex HTTP/2 stream.

### Server Implementation
```rust
use pbrs_grpc::{Request, Response, Status, Streaming};

impl Greeter for MyGreeter {
    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(4);

        tokio::spawn(async move {
            while let Ok(Some(req)) = inbound.message().await {
                let name = req.name().to_str().unwrap_or_default();
                let mut reply = HelloReply::new();
                reply.set_message(format!("hello {name}"));
                if tx.send(reply).await.is_err() {
                    break; // Outbound client receiver disconnected
                }
            }
        });

        Ok(Response::new(stream))
    }
}
```

### Client Call
```rust
let (tx, call) = client.stream_hello(Request::new(()));
let mut inbound = call.await?.into_inner();

// Send requests and half-close when done
tokio::spawn(async move {
    for name in ["barbara", "donald"] {
        let mut req = HelloRequest::new();
        req.set_name(name);
        if tx.send(req).await.is_err() {
            break;
        }
    }
    tx.close(); // Half-close client sender
});

// Concurrently consume replies until the server closes (OK final status).
let mut replies = Vec::new();
while let Some(reply) = inbound.message().await? {
    replies.push(reply.message().to_str().unwrap_or_default().to_owned());
}
assert_eq!(replies, ["hello barbara", "hello donald"]);
```

### Run this recipe
```bash
cargo test -p pbrs-grpc-example-greeter --lib recipe_bidi_asserts_content_status_shutdown
```

Expect `test result: ok. 1 passed; 0 failed`. The test asserts the
full-duplex content, drains to EOF (a non-OK trailer would surface as
`Err` instead of `None`), and ends with clean `shutdown()` drain.

---

## 6. Stream Lifecycle, Error Handling, and Clean Shutdown

### Streaming Boundaries
- **Client-Streaming Half-Close**: Calling `tx.close()` (or dropping `StreamSender`) sends an HTTP/2 `END_STREAM` flag on the client half. The server stream reading loop yields `None`, signaling end-of-input, allowing the server to generate and return its final `Response`.
- **Server-Streaming EOF**: The client consumes messages with `while let Some(msg) = stream.message().await?`. When the server drops its `StreamSender` or calls `tx.close()`, `stream.message().await?` returns `Ok(None)` (EOF).
- **Bidirectional Coordination**: Full-duplex streams coordinate independently on the same HTTP/2 stream. The client can half-close its write half (`tx.close()`) while continuing to read responses until the server completes and half-closes its write half.

### Explicit Error Handling & Cancellation
- **Explicit Status Errors**: Handlers return typed `Result<Response<T>, Status>`. Client calls propagate errors using `?` or inspect error codes with `status.code()`.
- **Aborting a Stream**: A sender can call `tx.fail(status)` to terminate the stream with an explicit error instead of a clean half-close.
- **Client Cancellation**: Dropping a `Streaming` receiver on client or server immediately transmits an HTTP/2 `RST_STREAM` frame (cancel), waking any task awaiting `Request::cancelled()` or `StreamSender::closed()`.

### Clean Server Shutdown
- Servers gracefully drain active connections using `serve_with_shutdown(listener, signal)`. In-flight RPCs finish, active streams drain, and new connections are refused:
```rust
use tokio::net::TcpListener;
use pbrs_grpc::Router;

let listener = TcpListener::bind("127.0.0.1:0").await?;
let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

tokio::spawn(async move {
    Router::new()
        .add_service(GreeterServer::new(MyGreeter))
        .serve_with_shutdown(listener, async {
            let _ = shutdown_rx.await;
        })
        .await
        .ok();
});

// Trigger clean drain and wait for shutdown
let _ = shutdown_tx.send(());
```

---

## 7. Fresh-directory setup (published crates)

The same four shapes run outside this repo against the published crates
(no workspace `path =` dependencies). `protoc` must be on `PATH`;
codegen shells out to it:

```toml
[dependencies]
pbrs = "0.1"
pbrs-grpc = "0.1.0-alpha.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[build-dependencies]
pbrs = "0.1"
```

```rust
// build.rs: explicit native stub selection (the default is also native).
fn main() {
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(&["proto/hello.proto"], &["proto"])
        .expect("compile_protos");
}
```

```bash
cargo run
```

The separate tonic entry point selects `emit_tonic_stubs(true)` with the
`protobuf-tonic` adapter instead; see the
[codegen guide](codegen.md). Both fresh-directory flows are proven
hermetically by `tests/onboarding.rs` (native unary plus all-four-shapes
consumers, and the tonic consumer), which assert reply content and exit
nonzero on any failure:

```bash
cargo test -p pbrs --test onboarding
```

