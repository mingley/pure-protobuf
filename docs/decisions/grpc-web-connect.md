# gRPC-Web and Connect protocol scope (TC-09)

Decision: ship native gRPC-Web server support as an optional `pbrs-grpc`
feature, but start with HTTP/2-only support over the existing transport. Do
not include the Connect protocol in the tonic-parity track; treat it as a
separate product decision after tower/prost/tonic-shaped parity lands.

Decided 2026-09-28 from the tonic parity audit in
[the comparison guide](../guides/comparison.md#7-feature-parity-with-tonic-tc-08)
and the TC-01 product shape in [better-tonic.md](better-tonic.md).

Since this decision, the optional native `grpc-web` feature has landed.
HTTP/1.1 browser serving is covered by the tested
[Tower recipe](http1-grpc-web.md). The sketch below records the original
design; consult the [migration guide](../guides/migration.md) for current
application setup.

## gRPC-Web recommendation

`pbrs-grpc` should support gRPC-Web because it is part of the tonic ecosystem
through `tonic-web`, and real tonic migrations can have browser clients even
when the backend is otherwise native gRPC. The feature should be opt-in so the
default runtime remains a small prior-knowledge HTTP/2 kernel.

The first version should be **HTTP/2-only**:

- gRPC-Web is defined over HTTP semantics and can run over HTTP/2. The protocol
  uses `application/grpc-web` / `application/grpc-web-text`, encodes trailers
  in the response body, and does not use HTTP/2 stream IDs, GOAWAY, or PING as
  application-visible features.
- `pbrs-grpc` already accepts prior-knowledge HTTP/2 and TLS with ALPN `h2`.
  Browser deployments can use HTTPS with HTTP/2 ALPN or put Envoy/nginx/a
  gRPC-Web proxy in front of the server.
- Cleartext browser deployments and tonic-web's common h2c example use
  `accept_http1(true)`. `pbrs-grpc` intentionally has no HTTP/1.1 parser, so
  full tonic-web parity for h2c/browser HTTP/1.1 needs a separate HTTP/1.1
  accept layer. That should not be hidden inside the first gRPC-Web card.

The first implementation is still useful: it lets pure-Rust services serve
native gRPC and gRPC-Web over the same HTTP/2 transport, covers TLS browser
paths, and keeps the dependency graph small. It should document that
HTTP/1.1-only clients need an edge proxy until a future HTTP/1.1 card lands.

## gRPC-Web implementation sketch

Feature flag: `grpc-web` on `pbrs-grpc`, default off.

Suggested module layout:

| Module | Responsibility |
|---|---|
| `pbrs-grpc/src/web/mod.rs` | Public feature-gated entrypoint and re-exports. |
| `pbrs-grpc/src/web/service.rs` | Wrapper around `Service` / `Router` dispatch that recognizes gRPC-Web content types and CORS preflight. |
| `pbrs-grpc/src/web/frame.rs` | Decode request frames and encode response frames, including the gRPC-Web trailer frame with the high bit set. |
| `pbrs-grpc/src/web/base64.rs` | Streaming-safe base64 for `application/grpc-web-text`, including chunk boundaries that do not align with gRPC frames. |
| `pbrs-grpc/src/web/cors.rs` | Minimal, explicit CORS policy builder matching tonic-web's gRPC-Web-only preflight behavior. |

Public API shape:

```rust
let web = pbrs_grpc::web::GrpcWeb::new(GreeterServer::new(service))
    .allow_origin("https://app.example.com")?;

Server::new(web).serve_tls(addr, tls).await?;
```

Router-level use should also be possible:

```rust
Router::new()
    .add_service(pbrs_grpc::web::GrpcWeb::new(GreeterServer::new(service)))
    .serve_tls(addr, tls)
    .await?;
```

Interop peer for tests:

1. A tonic-web server/client pair exercises the same content-type and trailer
   behavior from an independent implementation.
2. A small browser-oriented peer, preferably the official `grpc-web` JavaScript
   client or `@connectrpc/connect-web` in gRPC-Web mode, verifies CORS,
   `grpc-web-text`, and unary/server-streaming behavior against the pbrs
   server.
3. Negative h2c/HTTP/1.1 tests must assert a clear unsupported-protocol error
   rather than silently falling back to native gRPC.

Expected size:

- **TC-11, S/M**: binary gRPC-Web over HTTP/2 for unary and server-streaming,
  response-body trailers, content-type negotiation, and tonic-web interop.
- **TC-12, M**: `grpc-web-text`, streaming-safe base64, CORS preflight, browser
  peer fixture.
- **TC-13, L**: optional HTTP/1.1 accept path if the maintainer wants full
  tonic-web h2c/browser parity without an edge proxy.

Client-streaming and bidirectional browser gRPC-Web should stay out of the
initial scope because the public gRPC-Web spec and tonic-web limit browser
clients to unary and server-streaming.

## Connect recommendation

Do not ship Connect protocol support as part of tonic parity.

Connect is not a tonic runtime feature, and it is broader than a gRPC-Web
translation layer. It defines unary and streaming protocols over general HTTP
semantics, supports both binary protobuf and JSON payloads, has HTTP GET for
side-effect-free unary RPCs, does not use HTTP trailers, and supports HTTP/1.1
for everything except bidirectional streaming. That would require new request
classification, content types, error mapping, JSON codec integration,
idempotency-aware GET handling, and likely an HTTP/1.1 server surface.

The right scope is a future, separate adapter after the tonic migration path is
stable. If demand is proven, prefer a feature-gated adapter module or companion
crate rather than folding Connect semantics into the native gRPC dispatch path.

Interop peer for any future Connect work should be the Connect conformance
suite and a `connect-go` or `connect-es` peer, not tonic. Estimated size is
**L/XL**: one design card plus multiple implementation cards for unary proto,
JSON, streaming, compression, error model, and HTTP/1.1/HTTP/2 coverage.
