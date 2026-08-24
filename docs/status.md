# Status

## Verified

- `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings`,
  and `cargo test --workspace` pass.
- CI on `main` runs fmt, clippy, and tests. It needs `protoc` for the plugin
  and for protobuf-tonic `build.rs`.
- Conformance v35.1 required and recommended: 5631 binary+JSON + 909 text,
  0 unexpected.
- Official `protoc --rust_out` (4.35.1-release, `kernel=upb`) for
  `proto/person.proto` links against this crate as `protobuf` and
  parse→serialize→parse roundtrips (`rust_out_person/`).
- `rust_out_shared` runs official `rust/test/shared` googletest files
  (19 crates, 0 failed) against `protoc --rust_out kernel=upb`. Skips are
  only the files listed below.
- In-tree fuzz: `tests/fuzz_parse.rs` (empty / truncated / Person / TAT).
- grpc 0.9 unary remap (`grpc_remap/`): `ok name=ada message=Hello ada`
  through `protobuf-shim` → pbrs, not protobuf-tonic.
- 38 `google_shared` tests cover a plugin-generated subset of
  `rust/test/shared`.
- Plugin round-trip works, including `./scripts/gen.sh`.
- tonic 0.14 unary and bidi smoke tests pass in `protobuf-tonic`.
- `./bench` fails the process if a gated case loses encode or owned decode
  to prost, v4, or buffa owned. Twelve cases: empty, person, tat_populated,
  packed_256, map_64, nested_8, strings, unpacked_256, packed_fixed_256,
  packed_fixed64_256, packed_float_256, repeated_nested_8. View is gated
  except `tat_populated` (~3% band) and packed-fixed rows.

## Remaining

See `docs/upb.md`. Short list:

- There are no arena views.
- JSON and text go through `DynamicMessage`.
- Edition 2024 extensions, CORD / cpp VIEW, and gtest matchers are missing.
- Maps are `Vec` (scan on get).
- File / enum / method custom options are skipped on FileDescriptorSet
  parse. Message and field custom options are kept.

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
