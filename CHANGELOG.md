# Changelog

## 0.2.1 / 0.1.0-alpha.3 — 2026-11-12

Coordinated release of `pbrs` **0.2.1**, `pbrs-grpc` **0.1.0-alpha.3**, and
`protobuf-tonic` **0.1.0-alpha.3**. Both gRPC crates remain pre-release
previews while sustained soak and comparative qualification continue; see
[implementation status](docs/status.md) and
[release readiness](docs/release-readiness.md).

### pbrs 0.2.1

#### Packaging and runtime profiles

- Split Cargo features into `std`, `codegen`, `reflect`, `json`, `text`, and
  `conformance` (all enabled by default for source compatibility), and gate the
  `protoc-gen-pbrs` and `conformance` binaries with `required-features`.
  Runtime-only consumers using `default-features = false` (optionally adding
  `json` or `text`) skip compiling the code generator, bundled conformance
  bindings, and unused format dependencies.
- Add an explicit `alloc-only` runtime feature and `Config::alloc_only(true)`
  code generation for native binary messages on supported bare-metal targets
  (`target_os = "none"` with 32-bit and pointer compare-and-swap atomics). Host
  code generation stays `std`, and `std` wins when features unify. Reflection,
  JSON/text formats, gRPC transports, and the `rust_out` ABI remain `std`-only;
  see the [alloc-only guide](docs/guides/alloc-only.md).

#### Code generation and storage controls

- Add [`Config::owned_message_fields(true)`](docs/guides/codegen.md#owned-nested-message-fields)
  for eager nested-message materialization on full-traversal workloads, and
  [`Config::boxed(path)`](docs/guides/codegen.md#box-selected-nested-fields)
  for heap-allocating selected singular child messages while preserving standard
  accessors.
- Add [`Config::bytes(paths)`](docs/guides/bytes.md) to back selected `bytes`
  fields with shared `ProtoBytes` (`bytes::Bytes`), valid generated defaults,
  shared payload slicing across matching generated parents, and shared-byte
  merge forwarding through typed mutators.
- Add [`Config::btree_map(paths)`](docs/guides/maps.md) for deterministic,
  key-sorted `TreeMap` storage with typed map views and mutators.
- Add [`Config::safe_message_defaults(true)`](docs/guides/codegen.md#safe-generated-defaults)
  so generated message bindings compile inside `#![forbid(unsafe_code)]` crates
  without unsafe zeroed `Default` initialization.
- Expand `prost-build` and `tonic-build` migration parity on `Config`:
  `extern_path`, `type_attribute`, `message_attribute`, `enum_attribute`,
  `field_attribute` (including cold storage and comma-bearing attributes),
  `client_attribute`, `server_attribute`, `client_mod_attribute`,
  `server_mod_attribute`, `disable_comments`, `disable_comments_for(paths)`,
  `skip_debug`, `retain_enum_prefix`, `enable_type_names`, `type_name_domain`,
  `compile_well_known_types`, `include_file`, `file_descriptor_set_path`,
  `protoc_arg`, `emit_rerun_if_changed`, `build_client`, `build_server`,
  `use_arc_self`, and `codec_path`.
- Add selective metadata and format emission (`emit_reflection`, `emit_json`,
  `emit_text`, `no_reflect`), shared multi-file descriptor pools
  (`shared_descriptor_set(true)`), automatic multi-file `google.protobuf` WKT
  deduplication, Tonic-shaped native stubs (`tonic_compat(true)`), and Prost
  stub generation (`pbrs::codegen::prost_stubs`).
- Qualify generator-owned runtime, framework, and prelude paths so schemas
  naming `Send`, `Arc`, `Default`, `Some`, `T`, or lowercase `pbrs`/`std`/`core`
  compile cleanly; emit `Self_` for `SELF` enum values; reserve `View`/`Mut`
  companion names; resolve `_view` accessor collisions; and add an optional
  sixth `default_view` argument to `impl_generated_message!`.

#### Formats, reflection, and conformance

- Ship `google.protobuf.FieldMask` in `pbrs::wkt` with canonical camelCase
  ProtoJSON parsing and serialization, plus typed `Any::from_msg` and
  `Any::to_msg` helpers.
- Fix ProtoJSON round-trips for nested map messages, including recursive and
  imported children across singular, repeated, map, and `oneof` containers, and
  preserve explicit JSON/text flags through builder serialization.
- Stream text-format parsing and serialization without intermediate syntax
  trees.
- Support a [defined subset of Protocol Buffers Edition 2024](docs/edition-2024.md)
  in descriptors and generated consumers, including fail-closed `CLOSED` enum
  wire validation across singular, repeated, and map fields and selected
  `int32` typed extensions.
- Harden wire and runtime corner cases: preserve proto2 required-field checks
  across message merges while keeping explicit unvalidated merge paths, retain
  `MiniTable` groups and unknown fields, bound parser and `rust_out` arena
  group/map recursion, repair 32-bit (`i686`) lazy string/bytes empty-tag
  defaults, preserve unrelated duplicate-key winners on map removal while
  updating existing lookup indices in place, invalidate packed-field caches
  before fallible custom decode conversions, and reject malformed descriptor
  package/symbol names at load time.

#### Performance and wire runtime

- Add a table-driven wire parser for larger schemas while keeping hot inline
  loops for small messages, inline common varint decoding with architecture-gated
  NEON/SSE2 bulk varint helpers, check canonical one-byte tags directly in
  repeated-field runs, and validate eligible self-recursive messages over
  borrowed slices.
- Adopt a hybrid map layout with sorted unique-key equality for indexed maps,
  transfer owned byte buffers (`Vec<u8>`, `Box<[u8]>`, `Cow::Owned`) without
  payload copies, share parsed descriptor pools process-wide, emit compact Rust
  byte-string literals for `FILE_DESCRIPTOR_SET`, and trim small/medium string
  encode instructions. See [benchmark results](docs/benchmarks.md) and the
  [runtime/codegen measurement record](docs/evidence/runtime-and-codegen-20261029/README.md)
  for measured gains and remaining losses.

### pbrs-grpc 0.1.0-alpha.3

#### Ecosystem, gRPC-Web, and client/server APIs

- Add optional `prost` (`pbrs_grpc::codec::prost`), `tower`
  (`Router::into_tower_service` and `Channel::tower_unary`,
  `tower_server_streaming`, `tower_client_streaming`, `tower_bidi`), and `tonic`
  / `tonic-gzip` (MSRV 1.88) features so existing Prost messages, Tower/axum
  layers, and Tonic services interoperate on the native transport.
- Add the optional `grpc-web` feature for HTTP/2 `application/grpc-web` and
  base64 `application/grpc-web-text` unary and server-streaming calls, with
  bounded text decoding, pre-dispatch compression validation, and opt-in CORS
  preflight and response-header controls (`ServerConfig::grpc_web_allow_origin`,
  `grpc_web_allow_any_origin`, `grpc_web_cors_response_headers`).
- Stamp Tonic-shaped `TcpConnectInfo`, `UdsConnectInfo` (Unix), and
  `TlsConnectInfo` extensions onto incoming requests before interceptors and
  handlers run, alongside existing `Rpc`/`Request` peer accessors.
- Add `Endpoint` (`from_static`, `from_shared`, `connect_lazy`, `origin`,
  `executor`, `balance_list`, `balance_channel`) and `Channel::request_scheme()`,
  preserving per-endpoint executors and full URI authorities across static and
  watched balanced channels while draining replaced subchannels cleanly.
- Add read-only raw metadata accessors (`Metadata::get_raw`,
  `Metadata::get_all_raw`) for inspecting wire `HeaderValue` entries while
  keeping reserved protocol headers hidden, and enforce ASCII case-insensitive
  `-bin` suffix checks on metadata writes and removals.
- Add first-class `grpc.reflection.v1alpha` (`V1AlphaServerReflectionServer` and
  `build_v1alpha`) alongside `v1`, plus per-call and out-of-band ORCA load
  reporting (`pbrs_grpc::orca`).

#### TLS portability, security, and compression

- Scope the built-in `rustls-graviola` provider to `x86_64` and `aarch64` so
  `pbrs-grpc` builds and passes plaintext and caller-configured rustls mTLS CI
  on 32-bit `i686` Linux (Rust 1.85 and stable).
- Add `ServerTls::from_rustls` and `ClientTls::from_rustls` for caller-owned
  rustls configurations and providers (enforcing ALPN `h2`), OS root trust via
  the `native-roots` feature (`ClientTls::native_roots`), CRL revocation checks,
  SPIFFE verification, `ServerTls::optional_mtls`, TLS session resumption, and
  handshake observers.
- Add optional `zstd` message compression via `ruzstd` (`zstd` feature, MSRV
  1.87) and opt-in adaptive HTTP/2 flow-control windows (`adaptive_window`).

#### Retries, deadlines, and lifecycle hardening

- Harden A6 retry and hedging execution: supervise hedge attempts, honor
  invalid and negative `grpc-retry-pushback-ms` trailers, prevent unary and
  server-streaming retries after committed response headers or upload resets,
  map non-200 HTTP and CONNECT proxy errors per the gRPC HTTP fallback table,
  and preserve Channelz hedge stream accounting.
- Record unary client binary-log cancellations from call provenance and record
  consumed `StreamSender::fail` aborts in streaming binary logs without
  allocating cancellation state on unpolled drops.
  [Known hedging and binary-log boundaries](docs/release-readiness.md#retry-and-binary-log-claims)
  remain.
- Round `grpc-timeout` wire values upward when switching units, check local
  deadline and keepalive clock arithmetic for overflow, reject signed timeout
  headers, prioritize ready shutdown signals over queued accepts with Tokio
  cooperative budget consumption, stop A43 authorization policy watcher threads
  when providers drop, maintain one health `Watch` per active round-robin
  connection, and restore `UnwindSafe` / `RefUnwindSafe` on cancellation
  handles.

#### HTTP/2 engine, resource bounds, and performance

- Embed a customized HTTP/2 transport backend with indexed dispatch and
  cancellation wakes, coalesced server connection wakes, static header/status
  fast paths, least-loaded direct pool picking, zero-allocation first-value
  metadata lookup and `traceparent` parsing, and `Bytes::split_to` large-frame
  chunking. See the scoped
  [benchmark comparisons](docs/benchmarks.md#compare-pbrs-with-tonic-and-prost).
- Reuse bounded serialization, gzip output, and decoded stream buffers (capped
  at 4 MiB per buffer and 16 MiB process-wide), reserve up to 1,024 queued
  small DATA events per default client connection (one per stream when no
  explicit `data_frame_budget` is set), and credit discarded small-frame debt
  on subsequent reads.
- Reject overflowing transport byte counters and unrepresentable outbound frame
  lengths, release rejected or reset outbound payload owners outside HTTP/2
  engine locks, repair stream-pointer provenance under strict Miri, exercise
  the shipping HTTP/2 receive codec in CI fuzzing, and require `pbrs` `0.2.1`
  at runtime and build time.

### protobuf-tonic 0.1.0-alpha.3

- Require `pbrs` `0.2.1` at runtime and build time (Tonic `0.14+`, MSRV `1.88`).
- Add `prost_to_pbrs`, `pbrs_to_prost`, and `ConversionError` for single-buffer
  wire conversion between `prost::Message` and `pbrs` message types without
  extra payload copies.
- Reduce `ProtobufDecoder` buffering to a single owned `Bytes` parse, preserve
  peer connection metadata across Tonic service bridges, and build from checked
  descriptor sets without requiring `protoc` for downstream consumers.

### Release qualification

Successful `main` CI runs can rehearse exact-commit crate packaging, CycloneDX
SBOM generation, and GitHub build-provenance attestation automatically without
uploading to crates.io. Tagged publication additionally verifies that the
signed annotated tag targets the exact CI-tested commit and exchanges GitHub's
OIDC identity through crates.io Trusted Publishing; see
[the release policy](docs/RELEASE.md).

Sustained resource and recovery qualification remains open. Supported
boundaries and known gaps are tracked in
[implementation status](docs/status.md) and [the parity record](docs/parity.md).

---

## 0.2.0 / 0.1.0-alpha.2 — 2026-09-28

- **`pbrs` 0.2.0:** Expand public descriptor structures for edition and option
  fidelity, ship bundled conformance descriptor fallbacks so core builds do not
  require `protoc`, and pass official `conformance_test_runner` v35.1 through
  Edition 2023 (5,631 binary/JSON and 909 text cases).
- **`pbrs-grpc` 0.1.0-alpha.2:** Build internal service bindings from checked
  descriptor sets without requiring `protoc`, add `HTTPS_PROXY` / `NO_PROXY`
  HTTP CONNECT tunneling, and align `pbrs` `0.2.0` runtime and build
  requirements.
- **`protobuf-tonic` 0.1.0-alpha.2:** Build from checked descriptor sets without
  requiring `protoc` and update runtime/build dependencies to `pbrs` `0.2.0` on
  Tonic `0.14`.

## 0.1.0 / 0.1.0-alpha.1 — 2026-09-05

- Initial crates.io release of the `pbrs` Protocol Buffers runtime and code
  generator (`0.1.0`), alongside initial preview releases of `pbrs-grpc`
  (`0.1.0-alpha.1`) and `protobuf-tonic` (`0.1.0-alpha.1`).
