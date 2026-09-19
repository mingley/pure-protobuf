# Framework Comparisons: pbrs-grpc, Tonic, and gRPC-Go

This document consolidates architectural invariants, design choices, and functional differences comparing `pbrs-grpc` against other mainstream gRPC implementations: `tonic` (Rust) and `grpc-go` (Go).

---

## 1. Architecture, Runtime & Concurrency Model

| Capability / Concept | `pbrs-grpc` | `tonic` (Rust) | `grpc-go` (Go) |
|---|---|---|---|
| **Underlying HTTP/2 Stack** | Direct, prior-knowledge `h2` engine | `hyper` + `tower` layers | Internal Go HTTP/2 transport |
| **C / C++ Compiler Required** | **None** (pure Rust: rustls + Graviola) | None (default features) | None |
| **Unsafe Code in Kernel** | **Forbidden** (`#![forbid(unsafe_code)]`) | Minimal / hyper internal | Standard Go runtime |
| **Executor Model** | Direct `tokio::spawn` on active runtime | Configurable `SharedExec` | Goroutine per stream / worker pool |
| **Concurrency Limiting** | `ServerConfig::max_concurrent_rpcs` (returns `RESOURCE_EXHAUSTED`) | `tower::limit::ConcurrencyLimitLayer` (queues requests) | Stream semaphore / worker pools |
| **Load Shedding** | Fast failure at capacity bound | `tower::load_shed::LoadShedLayer` | Handlers manage shed |

---

## 2. Transport & Sockets

| Feature | `pbrs-grpc` | `tonic` | `grpc-go` |
|---|---|---|---|
| **Cleartext HTTP/2 (h2c)** | Default (`Channel::connect`) | `Endpoint::from_static("http://...")` | `insecure.NewCredentials()` |
| **TLS & ALPN** | `ClientTls` / `ServerTls` (ALPN `h2` enforced) | `ClientTlsConfig` / `ServerTlsConfig` | `credentials.NewTLS(...)` |
| **Skip-Verify Constructor** | **None** (certificate verification is mandatory) | Custom verifier possible | `InsecureSkipVerify` (supported) |
| **Mutual TLS (mTLS)** | Required client cert via `ServerTls::mtls` | `client_auth_optional` supported | Configurable via TLS ClientAuth |
| **Unix Domain Sockets** | Native `connect_unix` / `serve_unix_unlink` | Custom tower connector or `unix://` | `unix:///path` resolver |
| **In-Process Channels** | `Channel::from_io` / `Server::serve_connection` | Tower service connector | `bufconn.Listen` / custom dialer |
| **HTTP CONNECT Proxy** | Direct TCP only (no proxy traversal) | Hyper HTTP proxy connector | `HTTPS_PROXY` supported by default |

---

## 3. Addressing, Dialing & Connection Management

| Feature | `pbrs-grpc` | `tonic` | `grpc-go` |
|---|---|---|---|
| **Dial Target Format** | `host:port` string or `Target` | `http://` or `https://` URI | `dns:///`, `passthrough:///`, or `xds:///` |
| **Authority Override** | `Channel::origin` / `FooClient::origin` | `Endpoint::origin` (also sets scheme) | `WithAuthority` DialOption |
| **Connection Pooling** | `ChannelConfig::connections` (single authority) | Single channel or tower pool | SubConn pool with resolver |
| **Name Resolvers (DNS / xDS)** | Direct authority dialing only | Tower resolver / basic DNS | Pluggable resolver registry (DNS, xDS) |
| **Wait-For-Ready** | Call-level overlay: retries until ready | `Endpoint::connect_lazy` | `WithBlock` (deprecated) or `WaitForReady` |
| **Max Connection Age** | `ServerConfig::max_connection_age` (±10% jitter) | Hyper max age settings | `KeepaliveParams.MaxConnectionAge` |
| **Keepalive PINGs** | `keep_alive_interval` / `keep_alive_timeout` | `http2_keep_alive_interval` | `KeepaliveParams.Time` / `Timeout` |

---

## 4. Middleware, Interceptors & Overlays

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

`pbrs-grpc` maintains a disciplined scope focused on high-throughput, low-latency, and predictable resource utilization. It deliberately omits dynamic infrastructure layers that introduce non-deterministic state or require heavy external dependencies.

| Feature Area | `pbrs-grpc` Implementation | Rationale for Omission in Kernel |
|---|---|---|
| **Transparent Retries** | **At-most-once**: automatic redial if connection dies before stream commitment. | Bound to single transparent retry to avoid duplicate execution. |
| **Service-Config Retries** | **Omitted**: application retries stay at call sites using `Code::is_retryable`. | Avoids unbounded retry storms and complex JSON service configuration. |
| **Hedging** | **Omitted** | Hedging adds significant network/compute overhead; better handled in gateway proxies. |
| **xDS Protocol** | **Omitted** | Dynamic xDS control planes are best terminated at Envoy / service-mesh sidecars. |
| **Channelz & Binary Logging** | **Omitted** | Observability is provided via tracing extensions and structured metrics. |
| **Dynamic Config Reload** | **Omitted** | Configuration is immutable per server/channel instance; use graceful restart. |
