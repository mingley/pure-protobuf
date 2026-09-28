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
