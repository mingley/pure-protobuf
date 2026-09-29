# Framework Comparisons: pbrs-grpc, Tonic, and gRPC-Go

Use this guide to choose where `pbrs-grpc` differs from `tonic` and `grpc-go`.
You should already know the shape of the service you want to build.
Bottom line: `pbrs-grpc` favors a small, pure-Rust kernel with explicit limits
and deliberately leaves xDS/control-plane features outside the runtime.

---

## 1. Architecture, Runtime & Concurrency Model

Start here if you are choosing a runtime model or sizing concurrency. The main
difference is that `pbrs-grpc` fails fast at configured limits instead of
queuing unbounded work.

| Capability / Concept | `pbrs-grpc` | `tonic` (Rust) | `grpc-go` (Go) |
|---|---|---|---|
| **Underlying HTTP/2 Stack** | Direct, prior-knowledge `h2` engine | `hyper` + `tower` layers | Internal Go HTTP/2 transport |
| **C / C++ Compiler Required** | **None** (pure Rust: rustls + Graviola) | None (default features) | None |
| **Unsafe Code in Kernel** | Protocol modules forbid unsafe; two Linux-only OS helpers use scoped, documented unsafe | Minimal / hyper internal | Standard Go runtime |
| **Executor Model** | Direct `tokio::spawn` on active runtime | Configurable `SharedExec` | Goroutine per stream / worker pool |
| **Concurrency Limiting** | `ServerConfig::max_concurrent_rpcs` (returns `RESOURCE_EXHAUSTED`) | `tower::limit::ConcurrencyLimitLayer` (queues requests) | Stream semaphore / worker pools |
| **Load Shedding** | Fast failure at capacity bound | `tower::load_shed::LoadShedLayer` | Handlers manage shed |

---

## 2. Transport & Sockets

Use this section to pick a connection path. `pbrs-grpc` supports cleartext
HTTP/2, TLS, mutual TLS (mTLS), Unix sockets, in-process I/O, and env-driven
HTTP CONNECT tunneling.

| Feature | `pbrs-grpc` | `tonic` | `grpc-go` |
|---|---|---|---|
| **Cleartext HTTP/2 (h2c)** | Default (`Channel::connect`) | `Endpoint::from_static("http://...")` | `insecure.NewCredentials()` |
| **TLS & ALPN** | `ClientTls` / `ServerTls` (ALPN `h2` enforced) | `ClientTlsConfig` / `ServerTlsConfig` | `credentials.NewTLS(...)` |
| **Skip-Verify Constructor** | **None** (certificate verification is mandatory) | Custom verifier possible | `InsecureSkipVerify` (supported) |
| **Mutual TLS (mTLS)** | Required client cert via `ServerTls::mtls` | `client_auth_optional` supported | Configurable via TLS ClientAuth |
| **Unix Domain Sockets** | Native `connect_unix` / `serve_unix_unlink` | Custom tower connector or `unix://` | `unix:///path` resolver |
| **In-Process Channels** | `Channel::from_io` / `Server::serve_connection` | Tower service connector | `bufconn.Listen` / custom dialer |
| **HTTP CONNECT Proxy** | `HTTPS_PROXY` / `NO_PROXY` env tunneling; no per-channel config surface | Hyper HTTP proxy connector | `HTTPS_PROXY` supported by default |

---

## 3. Addressing, Dialing & Connection Management

`pbrs-grpc` dials an explicit authority by default. Resolver-managed channels
are opt-in with `Channel::connect_uri`; xDS remains unsupported.

| Feature | `pbrs-grpc` | `tonic` | `grpc-go` |
|---|---|---|---|
| **Dial Target Format** | `host:port` / `Target`; opt-in `dns:`, `passthrough:`, `ipv4:`, `ipv6:`, `unix:`, `unix-abstract:` | `http://` or `https://` URI | `dns:///`, `passthrough:///`, or `xds:///` |
| **Authority Override** | `Channel::origin` / `FooClient::origin` | `Endpoint::origin` (also sets scheme) | `WithAuthority` DialOption |
| **Connection Pooling** | `ChannelConfig::connections`; resolver-managed LB subchannels when opted in | Single channel or tower pool | SubConn pool with resolver |
| **Name Resolvers (DNS / xDS)** | DNS/static resolver registry; no xDS | Tower resolver / basic DNS | Pluggable resolver registry (DNS, xDS) |
| **Load Balancing** | `pick_first` default; `round_robin`, WRR, ring hash, least request, subsetting, priority, outlier detection via `loadBalancingConfig` | Tower/service-specific policy | Resolver/LB policy stack |
| **Wait-For-Ready** | Call-level overlay: retries until ready | `Endpoint::connect_lazy` | `WithBlock` (deprecated) or `WaitForReady` |
| **Max Connection Age** | `ServerConfig::max_connection_age` (±10% jitter) | Hyper max age settings | `KeepaliveParams.MaxConnectionAge` |
| **Keepalive PINGs** | `keep_alive_interval` / `keep_alive_timeout` | `http2_keep_alive_interval` | `KeepaliveParams.Time` / `Timeout` |

---

## 4. Middleware, Interceptors & Overlays

Use interceptors for request validation, outbound metadata, and trailer-time
inspection. Use per-RPC overlays when one call needs a different timeout,
compression setting, user-agent, or wait-for-ready behavior.

| Feature | `pbrs-grpc` | `tonic` | `grpc-go` |
|---|---|---|---|
| **Client Interceptor Point** | `ClientInterceptor` (`Outgoing` mutation) | Tower `Layer` or `Interceptor` | `UnaryClientInterceptor` / `StreamClientInterceptor` |
| **Server Inbound Interceptor**| `Interceptor` (`Rpc` validation) | Tower `Layer` or `Interceptor` | `UnaryServerInterceptor` / `StreamServerInterceptor` |
| **Server Outbound Interceptor**| `ResponseInterceptor` (trailer inspection) | Post-service tower layer | Stream wrapper or interceptor return |
| **Per-RPC Overlays** | `Outgoing` / `Request` setters (`timeout`, etc.) | Request extensions or tower context | `CallOption` bag |
| **Context Extensions** | Typed `Extensions` on request & response | `http::Extensions` on request | `context.Context` values |

---

<a id="omissions"></a>
## 5. Resilience, Retries & Explicit Omissions

`pbrs-grpc` keeps resilience behavior explicit. It provides the bounded retry,
hedging, resolver, and LB behavior shown below and leaves xDS control-plane
policy to gateways or service-mesh components. The runtime scope remains
focused on high-throughput, low-latency, and predictable resource utilization.

| Feature Area | `pbrs-grpc` Implementation | Rationale for Omission in Kernel |
|---|---|---|
| **Transparent Retries** | **At-most-once**: automatic redial if connection dies before stream commitment. | Bound to single transparent retry to avoid duplicate execution. |
| **Service-Config Retries** | **Opt-in**: `retryPolicy` via `Channel::service_config` or resolver service config, with attempts, backoff, throttling, per-attempt timeout, pushback, and retry stats. | Bounded to configured policies; calls without a policy stay at call sites using `Code::is_retryable`. |
| **Hedging** | **Opt-in unary**: bounded `hedgingPolicy`; no streaming hedging. | Speculation is explicit and capped by `maxAttempts` plus throttling. |
| **xDS Protocol** | **Omitted** | Dynamic xDS control planes are best terminated at Envoy / service-mesh sidecars. |
| **Channelz & Binary Logging** | **Shipped, opt-in**: mount `ChannelzService` or attach `BinaryLogger`; optional OTel observers require the `otel` feature. | Observability surfaces stay explicit and bounded. |
| **Dynamic Config Reload** | **Omitted** | Configuration is immutable per server/channel instance; use graceful restart. |

## 6. Benchmark fairness (SB-01)

Use only matched benchmark rows for transport comparisons. `rpc-bench` runs
both peers against one spec: TCP_NODELAY on, 16 MiB HTTP/2 windows, 1 MiB
frames, 256 streams, no adaptive window, and a 16 KiB header list. Those are
the native defaults, and the tonic peer is configured to match them.

Tonic ignores `tcp_nodelay` under `serve_with_incoming`, so the harness sets
it per accepted socket and verifies it with getsockopt. `FAIRNESS {...}`
records store the observed per-endpoint settings, and the harness refuses to
print side-by-side numbers when any endpoint diverges.

Pre-SB-01 tables in `docs/benchmarks.md` ran an unmatched tonic peer and are
superseded.

---

## 7. Feature parity with tonic (TC-08)

This matrix audits tonic 0.14.x's runtime surface from the local
`tonic-0.14.6`, `tonic-health-0.14.6`, `tonic-reflection-0.14.6`, and
`tonic-types-0.14.6` sources. `tonic-web` was not present locally; its row uses
docs.rs for `tonic-web` 0.14.0. Status meanings:

- **Parity**: `pbrs-grpc` has an equivalent runtime capability and test.
- **Partial**: the capability exists but API shape or semantics differ enough
  that a tonic migration may need code or design work.
- **Missing**: no native `pbrs-grpc` equivalent exists.
- **Not applicable**: not a tonic parity surface, or intentionally outside this
  runtime.

### Server builder, routing, and serving

| Tonic surface | `pbrs-grpc` equivalent | Test evidence | Status |
|---|---|---|---|
| `Server::add_service`, `add_optional_service` | [`Server::add_service` / `add_optional_service`](../../pbrs-grpc/src/server/accept.rs), [`Router::add_service` / `add_optional_service`](../../pbrs-grpc/src/server/router.rs) | [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs) generated routing; [`tests/reflection.rs`](../../pbrs-grpc/tests/reflection.rs) multi-service routing | **Parity** |
| `serve`, `serve_with_shutdown`, `serve_with_incoming`, `serve_with_incoming_shutdown` | [`Server::serve`](../../pbrs-grpc/src/server/accept.rs), `serve_until_shutdown`, `serve_with_incoming`, `serve_with_incoming_shutdown`; router variants in [`router.rs`](../../pbrs-grpc/src/server/router.rs) | [`tests/lifecycle.rs`](../../pbrs-grpc/tests/lifecycle.rs) graceful shutdown and cancellation; [`tests/resource_bounds.rs`](../../pbrs-grpc/tests/resource_bounds.rs) bounded drain | **Parity** |
| Existing TCP listener, custom incoming, Unix socket, and in-process serving | [`Incoming`](../../pbrs-grpc/src/server/accept.rs), `serve_listener`, `serve_unix*`, [`serve_connection`](../../pbrs-grpc/src/server/accept.rs) | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs) transport variants; [`tests/lifecycle.rs`](../../pbrs-grpc/tests/lifecycle.rs) `unary_all_boundaries_from_io` | **Parity** |
| `tls_config(ServerTlsConfig)` | TLS is an explicit serving entrypoint: [`Server::serve_tls*`](../../pbrs-grpc/src/server/accept.rs) plus [`ServerTls::new` / `mtls` / `optional_mtls`](../../pbrs-grpc/src/tls.rs). ALPN `h2` and certificate verification are mandatory; `key_log_file()` is opt-in for `SSLKEYLOGFILE`. | [`tests/tls.rs`](../../pbrs-grpc/tests/tls.rs) TLS/mTLS/optional mTLS/key-log opt-in; [`tests/reflection.rs`](../../pbrs-grpc/tests/reflection.rs) TLS reflection | **Partial**: no builder `tls_config`, no skip/custom verifier. |
| `tcp_nodelay(bool)` | TCP_NODELAY is always enabled by the TCP dial/accept paths; there is no off switch. | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs) socket serving; benchmark fairness verifies Nagle-off endpoints. | **Partial**: migration code that intentionally disables Nagle has no equivalent. |
| `tcp_keepalive`, `tcp_keepalive_interval`, `tcp_keepalive_retries` | [`ServerConfig`](../../pbrs-grpc/src/config.rs) and [`Server`](../../pbrs-grpc/src/server/accept.rs) expose all three. | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs) socket options and all-shape serving | **Parity** |
| `http2_keepalive_interval`, `http2_keepalive_timeout` | [`ServerConfig::keep_alive_interval` / `keep_alive_timeout`](../../pbrs-grpc/src/config.rs) and server builder methods. | [`tests/tls.rs`](../../pbrs-grpc/tests/tls.rs) `h2c_keepalive_still_serves` | **Parity** |
| `initial_stream_window_size`, `initial_connection_window_size` | [`ServerConfig`](../../pbrs-grpc/src/config.rs) and server/router/generated setters. | [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs) generated `*_window_size_still_serves_every_shape`; [`tests/resource_bounds.rs`](../../pbrs-grpc/tests/resource_bounds.rs) flow-control bounds | **Parity** |
| `http2_adaptive_window` | [`ServerConfig::adaptive_window(true)`](../../pbrs-grpc/src/config.rs) enables an opt-in BDP-estimating receive-window mode with a configured cap; fixed windows remain the default. | [`tests/adaptive_window.rs`](../../pbrs-grpc/tests/adaptive_window.rs); [`docs/evidence/tc-15-adaptive-window.md`](../evidence/tc-15-adaptive-window.md) | **Partial**: not the default; local evidence shows adaptive improves over fixed-small but the 16 MiB fixed default still wins on loopback large-payload cells. |
| `max_concurrent_streams`, `max_frame_size`, `max_header_list_size`, `http2_max_pending_accept_reset_streams` | [`ServerConfig`](../../pbrs-grpc/src/config.rs) exposes these and additional local-error/reset-memory defenses. | [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs) generated frame/header/window/pending-reset cases; [`tests/hostile.rs`](../../pbrs-grpc/tests/hostile.rs) hostile HTTP/2 caps | **Parity** |
| `concurrency_limit_per_connection` and `load_shed` tower layers | [`ServerConfig::max_concurrent_streams`](../../pbrs-grpc/src/config.rs) caps HTTP/2 streams per connection; [`max_concurrent_rpcs`](../../pbrs-grpc/src/config.rs) rejects excess handler work with `RESOURCE_EXHAUSTED`; optional `tower` wraps [`Router::into_tower_service`](../../pbrs-grpc/src/tower_server.rs) with `ConcurrencyLimitLayer` / `LoadShedLayer`. | [`tests/lifecycle.rs`](../../pbrs-grpc/tests/lifecycle.rs) concurrent RPC cap; [`tests/resource_bounds.rs`](../../pbrs-grpc/tests/resource_bounds.rs) overload rejection; [`tests/tower_server.rs`](../../pbrs-grpc/tests/tower_server.rs) Tower router adapter | **Partial**: native caps are fail-fast gRPC status; Tower layers operate around the whole router service, not per generated method. |
| `timeout` | [`ServerConfig::timeout`](../../pbrs-grpc/src/config.rs) is a gRPC deadline overlay applied when the client omits `grpc-timeout`; optional `tower` can wrap `Router::into_tower_service()` in `TimeoutLayer`. | [`tests/lifecycle.rs`](../../pbrs-grpc/tests/lifecycle.rs) deadline/drain tests; [`tests/policy_retry.rs`](../../pbrs-grpc/tests/policy_retry.rs) method timeout behavior; [`tests/tower_server.rs`](../../pbrs-grpc/tests/tower_server.rs) Tower router adapter | **Partial**: native timeout writes/enforces `grpc-timeout`; Tower timeout aborts the outer service future. |
| `accept_http1(true)` | None. [`Incoming::Io`](../../pbrs-grpc/src/server/accept.rs) must be prior-knowledge HTTP/2; the crate does not speak HTTP/1.1. | No `pbrs-grpc` test because no API exists. | **Missing** |
| `layer(...)` | Optional `tower` exposes [`Router::into_tower_service`](../../pbrs-grpc/src/tower_server.rs), so standard Tower layers can wrap a native router. Native interceptors and observers remain available for in-kernel hooks. | [`tests/tower_server.rs`](../../pbrs-grpc/tests/tower_server.rs) Greeter/health/reflection through the Tower router; [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs) interceptor coverage | **Partial**: layers wrap the whole router HTTP service, not a generated per-method service. |
| `trace_fn(...)` | [`Server::observer`](../../pbrs-grpc/src/server/accept.rs), OpenTelemetry, and binary logging provide telemetry; Tower tracing layers can wrap `Router::into_tower_service()`. | [`tests/telemetry.rs`](../../pbrs-grpc/tests/telemetry.rs), [`tests/otel.rs`](../../pbrs-grpc/tests/otel.rs), [`tests/binlog.rs`](../../pbrs-grpc/tests/binlog.rs) | **Partial**: no exact tonic `trace_fn` callback signature in the native builder. |

### Endpoint, Channel, dialing, and client overlays

| Tonic surface | `pbrs-grpc` equivalent | Test evidence | Status |
|---|---|---|---|
| `Endpoint::connect`, `connect_lazy`; `Channel::connect` | [`Channel::connect`](../../pbrs-grpc/src/client/channel.rs), `connect_with`, `connect_pool`, `connect_lazy`, generated `FooClient::connect*` | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs), [`tests/resolver.rs`](../../pbrs-grpc/tests/resolver.rs) wait-for-ready/lazy recovery | **Parity** |
| `Endpoint::from_shared` URI construction | [`Target`](../../pbrs-grpc/src/client/channel.rs) accepts `host:port`; resolver URI channels use [`Channel::connect_uri`](../../pbrs-grpc/src/client/channel.rs) with [`parse_target_uri`](../../pbrs-grpc/src/resolver/target.rs). | [`tests/resolver.rs`](../../pbrs-grpc/tests/resolver.rs) target parsing and resolver dials | **Partial**: tonic `http://` / `https://` URIs are rejected on `Channel::connect`; TLS and origin are explicit. |
| `timeout` and `connect_timeout` | [`ChannelConfig::timeout`](../../pbrs-grpc/src/config.rs) writes `grpc-timeout`; `connect_timeout` bounds TCP/Unix, TLS, and HTTP/2 SETTINGS. | [`tests/policy_retry.rs`](../../pbrs-grpc/tests/policy_retry.rs), [`tests/retry_safety.rs`](../../pbrs-grpc/tests/retry_safety.rs), [`tests/resolver.rs`](../../pbrs-grpc/tests/resolver.rs) | **Partial**: client timeout is a gRPC deadline, not tonic's outer future timeout. |
| `concurrency_limit`, `rate_limit`, `buffer_size` | [`ChannelConfig::max_concurrent_rpcs`](../../pbrs-grpc/src/config.rs) is a fail-fast in-flight cap; `stream_buffer` controls outbound streaming queue depth; optional `tower` exposes [`Channel::tower_unary`](../../pbrs-grpc/src/tower_client.rs) so `ConcurrencyLimitLayer`, `RateLimitLayer`, and `BufferLayer` can wrap one unary method. | [`tests/resource_bounds.rs`](../../pbrs-grpc/tests/resource_bounds.rs), [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs), [`tests/tower_client.rs`](../../pbrs-grpc/tests/tower_client.rs) | **Partial**: Tower client layers are opt-in per unary method; the default `Channel` path remains unbuffered. |
| `tcp_nodelay`, `tcp_keepalive*`, `local_address` | TCP_NODELAY always on; [`ChannelConfig::tcp_keepalive*`](../../pbrs-grpc/src/config.rs); [`local_address(SocketAddr)`](../../pbrs-grpc/src/config.rs) source-binds TCP dials. | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs) source bind and socket behavior | **Partial**: keepalive/local bind are present; no Nagle-off toggle and local bind takes a full `SocketAddr`. |
| `http2_keep_alive_interval`, `http2_keep_alive_timeout`, `keep_alive_while_idle` | [`ChannelConfig::keep_alive_interval`](../../pbrs-grpc/src/config.rs) sends PINGs while idle and active; timeout is configurable. | [`tests/tls.rs`](../../pbrs-grpc/tests/tls.rs) keepalive coverage | **Partial**: no separate `keep_alive_while_idle(false)` flag once PINGs are enabled. |
| `initial_stream_window_size`, `initial_connection_window_size`, `max_frame_size`, `http2_max_header_list_size` | [`ChannelConfig`](../../pbrs-grpc/src/config.rs) exposes fixed HTTP/2 window, frame, header list, HPACK table, reset-memory, and tiny-DATA budgets. | [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs) generated channel/window/frame coverage; [`tests/hostile.rs`](../../pbrs-grpc/tests/hostile.rs) protocol defenses | **Parity** |
| `http2_adaptive_window` | [`ChannelConfig::adaptive_window(true)`](../../pbrs-grpc/src/config.rs) enables the same opt-in BDP-estimating receive-window mode for client connections; fixed windows remain the default. | [`tests/adaptive_window.rs`](../../pbrs-grpc/tests/adaptive_window.rs); [`docs/evidence/tc-15-adaptive-window.md`](../evidence/tc-15-adaptive-window.md) | **Partial**: not the default; BDP pings share the h2 user `PingPong` with keepalive, but local measurements do not justify replacing the 16 MiB fixed default. |
| `user_agent`, `origin` | [`Channel::user_agent`](../../pbrs-grpc/src/client/channel.rs) prefixes the kernel UA; [`Channel::origin`](../../pbrs-grpc/src/client/channel.rs) overrides `:authority`; `https_scheme` covers `from_io`. | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs) client interceptor/user-agent/origin context | **Partial**: tonic `origin(Uri)` also carries scheme; pbrs keeps authority and scheme separate. |
| `tls_config` | [`ClientTls::webpki`, `webpki_mtls`, `ca`, `ca_mtls`, optional `native_roots`, and `key_log_file`](../../pbrs-grpc/src/tls.rs) plus `Channel::connect_tls*`. | [`tests/tls.rs`](../../pbrs-grpc/tests/tls.rs), [`tests/resolver.rs`](../../pbrs-grpc/tests/resolver.rs) TLS URI failover; `native-roots` is covered by the pure-Rust dependency audit | **Partial**: no custom verifier or `assume_http2`; ALPN `h2` is required. |
| `connect_with_connector` | [`Channel::from_io`](../../pbrs-grpc/src/client/channel.rs) speaks over an already-connected stream; custom acceptors use [`Incoming`](../../pbrs-grpc/src/server/accept.rs). | [`tests/lifecycle.rs`](../../pbrs-grpc/tests/lifecycle.rs) `from_io`; [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs) generated `from_io` all-shape coverage | **Partial**: no tower connector service that owns dialing and redial. |
| `balance_list`, `balance_channel` | Resolver-managed channels support static/IP/DNS/Unix targets and service-config LB policies (`pick_first`, `round_robin`, WRR, ring hash, least request, subsetting, priority, outlier detection). | [`tests/resolver.rs`](../../pbrs-grpc/tests/resolver.rs), [`tests/lb_ring_hash.rs`](../../pbrs-grpc/tests/lb_ring_hash.rs), [`tests/lb_priority.rs`](../../pbrs-grpc/tests/lb_priority.rs), [`tests/lb_health.rs`](../../pbrs-grpc/tests/lb_health.rs) | **Partial**: no tonic `Endpoint` list or dynamic `Sender<Change<...>>` API. |
| `executor` | No custom executor setter; connection and accept work uses `tokio::spawn` on the current runtime. | No `pbrs-grpc` test because no API exists. | **Missing** |

### Envelopes, metadata, status, streaming, compression, and codecs

| Tonic surface | `pbrs-grpc` equivalent | Test evidence | Status |
|---|---|---|---|
| `Request<T>` / `Response<T>` body, metadata, extensions, parts | [`Request`](../../pbrs-grpc/src/request.rs), [`Response`](../../pbrs-grpc/src/request.rs), `Parts`, `ResponseParts`, and re-exported `http::Extensions`. | [`tests/rpc.rs`](../../pbrs-grpc/tests/rpc.rs), [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs), [`tests/gaps.rs`](../../pbrs-grpc/tests/gaps.rs) | **Parity** |
| Request connection facts: `remote_addr`, peer certs, typed connect info | [`Rpc::remote_addr`, `local_addr`, `peer_identity`, `peer_cred`, `scheme`, `authority`](../../pbrs-grpc/src/server/rpc.rs) are stamped onto [`Request`](../../pbrs-grpc/src/request.rs). | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs), [`tests/tls.rs`](../../pbrs-grpc/tests/tls.rs) | **Partial**: facts are direct getters, not tonic `TcpConnectInfo`, `UdsConnectInfo`, and `TlsConnectInfo` extension types. |
| `MetadataMap`, ASCII/binary keys | [`Metadata`](../../pbrs-grpc/src/metadata.rs) / `MetadataMap` supports repeated ASCII keys and base64 `-bin` keys with reserved-key filtering. | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs) metadata and hop-by-hop rejection; [`tests/hostile.rs`](../../pbrs-grpc/tests/hostile.rs) bad metadata encodings | **Parity** |
| Per-call `set_timeout`, wait-for-ready, user-agent, compression | [`Request::set_timeout`, `set_wait_for_ready`, `set_user_agent`, `set_compress`](../../pbrs-grpc/src/request.rs); [`Outgoing`](../../pbrs-grpc/src/request.rs) for client interceptors. | [`tests/resolver.rs`](../../pbrs-grpc/tests/resolver.rs), [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs), [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs) | **Parity** |
| `Status`, `Code`, details, source, and transport error mapping | [`Status`](../../pbrs-grpc/src/status.rs) carries code/message/metadata/details/source; HTTP/2 and I/O mappings attach causes and retry evidence. | [`tests/rpc.rs`](../../pbrs-grpc/tests/rpc.rs) status/details; [`tests/retry_safety.rs`](../../pbrs-grpc/tests/retry_safety.rs) transport mapping; [`tests/binlog.rs`](../../pbrs-grpc/tests/binlog.rs) logged status details | **Parity** |
| `Streaming<T>` | [`Streaming`](../../pbrs-grpc/src/stream.rs), `StreamSender`, `Framed`, `futures_core::Stream`, and fused stream support. | [`tests/rpc.rs`](../../pbrs-grpc/tests/rpc.rs) all four shapes; [`tests/lifecycle.rs`](../../pbrs-grpc/tests/lifecycle.rs) stream cancellation/shutdown | **Parity** |
| Client and server interceptors | [`Channel::intercept`](../../pbrs-grpc/src/client/channel.rs), server/router/service [`intercept`](../../pbrs-grpc/src/server/accept.rs), and response interceptors. | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs), [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs), [`tests/reflection.rs`](../../pbrs-grpc/tests/reflection.rs) | **Parity** for interceptor hooks; tower `Layer` remains missing above. |
| Compression codings: gzip, deflate, zstd | [`compression::Codec`](../../pbrs-grpc/src/compression/mod.rs) supports gzip and deflate by default, plus optional pure-Rust zstd with the `zstd` feature. | [`tests/tls.rs`](../../pbrs-grpc/tests/tls.rs) gzip all shapes; [`tests/compression.rs`](../../pbrs-grpc/tests/compression.rs) zstd C-peer interop and RPC negotiation; [`tests/hostile.rs`](../../pbrs-grpc/tests/hostile.rs) gzip/deflate/zstd hostile inputs; [`tests/resource_bounds.rs`](../../pbrs-grpc/tests/resource_bounds.rs) compression bounds | **Parity** |
| `send_compressed`, `accept_compressed`, max decoding/encoding message size | Server/channel/generated setters map to [`ServerConfig`](../../pbrs-grpc/src/config.rs), [`ChannelConfig`](../../pbrs-grpc/src/config.rs), and `Request`/`Response` overlays. | [`tests/message_size.rs`](../../pbrs-grpc/tests/message_size.rs), [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs), [`tests/resource_bounds.rs`](../../pbrs-grpc/tests/resource_bounds.rs) | **Parity** |
| Generic `Codec`, `Encoder`, and `Decoder` customization | [`CodecMessage`](../../pbrs-grpc/src/codec.rs) is the native message seam. pbrs messages use the fast blanket impl; custom message families can implement it directly, and the optional `prost` feature supplies a prost wrapper. | [`tests/codec_generic.rs`](../../pbrs-grpc/tests/codec_generic.rs), [`tests/prost_codec.rs`](../../pbrs-grpc/tests/prost_codec.rs), [`protobuf-tonic/tests`](../../protobuf-tonic/tests) | **Parity** for native transport customization; API shape differs from tonic's `Codec` trait. |

### Ecosystem services, protocols, TLS, UDS, and shutdown

| Tonic surface | `pbrs-grpc` equivalent | Test evidence | Status |
|---|---|---|---|
| `tonic-health` | [`pbrs_grpc::health`](../../pbrs-grpc/src/health.rs) provides generated health service plus reporter. | [`tests/health.rs`](../../pbrs-grpc/tests/health.rs), [`tests/lb_health.rs`](../../pbrs-grpc/tests/lb_health.rs) | **Parity** |
| `tonic-reflection` v1 and v1alpha | [`pbrs_grpc::reflection`](../../pbrs-grpc/src/reflection.rs) serves v1 and a first-class `grpc.reflection.v1alpha.ServerReflection` service using the wire-compatible reflection message layout. | [`tests/reflection.rs`](../../pbrs-grpc/tests/reflection.rs) service list/file lookup and first-class v1alpha routing | **Parity** |
| `tonic-types` richer error model | [`pb::ErrorDetails`](../../pbrs-grpc/src/pb.rs), `Any`, `google.rpc.Status`, and [`Status::from_error_details`](../../pbrs-grpc/src/status.rs). | [`tests/rpc.rs`](../../pbrs-grpc/tests/rpc.rs), [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs) typed error details on all call shapes | **Parity** |
| `tonic-web` / gRPC-Web | None. `pbrs-grpc` currently speaks native gRPC over prior-knowledge HTTP/2 only. See [TC-09](../decisions/grpc-web-connect.md). | No `pbrs-grpc` test because no API exists. | **Missing** |
| Connect protocol | Not a tonic runtime surface; tonic does not ship Connect support. TC-09 recommends keeping it outside the native parity track. | No `pbrs-grpc` test because this is out of scope. | **Not applicable** |
| TLS roots, SNI/domain override, ALPN, identity/mTLS | [`ClientTls`](../../pbrs-grpc/src/tls.rs) supports WebPKI roots, optional native OS roots, caller CA bundles, SNI/domain override, client identity, mTLS, opt-in key logging, and mandatory ALPN `h2`; [`ServerTls`](../../pbrs-grpc/src/tls.rs) supports TLS, required-client-cert mTLS, optional client auth, and opt-in key logging. | [`tests/tls.rs`](../../pbrs-grpc/tests/tls.rs), [`tests/interop_cli.rs`](../../pbrs-grpc/tests/interop_cli.rs) TLS flag validation; `native-roots` pure-Rust audit | **Partial**: no custom verifier or `assume_http2`; certificate verification is still mandatory. |
| Unix domain sockets | [`Channel::connect_unix*`](../../pbrs-grpc/src/client/channel.rs), [`Server::serve_unix*`](../../pbrs-grpc/src/server/accept.rs), Unix resolver targets, and `PeerCred`. | [`tests/serving.rs`](../../pbrs-grpc/tests/serving.rs), [`tests/reflection.rs`](../../pbrs-grpc/tests/reflection.rs), [`tests/health.rs`](../../pbrs-grpc/tests/health.rs), [`tests/codegen.rs`](../../pbrs-grpc/tests/codegen.rs) | **Parity** |
| Graceful shutdown | [`serve_until_shutdown`, `serve_with_shutdown`, `serve_with_incoming_shutdown`](../../pbrs-grpc/src/server/accept.rs) and router/Unix/TLS variants drain in-flight RPCs and refuse new work. | [`tests/lifecycle.rs`](../../pbrs-grpc/tests/lifecycle.rs), [`tests/resource_bounds.rs`](../../pbrs-grpc/tests/resource_bounds.rs) | **Parity** |
