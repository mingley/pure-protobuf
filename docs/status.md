# Status

This page separates what is verified from what remains unfinished. It is for contributors checking whether a behavior is shipped, historical, or still a boundary. The bottom line: many core protobuf and gRPC paths are tested, but this page is not a production certification.

## Recovery classification

Interrupted notes labeled most Distinct kernel behavior as Remaining. The
committed source classifies those notes this way:

| Classification | Meaning |
|---|---|
| **Shipped** | The Verified list and the Distinct notes below. This is not a production certification. |
| **Unfinished** | [TODO.md](../TODO.md) and the [leadership plan](ROADMAP.md). GR-01 and GR-02 are checked off there. No arena views, Edition 2024, xDS client/control plane, CRL/SPIFFE, remaining WKT field-wise JSON/text, GR-03+ production qualification, or full grpc.stats / OpenTelemetry coverage. `name_80` leftover remains. Service-config retry/hedging, channelz, binary logging, ORCA, authz, and optional OTel basics ship in source; that is not production certification. |
| **Discarded** | [Closed inventory](inventory/README.md). Do not merge those diffs. `#34` already landed; `#39` flatten and `#57` heap-copy did not. |
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

### Workspace and CI

- `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings`,
  and `cargo test --workspace` pass.
- CI on `main`, PRs, and the release SHA through `workflow_call` runs fmt,
  clippy, tests, docs `-D warnings`, official conformance, grpc-interop,
  `msrv-core`, `msrv-tonic`, macOS `tcp::tests` + onboarding, isolated
  `package_consumer` unpacks, and generated-output drift.
- `msrv-core` uses rustc 1.85 `--lib` for `pbrs` and `pbrs-grpc`.
- `msrv-tonic` uses rustc 1.88 for `protobuf-tonic`.
- The `test` job still apt-installs `protobuf-compiler` for the plugin and
  adapter `build.rs`.

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
- `rust_out_shared` runs official `rust/test/shared` googletest files
  (19 crates, 0 failed) against `protoc --rust_out kernel=upb`. Skips are only
  the files listed below.
- In-tree fuzz covers `tests/fuzz_parse.rs` for empty, truncated, Person, and
  TestAllTypes (TAT) inputs.
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
- Codec survey results live in `tonic-bench` and `docs/benchmarks.md`, using
  `proto/codec_cases.proto` against prost and v4 upb. Typical unary
  `rpc_mixed` is already about 2x prost and beats v4. `name_4kib` combined
  beats prost and is gated. `tags_32` decode beats v4 and is gated. This is not
  kernel `./bench` and is not in CI.

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

## Shipped Capabilities and Boundaries

The native gRPC kernel (`pbrs-grpc`) supports all four call shapes and five
transport backends: TCP/h2c, TLS, mTLS, Unix Domain Sockets, and in-process
`from_io`.

### Transport and Security Invariants

- **Mandatory certificate verification**: in TLS and mTLS, certificate verification is not optional. There is no skip-verify constructor in the kernel; clients verify through `ClientTls::webpki` or pinned CAs through `ClientTls::ca`.
- **ALPN requirement**: every TLS handshake requires ALPN `h2`.
- **Peer information**: mTLS peer identities are exposed through `Rpc::peer_identity`; Unix caller credentials (`SO_PEERCRED`) are exposed through `Rpc::peer_cred`.
- **In-process channels**: `Channel::from_io` provides memory-backed streaming without socket overhead, but does not perform transparent retry or connection pooling.

### Name Resolution, Load Balancing, Retry, and Proxy

- **Direct dialing remains default**: `Channel::connect` and generated `FooClient::connect` dial one `host:port`.
- **Resolver URI channels are opt-in**: `Channel::connect_uri` accepts `dns:`, `passthrough:`, `ipv4:`, `ipv6:`, `unix:`, and `unix-abstract:`. DNS hostnames need explicit `DnsConfig` refresh and retry bounds.
- **Service config adoption**: DNS TXT service configs can be adopted by resolver-managed channels; invalid updates keep the last good document.
- **Load balancing**: resolver-managed channels default to `pick_first`. `loadBalancingConfig` can select `round_robin`, `weighted_round_robin`, `ring_hash`, `least_request`, `random_subsetting_experimental`, `priority`, or `outlier_detection`. These are source- and test-covered, not production-qualified traffic management.
- **Transparent retry**: at most once before stream commitment.
- **Service-config retry**: `Channel::service_config` and resolver service config support `retryPolicy` with backoff, throttling, per-attempt receive timeout, pushback, and retry stats. Unary and server-streaming calls retry before response commitment; client-streaming and bidirectional calls keep only transparent setup redial.
- **Hedging**: unary `hedgingPolicy` ships, bounded by `maxAttempts` and gated by throttling after the first send. Streaming hedging is not applicable.
- **HTTP CONNECT proxy**: TCP dials consult `HTTPS_PROXY` / `NO_PROXY` and tunnel with CONNECT, including TLS end-to-end through the tunnel. There is no per-channel proxy option.
- **xDS**: xDS target URIs and the xDS control plane are not implemented.

### Messaging and Protocol Limits

- **Message caps**: inbound messages are capped at 4 MiB by default through `max_decoding_message_size`; oversize frames fail with `Code::ResourceExhausted`.
- **Metadata and trailers**: ASCII metadata and binary `-bin` trailers are supported. OK-path custom trailers are available through `Response::trailers` and `Streaming::trailers`.
- **Rich status**: packed `google.rpc.Status` is supported on `grpc-status-details-bin`. When ASCII `grpc-status` headers and binary status details disagree, wire ASCII trailers take precedence.
- **Defensive caps**: rapid reset (CVE-2023-44487) defense uses `ServerConfig::max_pending_accept_reset_streams`; CONTINUATION flood prevention uses `max_header_list_size` (16 KiB); connection recycling uses `max_connection_age` (plus or minus 10% jitter).

### Observability, ORCA, and Authorization

- **channelz**: the registry tracks live channels, subchannels, servers, sockets, and bounded channel traces; `ChannelzService` serves `grpc.channelz.v1`.
- **binary logging**: `binlog::BinaryLogger` attaches to channels, servers, and routers with explicit filters and header/message caps.
- **OpenTelemetry**: the optional `otel` feature provides metrics and tracing observers. Implemented metrics are the A66/A94 basics documented in `otel::Metrics`; retry, message-size, full connection, WRR gauge, xDS, and dynamic-label instruments are still incomplete.
- **ORCA**: per-call trailers and out-of-band ORCA service support `weighted_round_robin`.
- **Authorization**: A43 JSON authorization policies can be enforced on `Server` and `Router`; A59 audit logging hooks ship.

### Explicit Omissions and Boundaries

- **xDS protocol**: omitted. xDS target URIs are rejected; xDS-managed routing should terminate at service-mesh ingress or L4/L7 sidecars.
- **Production qualification**: shipped in source and covered by tests is not the same as fleet- or claim-grade qualification.
- **Edition 2024**: Edition 2024 is currently untested and unsupported; conformance covers up to Edition 2023.
- **Service-config scope**: client-streaming and bidirectional streaming do not replay request bodies under policy retry; they stay call-site retries with no replay buffer.

For a consolidated cross-framework comparison matrix, see
[docs/guides/comparison.md](guides/comparison.md).

## Unfinished

Tracked in [TODO.md](../TODO.md) and [ROADMAP.md](ROADMAP.md). The notes above
document shipped behavior and explicit omissions; they are not an open work
queue.

Still not done: arena views, Edition 2024, `name_80` leftover, xDS
client/control plane, CRL/SPIFFE, full grpc.stats / OpenTelemetry coverage,
remaining WKT field-wise JSON/text, and GR-03+. Service-config retries,
hedging, channelz, binary logging, ORCA, authz, and optional OTel basics ship
in source and tests. Do not treat a clean checkout as production
certification.

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

Credential: repository secret `CRATES_IO_TOKEN` (`CARGO_REGISTRY_TOKEN`). This
is not Trusted Publishing because `id-token: write` is not set. See
[RELEASE.md](RELEASE.md). Do not publish as `protobuf`. Nearby name `pb-rs` is
quick-protobuf.
