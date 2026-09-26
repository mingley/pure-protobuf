# Status

## Recovery classification

Interrupted notes labeled most Distinct kernel behavior as Remaining.
Classification against committed source:

- **Shipped:** the Verified list and the Distinct notes below. Not a
  production certification.
- **Unfinished:** [TODO.md](../TODO.md) and the
  [leadership plan](ROADMAP.md). GR-01 and GR-02 are checked off there.
  No arena views, Edition 2024, xDS,
  application retries, hedging, channelz, binary logging, or grpc.stats /
  OpenTelemetry. `name_80` leftover remains. Field-wise WKT JSON/text is
  not closed.
- **Discarded:** [closed inventory](inventory/README.md). Do not merge
  those diffs. `#34` already landed; `#39` flatten and `#57` heap-copy
  did not.
- **Missing evidence:** recorded CI/conformance/interop numbers are
  historical results, not GR-03+ qualification. macOS source-bind is in
  the required `macos` job; that does not qualify GR-03+.

The macOS `127.0.0.2` source-bind failure (OS error 49 /
`AddrNotAvailable` at
[fa48d599](https://github.com/mingley/pure-protobuf/commit/fa48d599b3fdd797d53fff98647dea25a601aae3))
is fixed in this slice: TCP tests listen on `127.0.0.1`, dial with the
shipped `connect(..., Some(bind))`, and assert the accepted peer IP
equals the bound source IP. `127.0.0.2` is used when that alias is
bindable; otherwise a distinct non-loopback IPv4 that survives the same
listen+accept proof. Bind failure is not treated as success.

## Verified

- `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings`,
  and `cargo test --workspace` pass.
- CI on `main` / PRs (and on the release SHA via `workflow_call`) runs
  fmt, clippy, tests, docs `-D warnings`, official conformance, grpc-interop,
  `msrv-core` (rustc 1.85 `--lib` for `pbrs` and `pbrs-grpc`), `msrv-tonic`
  (rustc 1.88 `protobuf-tonic`), macOS `tcp::tests` + onboarding,
  isolated `package_consumer` unpacks, and generated-output drift. The
  `test` job still apt-installs `protobuf-compiler` for the plugin and
  adapter `build.rs`.
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
  (same split as unary). Server-stream `Status` trailers also work when
  the handler returns `Ok(Response(stream))` and the stream errors
  before any item (empty stream + error, or first item `Err`): headers
  stay on `Response.metadata`, trailers on `Status.metadata`. Handler
  `Err(Status)` before opening a stream still lands on the call
  `Result`. Client-stream headers need the reply `Response`.
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
  echo is `Response.metadata` (headers). Kernel
  `pbrs_grpc::Response::trailers()` carries OK-path custom trailers;
  `Streaming::trailers()` waits for end-of-stream. `protobuf-tonic` uses
  tonic's `Response`, which has no `trailers()`, so
  `x-grpc-test-echo-trailing-bin` is absent on that adapter's OK path.
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
  kernel-vs-kernel. Loopback `rpc-bench` latency is process-gated
  (kernel median ns strictly below tonic 0.14 on empty_unary and
  large_unary). Server-streaming and bidi ping-pong throughput are gated at
  90% of tonic 0.14 (same noise band). Client-streaming upload is gated at
  90% of tonic 0.14 the same way. QPS is reported, not gated (empty/large at
  conc=1/conns=1 and conc=16/conns=4). Nonzero RPC errors still fail
  the process. `scripts/grpc-server-bench.sh` also reports loopback bidi
  ping-pong and client-streaming upload against grpc-go (not the Xeon unary
  tables).   `Channel::connect_pool` opens independent h2 driver
  tasks.   `Channel`, `GreeterClient` / `TestServiceClient`, and
  `GreeterServer` / `TestServiceServer` expose
  `max_decoding_message_size` / `max_encoding_message_size`
  (default 4 MiB inbound, unlimited outbound). Oversize encode or decode is
  `RESOURCE_EXHAUSTED` on every call shape (`pbrs-grpc/tests/message_size.rs`).
  Not a
  latency or QPS win. `protobuf-tonic` stays the tonic adapter.

## Shipped Capabilities and Boundaries

The native gRPC kernel (`pbrs-grpc`) provides comprehensive support across all four call shapes and five transport backends (TCP/h2c, TLS, mTLS, Unix Domain Sockets, and in-process `from_io`).

### Transport and Security Invariants
- **Mandatory Certificate Verification**: In TLS and mTLS, certificate verification is not optional. There is no skip-verify constructor in the kernel; clients verify using `ClientTls::webpki` or pinned CAs via `ClientTls::ca`.
- **ALPN Requirement**: All TLS handshakes mandate ALPN `h2`.
- **Peer Information**: Peer identities on mTLS are exposed via `Rpc::peer_identity`; Unix caller credentials (`SO_PEERCRED`) are exposed via `Rpc::peer_cred`.
- **In-Process Channels**: `Channel::from_io` provides memory-backed streaming without socket overhead, but does not perform transparent retry or connection pooling.

### Messaging and Protocol Limits
- **Message Caps**: Inbound messages are capped at 4 MiB by default via `max_decoding_message_size`; exceeding frames fail with `Code::ResourceExhausted`.
- **Metadata and Trailers**: Full support for ASCII metadata and binary `-bin` trailers. OK-path custom trailers are retrievable via `Response::trailers` and `Streaming::trailers`.
- **Rich Status**: Supports packed `google.rpc.Status` on `grpc-status-details-bin`. When ASCII `grpc-status` headers and binary status details disagree, wire ASCII trailers take precedence.
- **Defensive Caps**: Rapid reset (CVE-2023-44487) defense via `ServerConfig::max_pending_accept_reset_streams`, CONTINUATION flood prevention via `max_header_list_size` (16 KiB), and graceful connection recycling via `max_connection_age` (±10% jitter).

### Explicit Omissions and Boundaries
- **xDS Protocol**: Omitted; name resolution and dynamic traffic steering are expected to terminate at service-mesh ingress or L4/L7 sidecars.
- **channelz & Binary Logging**: Omitted from kernel to prevent unbounded runtime memory retention.
- **Hedging**: Speculative hedging is omitted to avoid latency spikes and traffic amplification.
- **Edition 2024**: Edition 2024 is currently untested and unsupported (conformance covers up to Edition 2023).
- **Service-Config Retries**: Application-level retries remain at the call site evaluated against `Code::is_retryable`.

For a consolidated cross-framework comparison matrix, see [docs/guides/comparison.md](guides/comparison.md).

## Unfinished

Tracked in [TODO.md](../TODO.md) / [ROADMAP.md](ROADMAP.md). The notes above document shipped behavior and explicit omissions; they are not an open work queue. Still not done: arena views, Edition 2024, `name_80` leftover, xDS, streaming policy retries, channelz, binary logging, grpc.stats / OpenTelemetry, remaining WKT field-wise JSON/text, and GR-03+. Unary service-config retries and hedging ship (`Channel::service_config`, `tests/policy_retry.rs`). Do not treat a clean checkout as production certification.

## Skipped rust/test/shared files

- `ctype_cord_test.rs`
- `gtest_matchers_test.rs`
- `no_internal_access_test.rs` (`__internal` is a module)
- `package_disambiguation_test.rs` (empty)
- `extensions_test.rs` (edition 2024 proto)
- edition2023 `str_view` cpp VIEW (ordinary string)
- `proto!` `#[cfg(bzl)]` qualified paths

## Publish

The only publisher is [`.github/workflows/release.yml`](../.github/workflows/release.yml)
(`v*` tags or confirmed dispatch after required CI on that SHA). `main`
pushes do not publish. Credential: repository secret `CRATES_IO_TOKEN`
(`CARGO_REGISTRY_TOKEN`). Not Trusted Publishing (`id-token: write` is
not set). See [RELEASE.md](RELEASE.md). Do not publish as `protobuf`.
Nearby name `pb-rs` is quick-protobuf.
