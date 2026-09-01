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
`Status::from_error_details` is the typed bag after this example README greeter handler Err; those trailers reach the client.
`Status::from_error_details` is the typed bag after this example README greeter client interceptor Err; a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this example README greeter client interceptor already ran, so a local Err never consumes that budget.
`Status::from_error_details` is the typed bag after this example README greeter StreamSender fail on a server response producer; those trailers ship after any messages already sent.
