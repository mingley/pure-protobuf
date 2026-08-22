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
- 38 `google_shared` tests cover a subset of `rust/test/shared`.
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
  per-message `Vec` and still lost the Codec bench to `ProstCodec`.
  Inline string parse no longer `Wire::ensure`s the parent frame
  (`len ≤ 23` copies into `ProtoString`). Hello decode is a smaller
  loss (25.0 vs 21.5 ns; combined 31.8 vs 25.0) than #30/#31 (45.4 vs
  22.1 decode, 52.2 vs 25.8 combined). Residual is `merge_inner` glue,
  not the dropped Arc. 4 KiB still shares the Wire window and still
  loses. Encode is close. Not kernel `./bench`. Not in CI. Not a win.
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

- Regenerated code is required (`protoc-gen-pbrs`). Official
  `protoc --rust_out` kernel=upb of `proto/person.proto` compiled
  against pbrs as crate `protobuf` is 234 rustc errors (gencode
  4.35.1-release). Dominant miss is 147× E0433 no `__internal::runtime`
  (`OwnedMessageInner`, `MiniTable*`, Arena ABI). Also 6× no
  `entity_tag`, 6× no `EntityType`, 4× `into_proxied` arity (rust_out
  `(self, Private)` vs pbrs `(self)`), 70× rust_out types miss pbrs
  `Message` supertraits, 1× no `assert_compatible_gencode_version`.
  `pbrs::__internal` exports only `Private` and `SealedInternal`.
  rust_out will not link.
- There are no arena views.
- JSON and text go through `DynamicMessage`.
- Edition 2024 extensions, CORD / cpp VIEW, and gtest matchers are missing.
- Maps are `Vec` (scan on get).
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
