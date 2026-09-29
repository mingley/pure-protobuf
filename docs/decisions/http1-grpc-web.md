# HTTP/1.1 gRPC-Web via Tower recipe (TC-13)

Decision: do **not** add a native HTTP/1.1 accept layer to `pbrs-grpc` now.
Document and test a recipe that serves `Router::into_tower_service()` under
`hyper-util`'s HTTP/1.1+HTTP/2 auto connection builder and wraps it in
`tonic_web::GrpcWebLayer`.

Decided 2026-09-29 after TC-04/TC-05 exposed the native router as a Tower
service and TC-09 scoped native gRPC-Web separately.

## Why recipe first

`pbrs-grpc` is a prior-knowledge HTTP/2 native gRPC kernel. Adding an HTTP/1.1
accept loop inside `pbrs-grpc/src/server` would introduce a second HTTP parser,
new connection-state semantics, and a larger default-adjacent maintenance
surface. The Tower adapter already gives users a lower-risk composition point:

```rust
let grpc = Router::new()
    .add_service(GreeterServer::new(service))
    .into_tower_service();
let grpc_web = tower::ServiceBuilder::new()
    .layer(tonic_web::GrpcWebLayer::new())
    .service(grpc);

hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
    .serve_connection(io, TowerToHyperService::new(grpc_web))
    .await?;
```

That recipe uses established Hyperium/Tonic/Tower crates in an example/test
workspace, not the `pbrs-grpc` core dependency graph. It also stays out of the
server worker's TC-11 path, which can continue building native HTTP/2
gRPC-Web support inside `pbrs-grpc` without fighting an HTTP/1.1 accept layer.

## Evidence

`examples/axum-cohost/tests/grpc_web_http1.rs` proves the recipe with a raw
HTTP/1.1 client request:

- request content type `application/grpc-web+proto`;
- `x-grpc-web: 1`;
- body is a unary gRPC-Web frame carrying `HelloRequest`;
- response body contains a normal message frame and a gRPC-Web trailer frame
  with the high bit set.

The server side is `Router::into_tower_service()` wrapped by
`tonic_web::GrpcWebLayer` and served by
`hyper_util::server::conn::auto::Builder`, so the same listener can be paired
with HTTP/1.1 REST routes and HTTP/2 native gRPC in user code.

## Dependency impact

No dependency is added to `pbrs-grpc` for this decision. The example crate uses
the established Hyperium/Tonic/Tower ecosystem:

- `tonic-web` from the Tonic project;
- `hyper-util`;
- `tower`;
- `http-body-util` and `bytes` for the test client.

The default `pbrs-grpc` graph remains unchanged.

## Follow-up cards

- TC-11 remains the native `pbrs-grpc` HTTP/2 gRPC-Web adapter.
- Add a future card only if users need `pbrs-grpc` itself to own an HTTP/1.1
  accept loop rather than composing through Hyper/Tower. That card should cover
  HTTP/1.1 parser choice, keepalive semantics, request-body limits, CORS,
  gRPC-Web text, and interaction with native HTTP/2 gRPC on one listener.
