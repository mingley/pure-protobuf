# greeter

A crate that depends on `pbrs-grpc` the way a user would: own proto, `build.rs`
with `compile_protos` (kernel stubs are the default), generated `Greeter`
trait / server / client, plus `grpc.health.v1` and `grpc.reflection.v1`.

The proto has all four gRPC shapes (`SayHello`, `ClientHello`, `ServerHello`,
`StreamHello`). `cargo run` still prints the unary path:

```bash
cargo run -p pbrs-grpc-example-greeter
# prints: hello world
```

`src/lib.rs` is the whole service. Tests cover every shape, health `Check`
and `Watch` (dropping the stream ends the subscription), and reflection
`list_services`.
`Status::from_error_details` is the typed bag after this example README greeter interceptor Err; those trailers reach the client without reading the body.
Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter interceptor Err is trailers without reading the body.
Distinct from an example README greeter client interceptor: that runs on the outbound call before the stream opens; this example README greeter interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this example README greeter handler Err; those trailers reach the client.
Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter handler Err is after the handler ran.
Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this example README greeter client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this example README greeter client interceptor Err; a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this example README greeter client interceptor already ran, so a local Err never consumes that budget.
Distinct from an example README greeter interceptor: that runs on the inbound RPC before the handler; this example README greeter client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this example README greeter StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter StreamSender fail is trailers after any messages already sent.
