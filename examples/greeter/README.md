# greeter

A crate that depends on `pbrs-grpc` the way a user would: own proto, `build.rs`
with `emit_kernel_stubs(true)`, generated `Greeter` trait / server / client,
plus `grpc.health.v1` and `grpc.reflection.v1`.

```bash
cargo run -p pbrs-grpc-example-greeter
# prints: hello world
```

`src/lib.rs` is the whole service. The binary calls `run()`, which binds
loopback, serves, and issues `SayHello`. Tests also check health `Check` and
reflection `list_services`.

The in-tree `pbrs-grpc-hello` binary exercises all four RPC shapes; this
example is the unary path from the [guide](../../docs/grpc.md).
