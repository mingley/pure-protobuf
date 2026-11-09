# Implementation status

`pbrs` provides protobuf messages, code generation, reflection, JSON and text
format. `pbrs-grpc` provides the native HTTP/2 client and server;
`protobuf-tonic` integrates pbrs messages with tonic 0.14+. Start with the
[documentation index](README.md) for working examples and task guides.

At source [`6024ac88`](https://github.com/mingley/pure-protobuf/commit/6024ac884e6ccda3b14c77ca115732629e2217d7), all 12 required
[CI jobs](https://github.com/mingley/pure-protobuf/actions/runs/42675951341)
and all six [Miri lanes](https://github.com/mingley/pure-protobuf/actions/runs/42675951169)
passed. Five enabled [compatibility jobs](https://github.com/mingley/pure-protobuf/actions/runs/42675950887)
passed; the optional fuzz and sanitizer jobs were skipped. These checks cover
the named source revision; later changes need their own checks.

The earlier [parity audit](https://github.com/mingley/pure-protobuf/actions/runs/42200175988)
at `10d94697` includes known differences as well as successful comparisons.
That revision's [Miri record](https://github.com/mingley/pure-protobuf/actions/runs/42200175953)
also includes interpreted i686 checks for the alloc publication cell. The other
lanes do not establish 32-bit coverage.

Choose the default lazy message layout for sparse reads and unchanged
forwarding, [owned nested messages](guides/codegen.md#owned-nested-message-fields)
for complete traversal, or [`boxed(path)`](guides/codegen.md#box-selected-nested-fields)
for selected singular children. The opt-in
[safe-default profile](guides/codegen.md#safe-generated-defaults) supports
forbid-unsafe generated consumers; runtime dependencies retain their own
unsafe implementations. The
[namespace controls](../tests/proof/namespace_symbols/README.md) cover names
such as `Default`, `Some`, `pbrs`, `std` and `core`, plus runtime aliases,
while preserving user paths and literals.

[`Config::bytes(paths)`](guides/bytes.md) selects shared `ProtoBytes` storage
with owned shared getters and valid generated defaults. Shared input can be
retained; borrowed input copies payloads, and older eager parents can also copy.

The [field-view consumers](../tests/proof/field_views/README.md) check 25 schema
cases across four profiles, including accessor collisions and unchanged valid
APIs. Map removal preserves unrelated duplicate-key winners and repairs an
existing index. Its [cost record](evidence/map-removal-20261106/README.md) retains
small-map regressions and the remaining target-deletion difference from Prost.

Direct endpoints support URI origins independently of dialing and TLS;
the [transport guide](guides/transport-controls.md#request-origin-and-dial-target)
explains logical request scheme, authority and clone overrides. The
[benchmark results](benchmarks.md) retain local naming gains, safe-default
costs and origin overhead alongside earlier results. Full tonic/prost parity
and an overall performance ranking remain unproven.

Next comparisons should cover default nested full reads, string-key map
lookup allocations with small-map controls, compression using the same Prost
codec, and matched consumer builds.

Selected [ordered maps](guides/maps.md) use unique key-sorted storage and typed
views. Regenerate matching ancestors to reach that storage during encoding;
older lazy ancestors can retain unsorted or duplicate-key wire. Nested-map JSON
uses resolved descriptors, including imported and recursive children.

The [explicit alloc-only profile](guides/alloc-only.md) compiles concrete native
and Prost messages for `thumbv7em-none-eabi`, with std host codegen kept separate.
Five generated profiles pass std-backed host functional checks. Firmware and
other targets remain unverified; reflection, formats, transports and the
`rust_out` ABI stay std-only. Existing no-default builds retain std, and the
`std` feature wins when enabled.

Static balance helpers preserve per-endpoint executors and full URI identities.
RPCs use the selected endpoint's authority unless `Channel::origin` pins an
authority on that clone. Connection settings remain shared; per-endpoint
origins, TLS and hostname lookup remain unsupported. The
[transport guide](guides/transport-controls.md#connection-executors) explains
how configured channel getters differ from selected wire routing.
See the [parity summary](parity.md) and
[API comparison](guides/comparison.md) for tested behavior and boundaries.

The source declares core `0.2.1` and adapter `0.1.0-alpha.3` candidates;
check the registry before treating them as published. The
[release rehearsal](https://github.com/mingley/pure-protobuf/actions/runs/42682463666)
at `6024ac88` used that revision's successful CI, packaged all three crates,
and produced a CycloneDX SBOM and provenance attestation. Its
[evidence artifact](https://github.com/mingley/pure-protobuf/actions/runs/42682463666/artifacts/13703810774)
has SHA-256 `b179a51a7e1e88cdb9fa6b8956f79dbb9201827434f64dace256fdf36fcc6506`
and expires February 7, 2027. The rehearsal did not authenticate to crates.io
or upload the crates. An eventual release needs the same checks on its final
source revision.

Earlier rehearsals are available for
[10d94697](https://github.com/mingley/pure-protobuf/actions/runs/42205359709)
and [1472791](https://github.com/mingley/pure-protobuf/actions/runs/42530248925).

Sustained production qualification remains open; use the
[release checklist](release-readiness.md) to track candidate evidence, the
[release policy](RELEASE.md) for requirements, and the [work queue](../TODO.md)
for implementation work.

The [November 5 status history](evidence/status-history-20261105.md) preserves
earlier checks and measurements with their original source revisions. The
capability sections below describe implementation scope. These checks are
not a production certification.

## Recovery classification

Older notes mixed implemented features, experiments and qualification gaps.
Read their status as follows:

| Classification | Meaning |
|---|---|
| **Implemented** | Source capabilities below, subject to their documented limits. Implementation is not production qualification. |
| **Unfinished** | Full Edition 2024/typed-extension coverage, generated borrowed views, a complete official upb-compatible kernel, xDS, full telemetry and sustained production/performance qualification. The [queue](../TODO.md) tracks concrete work. |
| **Archived experiments** | [Closed inventory](inventory/README.md). The `#39` flatten and `#57` heap-copy drafts were not accepted as wins; later related implementations have independent evidence. Do not resurrect a closed diff from an old result. |
| **Missing evidence** | Recorded CI, conformance, and interop numbers are historical results, not GR-03+ qualification. macOS source-bind is in the required `macos` job; that does not qualify GR-03+. |

The macOS `127.0.0.2` source-bind failure (OS error 49 /
`AddrNotAvailable` at
[fa48d599](https://github.com/mingley/pure-protobuf/commit/fa48d599b3fdd797d53fff98647dea25a601aae3))
is fixed in this slice. TCP tests listen on `127.0.0.1`, dial with the shipped
`connect(..., Some(bind))`, and assert that the accepted peer IP equals the
bound source IP. The tests use `127.0.0.2` when that alias is bindable;
otherwise they use a distinct non-loopback IPv4 that survives the same
listen+accept proof. Bind failure is not treated as success.

## Verified

The entries below are historical results at the revisions they name.

### Workspace and CI

- Final local execution check on `3a7aa128` source plus the documentation and
  evidence updates: `PATH=/opt/homebrew/bin:$PATH CARGO_BUILD_JOBS=2 cargo test
  --locked --workspace --quiet` passed, including documentation and compiled
  consumer recipes. Existing compiler-version and documentation ignores remain;
  none were added to make the run pass. The separate
  [full source CI checkpoint](https://github.com/mingley/pure-protobuf/actions/runs/36622116837)
  was still running when this record was prepared; local success is not its
  completion verdict.
- At audited revision `37683917`, [CI run 36594564013](https://github.com/mingley/pure-protobuf/actions/runs/36594564013)
  passed formatting, Clippy and workspace tests; its final rustdoc step failed.
  See the [audit](audit-2026-09-29.md#correctness-and-ci) for the separate
  compatibility failure and this update's verification scope.
- CI on `main`, PRs, and the release SHA through `workflow_call` runs fmt,
  clippy, tests, docs `-D warnings`, official conformance, grpc-interop,
  `msrv-core`, `msrv-tonic`, macOS `tcp::tests` + onboarding, isolated
  `package_consumer` unpacks, and generated-output drift.
- `msrv-core` uses rustc 1.85 `--lib` for `pbrs` and `pbrs-grpc`.
- `msrv-tonic` uses rustc 1.88 for `protobuf-tonic`.
- The `test` job installs `protobuf-compiler` for codegen tests and guide
  snippet checks. Both adapter builds use checked descriptors.

### Public guide recipes (QG-07)

- Six named recipes in the [operations guide](guides/operations.md) are
  extracted into a fresh consumer with the documented features, compiled,
  and exercised over bounded in-memory HTTP/2. They cover health Check/Watch,
  reflection, rich `ErrorDetails` trailers and the actual `from_io` greeting.
- The [production-service guide](guides/production-service.md) reuses the
  compiled TLS/mTLS demo for admission/stream limits, overload recovery and
  drain. Its fresh-consumer test remains part of onboarding.
- API mutation to a nonexistent method fails compilation; surrounding prose
  changes do not affect the extracted program. Offline links, anchors,
  caveats and negative syntax tests remain checked; prose-equality pins in
  the greeter example were replaced by a shorter error-lifecycle overview.
- Local 2026-09-29 validation: documentation 24 passed; onboarding 15,
  pbrs-build 30 and plugin 44 passed, with three existing compiler-version
  tests ignored; greeter 16 passed and warning-strict rustdoc passed. The
  [task card](plan/world-class/tasks.json) records exact commands. This
  bounded recipe validation does not replace QG-06's sustained soak.

### Protobuf compatibility

- Conformance v35.1 (`--maximum_edition 2023`, `protoc` hidden / vendored
  file descriptor set) passes required x2 binary+JSON and text suites:
  5631 binary+JSON + 909 text, 0 unexpected. `--enforce_recommended` reports
  the same result. There is no skip list.
- The empty file-descriptor-set hole closed in #6. `build.rs` used to write
  `[]` when `protoc` was missing; #6 ships `vendor/google/conformance_fds.bin`
  and falls back to it. That was the 2090 JsonOutput / `missing desc` cluster.
  CI printed the same totals.
- Official `protoc --rust_out` 4.35.1-release (`kernel=upb`) for
  `proto/person.proto` links against this crate as `protobuf` and
  parse -> serialize -> parse roundtrips (`rust_out_person/`).
- `rust_out_shared` targets 19 original `rust/test/shared` consumer crates.
  At `37683917`, `test_map_int32_enum` failed and stopped the runner after the
  first crate. At `c89608bd`, all 19 crates pass (233 original tests), with
  three additional generated-API regressions. The original assertions are
  unchanged. The additional suite exclusions are listed below.
- The separate `3a7aa128` closed-enum follow-up passes the same 233 original
  tests plus six regressions in ordinary execution and Miri; core
  library/native shared/runtime/kernel tests pass 96/38/26/18, with Miri
  covering library/runtime/kernel 96/26/18. Its
  [Linux compatibility run](https://github.com/mingley/pure-protobuf/actions/runs/36622048393)
  and [Linux kernel Miri run](https://github.com/mingley/pure-protobuf/actions/runs/36622048523)
  also passed. This is scoped kernel recovery, not full upb compatibility.
- Fixed-input parser smoke tests coexist with coverage-guided targets in
  `fuzz/`. The [2026-09-28 campaign](evidence/fuzz-2026-09-28.md) ran five
  targets for about five minutes each and found defects that were fixed.
  It does not satisfy the longer qualification campaign.
- The grpc 0.9 unary remap (`grpc_remap/`) reports
  `ok name=ada message=Hello ada` through `protobuf-shim` -> pbrs, not
  protobuf-tonic.
- 38 `google_shared` tests cover a plugin-generated subset of
  `rust/test/shared`.
- Plugin round-trip works, including `./scripts/gen.sh`.
- File, enum, method, message, and field custom options survive
  FileDescriptorSet parse (`custom_option(n)`; file options on
  `FileDescriptor` / `DescriptorPool::get_file`).

### protobuf-tonic

- `protobuf-tonic` on this Tonic 0.14 stack covers all four RPC shapes:
  unary, client-stream, server-stream, and bidirectional streaming. It also
  covers `Status` code+message.
- Initial `Response` metadata is headers. `Status` metadata is HTTP/2 trailers.
  Client-stream, server-stream, and bidirectional streaming carry both with the
  same split as unary.
- Server-stream `Status` trailers also work when the handler returns
  `Ok(Response(stream))` and the stream errors before any item: empty stream
  plus error, or first item `Err`. Headers stay on `Response.metadata`;
  trailers stay on `Status.metadata`.
- Handler `Err(Status)` before opening a stream still lands on the call
  `Result`. Client-stream headers need the reply `Response`.
- `tests/interop.rs` has same-process analogues of official interop names:
  `unimplemented_method`, `unimplemented_service`, `special_status_message`,
  `empty_unary`, `large_unary`, `empty_stream`, `cancel_after_begin`,
  `cancel_after_first_response`, `timeout_on_sleeping_server`, and
  `custom_metadata`.
- `large_unary` sizes (271828 / 314159) are `hello.proto` string fields
  (`name` / `message`), not official `SimpleRequest.payload.body` /
  `response_size`.
- Cancel analogues abort the client future (`JoinError::Cancelled`), not a
  `Status`.
- `timeout_on_sleeping_server` is unary `Request::set_timeout` ->
  `Code::Cancelled` / "Timeout expired", not `DeadlineExceeded`.
- `custom_metadata` on unary `SayHello`: the client sends
  `x-grpc-test-echo-initial` and `x-grpc-test-echo-trailing-bin`; the ASCII
  echo is `Response.metadata` headers. Kernel `pbrs_grpc::Response::trailers()`
  carries OK-path custom trailers, and `Streaming::trailers()` waits for
  end-of-stream. `protobuf-tonic` uses Tonic's `Response`, which has no
  `trailers()`, so `x-grpc-test-echo-trailing-bin` is absent on that adapter's
  OK path.
- This is same-process tonic, not official interop, and not a Google peer.
- Gzip is covered in `tests/gzip.rs`.
- Generated stubs expose `with_interceptor` and
  `max_decoding_message_size` / `max_encoding_message_size`
  (`tests/interceptor_size.rs`).
- The `tonic-bench` codec survey uses `proto/codec_cases.proto` against prost
  and v4/upb. Its [recorded results](benchmarks.md) include wins and losses;
  they are historical diagnostics, not current adapter throughput claims.
  CI runs harness correctness checks separately from performance campaigns.

### Benchmarks and native gRPC

- `./bench` fails the process if a gated case loses encode or owned decode to
  prost, v4, or buffa owned.
- The twelve cases are empty, person, tat_populated, packed_256, map_64,
  nested_8, strings, unpacked_256, packed_fixed_256, packed_fixed64_256,
  packed_float_256, and repeated_nested_8.
- View is gated except `tat_populated`, `person`, and packed-fixed rows.
- `pbrs-grpc` is a native HTTP/2 gRPC kernel over pbrs. It is not tonic.
- Official `grpc.testing.TestService` interop binaries
  (`pbrs-grpc-interop-server` / `pbrs-grpc-interop-client`) pass the shared
  uncompressed `_TEST_CASES` against Go `interop/client` and `interop/server`
  (`--use_tls=false`), plus the four gzip cases kernel-vs-kernel.
- Loopback `rpc-bench` latency is process-gated: kernel median ns must be
  strictly below tonic 0.14 on `empty_unary` and `large_unary`.
- Server-streaming and bidirectional ping-pong throughput are gated at 90% of
  tonic 0.14, within the same noise band. Client-streaming upload is gated at
  90% of tonic 0.14 the same way.
- QPS is reported, not gated, for empty/large at conc=1/conns=1 and
  conc=16/conns=4. Nonzero RPC errors still fail the process.
- `scripts/grpc-server-bench.sh` also reports loopback bidirectional ping-pong
  and client-streaming upload against grpc-go, not the Xeon unary tables.
- `Channel::connect_pool` opens independent h2 driver tasks.
- `Channel`, `GreeterClient` / `TestServiceClient`, and `GreeterServer` /
  `TestServiceServer` expose `max_decoding_message_size` /
  `max_encoding_message_size` with 4 MiB inbound by default and unlimited
  outbound by default.
- Oversize encode or decode is `RESOURCE_EXHAUSTED` on every call shape
  (`pbrs-grpc/tests/message_size.rs`).
- These results are not a latency or QPS win claim. `protobuf-tonic` remains
  the Tonic adapter.
- [SB-21](evidence/sb21-claims-scenarios.md) now exports scheduled-send
  histograms and conserved measurement-window accounting, with randomized
  repeats and frozen scenario execution. Reference aggregate arrival-rate
  normalization/independent accounting and a valid two-harness headroom
  smoke remain open; nominal Go offered load is per outstanding slot.
- [SB-24](evidence/sb24-matrix.md) passes 144 local transport wiring cells,
  not a throughput campaign. Native/native TLS sessions used AES-256;
  native/tonic used AES-128; tonic/tonic is configured for AES-128 but lacks
  actual load-session telemetry. Cipher equality, full peer settings,
  Go/C++ parity and dedicated-host resource/headroom evidence remain open.
  This slice did not change the core cipher-policy API.
- [RX-09](evidence/rx-09.md) retains 48 Callgrind captures comparing the
  pre-change source with `9a04f82b`. Within the native workloads, unary/streaming
  instructions fell 9.33%/8.58% and allocated bytes fell 19.07%/15.93%;
  allocation counts were essentially unchanged. Response bytes/counts and
  codec/handler work must be equalized before any beat-tonic claim.
  [SB-23](evidence/sb23-perf-ci.md) retains real failed-build evidence and
  two successful corrected-parser pairs with 88 cells per revision. SB-23 is
  complete; unavailable instructions are excluded. SB-20 thresholds remain
  advisory until 30 comparable artifact runs exist in a compatible host cohort;
  the retained Intel and AMD pairs cannot be pooled.

## Shipped Capabilities and Boundaries

The native gRPC kernel (`pbrs-grpc`) supports all four call shapes and five
transport backends: TCP/h2c, TLS, mTLS, Unix Domain Sockets, and in-process
`from_io`.

### Transport and Security Invariants

- **Certificate verification**: Built-in `ClientTls::webpki` and `ClientTls::ca` constructors verify the server using WebPKI or a configured CA. `ClientTls::from_rustls` retains the caller's verification policy; callers must provide an authenticating verifier. The wrapper cannot certify an opaque verifier; disabling verification is unsupported. There is no built-in skip-verify constructor.
- **ALPN requirement**: every TLS handshake requires ALPN `h2`.
- **Peer information**: mTLS peer identities are exposed through `Rpc::peer_identity`; Unix caller credentials (`SO_PEERCRED`) are exposed through `Rpc::peer_cred`.
- **In-process channels**: `Channel::from_io` provides memory-backed streaming without socket overhead, but does not perform transparent retry or connection pooling.

### Name Resolution, Load Balancing, Retry, and Proxy

- **Direct dialing remains default**: `Channel::connect` and generated `FooClient::connect` dial one `host:port`.
- **Resolver URI channels are opt-in**: `Channel::connect_uri` accepts `dns:`, `passthrough:`, `ipv4:`, `ipv6:`, `unix:`, and `unix-abstract:`. DNS hostnames need explicit `DnsConfig` refresh and retry bounds.
- **Service config adoption**: DNS TXT service configs can be adopted by resolver-managed channels; invalid updates keep the last good document.
- **Load balancing**: resolver-managed channels default to `pick_first`. `loadBalancingConfig` can select `round_robin`, `weighted_round_robin`, `ring_hash`, `least_request`, `random_subsetting_experimental`, `priority`, or `outlier_detection`. These are source- and test-covered, not production-qualified traffic management.
- **Transparent retry**: Sequential calls retry transparently at most once per attempt with proof of non-execution. Hedged attempts do not always retain response commitment when deciding a redial; see the [retry contract](retry-contract.md).
- **Service-config retry**: `Channel::service_config` and resolver service config support `retryPolicy` with backoff, throttling, per-attempt receive timeout, pushback, and retry stats. Unary and server-streaming calls retry before response commitment; client-streaming and bidirectional calls keep only transparent setup redial.
- **Hedging**: Unary `hedgingPolicy` is opt-in and bounded by `maxAttempts`. Another hedge can continue or win after ordinary response headers. Hedging ignores non-negative pushback delays and may delay a scheduled attempt after a non-fatal result; see the [retry contract](retry-contract.md). Streaming hedging is not supported.
- **HTTP CONNECT proxy**: TCP dials consult `HTTPS_PROXY` / `NO_PROXY` and tunnel with CONNECT, including TLS end-to-end through the tunnel. There is no per-channel proxy option.
- **xDS**: xDS target URIs and the xDS control plane are not implemented.

### Messaging and Protocol Limits

- **Message caps**: inbound messages are capped at 4 MiB by default through `max_decoding_message_size`; oversize frames fail with `Code::ResourceExhausted`.
- **Metadata and trailers**: ASCII metadata and binary `-bin` trailers are supported. OK-path custom trailers are available through `Response::trailers` and `Streaming::trailers`.
- **Rich status**: packed `google.rpc.Status` is supported on `grpc-status-details-bin`. When ASCII `grpc-status` headers and binary status details disagree, wire ASCII trailers take precedence.
- **Defensive caps**: rapid reset (CVE-2023-44487) defense uses `ServerConfig::max_pending_accept_reset_streams`; CONTINUATION flood prevention uses `max_header_list_size` (16 KiB); connection recycling uses `max_connection_age` (plus or minus 10% jitter).

### Observability, ORCA, and Authorization

- **channelz**: the registry tracks live channels, subchannels, servers, sockets, and bounded channel traces; `ChannelzService` serves `grpc.channelz.v1`.
- **binary logging**: `binlog::BinaryLogger` attaches to channels, servers, and routers with filters and optional header/message caps; full payload logging is supported. Client logs may include uncommitted attempts or omit early rejection trailers; see the [gRFC coverage](grfc.md).
- **OpenTelemetry**: the optional `otel` feature provides metrics and tracing observers. Implemented metrics are the A66/A94 basics documented in `otel::Metrics`; retry, message-size, full connection, WRR gauge, xDS, and dynamic-label instruments are still incomplete.
- **ORCA**: per-call trailers and out-of-band ORCA service support `weighted_round_robin`.
- **Authorization**: A43 JSON authorization policies can be enforced on `Server` and `Router`; A59 audit logging hooks ship.

### Explicit Omissions and Boundaries

- **xDS protocol**: omitted. xDS target URIs are rejected; xDS-managed routing should terminate at service-mesh ingress or L4/L7 sidecars.
- **Production qualification**: shipped in source and covered by tests is not the same as fleet- or claim-grade qualification.
- **Edition 2024**: scoped descriptor and generated-consumer support is implemented; repeated/map CLOSED enums fail closed and full typed extensions remain open. The [Edition contract](edition-2024.md) defines the subset. Full conformance is still qualified only through Edition 2023.
- **Service-config scope**: client-streaming and bidirectional streaming do not replay request bodies under policy retry; they stay call-site retries with no replay buffer.

For a consolidated cross-framework comparison matrix, see
[docs/guides/comparison.md](guides/comparison.md).

## Unfinished

Tracked in [TODO.md](../TODO.md) and [ROADMAP.md](ROADMAP.md). The notes above
document shipped behavior and explicit omissions; they are not an open work
queue.

Still open: complete Edition 2024 and typed extensions; generated borrowed
views; the full upb-compatible kernel; xDS; remaining format/telemetry
coverage; and sustained production and comparative performance evidence.
The completed QG-05 compatibility recovery and QG-07 guide checks do not
close those qualification gates. CRL and SPIFFE verification, service-config
retries, hedging, channelz, binary logging, ORCA, authorization and optional
OpenTelemetry hooks exist in source. Their limits remain documented in the
[gRFC matrix](grfc.md) and the [audit](audit-2026-09-29.md).

## Skipped rust/test/shared files

- `ctype_cord_test.rs`
- `gtest_matchers_test.rs`
- `no_internal_access_test.rs` (`__internal` is a module)
- `package_disambiguation_test.rs` (empty)
- `extensions_test.rs` (edition 2024 proto)
- edition2023 `str_view` cpp VIEW (ordinary string)
- `proto!` `#[cfg(bzl)]` qualified paths

## Publish

Only [`.github/workflows/release.yml`](../.github/workflows/release.yml)
publishes crates. It runs on `v*` tags or confirmed dispatch after required CI
on that SHA. `main` pushes do not publish.

Crates.io Trusted Publishing exchanges the release workflow's GitHub OIDC
identity for a short-lived `CARGO_REGISTRY_TOKEN`; no long-lived registry
secret is used. External publisher configuration and the hosted provenance
rehearsal remain tracked by QG-09. See [RELEASE.md](RELEASE.md). Do not publish
as `protobuf`. Nearby name `pb-rs` is quick-protobuf.
