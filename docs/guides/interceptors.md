# Interceptors, Metadata, and Request Context

Client interceptors prepare outbound calls; server interceptors validate
incoming requests. Use metadata for values sent to the peer and extensions
for Rust values kept within the process. The examples below assume generated
client/server stubs and a service to wrap.

---

## 1. Interceptor Lifecycle Overview

Choose the interceptor point based on when you need to act:

| Point | Runs when | Common uses |
|---|---|---|
| **`ClientInterceptor`** | Before the client opens an outbound HTTP/2 stream | Add headers, set timeout, set user-agent, add extensions |
| **`Interceptor` (Server Inbound)** | After request headers arrive, before handler dispatch | Validate credentials, check rate limits, reject early with `Status` |
| **`ResponseInterceptor` (Server Outbound)** | After the handler finishes | Inspect or modify outbound trailers, record metrics, log outcomes |

---

<a id="metadata"></a>
## 2. Metadata Handling (ASCII vs Binary)

gRPC metadata travels as HTTP/2 headers and trailers.

- **ASCII metadata** uses standard ASCII strings, such as `authorization` or
  `x-request-id`.
- **Binary metadata** uses keys ending in `-bin`, such as `auth-token-bin`.
  Values are base64-encoded on the wire and decoded back into bytes.
- **Reserved headers** are owned by the kernel. Do not insert pseudo-headers
  (`:method`, `:scheme`, `:path`, `:authority`) or core transport headers
  (`content-type`, `te`, `grpc-timeout`, `grpc-encoding`) as metadata.

### Reading and Writing Metadata

Use the ASCII and binary helpers instead of encoding binary values yourself.

```rust
// Client: attaching headers
let mut req = Request::new(payload);
req.metadata_mut().insert("x-request-id", "req-12345")?;
req.metadata_mut().insert_bin("auth-token-bin", &[0x01, 0x02, 0x03])?;

// Server: reading headers
let req_id = request.metadata().get("x-request-id");
let token_bytes = request.metadata().get_bin("auth-token-bin");
```

---

## 3. Client Interceptors and Outgoing Overlays

Attach a client interceptor to a `Channel` with `.intercept()`. Return
`Ok(())` to continue the call, or return `Err(Status)` to abort before the
stream opens.

```rust
use pbrs_grpc::{Channel, ClientInterceptor, Outgoing, Status};

let channel = Channel::connect("127.0.0.1:50051").await?
    .intercept(|outgoing: &mut Outgoing| {
        // 1. Add authentication header
        outgoing.metadata_mut().insert("authorization", "Bearer secret-token")?;

        // 2. Adjust per-call configuration overlays
        if !outgoing.user_agent_is_set() {
            outgoing.set_user_agent("my-custom-client/1.0")?;
        }

        // Return Ok(()) to proceed, or Err(Status) to abort the call
        Ok(())
    });
```

### Supported Outgoing Overlays

| Overlay | Effect |
|---|---|
| `outgoing.set_user_agent(prefix)` | Prepends a custom prefix to the kernel `user-agent` |
| `outgoing.set_timeout(duration)` | Overrides the deadline duration |
| `outgoing.set_wait_for_ready(true)` | Enables or disables wait-for-ready for this call |
| `outgoing.set_compress(true)` | Enables outbound message gzip compression |

---

## 4. Server Inbound Interceptors (Authentication Example)

Use a server inbound interceptor when a request must be rejected before the
handler runs. This example accepts only one authorization value.

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
    .serve("0.0.0.0:50051".parse()?)
    .await?;
```

---

## 5. Request and Response Extensions

Use extensions for request-local Rust values that should not be serialized into
HTTP/2 metadata. Insert them in middleware, then read them in the handler.

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
