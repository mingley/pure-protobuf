# pbrs

Pure-Rust protobuf kernel. Application API matches Google protobuf v4:
`Parse`, `Serialize`, `proto!`, `ProtoStr`, `RepeatedView`,
`DynamicMessage`.

Not crates.io `protobuf` 4.x (upb/C). Not prost. Not
[pb-rs](https://crates.io/crates/pb-rs). Google `protoc --rust_out` will
not link. Regenerate with `protoc-gen-pbrs`.

```toml
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
```

```bash
./scripts/gen.sh -I proto -o gen proto/your.proto
```

`protoc` on PATH. Plugin is `protoc-gen-pbrs` / `--pbrs_out`.

Docs:

- [Architecture](docs/architecture.md)
- [Design](docs/design.md)
- [Relative to upb](docs/upb.md)
- [Benchmarks](docs/benchmarks.md)
- [Status](docs/status.md)
- [tonic 0.14](protobuf-tonic/README.md)

Conformance (protobuf v35.1 runner): 5631 binary+JSON + 909 text, 0
unexpected, including `--enforce_recommended`.

```bash
./scripts/fetch-protobuf.sh
./scripts/conformance.sh
```

MIT OR Apache-2.0.
