# Status

## Verified

- `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings`,
  and `cargo test --workspace` pass.
- CI on `main` runs fmt, clippy, tests, and official conformance
  (`./scripts/conformance.sh`: required ×2 and recommended, v35.1, cmake
  protoc from the pin, not system, no skip list). The `test` job still
  apt-installs `protobuf-compiler` for the plugin and protobuf-tonic
  `build.rs`.
- Conformance v35.1 (`--maximum_edition 2023`, `protoc` hidden / vendored
  FDS): required ×2: 5631 binary+JSON + 909 text, 0 unexpected.
  `--enforce_recommended`: same. No skip list. Empty-FDS hole was closed
  in #6: `build.rs` used to write `[]` when `protoc` was missing; #6 ships
  `vendor/google/conformance_fds.bin` and falls back to it (that was the
  2090 JsonOutput / `missing desc` cluster). CI printed the same totals.
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
- `protobuf-tonic` on this tonic 0.14 stack covers all four RPC shapes
  (unary, client-stream, server-stream, bidi) including `Status`
  code+message. Initial `Response` metadata is headers; `Status` metadata
  is HTTP/2 trailers. Client-stream, server-stream, and bidi carry both
  (same split as unary). Server-stream trailers still fail before a
  stream. Client-stream headers need the reply `Response`.
  `tests/interop.rs` has same-process analogues of
  official interop names (`unimplemented_method`, `unimplemented_service`,
  `special_status_message`, `empty_unary`, `large_unary`, `empty_stream`,
  `cancel_after_begin`, `cancel_after_first_response`,
  `timeout_on_sleeping_server`, `custom_metadata`). `large_unary` sizes
  (271828 / 314159) are `hello.proto` string fields (`name` / `message`),
  not official `SimpleRequest.payload.body` / `response_size`. Cancel
  analogues abort the client future (`JoinError::Cancelled`, not a
  `Status`). `timeout_on_sleeping_server` is unary `Request::set_timeout`
  → `Code::Cancelled` / "Timeout expired", not `DeadlineExceeded`.
  `custom_metadata` (unary SayHello): client sends
  `x-grpc-test-echo-initial` and `x-grpc-test-echo-trailing-bin`; ascii
  echo is `Response.metadata` (headers). tonic 0.14 has no first-class
  OK-path custom trailers (`Response` has no `trailers()`);
  `x-grpc-test-echo-trailing-bin` is absent on the OK path. That bag is
  not trailers. `Status.metadata` on `Err` remains the trailer path.
  Same-process tonic, not official interop, not a native gRPC kernel, no
  Google peer. Compression is uncovered. `ProtobufCodec` dropped the
  per-message `Vec` and still lost the Codec bench to `ProstCodec`
  (hello 52.2 vs 25.8 ns combined, 4 KiB 190.6 vs 166.1). Smaller loss
  than #29 (93.6 vs 22.4). Remaining gap is `Parse` / `merge_from_bytes`
  (hello decode 45.4 vs 22.1). Inline string parse no longer
  `Wire::ensure`s the parent frame (`len ≤ 23` copies into
  `ProtoString`). Encode is close. Not kernel `./bench`. Not in CI.
  Not a win.
- `./bench` fails the process if a gated case loses encode or owned decode
  to prost, v4, or buffa owned. Twelve cases: empty, person, tat_populated,
  packed_256, map_64, nested_8, strings, unpacked_256, packed_fixed_256,
  packed_fixed64_256, packed_float_256, repeated_nested_8. View is gated
  except `tat_populated` (~3% band) and packed-fixed rows.
- File, enum, method, message, and field custom options survive
  FileDescriptorSet parse (`custom_option(n)`; file options on
  `FileDescriptor` / `DescriptorPool::get_file`).

## Remaining

See `docs/upb.md`. Short list:

- There are no arena views.
- JSON and text go through `DynamicMessage`.
- Edition 2024 extensions, CORD / cpp VIEW, and gtest matchers are missing.
- Maps are `Vec` (scan on get).
- Plugin-gencode hello Parse is still slower than prost. Codec line
  of record is #31: 52.2 vs 25.8 ns combined. Leftover is
  `merge_inner` glue (Default / `CachedSize::dirty` / tag loop),
  not `merge_bytes`. Closed inventories: [#32](https://github.com/mingley/pure-protobuf/pull/32)
  (hello ~23 ns vs prost, discarded-Arc path),
  [#36](https://github.com/mingley/pure-protobuf/pull/36)
  (wrapper 6.8–7.6 ns; Default 1.0 vs 0.4, dirty 0.3 inside it;
  do not mix hosts with #31 / #34),
  [#39](https://github.com/mingley/pure-protobuf/pull/39)
  (flatten `merge_from_bytes` → `merge_inner` made hello Parse
  worse, 24.5 → ~32 ns, discarded),
  [#41](https://github.com/mingley/pure-protobuf/pull/41)
  (4 KiB still `Wire::ensure`s the parent frame; leftover
  ~21–23 ns). Do not merge those diffs. [#27](https://github.com/mingley/pure-protobuf/pull/27)
  (234 rust_out link errors) is superseded by #42.

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
