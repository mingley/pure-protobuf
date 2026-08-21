# Status

Gated:

- `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings`,
  `cargo test --workspace`
- CI on `main` (fmt, clippy, tests). Needs `protoc` for plugin and
  protobuf-tonic `build.rs`
- conformance v35.1 required and recommended: 5631 + 909, 0 unexpected
- 38 `google_shared` tests (rust/test/shared subset)
- plugin round-trip including `./scripts/gen.sh`
- tonic 0.14 unary + bidi smoke (`protobuf-tonic`)

## Demo to Google rust / gRPC

What to say:

1. Pure-Rust kernel with the v4 *application* trait set, not a rust_out
   replacement. `protoc --rust_out kernel=upb` will not link.
2. Conformance is the official runner, same pin rust_upb uses, including
   recommended.
3. Small-message encode/decode beats the crates.io rust+upb wrapper on the
   gated suite. That wrapper's cost is arena + FFI + extra copy, not upb C
   being slow.
4. Views are `&Owned`. buffa `decode_view` and upb arena views are a
   different product. We lose packed-fixed decode to v4 and unpacked decode
   to prost/buffa owned. Numbers in `docs/benchmarks.md`.
5. tonic works only through `protobuf-tonic`, tonic 0.14+, regenerate
   stubs. Not a `tonic-prost` drop-in. `protoc` still required at codegen.

What not to say:

- "drop-in for protobuf 4.x" (gencode and `__internal` differ)
- "zero-copy kernel" (`FooView` is not a wire view)
- "faster than upb C" (we timed the rust wrapper)
- "all cases" (extended suite has losses)

## Skipped rust/test/shared files

- `ctype_cord_test.rs`
- `gtest_matchers_test.rs`
- `no_internal_access_test.rs` (`__internal` is a module)
- `package_disambiguation_test.rs` (empty)
- `extensions_test.rs` (edition 2024 proto)
- edition2023 `str_view` cpp VIEW (ordinary string)
- `proto!` `#[cfg(bzl)]` qualified paths

## Publish

`publish = false`. crates.io `pbrs` was free as of 2026-08-20. Do not
publish as `protobuf`. Nearby name `pb-rs` is quick-protobuf.
