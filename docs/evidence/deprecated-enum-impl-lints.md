# GN-13: deprecated enum implementation lint scopes

Base: `03e2201d`, with the GN-14 enum-option field correction in `9fd03a6c`.
This is a correctness change, with no performance claim.

Generated deprecated enum implementations referred to their own deprecated
type and tuple field without a scoped lint allowance. A strict consumer failed
even when application code did not use the deprecated public API.

The generator now emits a reasoned `#[allow(deprecated)]` immediately before
each implementation of a deprecated enum. This covers inherent constants and
name lookup, conversions, defaults, debugging, and the enum/proxy traits. The
public type and deprecated values retain their `#[deprecated]` annotations.
Enums that are not deprecated emit no additional allowance. No API, dependency
or enum conversion behavior changes.

The regression uses pinned-protoc proto3 open and proto2 closed enums with
aliases and a deprecated value. It compiles generated code with
`deny(warnings)` and strict Clippy groups, exercises defaults, aliases, integer
conversions, debugging, known and unknown values, and view conversions, and
checks rustdoc with warnings denied. The source assertions require exactly ten
allowances per enum and require each allowance to attach directly to an impl.
A separate dependent crate must fail `deny(deprecated)` when using each enum.

Pins used on Linux x86_64:

- Rust `1.99.0`, commit `b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`.
- Cargo `1.99.0`, commit `5f94df478`.
- `libprotoc 35.1`, protobuf `v35.1` at
  `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`.
- Repository `Cargo.lock`, with no new dependencies.

Before the generator change:

```sh
cargo test --locked -p pbrs --test plugin protoc_plugin_deprecated_enums_keep_implementation_lints_scoped -- --nocapture
```

The regression failed in consumer Clippy with 46 deprecated type/tuple-field
errors; both the consumer library and its test build were rejected. The
generator correction was applied only after this failure was reproduced.

The same command passed after the generator change: one targeted integration
test passed, including all nested checks and both expected caller rejections.
Nested checks run:

```sh
cargo clippy --offline --quiet --all-targets -- -D warnings
cargo test --offline --quiet
RUSTDOCFLAGS='-D warnings' cargo doc --offline --quiet --no-deps
cargo check --offline --quiet # separate caller, expected failure for each enum
```

After-check build settings are `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0` and `CARGO_BUILD_JOBS=2`.
Nested consumers use the existing shared integration-consumer target and
serialized Cargo helper.

Full codegen, documentation and regenerated bundled Clippy/rustdoc/conformance
gates are coordinated with GN-12. This targeted evidence does not replace those
integration gates.
