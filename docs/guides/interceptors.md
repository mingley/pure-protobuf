# Interceptors, Metadata, and Request Context

This guide explains how to write client and server interceptors, manipulate HTTP/2 metadata, configure call-site overlays, and share request-scoped context across middleware in `pbrs-grpc`.

---

## 1. Interceptor Lifecycle Overview

`pbrs-grpc` provides three specialized interceptor points:

1. **`ClientInterceptor`**: Executes on the client before opening an outbound HTTP/2 stream. Inspects or mutates `Outgoing` headers, timeout, user-agent, or extensions.
2. **`Interceptor` (Server Inbound)**: Executes on the server after receiving request headers but before dispatching to the service handler. Can validate credentials, check rate limits, or reject requests early with `Status`.
3. **`ResponseInterceptor` (Server Outbound)**: Executes after the handler finishes, allowing inspection or modification of outbound trailers, metrics, and logging.

---

<a id="metadata"></a>
## 2. Metadata Handling (ASCII vs Binary)

gRPC metadata is exchanged as HTTP/2 headers and trailers:

- **ASCII Metadata**: Standard strings with ASCII characters (e.g. `authorization`, `x-request-id`).
- **Binary Metadata**: Keys ending in `-bin` (e.g. `auth-token-bin`). Values are automatically base64-encoded on the wire and decoded transparently into bytes.
- **Reserved Headers**: The pseudo-headers (`:method`, `:scheme`, `:path`, `:authority`) and core transport headers (`content-type`, `te`, `grpc-timeout`, `grpc-encoding`) are managed by the kernel and cannot be manually inserted as metadata.

### Reading and Writing Metadata
```rust
// Client: attaching headers
let mut req = Request::new(payload);
req.metadata_mut().insert("x-request-id", "req-12345");
req.metadata_mut().insert_bin("auth-token-bin", &[0x01, 0x02, 0x03]);

// Server: reading headers
let req_id = request.metadata().get("x-request-id");
let token_bytes = request.metadata().get_bin("auth-token-bin");
```

---

## 3. Client Interceptors and Outgoing Overlays

Attach interceptors to a `Channel` using `.intercept()`:

```rust
use pbrs_grpc::{Channel, ClientInterceptor, Outgoing, Status};

let channel = Channel::connect("127.0.0.1:50051").await?
    .intercept(|outgoing: &mut Outgoing| {
        // 1. Add authentication header
        outgoing.metadata_mut().insert("authorization", "Bearer secret-token");

        // 2. Adjust per-call configuration overlays
        if !outgoing.user_agent_is_set() {
            outgoing.set_user_agent("my-custom-client/1.0");
        }

        // Return Ok(()) to proceed, or Err(Status) to abort the call
        Ok(())
    });
```

### Supported Outgoing Overlays
- `outgoing.set_user_agent(prefix)`: Prepends a custom prefix to the kernel `user-agent`.
- `outgoing.set_timeout(duration)`: Overrides the deadline duration.
- `outgoing.set_wait_for_ready(true)`: Enables or disables wait-for-ready for this call.
- `outgoing.set_compress(true)`: Enables outbound message gzip compression.

---

## 4. Server Inbound Interceptors (Authentication Example)

Server inbound interceptors intercept requests before handlers execute:

```rust
use pbrs_grpc::{Interceptor, Rpc, Status};

let auth_interceptor = |rpc: &mut Rpc| -> Result<(), Status> {
    let auth = rpc.metadata().get("authorization");
    match auth {
        Some("Bearer valid-token") => Ok(()),
        _ => Err(Status::unauthenticated("missing or invalid authorization header")),
    }
};

Server::new(MyService)
    .intercept(auth_interceptor)
    .serve("0.0.0.0:50051")
    .await?;
```

---

## 5. Request and Response Extensions

Extensions allow middleware to pass arbitrary Rust types down the call chain without encoding them into HTTP/2 wire headers:

```rust
#[derive(Clone, Debug)]
struct AuthenticatedUser {
    user_id: u64,
    role: String,
}

// In an inbound interceptor:
rpc.extensions_mut().insert(AuthenticatedUser {
    user_id: 42,
    role: "admin".to_string(),
});

// Inside your service handler:
impl Greeter for MyGreeter {
    async fn say_hello(&self, req: Request<HelloRequest>) -> Result<Response<HelloReply>, Status> {
        let user = req.extensions().get::<AuthenticatedUser>()
            .ok_or_else(|| Status::unauthenticated("no user context"))?;
        
        println!("User {} (role: {}) executed RPC", user.user_id, user.role);
        // ...
        Ok(Response::new(reply))
    }
}
```
