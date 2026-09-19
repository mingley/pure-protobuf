# Production Service Configuration and Lifecycle

This guide covers operational configurations for running `pbrs-grpc` services in production environments: transport security, deadlines, graceful lifecycle management, and connection topologies.

---

<a id="tls"></a>
## 1. Transport Security (TLS and mTLS)

`pbrs-grpc` uses a pure-Rust TLS implementation via `rustls` and `Graviola`. No external C compiler or OpenSSL installation is required.

### Server TLS Configuration
```rust
use pbrs_grpc::{Certificate, Identity, Server, ServerTls};

// Load certificate chain and private key (PEM format)
let cert_pem = std::fs::read("certs/server.crt")?;
let key_pem = std::fs::read("certs/server.key")?;
let identity = Identity::from_pem(cert_pem, key_pem)?;

// Server TLS with mandatory ALPN h2
let tls = ServerTls::new(identity);
Server::new(MyService)
    .serve_tls("0.0.0.0:50051", tls)
    .await?;
```

### Mutual TLS (mTLS)
To enforce client certificate authentication, provide a client CA root:

```rust
let client_ca_pem = std::fs::read("certs/client_ca.crt")?;
let client_ca = Certificate::from_pem(client_ca_pem)?;

let mtls = ServerTls::mtls(identity, client_ca);
Server::new(MyService)
    .serve_tls("0.0.0.0:50051", mtls)
    .await?;
```

In your handler, inspect verified client identity via `Rpc::peer_identity` or `Request::peer_identity`:
```rust
let client_certs = request.peer_identity();
```

### Client TLS Dialing
```rust
use pbrs_grpc::{Certificate, Channel, ClientTls};

// Standard WebPKI CA roots
let client = GreeterClient::connect_tls("example.com:443", ClientTls::webpki()).await?;

// Pinned custom root CA
let ca_pem = std::fs::read("certs/ca.crt")?;
let ca = Certificate::from_pem(ca_pem)?;
let client = GreeterClient::connect_tls("internal.service:50051", ClientTls::ca(ca)).await?;
```

---

<a id="deadlines"></a>
<a id="timeouts"></a>
<a id="wait-for-ready"></a>
## 2. Timeouts, Deadlines, and Cancellation

### Setting Timeouts
Timeouts can be set globally on channels or per-RPC:

```rust
use std::time::Duration;

// 1. Channel-level default timeout overlay
let channel = Channel::connect("127.0.0.1:50051").await?
    .timeout(Duration::from_secs(3));

// 2. Call-site per-request timeout
let mut req = Request::new(payload);
req.set_timeout(Duration::from_millis(500));
```

The timeout duration is serialized into the `grpc-timeout` header. The server computes the deadline `Instant` upon dispatch.

### Server Cancellation Detection
Long-running server tasks can check for client cancellations:

```rust
tokio::select! {
    res = do_expensive_work() => {
        Ok(Response::new(res))
    }
    _ = request.cancelled() => {
        Err(Status::cancelled("client aborted request"))
    }
}
```

### Wait-for-Ready and Connect Timeouts
- **`ChannelConfig::connect_timeout`**: Bounds the initial TCP dial and HTTP/2 preface handshake.
- **`Channel::wait_for_ready`**: When enabled, RPCs made while the channel is connecting or reconnecting queue instead of failing immediately with `UNAVAILABLE`.

---

<a id="graceful-shutdown"></a>
<a id="connection-age"></a>
## 3. Connection Lifecycle and Graceful Drain

### Graceful Server Shutdown
Use `serve_with_shutdown` to drain active requests cleanly:

```rust
let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

// Trigger shutdown on SIGTERM / Ctrl+C
tokio::spawn(async move {
    tokio::signal::ctrl_c().await.ok();
    let _ = shutdown_tx.send(());
});

Server::new(MyService)
    .serve_with_shutdown("0.0.0.0:50051", async {
        shutdown_rx.await.ok();
    })
    .await?;
```
During shutdown:
1. The server stops accepting new connections.
2. An HTTP/2 `GOAWAY` frame is sent on all active connections with the highest processed stream ID.
3. In-flight requests are permitted to complete before sockets are closed.

### Max Connection Age and Idle Limits
To balance traffic across backend pods behind L4 balancers:
- `ServerConfig::max_connection_age`: Sends a graceful `GOAWAY` after a connection reaches maximum age (automatically jittered ±10% to prevent thundering herd).
- `ServerConfig::max_connection_idle`: Closes connections with no active streams after the idle duration elapses.

---

<a id="router"></a>
## 4. Multi-Service Routing

Mount multiple gRPC services on a single listener using `Router`:

```rust
use pbrs_grpc::Router;

let router = Router::new()
    .add_service(GreeterServer::new(MyGreeter))
    .add_service(EchoServer::new(MyEcho))
    .add_service(HealthServer::new(health_reporter));

router.serve("0.0.0.0:50051").await?;
```

Unmatched service requests are answered with `Code::Unimplemented`.

---

<a id="unix-sockets"></a>
<a id="in-process"></a>
## 5. Local IPC (Unix Domain Sockets & In-Process Pipes)

### Unix Domain Sockets (UDS)
UDS provides low-overhead IPC on Linux and macOS:

```rust
// Server: serve_unix_unlink cleans stale socket files on restart
Server::new(MyService)
    .serve_unix_unlink("/tmp/grpc-service.sock")
    .await?;

// Client
let client = GreeterClient::connect_unix("/tmp/grpc-service.sock").await?;
```

On Linux, inspect caller PID/UID via `Rpc::peer_cred` (backed by `SO_PEERCRED`).

### In-Process Duplex Pipes (`from_io`)
For integration testing and in-memory communication without network overhead:

```rust
let (client_io, server_io) = tokio::io::duplex(64 * 1024);

tokio::spawn(async move {
    Server::new(MyService)
        .serve_connection(server_io)
        .await
        .unwrap();
});

let channel = Channel::from_io(client_io);
let client = GreeterClient::new(channel);
```

---

<a id="compression"></a>
## 6. Compression Negotiation

`pbrs-grpc` supports message-level gzip compression:
- **Server**: Inbound gzip is accepted by default. Use `Server::send_compressed(true)` to enable outbound compression on responses.
- **Client**: Call `Channel::send_compressed(true)` or `Request::set_compress(true)` to compress outbound requests.
- **Negotiation**: Gzip is only transmitted if the peer advertises support in `grpc-accept-encoding`.
