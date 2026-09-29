# Serve gRPC and HTTP routes together

This example serves an axum `/ready` route, a native Greeter service, gRPC
health, and reflection on `127.0.0.1:50051`. The native router becomes a Tower
service and `tonic_web::GrpcWebLayer` adds gRPC-Web handling.

From the repository root:

```sh
cargo run -p pbrs-grpc-example-axum-cohost
```

In another terminal:

```sh
curl http://127.0.0.1:50051/ready
grpcurl -plaintext 127.0.0.1:50051 list
grpcurl -plaintext -d '{"name":"Ada"}' 127.0.0.1:50051 helloworld.Greeter/SayHello
```

The server runs until stopped. `/ready` returns a fixed `ok`; connect real
readiness logic before adapting this for deployment.

The [HTTP/1.1 gRPC-Web test](tests/grpc_web_http1.rs) checks the adapter with a
raw HTTP client:

```sh
cargo test -p pbrs-grpc-example-axum-cohost
```

See [the source](src/main.rs), [Tower integration](../../docs/grpc.md#message-codecs-and-tower-integration),
and [the HTTP/1.1 decision](../../docs/decisions/http1-grpc-web.md).
The native `pbrs-grpc` listener itself remains HTTP/2-only.
