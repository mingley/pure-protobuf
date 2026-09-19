# Implementing the Four gRPC Call Shapes

This guide provides concrete, task-oriented walkthroughs for all four gRPC communication patterns supported by `pbrs-grpc`:

1. **Unary RPC**: Single request, single response.
2. **Server-Streaming RPC**: Single request, stream of responses.
3. **Client-Streaming RPC**: Stream of requests, single response.
4. **Bidirectional (Bidi) Streaming RPC**: Concurrent streams of requests and responses.

All patterns match the runnable reference implementation in `examples/greeter/`.

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
req.set_name("Ada");

let reply = client.say_hello(Request::new(req)).await?;
println!("Response: {}", reply.get_ref().message().to_str().unwrap_or_default());
```

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
req.set_name("Edsger");

let mut stream = client.server_hello(Request::new(req)).await?.into_inner();

// Read to EOF: loop terminates when `stream.message().await?` returns `None`.
while let Some(reply) = stream.message().await? {
    println!("Chunk: {}", reply.message().to_str().unwrap_or_default());
}
```

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

for name in ["Grace", "Alan"] {
    let mut req = HelloRequest::new();
    req.set_name(name);
    // Sends apply backpressure when the outbound queue is full
    tx.send(req).await.map_err(|e| {
        Status::unavailable(format!("send error: {e}"))
    })?;
}

// Half-close: informs the server no more requests will be sent
tx.close();

// Await the consolidated response
let response = call.await?;
println!("Summary: {}", response.get_ref().message().to_str().unwrap_or_default());
```

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
    for name in ["Barbara", "Donald"] {
        let mut req = HelloRequest::new();
        req.set_name(name);
        if tx.send(req).await.is_err() {
            break;
        }
    }
    tx.close(); // Half-close client sender
});

// Concurrently consume replies until server closes
while let Some(reply) = inbound.message().await? {
    println!("Bidi reply: {}", reply.message().to_str().unwrap_or_default());
}
```

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

