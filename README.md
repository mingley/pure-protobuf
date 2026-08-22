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

Conformance (protobuf v35.1, `--maximum_edition 2023`, `18a7af1`):
required ×2: 3323 successes, 0 skipped, 0 expected failures, 2090
unexpected failures. Recommended: 3323 successes, 0 skipped, 0 expected
failures, 2308 unexpected failures. Both failed. 5631 is 3323+2308
attempted recommended binary+JSON, not passes. Text never started
(runner short-circuits after binary+JSON). Failure tail:
`Required.Proto3.ProtobufInput.ValidDataScalar.*.JsonOutput` / Failed
to parse input or produce output. CI does not run the runner.

```bash
./scripts/fetch-protobuf.sh
./scripts/conformance.sh
```

MIT OR Apache-2.0.
