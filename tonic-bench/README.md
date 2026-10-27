# Compare message codecs

This survey compares pbrs, Prost and Google's protobuf v4/upb on the same
message shapes. It measures message encoding, decoding, field access and
mutation within one process. It does not run a gRPC transport.

From the repository root, with Rust, Cargo, a C compiler and `protoc` installed:

```sh
python3 -B -m unittest discover -s tonic-bench -p 'test_checked_v4.py' -q
PURE_PROTOBUF_CHECKED_V4=1 cargo run --locked --release --manifest-path tonic-bench/Cargo.toml
```

The checked v4 bindings avoid needing the experimental Rust generator for a
normal run. The [binding guide](checked_v4/README.md) describes their source,
integrity checks and regeneration procedure. The survey prints its tables to
stdout; save the output together with the source revision and tool versions.

Read encode, decode and read-every-field rows separately. An owned lazy message
can defer work until a field is read. Repeated encoding can reuse cached sizes;
that differs from encoding a newly built or modified message. V4/upb also has
C arena costs that a Rust allocation counter does not fully observe.

For RPC measurements use the [gRPC harness](../rpc-bench/README.md). For
generator and downstream compilation costs use the
[codegen harness](../bench/codegen/README.md). The
[results page](../docs/benchmarks.md) links the recorded comparisons.
