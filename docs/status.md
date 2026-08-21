# Status

## Verified

- `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings`,
  and `cargo test --workspace` pass.
- CI on `main` runs fmt, clippy, and tests. It needs `protoc` for the plugin
  and for protobuf-tonic `build.rs`.
- Conformance v35.1 required and recommended: 5631 binary+JSON + 909 text,
  0 unexpected.
- 38 `google_shared` tests cover a subset of `rust/test/shared`.
- Plugin round-trip works, including `./scripts/gen.sh`.
- tonic 0.14 unary and bidi smoke tests pass in `protobuf-tonic`.
- `./bench` fails the process if a gated case loses encode or owned decode
  to prost, v4, or buffa owned. The nine cases are empty, person,
  tat_populated, packed_256, map_64, nested_8, strings, unpacked_256, and
  packed_fixed_256. View is gated except `tat_populated` (~3% band) and
  `packed_fixed_256`.

## Remaining

See `docs/upb.md`. Short list:

- Regenerated code is required. Google rust_out `OwnedMessageInner` will
  not link.
- There are no arena views.
- JSON and text go through `DynamicMessage`.
- Edition 2024 extensions, CORD / cpp VIEW, and gtest matchers are missing.
- Maps are `Vec` (scan on get).
- memcpy-packed fields other than `packed_fixed32` are Cold.
- There is no in-tree fuzzing.

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
