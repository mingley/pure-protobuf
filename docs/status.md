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
  Same-process tonic, not official interop, no Google peer. Gzip is covered
  (`tests/gzip.rs`). Generated stubs expose
  `with_interceptor` and `max_decoding_message_size` /
  `max_encoding_message_size` (`tests/interceptor_size.rs`). Codec survey
  (`tonic-bench`, `proto/codec_cases.proto`) vs prost and v4 upb is in
  `docs/benchmarks.md`. Typical unary `rpc_mixed` is already ~2× prost
  and beats v4. `name_4kib` combined beats prost (gated). `tags_32`
  decode beats v4 (gated). Not kernel `./bench`. Not in CI.
- `./bench` fails the process if a gated case loses encode or owned decode
  to prost, v4, or buffa owned. Twelve cases: empty, person, tat_populated,
  packed_256, map_64, nested_8, strings, unpacked_256, packed_fixed_256,
  packed_fixed64_256, packed_float_256, repeated_nested_8. View is gated
  except `tat_populated`, `person`, and packed-fixed rows.
- File, enum, method, message, and field custom options survive
  FileDescriptorSet parse (`custom_option(n)`; file options on
  `FileDescriptor` / `DescriptorPool::get_file`).
- `pbrs-grpc` is a native HTTP/2 gRPC kernel over pbrs. It is not tonic.
  Official `grpc.testing.TestService` interop binaries
  (`pbrs-grpc-interop-server` / `pbrs-grpc-interop-client`) pass the
  shared uncompressed `_TEST_CASES` against Go `interop/client` and
  `interop/server` (`--use_tls=false`) and the four gzip cases
  kernel-vs-kernel. Loopback `rpc-bench` empty_unary / large_unary is
  process-gated strictly faster than tonic 0.14. The same binary reports
  max QPS (not gated) at a few concurrency levels. `protobuf-tonic` stays
  the tonic adapter.

## Remaining

See `docs/upb.md`. Short list:

- Native gRPC is `pbrs-grpc`. Official `grpc.testing` TestService interop
  (`empty_unary` … `timeout_on_sleeping_server`, plus the four gzip cases)
  is implemented. TLS, health, reflection, GCP-auth, and ORCA stay out of
  that crate; tonic adapter still covers health/gzip/reflection via tonic
  crates.
- There are no arena views.
- JSON and text go through `DynamicMessage`.
- Edition 2024 extensions, CORD / cpp VIEW, and gtest matchers are missing.
- Maps are `Vec` (scan on get).
- `name_4kib` Codec combined beats prost (gated). `blob_4kib` still
  wins. `rpc_sparse` decode and `tags_32` decode vs v4 are gated.
  Leftover unary item is `name_80` combined. Flatten `merge_inner`
  (#39) stays discarded. Survey: `docs/benchmarks.md`. Closed
  inventories (notes + harnesses, not merged as wins):
  `docs/inventory/`.
  [#27](https://github.com/mingley/pure-protobuf/pull/27) rust_out 234
  errors is superseded by #42.

## Skipped rust/test/shared files

- `ctype_cord_test.rs`
- `gtest_matchers_test.rs`
- `no_internal_access_test.rs` (`__internal` is a module)
- `package_disambiguation_test.rs` (empty)
- `extensions_test.rs` (edition 2024 proto)
- edition2023 `str_view` cpp VIEW (ordinary string)
- `proto!` `#[cfg(bzl)]` qualified paths

## Publish

`pbrs` is registry-ready (`cargo publish -p pbrs --dry-run` on
crates.io). Live upload is not done from this tree. Do not publish as
`protobuf`. Nearby name `pb-rs` is quick-protobuf. `protobuf-tonic`
keeps a path (git until a registry version exists) dependency on `pbrs`;
`cargo publish -p protobuf-tonic` cannot succeed until that version
exists.
