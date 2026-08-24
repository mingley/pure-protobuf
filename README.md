# pbrs

pbrs is a pure-Rust protobuf kernel. The application API matches Google
protobuf v4: `Parse`, `Serialize`, `Clear`, `proto!`, `ProtoStr`,
`RepeatedView`, `DynamicMessage`.

It is not crates.io `protobuf` 4.x (upb/C), not prost, and not
[pb-rs](https://crates.io/crates/pb-rs). Google `protoc --rust_out` will
not link. Generate with `protoc-gen-pbrs`.

```toml
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
```

```bash
./scripts/gen.sh -I proto -o gen proto/your.proto
```

`protoc` must be on PATH. The plugin is `protoc-gen-pbrs` / `--pbrs_out`.

Docs:

- [Architecture](docs/architecture.md)
- [Design](docs/design.md)
- [Relative to upb](docs/upb.md)
- [Benchmarks](docs/benchmarks.md)
- [Status](docs/status.md)
- [tonic 0.14](protobuf-tonic/README.md)

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
