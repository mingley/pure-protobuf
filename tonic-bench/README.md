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
integrity checks and regeneration procedure. Save the tables and raw samples
with the source revision and tool versions. The default build uses the
standard allocator for timing.

For a separate Rust heap retention diagnostic, enable `allocation-counts`:

```sh
PURE_PROTOBUF_CHECKED_V4=1 cargo run --locked --release --manifest-path tonic-bench/Cargo.toml --features allocation-counts
```

The raw report records `allocation_counts`. The opt-in build includes
allocation accounting in the timed code.

## Compare owned output

Use `owned-output` to include allocation and drop of the encoded result
for first and repeated encodes on all three codecs:

~~~sh
PURE_PROTOBUF_CHECKED_V4=1 cargo run --locked --release \
  --manifest-path tonic-bench/Cargo.toml --features owned-output > survey.txt
~~~

Save the survey's standard output to a file to count faster, tied and slower
medians for each operation. The same command accepts `raw.json` from the
hosted survey artifact. Add `--losses` to list each slower fixture and its
timings in nanoseconds.

~~~sh
python3 tonic-bench/summary.py survey.txt --losses
~~~

The raw report records `pbrs_prost_encode_output` as `owned` or `reused`.
The upb binding returns owned bytes in both modes. The separate mutation
rows use reused buffers for pbrs and Prost.

Read encode, decode and read-every-field rows separately. An owned lazy message
can defer work until a field is read. Repeated encoding can reuse cached sizes;
that differs from encoding a newly built or modified message. V4/upb also has
C arena costs that a Rust allocation counter does not fully observe.

For RPC measurements use the [gRPC harness](../rpc-bench/README.md). For
generator and downstream compilation costs use the
[codegen harness](../bench/codegen/README.md). The
[results page](../docs/benchmarks.md) links the recorded comparisons.
