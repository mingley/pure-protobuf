# pbrs

pbrs is a pure-Rust protobuf kernel. The application API matches Google
protobuf v4: `Parse`, `Serialize`, `Clear`, `proto!`, `ProtoStr`,
`RepeatedView`, `DynamicMessage`.

It is not crates.io `protobuf` 4.x (upb/C), not prost, and not
[pb-rs](https://crates.io/crates/pb-rs). Generate with `protoc-gen-pbrs`,
or from a `build.rs` with `pbrs::codegen::compile_protos` (prost-build
shape). Official `protoc --rust_out kernel=upb` also links against this
crate as `protobuf` via the MiniTable stand-in in `src/runtime.rs`
(`rust_out_person` roundtrips).

```toml
pbrs = "0.1"
# until this version is on crates.io:
# pbrs = { git = "https://github.com/mingley/pure-protobuf" }
```

`protobuf-tonic` still depends on `pbrs` by path (and git in published docs)
until a registry version exists. Do not `cargo publish -p protobuf-tonic`
against that path dep.

`pbrs-grpc` is a separate HTTP/2 gRPC kernel over pbrs. It does not depend
on tonic. The tonic adapter does not depend on `pbrs-grpc`. Use one, the
other, or neither. Generate kernel client and server stubs for any
`.proto` service with `Config::emit_kernel_stubs(true)`; see the
[gRPC guide](docs/grpc.md). [`examples/greeter`](examples/greeter) is a
complete user crate (own proto, `build.rs`, health, reflection).

```bash
./scripts/gen.sh -I proto -o gen proto/your.proto
```

```rust
// build.rs
fn main() {
    pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"]).unwrap();
}
```

`protoc` must be on PATH. The plugin is `protoc-gen-pbrs` / `--pbrs_out`.

Docs:

- [Architecture](docs/architecture.md)
- [Design](docs/design.md)
- [Relative to upb](docs/upb.md)
- [Benchmarks](docs/benchmarks.md)
- [Status](docs/status.md)
- [tonic 0.14](protobuf-tonic/README.md)
- [HTTP/2 gRPC kernel](pbrs-grpc/README.md) and its [guide](docs/grpc.md)

Conformance (official `conformance_test_runner` v35.1,
`--maximum_edition 2023`, `protoc` hidden / vendored FDS):
required ×2: 5631 binary+JSON + 909 text, 0 unexpected.
`--enforce_recommended`: same. No skip list. Empty-FDS hole was closed
in #6: `build.rs` used to write `[]` when `protoc` was missing; #6 ships
`vendor/google/conformance_fds.bin` and falls back to it (that was the
2090 JsonOutput / `missing desc` cluster). CI runs the official runner
(required ×2 and recommended, v35.1, cmake protoc not system, no skip
list) and printed the same totals.

```bash
./scripts/fetch-protobuf.sh
./scripts/conformance.sh
```

MIT OR Apache-2.0.
