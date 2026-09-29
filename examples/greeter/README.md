# Greeter example

Run a complete native gRPC service with generated messages, a client, health
checks, and reflection. The example uses a local `.proto` file and `build.rs`;
install `protoc` before building.

From the repository root:

```sh
cargo run -p pbrs-grpc-example-greeter
cargo test -p pbrs-grpc-example-greeter
```

The binary prints `hello world` after checking all four RPC shapes and draining
the server. Tests also cover health `Check`/`Watch`, reflection, metadata,
interceptors, cancellation, and error details.

## Find the code

| File | Purpose |
|---|---|
| [proto/hello.proto](proto/hello.proto) | Service and messages for unary, client-streaming, server-streaming, and bidirectional calls. |
| [build.rs](build.rs) | Generate native messages and stubs. |
| [src/lib.rs](src/lib.rs) | Service implementation, client helpers, and tests. |
| [src/production.rs](src/production.rs) | Loopback TLS/mTLS, resource limits, readiness, and bounded shutdown. |
| [src/telemetry.rs](src/telemetry.rs) | Observer and tracing examples. |

The [RPC-shape guide](../../docs/guides/rpc-shapes.md) walks through each method.
The [TLS guide](../../docs/guides/production-service.md) explains how to run the
certificate fixtures and choose deployment settings.

## Errors and interceptors

Use `Status::from_error_details` to build a status with structured details.
Where the error occurs determines what the client observes:

| Error source | When it runs | What happens |
|---|---|---|
| Client interceptor | Before opening an HTTP/2 stream | Rejects locally. No stream opens and no RPC admission slot is consumed. |
| Server interceptor | After headers, before the handler | Sends error trailers without reading the request body. |
| Handler | After request dispatch | Returns a status to the client in response trailers. |
| Server response interceptor | After a successful handler result | Can replace that result with a trailers-only error. |
| Response stream producer | After zero or more response messages | `StreamSender::fail` ends the stream with error trailers after queued messages. |
| Client response interceptor | After a successful receive | Can turn the received result into a failed `Call` locally. |

A peer reset can prevent queued messages or trailers from arriving.
`Outgoing::connected` reports the live socket when the client interceptor runs.
A lazy first call can report `false` even with wait-for-ready enabled: waiting
for readiness happens later when the `Call` is polled.

See [interceptors and context](../../docs/guides/interceptors.md) for metadata,
extensions, and per-call settings.
