# Migrating to pure-protobuf

Choose which layer to migrate first: protobuf messages, the gRPC transport,
or both. You can keep Tonic with `pbrs` messages, keep Prost messages on the
native transport, or generate native `pbrs` messages and services together.
The examples below cover those paths and the current limits of Google's
`protobuf` 4.x compatibility.

---

## 1. Migrating from Prost

Start with the trait model. `pbrs` provides its own `Parse`, `Serialize`,
`Clear`, and `Message` traits, shaped after the Google Protobuf v4 application
API. Generated `pbrs` messages do not implement `prost::Message`; these are
also distinct Rust traits from those exported by Google's crate.

### Trait Comparison
| Feature | Prost (`prost`) | pure-protobuf (`pbrs`) |
|---|---|---|
| **Core Trait** | `prost::Message` | `pbrs::Message` |
| **Parsing** | `Message::decode(bytes)` | `Parse::parse(&bytes)` |
| **Serialization** | `Message::encode(&mut buf)` | `Serialize::serialize(&self) -> Result<Vec<u8>, ...>` |
| **Field Presence** | `Option<T>` for optional / message | Accessors: `has_foo()`, `foo()`, `clear_foo()` |
| **Repeated Fields** | `Vec<T>` | `RepeatedView<T>` / `Vec<T>` |
| **String Fields** | Standard `String` | Small-string optimized (SSO <= 23 bytes) |
| **C/C++ Dependencies** | None (pure Rust) | None (pure Rust) |

### Code Migration Example

Replace `prost::Message::decode` / `encode` calls with the `Parse` and
`Serialize` traits from `pbrs::prelude`.

```rust
// Prost pattern:
// use prost::Message;
// let msg = MyMessage::decode(&bytes[..])?;
// let mut out = Vec::new();
// msg.encode(&mut out)?;

// pbrs pattern:
use pbrs::prelude::*;

let msg = MyMessage::parse(&bytes)?;
let out = msg.serialize()?;
```

### Serialized byte order

Generated `pbrs` messages emit known fields in ascending field-number order,
including messages whose implementation moves some fields into cold storage.
Current generated output therefore matches Prost's known-field ordering for
the same schema and values. Older `pbrs` generator output emitted inline fields
before cold fields; after regenerating, byte-keyed caches, fingerprints, and
golden files can change even though both byte strings decode to the same
message. Invalidate or version those artifacts when migrating or regenerating.

Unknown fields are emitted after known fields in capture order. Do not treat
cross-runtime serialization as a canonical fingerprint when unknown fields or
map iteration order can differ.

---

## 2. Using pbrs with Existing Tonic Services

Keep Tonic when you want its routing, transport, and middleware stack. Generate
Tonic-shaped service stubs and use the `protobuf-tonic` codec adapter.

```toml
[dependencies]
pbrs = "0.2"
protobuf-tonic = "0.1.0-alpha.2"
tonic = { version = "0.14", default-features = false, features = ["transport", "codegen"] }
```

In `build.rs`, explicitly configure Tonic stub generation:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(true)
        .compile_protos(&["proto/service.proto"], &["proto"])?;
    Ok(())
}
```

This generates `#[tonic::async_trait]` service stubs. They use `pbrs` message
types through `protobuf-tonic::ProtobufCodec` instead of `prost::Message`.

#### Keep an existing `tonic-prost-build` pipeline

Projects that already configure `tonic-prost-build` can keep that service
generator and redirect its messages and codec to pbrs:

```rust
tonic_prost_build::configure()
    .codec_path("::protobuf_tonic::ProtobufCodec")
    .extern_path(".my.api.v1", "crate::generated::my_api_v1")
    .compile_protos(&["proto/service.proto"], &["proto"])?;
```

Generate `crate::generated::my_api_v1` with `pbrs::codegen` first. Add one
`extern_path` mapping for every protobuf package used by the services,
including imported packages. `codec_path` is global for a generation run, so
every request and response type in that run must be a pbrs type; do not mix
prost and pbrs messages in the same configured run. The
`protobuf-tonic/tests/codec_path.rs` fixture verifies unary and bidirectional
streaming calls generated this way with `tonic-prost-build` 0.14.6.

An application may still use both representations. Run the generator once for
each package or message set: retain the default tonic/prost codec for existing
packages, and set `codec_path` only for pbrs-backed packages. Both generated
service modules can be mounted on the same tonic server. Native services use
the same pattern by combining the normal pbrs generator with
`pbrs::codegen::prost_stubs` (and the `pbrs-grpc/prost` feature).

At a migration boundary, convert wire-compatible representations without a
generated field-by-field adapter:

```rust
let pbrs_request: new_api::Request =
    protobuf_tonic::prost_to_pbrs(&prost_request)?;
let prost_request: old_api::Request =
    protobuf_tonic::pbrs_to_prost(&pbrs_request)?;
```

Each conversion performs one source encode, one target decode, and one owned
wire-buffer allocation. The buffer is moved—not copied—into the decoder; on
the prost-to-pbrs path it becomes pbrs' shared lazy backing storage. See the
[coexistence evidence](../evidence/tc32-coexistence.md) for coverage and current
qualification limits.

This route changes integration cost, not codec performance. Current PK-27
evidence shows that when handlers read every field, pbrs messages cost more CPU
than prost: +10% to +60% instructions per RPC in the measured public profiles,
with larger parse-then-touch gaps for deeply nested and `Any`-heavy messages.
See the [adoption evidence and qualification status](../plan/adoption/README.md)
before choosing a production workload.

### Tonic-shaped API over the native transport

If you want Tonic-shaped handlers but not Tonic's transport stack, generate the
compat mode instead:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::Config::new()
        .tonic_compat(true)
        .compile_protos(&["proto/service.proto"], &["proto"])?;
    Ok(())
}
```

The generated client/server names stay familiar, but they run on
`pbrs-grpc::Channel`, `Server`, and `Router`. The mode uses
`pbrs_grpc::compat::{Request, Response, Status, Streaming}` and accepts
`impl IntoRequest<T>` / `impl IntoStreamingRequest<T>` on client methods.

Mechanical rewrite table:

| Tonic code | Native compat rewrite |
|---|---|
| `use tonic::{Request, Response, Status};` | `use pbrs_grpc::compat::{Request, Response, Status};` |
| `tonic::Streaming<T>` | `pbrs_grpc::compat::Streaming<T>` |
| `tonic::transport::Server::builder().add_service(FooServer::new(svc)).serve(addr)` | `FooServer::new(svc).serve(addr)` or mount on `pbrs_grpc::Server` / `Router` |
| `FooClient::connect("http://host:port").await?` | `FooClient::connect("host:port").await?` |
| `client.unary(Request::new(msg)).await?` | unchanged, with compat `Request` |
| `client.client_stream(tokio_stream::iter(items)).await?` | `client.client_stream(pbrs_grpc::compat::iter(items)).await?` or pass any `futures_core::Stream<Item = T> + Send + 'static` |
| `Response::new(stream)` where `stream: Stream<Item = Result<T, Status>>` | unchanged; generated server stubs convert that stream to native `Streaming<T>` |

### Behavior differences from Tonic 0.14

The table below covers wire and call outcomes that migration code commonly
branches on. “Tonic” means the 0.14.x transport and Prost codec defaults. Test
names are repository integration tests; the `tonic_behavior` documentation
contract fails if a row or its evidence is removed. No compatibility switch is
needed for the current differences, so pbrs-grpc does not expose one.

| Behavior | pbrs-grpc | Tonic 0.14 | Test that pins pbrs-grpc |
|---|---|---|---|
| Inbound message exceeds the decoding limit | Returns `RESOURCE_EXHAUSTED`; the handler does not receive the oversized message. | Returns `RESOURCE_EXHAUSTED`. | `message_size::server_oversize_decode_is_resource_exhausted` |
| Outbound message exceeds the encoding/response decoding limit | The endpoint enforcing its configured limit returns `RESOURCE_EXHAUSTED`. | Returns `RESOURCE_EXHAUSTED` for configured encode/decode limits. | `message_size::server_oversize_encode_is_resource_exhausted` and `message_size::channel_oversize_outbound_is_resource_exhausted` |
| Client timeout | Expiry returns `DEADLINE_EXCEEDED`, including while request flow control is stalled. | Expiry returns `DEADLINE_EXCEEDED`. | `retry_safety::request_send_window_stall_obeys_deadline_for_both_single_request_shapes` |
| Explicit client cancellation | `CallHandle::cancel` returns `CANCELLED`; it is not reported as a deadline. | Dropping/cancelling the request reports `CANCELLED` when a status is observable. | `retry_safety::request_send_window_stall_obeys_cancellation` |
| Empty or missing unary request message | A zero-byte body is accepted as the protobuf default message. A framed zero-length protobuf message is also the default. | A framed zero-length message is the protobuf default, but a body containing no message is rejected as an internal “Missing request message” error. | `hostile::an_empty_body_decodes_to_a_default_message` |
| Trailers-only error | Reads `grpc-status` and percent-decodes `grpc-message` from the initial headers, even when the peer resets the request body early. | Reads trailers-only status from the initial headers. | `retry_safety::trailers_only_rejection_survives_early_request_body_reset` |
| Metadata key normalization | Valid ASCII metadata keys are normalized to lowercase HTTP/2 names; invalid, reserved, or incorrectly suffixed binary keys return `INVALID_ARGUMENT`. | Metadata keys are lowercase on the wire; its typed metadata API likewise rejects invalid names. | `tonic_behavior::metadata_keys_follow_lowercase_http2_rules` and `regress_metadata::regression_bad_metadata_encodings_safely_rejected` |
| `grpc-message` encoding | Emits percent-encoded bytes and decodes percent escapes; spaces therefore arrive as spaces rather than literal `%20`. | Uses the same gRPC percent-encoding rules. | `retry_safety::trailers_only_rejection_survives_early_request_body_reset` |
| Keepalive and GOAWAY | Keepalive PING interval/timeout are explicit channel/server settings. Graceful GOAWAY stops new streams on that connection; a safe unary or server-streaming attempt can redial, while an ambiguous committed attempt is not replayed. | Keepalive is configured on `Endpoint`/`Server`; GOAWAY drains the connection and reconnects for later calls, without replaying a committed request. | `tls::h2c_keepalive_still_serves` and `retry_safety::scenario_c_unary_response_headers_committed_no_retry_on_stream_error` |

The missing-body row is the only semantic difference in this table that can
change an application branch. During a staged migration, validate required
request fields in the handler rather than relying on transport rejection; that
works identically for a default message produced by an empty frame and by a
missing body. Timeout and cancellation remain distinct: use
`Code::DeadlineExceeded` for elapsed deadlines and `Code::Cancelled` for an
explicit caller cancellation.

The repository keeps self-contained ports of Tonic 0.14's examples in
`pbrs-grpc/tests/compat_fixtures.rs`, with copied protos under
`pbrs-grpc/tests/fixtures/compat/`. The copied proto files retain the upstream
gRPC Apache-2.0 headers, and the adapted Rust fixtures retain the Tonic MIT
attribution.

| Upstream Tonic example sources | Compat fixture | Upstream lines | Fixture lines | Diff count |
|---|---|---:|---:|---:|
| `examples/src/helloworld/{server,client}.rs` | `helloworld_compat.rs` | 62 | 57 | +31 / -31 |
| `examples/src/routeguide/{server,client}.rs` | `routeguide_compat.rs` | 315 | 246 | +135 / -173 |

### Tonic public example ports

`examples/tonic-ports/` is a publish=false workspace member that ports tonic's
public examples to two adoption paths from TC-01:

- **(a) tonic-shaped native mode**: pbrs messages generated with
  `Config::tonic_compat(true)`, then served over `pbrs-grpc`.
- **(c) prost messages over native transport**: prost-build messages plus
  `pbrs::codegen::prost_stubs` native clients/servers over `pbrs-grpc`.

Each binary starts a server and client in-process; the crate test runs every
binary's underlying runner. TLS uses the repository test certificates in
`pbrs-grpc/tests/tls_data`. Compression covers gzip by default and zstd when
the example crate's `zstd` feature is enabled.

Line statistics compare the listed upstream tonic 0.14.6 example source files
against the corresponding port section in `examples/tonic-ports/src/lib.rs`
plus its tiny bin wrapper. Shared harness code is intentionally centralized, so
the counts are a migration-diff signal rather than a claim that every line maps
one-to-one.

| Tonic example | Upstream tonic sources | Adoption paths proved | Upstream lines | Port lines | Diff count | Blockers / gaps |
|---|---|---|---:|---:|---:|---|
| Helloworld | `src/helloworld/{server,client}.rs` | tonic_compat + prost-native | 62 | 96 | +83 / -49 | None. |
| RouteGuide | `src/routeguide/{server,client}.rs` | tonic_compat + prost-native; all four RPC shapes | 315 | 363 | +340 / -292 | Uses deterministic in-memory features instead of the JSON route database and random long-running client loop. |
| Streaming | `src/streaming/{server,client}.rs` | tonic_compat + prost-native; server-streaming and bidi, plus unary/client-streaming smoke | 235 | 230 | +217 / -222 | Infinite/throttled client loop is shortened to finite in-process streams for tests. |
| Interceptor | `src/interceptor/{server,client}.rs` | native server and client interceptors with tonic_compat + prost-native handlers | 139 | 96 | +76 / -119 | Native interceptors operate on `Rpc`/`Outgoing`, not tonic's `Request<()>` interceptor wrapper type. |
| Health | `src/health/server.rs` | native health service alongside tonic_compat + prost-native Greeter services | 67 | 41 | +36 / -62 | The tonic sample's status-flipping background loop is reduced to a deterministic `SERVING` check. |
| Reflection | `src/reflection/server.rs` | native reflection service alongside tonic_compat + prost-native Greeter services | 46 | 51 | +46 / -41 | Uses `pbrs-grpc` reflection client to list services, not grpcurl. |
| TLS | `src/tls/{server,client}.rs` | TLS serving/dialing with tonic_compat + prost-native Greeter services | 88 | 38 | +36 / -86 | Uses repo test TLS fixtures; no custom verifier or skip-verify equivalent by design. |
| UDS | `src/uds/{server,client_standard}.rs` | Unix socket serving/dialing with tonic_compat + prost-native Greeter services | 100 | 35 | +31 / -96 | Unix-only; non-Unix runner is a no-op so the crate still builds everywhere. |
| Compression | `src/compression/{server,client}.rs` | gzip with tonic_compat + prost-native; zstd with `--features zstd` | 73 | 62 | +58 / -69 | Zstd is pbrs-grpc-native parity, not a tonic upstream example because tonic has no public zstd example. |
| Richer error details | `src/richer-error/{server,client}.rs` | `google.rpc` details with tonic_compat + prost-native Greeter services | 123 | 95 | +79 / -107 | No gap: `pbrs-grpc` supports typed `ErrorDetails` (`BadRequest`, `Help`, `LocalizedMessage`) through `Status::from_error_details`. |

TC-22 extends the same crate with balancing, custom JSON codecs, tracing,
authentication, cancellation, and Tower examples, listed below. HTTP/1.1
gRPC-Web is covered by [axum co-host](../../examples/axum-cohost/README.md).
The standalone cross-stack harness lives under `tests/interop/tonic`.

| Tonic example | Upstream tonic sources | pbrs-grpc port surface | Upstream lines | Port lines | Diff count | Blockers / gaps |
|---|---|---|---:|---:|---:|---|
| Load balance | `src/load_balance/client.rs` | `Channel::connect_uri` with DNS provider + `loadBalancingConfig` `round_robin` | 29 | 97 | +85 / -17 | Uses pbrs resolver providers instead of tonic `Channel::balance_list`. |
| Dynamic load balance | `src/dynamic_load_balance/client.rs` | Rebuilds resolver-managed channels from changed DNS provider state | 80 | 10 | +8 / -78 | This example does not use the newer `Endpoint::balance_channel` facade, which accepts a watched full list rather than tonic's keyed `Change` updates. |
| JSON codec | `src/json-codec/{server,client,common}.rs` | Custom `CodecMessage` JSON request/response types over native transport | 156 | 97 | +73 / -132 | pbrs-grpc custom codec seam is per message type, not tonic's `Codec` trait/codegen helper shape. |
| Tracing | `src/tracing/{server,client}.rs` | `LifecycleObserver` server call start/end counters | 89 | 47 | +37 / -79 | Demonstrates observer hooks; exact tonic `trace_fn` span factory is still not a native builder API. |
| Authentication | `src/authentication/{server,client}.rs` | Server `Interceptor` checks `authorization`; client `Channel::intercept` inserts it | 68 | 45 | +39 / -62 | Native interceptors use `Rpc` / `Outgoing` instead of tonic `Request<()>`. |
| Cancellation | `src/cancellation/{server,client}.rs` | Handler selects on `Request::cancelled`; client drops via timeout | 115 | 35 | +26 / -106 | Uses native cancellation token from request, not `tokio_util::CancellationToken`. |
| h2c | `src/h2c/{server,client}.rs` | Native prior-knowledge h2c (`Channel::connect` / `Server::serve_listener`) | 218 | 7 | +5 / -216 | Tonic example's HTTP/1.1 Upgrade path remains outside pbrs-grpc, which speaks prior-knowledge HTTP/2. |
| Tower middleware | `src/tower/{server,client}.rs` | `Channel::tower_unary` wrapped in Tower layers | 192 | 18 | +15 / -189 | Server Tower middleware around `Router::into_tower_service` remains documented separately; this runner proves client-side layers. |

Cross-stack executable coverage in `tests/interop/tonic --test example_ports`
now verifies pbrs servers against original tonic clients and pbrs clients
against original tonic servers for:

- helloworld unary;
- routeguide unary, server-streaming, client-streaming, and bidi;
- streaming echo unary, server-streaming, and bidi;
- compression (tonic gzip client ↔ pbrs gzip server, and pbrs gzip-upload client ↔ tonic gzip server when the tonic service enables `accept_compressed(Gzip)`);
- richer error details (tonic client receives packed `grpc-status-details-bin`
  from the pbrs error-details server).

The earlier pbrs-client to tonic-server compressed-upload failure was a tonic
test-server configuration issue: tonic's default server rejects compressed
requests unless the generated service enables
`accept_compressed(CompressionEncoding::Gzip)`. The interop test now covers
both outcomes. With tonic's default server, a pbrs client configured with
`Channel::send_compressed()` still sends gzip and surfaces the server's
`UNIMPLEMENTED`, matching grpc-go's behavior for a caller-forced compressed
request when the peer omits gzip from `grpc-accept-encoding`.

### Tower middleware parity

Enable `pbrs-grpc`'s optional `tower` feature when you want Tonic-style
middleware composition without moving back to Tonic's transport:

```toml
pbrs-grpc = { version = "0.1.0-alpha.2", features = ["tower"] }
```

| Tonic middleware | Native migration path | Delta |
|---|---|---|
| `Server::builder().layer(layer)` | Wrap `Router::into_tower_service()` with `tower::ServiceBuilder` and mount that service in axum/hyper. | The layer sees HTTP requests around the whole router, not a generated per-method service. Native `Interceptor` / `ResponseInterceptor` remain the lighter in-kernel hooks. |
| `Server::builder().trace_fn(...)` | Use `Server::observer`, optional `otel` tracing, or a Tower tracing layer around `Router::into_tower_service()`. | There is no exact in-kernel span factory callback with tonic's `trace_fn` signature. |
| `concurrency_limit_per_connection(n)` | Prefer `ServerConfig::max_concurrent_streams(n)` for per-connection HTTP/2 stream pressure, or a Tower `concurrency_limit` around `RouterService` for service-level pressure. | Native `max_concurrent_rpcs` is process-wide fail-fast; Tower `concurrency_limit` queues unless combined with `load_shed`. |
| `load_shed()` | `ServiceBuilder::new().load_shed().service(router.into_tower_service())`, or native RPC caps returning `RESOURCE_EXHAUSTED`. | Tower load-shed returns a service error before native gRPC trailers; native caps return gRPC status. |
| `timeout(duration)` | `ServerConfig::timeout(duration)` for gRPC deadline semantics, or `tower::timeout` around `RouterService` for outer future timeout semantics. | Native timeout writes/enforces `grpc-timeout`; Tower timeout aborts the service future. |
| `Endpoint::concurrency_limit(n)` | `ChannelConfig::max_concurrent_rpcs(n)` for native fail-fast slots, or wrap `Channel::tower_unary()` in `tower::limit::ConcurrencyLimitLayer`. | `tower_unary` is per unary method and opt-in; default `Channel` remains unbuffered. |
| `Endpoint::rate_limit(...)` | Wrap `Channel::tower_unary()` with `tower::limit::RateLimitLayer`. | Native transport has no built-in token bucket. |
| `Endpoint::buffer_size(n)` | Wrap `Channel::tower_unary()` with `tower::buffer::BufferLayer`. | Buffering is explicit at the tower adapter; the default `Channel` path has no queue. |

### Endpoint-style dialing

Use `pbrs_grpc::Endpoint` when porting code that already builds tonic
`Endpoint` values:

| Tonic client API | Native migration path | Notes |
|---|---|---|
| `Endpoint::from_shared("http://127.0.0.1:50051")?.connect()` | `pbrs_grpc::Endpoint::from_shared("http://127.0.0.1:50051")?.connect().await?` | `http` maps to plaintext `Channel`; `https` maps to WebPKI TLS using the URI host as server name. |
| `Endpoint::connect_with_connector(connector)` | `endpoint.connect_with_connector(|uri| async move { /* return AsyncRead + AsyncWrite */ })` | The connector owns dialing/TLS. `https` stamps `:scheme https`; reconnect semantics still belong in a resolver. |
| `Endpoint::balance_list(endpoints)` | `Endpoint::balance_list(endpoints, ChannelConfig::new()).await?` | Static plaintext TCP endpoint lists feed the existing `round_robin` resolver/LB path. |
| `Endpoint::balance_channel(rx)` | `Endpoint::balance_channel(rx, ChannelConfig::new()).await?` | `rx` is a `watch::Receiver<Vec<Endpoint>>`; updates are observed by the resolver refresh loop. |

---

## 3. Migrating from Google upb (`protobuf` 4.x crate)

Google's official `protobuf` 4.x crate wraps the C-based `upb` kernel through
foreign function interface (FFI). `pbrs` keeps the runtime in Rust.

- **Build complexity**: `protobuf` 4.x requires a C/C++ compiler toolchain.
  `pbrs` is 100% pure Rust and builds with standard `cargo build`.
- **Memory management**: `upb` allocates messages in arenas (`upb_Arena`).
  `pbrs` uses Rust heap allocation with small-string optimizations and
  zero-allocation empty collections.
- **Safety**: `pbrs` removes the C FFI boundary, memory leaks, and
  segmentation faults from unsafe arena lifetimes.

---

## 4. Summary of Code Generation Differences

Choose the generator mode that matches the service stack you want to keep.

| Aspect | `prost-build` | `pbrs::codegen` |
|---|---|---|
| **Stub Selection** | `compile_protos` (tonic stubs optional) | Default: `pbrs-grpc` native kernel stubs; `.emit_tonic_stubs(true)` for Tonic |
| **Descriptor Format** | Bundled or system `protoc` | System `protoc` or bundled descriptor sets |
| **Well-Known Types** | Separate `prost-types` crate | Built-in WKTs in `pbrs::wkt` (`Timestamp`, `Duration`, `Any`, `Empty`, etc.) |
