# pbrs-grpc

A pure-Rust gRPC kernel over [pbrs](../README.md). No `unsafe` in the kernel,
no C or C++ compiled into the build, no tonic. TLS uses rustls with Graviola
(rustc only; no `aws-lc-rs` or `ring`).

```toml
[dependencies]
# until these crates are on crates.io:
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
pbrs-grpc = { git = "https://github.com/mingley/pure-protobuf" }

[build-dependencies]
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
```

```rust
// build.rs
pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])?;
```

That generates a service trait, a server, and a client for every `service` in
your `.proto`. Implement the trait:

```rust
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {}", request.get_ref().name()));
        Ok(Response::new(reply))
    }
}

GreeterServer::new(MyGreeter).serve(addr).await?;
```

Methods you omit answer `UNIMPLEMENTED`.

and call it:

```rust
let client = GreeterClient::connect("127.0.0.1:50051").await?;
let reply = client.say_hello(Request::new(req)).await?;
```

All four call shapes, `Router` for several services, TLS (rustls + Graviola,
no C compiler) and mTLS, `grpc.health.v1`, `grpc.reflection.v1`, interceptors
(server `Rpc`/`Request` metadata/timeout/deadline/`peer_timeout`/`rpc_timeout`/`:authority`/`:scheme`/path/service/method/`local_addr`/`peer_identity`/`peer_cred`/`limits`/`accepts_gzip`/`encoding`/`compresses_outbound`/extensions, client `Outgoing` with path/service/method, `:authority`, `:scheme`, `user-agent` (`user_agent_is_set`), message caps, timeout/deadline Instant (`set_timeout` is the `Call` deadline on every shape), wait-for-ready (`wait_for_ready_is_set`), compression (`compress_is_set`), channel overlays (`rpc_timeout` / `waits_for_ready` / `compresses_outbound` / `gzip_level` / `accepts_compressed` / `concurrent_rpc_limit` / `stream_buffer_size` / `send_buffer_size` / `limits`; `clear_*` opts out of the already-applied default), inbound gzip (`accepts_compressed`; default on), caller and stacked-interceptor extensions; `Err` with `with_error_details` fails the `Call` on every shape and nothing is sent),
received `Response::encoding` (`None` for identity, including an explicit `identity` token; `Some("gzip")` when the peer advertised gzip),
typed `google.rpc.Status` / `ErrorDetails` (`ErrorInfo` / `RetryInfo` / `DebugInfo` / `QuotaFailure` / `PreconditionFailure` / `BadRequest` / `RequestInfo` / `ResourceInfo` / `Help` / `LocalizedMessage`) on `grpc-status-details-bin`,
`Code::is_retryable` / `Status::is_retryable` (gRPC A6: `UNAVAILABLE` only), `Status::error_info` / `ErrorInfo::with_reason` / `ErrorInfo::with_metadata`, `Status::bad_request` / `BadRequest::with_field` builds packed field violations on this crate README, `BadRequest::with_field_entry` builds an extra packed field violation on this crate README, `FieldViolation::with_field` builds a nested field path on this crate README, `FieldViolation::with_reason` builds a nested field-violation reason on this crate README, `FieldViolation::with_localized_message` builds a nested field-violation locale on this crate README, `Status::quota_failure` / `QuotaFailure::with_violation` builds packed quota subjects on this crate README, `QuotaFailure::with_violation_entry` builds an extra packed quota violation on this crate README, `quota_failure::Violation::with_subject` builds a nested quota subject on this crate README, `quota_failure::Violation::with_api_service` builds a nested quota API service on this crate README, `quota_failure::Violation::with_quota_metric` builds a nested quota metric on this crate README, `quota_failure::Violation::with_quota_id` builds a nested quota id on this crate README, `quota_failure::Violation::with_quota_dimension` builds a nested quota dimension pair on this crate README, `quota_failure::Violation::with_quota_value` builds a nested quota value on this crate README, `quota_failure::Violation::with_future_quota_value` builds a nested future quota value on this crate README, `Status::precondition_failure` / `PreconditionFailure::with_violation` builds packed type and subject on this crate README, `PreconditionFailure::with_violation_entry` builds an extra packed precondition violation on this crate README, `precondition_failure::Violation::with_type` builds a nested precondition type on this crate README, `Status::help` / `Help::with_link` builds packed documentation links on this crate README, `Help::with_link_entry` builds an extra packed docs URL on this crate README, `help::Link::with_url` builds a nested docs URL on this crate README, `Status::localized_message` / `LocalizedMessage::with_locale` builds packed locale text on this crate README, `Status::request_info` / `RequestInfo::with_request_id` builds packed request_id on this crate README, `Status::resource_info` / `ResourceInfo::with_resource` builds packed resource type and name on this crate README, `ResourceInfo::with_description` builds a packed resource description on this crate README, `Status::debug_info` / `DebugInfo::with_stack` builds packed operator stack on this crate README, `DebugInfo::with_stack_entry` builds an extra packed stack frame on this crate README, `Status::retry_delay` / `RetryInfo::with_retry_delay`, `Status::from_error` wrapping local errors, `Status::with_cause` attaching `Error::source` onto an existing status, `Status::set_error_details` / `Status::set_from_error_details` replace the protobuf without dropping trailing metadata on this crate README, `Status::with_details` ships raw trailer bytes on this crate README, Distinct from `with_error_details` packing Anys onto a status. `Status::with_rpc` keeps existing trailers on this crate README, Distinct from `from_rpc` minting a fresh status. `pb::Status::with_details` builds a packed `google.rpc.Status` on this crate README, Distinct from `Status::with_details` shipping raw trailer bytes. `ErrorDetails::to_anys` returns the `Any` list on this crate README, Distinct from `from_error_details` encoding the bag as a trailer. `ErrorDetails::from_rpc` unpacks the `Any` list on this crate README, Distinct from `Status::error_details` unpacking a kernel Status trailer. `Any::pack` packs one message into an `Any` on this crate README, Distinct from `with_error_details` packing Anys onto a status. `Any::pack_with` takes an explicit type URL on this crate README, Distinct from `pack` using `type.googleapis.com/<FULL_NAME>`. `Status::set_details` ships raw trailer bytes on this crate README, Distinct from `set_error_details` packing Anys onto a status. `Any::unpack` decodes the payload on this crate README, Distinct from `is` checking the type URL. `Any::is` is a type-URL check on this crate README, Distinct from `unpack` decoding the payload. `ErrorDetails::new` is an empty bag on this crate README, Distinct from `from_rpc` unpacking the `Any` list. `ErrorDetails::with_error_info` plants packed ErrorInfo on this crate README bag. `ErrorDetails::with_retry_info` plants packed RetryInfo on this crate README bag. `ErrorDetails::with_debug_info` plants packed DebugInfo on this crate README bag. `ErrorDetails::with_quota_failure` plants packed QuotaFailure on this crate README bag. `ErrorDetails::with_precondition_failure` plants packed PreconditionFailure on this crate README bag. `ErrorDetails::with_bad_request` plants packed BadRequest on this crate README bag. `ErrorDetails::with_request_info` plants packed RequestInfo on this crate README bag. `ErrorDetails::with_resource_info` plants packed ResourceInfo on this crate README bag. `ErrorDetails::with_help` plants packed Help on this crate README bag. `ErrorDetails::with_localized_message` plants packed LocalizedMessage on this crate README bag. `ErrorDetails::with_unknown` plants a non-standard Any on this crate README bag. `Duration::from_std` builds the protobuf from `std` on this crate README, Distinct from `try_to_std` converting this protobuf to `std`. `Duration::try_to_std` converts this protobuf to `std` on this crate README, Distinct from `from_std` building the protobuf from `std`. `Status::details` returns raw trailer bytes on this crate README, Distinct from `rpc` parsing a packed `google.rpc.Status`. `Status::new` takes a code and message on this crate README, Distinct from `from_code` being code-only. `Status::from_code` is code-only on this crate README, Distinct from `new` taking a code and message. `Status::rpc` parses a packed `google.rpc.Status` on this crate README, Distinct from `details` returning raw trailer bytes. `Status::set_code` mutates in place on this crate README, Distinct from `with_code` being the builder. `Status::with_code` is the builder on this crate README, Distinct from `set_code` mutating in place. `Status::set_message` mutates in place on this crate README, Distinct from `with_message` being the builder. `Status::with_message` is the builder on this crate README, Distinct from `set_message` mutating in place. `Code::from_i32` interprets a wire i32 on this crate README, Distinct from `to_i32` emitting the wire i32. `Code::to_i32` emits the wire i32 on this crate README, Distinct from `from_i32` interpreting a wire i32. `Code::name` is the canonical name on this crate README, Distinct from `description` being the one-line google.rpc.Code text. `Code::description` is the one-line google.rpc.Code text on this crate README, Distinct from `name` being the canonical name. `Status::is_ok` is Code::Ok on this crate README, Distinct from `is_retryable` being UNAVAILABLE only. `Status::code` is the ASCII `grpc-status` code on this crate README, Distinct from `message` being the ASCII `grpc-message`. `Status::message` is the ASCII `grpc-message` on this crate README, Distinct from `code` being the ASCII `grpc-status` code. `Code::is_retryable` is the A6 set on a Code on this crate README, Distinct from `Status::is_retryable` being the same A6 set on a Status. `Status::is_retryable` is the A6 set on a Status on this crate README, Distinct from `Code::is_retryable` being the same A6 set on a Code. `Status::metadata` borrows this status trailers map on this crate README, Distinct from `metadata_mut` mutating it. `Status::metadata_mut` mutates this status trailers map on this crate README, Distinct from `metadata` borrowing it. `ParseCodeError` rejects a string on this crate README, Distinct from `Code::from_i32` mapping an unrecognised wire i32 to `Unknown`. `Status::code` is the ASCII `grpc-status` trailer on this crate README, Distinct from `rpc` being the packed protobuf. `Status::message` is the ASCII `grpc-message` trailer on this crate README, Distinct from `rpc` being the packed protobuf. `Status::rpc` is the packed protobuf on this crate README, Distinct from `code` being the ASCII `grpc-status` trailer. `Status::rpc` is the packed protobuf on this crate README, Distinct from `message` being the ASCII `grpc-message` trailer. `Status::details` returns raw trailer bytes on this crate README, Distinct from `code` being the ASCII `grpc-status` trailer. `Status::details` returns raw trailer bytes on this crate README, Distinct from `message` being the ASCII `grpc-message` trailer. `Status::code` is the ASCII `grpc-status` trailer on this crate README, Distinct from `details` returning raw trailer bytes. `Status::message` is the ASCII `grpc-message` trailer on this crate README, Distinct from `details` returning raw trailer bytes.
There is no `http2_keep_alive_while_idle` setter: once `ChannelConfig::keep_alive_interval` is set, idle connections PING too. Distinct from tonic's `Endpoint::http2_keep_alive_while_idle`, which defaults off. Distinct from grpc-go `PermitWithoutStream`, which is that same idle-PING flag.
There is no grpc-go `EnforcementPolicy` / `MinTime` setter: inbound client PINGs are not GOAWAY'd. Distinct from `ServerConfig::data_frame_budget` (`too_many_data_frames`, not `too_many_pings`). Distinct from `PermitWithoutStream` / tonic `http2_keep_alive_while_idle`.
`ChannelConfig::tcp_keepalive_interval` is `TCP_KEEPINTVL` after idle `tcp_keepalive`. Distinct from `keep_alive_interval`, which sends HTTP/2 PINGs. This does not turn `SO_KEEPALIVE` on by itself. Probe retry count is `tcp_keepalive_retries` (`TCP_KEEPCNT`).
`ChannelConfig::tcp_keepalive_retries` is `TCP_KEEPCNT` after idle `tcp_keepalive`. Distinct from `tcp_keepalive_interval`, which is probe spacing (`TCP_KEEPINTVL`), not how many probes. This does not turn `SO_KEEPALIVE` on by itself.
HTTP/2 PING keepalive, TCP `SO_KEEPALIVE`, max connection age (jittered ±10%) and idle, automatic
redial of a dead connection, lazy connect with wait-for-ready, in-process
`Channel::from_io` / `Server::serve_connection`, Unix domain
sockets (h2c; `serve_unix_unlink` after a crash, without stealing a live listener), graceful drain with `GOAWAY`, per-message gzip, deadlines,
cancellation (dropping a `Call` or a received `Streaming` resets the stream; a `CallHandle` taken before await still cancels while waiting for server-streaming or bidi headers, after streaming headers, and after a client-streaming sender is closed; `StreamSender::fail` on a client request sender resets CANCEL and resolves a client-streaming or pre-headers bidi `Call` with that status, not `UNAVAILABLE` from the reset; after bidi headers the received `Streaming` sees `CANCELLED`, not that status; `Request::cancelled` for spawned work), ASCII and `-bin` metadata, OK-path custom trailers,
mTLS client certificates on `Rpc::peer_identity`, Unix `SO_PEERCRED` on `Rpc::peer_cred`,
`Incoming::peer` / `ConnectionInfo` for custom acceptors, `Channel::https_scheme`
for already-encrypted `from_io` streams, `Channel::origin` / `FooClient::origin` to override `:authority` without changing the dial. Distinct from `Target` (dial) and from `ClientTls` (SNI). Distinct from tonic's `Endpoint::origin`, which takes a `Uri` and also sets `:scheme`. Outbound
RPCs send `user-agent: pbrs-grpc/<version>`; prefix it with `Channel::user_agent`, `Request::set_user_agent`, or `Outgoing::set_user_agent`.
`Target` / `Channel::connect` / `FooClient::connect` take `host:port`, not a tonic `http://` / `https://` URI. Distinct from `Channel::connect_tls` (TLS dial) and from `Channel::origin` (`:authority` overlay). Distinct from tonic's `Endpoint::from_static`, which infers TLS from the URI scheme. A URI-shaped string is `INVALID_ARGUMENT`.
`Target` / `Channel::connect` / `FooClient::connect` take `host:port`, not a grpc-go `dns:///` / `passthrough:///` / `xds:///` resolver URI. Distinct from tonic `http://` / `https://` URIs. `ChannelConfig::connections` pools to one authority; it does not speak xDS. A resolver URI is `INVALID_ARGUMENT`.
`Target` / `Channel::connect` / `FooClient::connect` take `host:port`, not a grpc-go `unix-abstract://` abstract-socket URI. Distinct from tonic `unix://` (also `INVALID_ARGUMENT`). `Channel::connect_unix` takes a filesystem path, not a Linux abstract name. An abstract-socket URI is `INVALID_ARGUMENT`.
`Target` / `Channel::connect` / `FooClient::connect` take `host:port`, not a `grpc://` / `grpcs://` URI. Distinct from tonic `https://` (also `INVALID_ARGUMENT`). `Channel::connect_tls` dials TLS; a `grpcs://` URI is not a silent TLS dial. A `grpc://` URI is `INVALID_ARGUMENT`.
There is no tonic `Endpoint::buffer_size`: that is tower `Buffer` request slots (default 1024), not these bytes. Distinct from `ChannelConfig::stream_buffer` (decoded-message queue depth). Distinct from grpc-go `ReadBufferSize` / `WriteBufferSize`, which are socket byte buffers (default 32 KiB), not this HTTP/2 send buffer.
There is no tonic `Endpoint::rate_limit`: that is tower `RateLimitLayer` (at most N RPCs per duration). Distinct from `ChannelConfig::max_concurrent_rpcs` (in-flight slots). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no tonic `Endpoint::concurrency_limit`: that is tower `ConcurrencyLimitLayer` (wait when `poll_ready` is pending). Distinct from `ChannelConfig::max_concurrent_rpcs` (`RESOURCE_EXHAUSTED` on `try_acquire`, not wait). Distinct from `Endpoint::rate_limit` (token bucket). Distinct from tonic `Server::concurrency_limit_per_connection` (server per-connection wait layer). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no tonic `Endpoint::executor`: that is `SharedExec` on tonic's hyper stack. Distinct from `ChannelConfig::connections` (`tokio::spawn` on the current runtime). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no tonic `Server::executor`: that is `SharedExec` on tonic's hyper stack. Distinct from `ServerConfig::max_concurrent_connections` (`tokio::spawn` on the current runtime). Distinct from tonic `Endpoint::executor` (client `ChannelConfig::connections`). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no grpc-go `NumStreamWorkers`: that is a worker pool for stream dispatch (0 means a goroutine per stream). Distinct from `ServerConfig::max_concurrent_rpcs` (in-flight handler slots). Distinct from tonic `Server::executor` (`SharedExec`, which executor, not a worker pool).
There is no tonic `Server::concurrency_limit_per_connection`: that is tower `ConcurrencyLimitLayer` on each spawned connection. Distinct from `ServerConfig::max_concurrent_rpcs` (process-wide handler slots). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no grpc-go `UnknownServiceHandler`: that is a catch-all bidi handler for unregistered services. Distinct from `Router` (`UNIMPLEMENTED`, not a fallback `Service`). Distinct from `Server` (one service). Distinct from `Service::ALIASES` (a known path alias).
There is no grpc-go `WaitForHandlers`: grpc-go `Stop` can return before handlers exit. Distinct from `Server::serve_with_shutdown` (drain always waits). Distinct from `ServerConfig::max_connection_age_grace` (GOAWAY then force-close). Distinct from `HealthReporter::shutdown` (serving status, not drain).
There is no tonic `Server::load_shed`: that is tower `LoadShedLayer` (fail when `poll_ready` is pending, instead of waiting). Distinct from `ServerConfig::max_concurrent_rpcs` (`RESOURCE_EXHAUSTED` on `try_acquire`, not wait). Distinct from tonic `Server::concurrency_limit_per_connection` (per-connection wait layer). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no grpc-go `SharedWriteBuffer`: that reuses a per-connection transport write buffer after flush. Distinct from `ServerConfig::max_send_buffer_size` (HTTP/2 write-byte backpressure per connection, not a shared pool). Distinct from grpc-go `WriteBufferSize` / `ReadBufferSize` (socket byte buffers). Distinct from tonic `Endpoint::buffer_size` (tower `Buffer` request slots).
There is no tonic `Server::timeout` tower layer: that is `TimeoutLayer` wrapping every request handler. Distinct from `ServerConfig::timeout` (gRPC deadline overlay when the client omits `grpc-timeout`). Distinct from `ChannelConfig::timeout` (client overlay). Distinct from `keep_alive_timeout` (PING ACK). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no tonic `Endpoint::timeout` that omits `grpc-timeout`: that times out the client future without informing the server. Distinct from `ChannelConfig::timeout` (writes `grpc-timeout` when the request omits one). Distinct from `ServerConfig::timeout` (server overlay). Distinct from `connect_timeout` (dial bound). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no grpc-go `ConnectionTimeout`: that is one deadline from accept through HTTP/2 handshake (default 120 s). Distinct from `ServerConfig::handshake_timeout` (20 s on TLS accept and 20 s on the HTTP/2 preface, separately). Distinct from `ChannelConfig::connect_timeout` (client whole dial). Distinct from `ServerConfig::timeout` (RPC deadline overlay). Distinct from `keep_alive_timeout` (PING ACK). Distinct from `max_connection_age` (live connections after handshake).
There is no tonic `Endpoint::connect_with_connector`: that is a tower `Service<Uri>` that still dials. Distinct from `Channel::from_io` (already-connected bytes, no connector and no URI). Distinct from `connect_unix` (filesystem path, not a connector). Distinct from `tower` integration, which is protobuf-tonic keeping tonic.
There is no grpc-go `WithBlock`: that is a DialOption that makes deprecated `Dial` wait until READY. Distinct from `Channel::connect` (already waits for the TCP dial and HTTP/2 preface; no READY state). Distinct from `connect_lazy` (first RPC dials). Distinct from wait-for-ready (RPC queue, not Dial). Distinct from `Channel::connected` (live-socket snapshot). Distinct from `GetState` / `WaitForStateChange`.
There is no grpc-go `WithDisableRetry`: that disables service-config retries and does not impact transparent retries. Distinct from `Code::is_retryable` (application retries at the call site). Distinct from transparent retry (cannot be turned off). Distinct from `from_io` (no transparent retry). Distinct from hedging (not implemented).
There is no tonic `Endpoint::tls_config_with_verifier`: that replaces WebPKI with a custom rustls `ServerCertVerifier`. Distinct from `ClientTls::webpki` (always verifies against Mozilla's CA set). Distinct from `ClientTls::ca` (pin a CA, still verifies). Distinct from a skip-verify constructor (there is none).
There is no tonic `Endpoint::http2_adaptive_window`: that enables hyper adaptive flow control and overrides stream and connection windows. Distinct from `ChannelConfig::initial_stream_window_size` (fixed SETTINGS window). Distinct from `initial_connection_window_size` (connection window, still fixed). Distinct from `data_frame_budget` (`h2 Auto` tiny-DATA budget, not window adaptation). Distinct from tonic `Server::http2_adaptive_window` (server adaptive override).
There is no grpc-go `WithDisableHealthCheck`: that disables LB channel health checking for all SubConns. Distinct from `HealthReporter` (`grpc.health.v1` serving status; `Channel` does not run LB health probes). Distinct from `HealthReporter::shutdown` (serving status, not a DialOption). Distinct from `Server::serve_with_shutdown` (drain wait, not health probes). Distinct from `Channel::connect` (one duplex, no SubConns).
There is no grpc-go `WithDefaultServiceConfig`: that is JSON used when the name resolver does not provide a service config, or when `WithDisableServiceConfig` ignores the resolver. Distinct from `ChannelConfig` (typed `Copy` fields, not JSON; no resolver). Distinct from grpc-go `WithDisableRetry` (`retryPolicy` only). Distinct from `ChannelConfig::timeout` (kernel overlay, not methodConfig timeout). There is no `WithDisableServiceConfig`: nothing to ignore.
There is no grpc-go `WithIdleTimeout` idle mode: that shuts down the name resolver and load balancer after channel idle (default 30 min; zero disables). Distinct from `ChannelConfig::max_connection_idle` (closes the socket; unset by default; sub-millisecond values are raised to 1 ms, not disabled). Distinct from `max_connection_age` (age, not idle). Distinct from `ServerConfig::max_connection_idle` (server GOAWAY). There is no resolver or load balancer to shut down.
There is no grpc-go `WithMaxCallAttempts`: that caps retries and hedging per call (default 5; values below 2 become 5). Distinct from transparent retry (at most once, cannot be raised). Distinct from grpc-go `WithDisableRetry` (on/off of service-config retry, not a count). Distinct from `Code::is_retryable` (application retries at the call site, unbounded by this kernel). Distinct from hedging (not implemented).
There is no grpc-go `WithAuthority`: that sets `:authority` and the TLS authentication server name. Distinct from `Channel::origin` (`:authority` only). Distinct from `ClientTls` (SNI / certificate name). Distinct from tonic `Endpoint::origin` (Uri, also `:scheme`). There is no `CallAuthority`: interceptors cannot override `:authority` per call.
There is no grpc-go `WithConnectParams`: that is exponential reconnect backoff plus `MinConnectTimeout` for creating and maintaining connections. Distinct from `Channel::connect_with` (redials a dead slot on the next RPC with no channel-level reconnect backoff). There is no `WithBackoffMaxDelay` / `WithBackoffConfig` (deprecated aliases). Distinct from `Channel::wait_for_ready` (handshake retries at `[20, 40, 80, 160, 320, 640, 1000]` ms, not channel reconnect). Distinct from `ChannelConfig::connect_timeout` (max dial bound, default 20 s; not grpc-go `MinConnectTimeout`, also default 20 s). Distinct from transparent retry (one redial of the same RPC, not connect backoff).
There is no grpc-go `WithNoProxy`: grpc-go honors `HTTPS_PROXY` by default; that DialOption disables it. Distinct from `Channel::connect` (TCP `host:port` dialed directly; no HTTP CONNECT proxy). There is no `WithLocalDNSResolution`: that resolves locally so the proxy CONNECT sees an IP. Distinct from `Channel::from_io` (already-connected bytes, not a proxy bypass). Distinct from `Channel::connect_unix` (filesystem path; this dialer is skipped). Distinct from `ChannelConfig::local_address` (source bind, not proxy).
There is no grpc-go `WithInsecure`: modern grpc-go `NewClient` requires credentials (`insecure.NewCredentials()` or TLS). Distinct from `Channel::connect` (h2c by default). Distinct from `Channel::connect_tls` (TLS is that constructor plus `ClientTls`). There is no `WithTransportCredentials` DialOption. Distinct from a skip-verify constructor (there is none). Distinct from `Channel::https_scheme` (`from_io` label; it does not handshake).
There is no grpc-go `WithUnaryInterceptor`: that is a DialOption for unary RPCs only. `WithStreamInterceptor` is the stream split. `WithChainUnaryInterceptor` / `WithChainStreamInterceptor` append DialOption lists. Distinct from `ClientInterceptor` (one hook for every call shape, attached with `Channel::intercept` after connect, not a DialOption). Calling intercept twice stacks; there is no chain DialOption. Distinct from `Interceptor` (inbound before the handler). Distinct from `ResponseInterceptor` (after Ok or after receive). Distinct from tonic `Interceptor` / `InterceptorLayer` (tower; this kernel has no tower).
There is no grpc-go `WithDefaultCallOptions`: that is a DialOption bag of per-call options (`WaitForReady`, `MaxCallRecvMsgSize`, compressor, …) applied as channel defaults. Distinct from `Channel` clone overlays (`timeout`, `wait_for_ready`, `send_compressed`, message caps: typed methods, not a `CallOption` list). Distinct from grpc-go `WithDefaultServiceConfig` (JSON service config, not CallOptions). Distinct from `ChannelConfig` (handshake `Copy` fields). Distinct from `Channel::intercept` (per-RPC mutation after connect).
There is no grpc-go `WithCompressor`: that is a DialOption plugging a custom `encoding.Compressor` (deprecated; `encoding.RegisterCompressor` is global). Distinct from `Channel::send_compressed` (gzip on or off, not a compressor plugin). There is no `WithDecompressor` (deprecated inbound plugin). Distinct from encodings other than gzip (`UNIMPLEMENTED`, not a plugin). Distinct from `Channel::gzip_compression_level` (deflate effort, not a plugin). Distinct from grpc-go `UseCompressor` (a CallOption name, not this overlay).
There is no grpc-go `WithContextDialer`: that is a DialOption plugging a custom `func(context.Context, string) (net.Conn, error)` that still dials. `WithDialer` is the deprecated context-less form. Distinct from `Channel::connect_lazy` (first RPC still dials TCP `host:port`; no replacement hook). Distinct from tonic `Endpoint::connect_with_connector` (tower `Service<Uri>` that still dials). Distinct from `Channel::from_io` (already-connected bytes; it does not dial). Distinct from `Channel::connect_unix` (filesystem path, not a custom TCP dialer). Distinct from `ChannelConfig::local_address` (source bind, still this kernel's TCP dialer). Distinct from grpc-go `WithNoProxy` (proxy bypass, not a dial function). Distinct from grpc-go `WithBlock` (handshake wait, not a dial function).
There is no grpc-go `WithPerRPCCredentials`: that is a DialOption plugging `credentials.PerRPCCredentials` that add per-RPC metadata. Distinct from `ClientTls` (transport TLS, not call credentials). There is no `WithCredentialsBundle` (transport plus per-RPC credentials). Distinct from GCP-auth (a library, not this DialOption). Distinct from grpc-go `WithTransportCredentials` (transport; TLS is `Channel::connect_tls` plus `ClientTls`). Distinct from `ClientInterceptor` (user hook that can add metadata, not a credentials plugin). Distinct from `Channel::intercept` (attaches that hook after connect).
There is no tonic `ServerTlsConfig::client_auth_optional`: that requests a client certificate but does not require one. Distinct from `ServerTls::mtls` (always requires a client certificate issued by that CA). Distinct from `ServerTls::new` (clients are not asked). Distinct from a skip-verify constructor (there is none). Distinct from `ClientTls::ca_mtls` / `ClientTls::webpki_mtls` (client presents; this is the server require).
There is no tonic `ServerTlsConfig::timeout`: that is a TLS-handshake-only timeout on the tonic acceptor. Distinct from `ServerTls` (no timeout setter; the bound is `ServerConfig::handshake_timeout`: 20 s TLS accept and 20 s HTTP/2 preface, separately). Distinct from grpc-go `ConnectionTimeout` (one 120 s deadline covering both). Distinct from `ChannelConfig::connect_timeout` (client whole dial). Distinct from tonic `ClientTlsConfig::timeout` (client TLS handshake). Distinct from `ServerConfig::timeout` (RPC deadline overlay).
There is no tonic `ServerTlsConfig::use_key_log`: that enables rustls `KeyLogFile` (`SSLKEYLOGFILE`). Distinct from `ServerTls::new` (does not enable rustls key logging). Distinct from tonic `ClientTlsConfig::use_key_log` (client handshake). Distinct from `ServerTls::mtls` (client cert require, not key log). Distinct from a skip-verify constructor (there is none).
There is no tonic `Server::trace_fn`: that intercepts inbound headers and installs a `tracing::Span` on each response future. Distinct from `Server` (no span installer). Distinct from `Interceptor` (envelope mutation, not a span). Distinct from grpc.stats `Handler` (Begin/End/payload). Distinct from binary logging (`grpc.binarylog.v1`). Distinct from OpenTelemetry. Distinct from tonic `Server::layer` (tower).
A `Router` serves `grpc.reflection.v1alpha.ServerReflection` as a path alias of v1 so older grpcurl still lists. Distinct from a second proto. Distinct from `Server::new`, which already answers that path because it does not look up `Service::NAME`.
`Outgoing::user_agent_is_set` is occupancy on this crate README interceptor path, so a later interceptor can prefix only when unset.
`Outgoing::wait_for_ready_is_set` is occupancy on this crate README interceptor path, so a later interceptor can fill wait-for-ready only when unset.
`Outgoing::compress_is_set` is occupancy on this crate README interceptor path, so a later interceptor can fill compress only when unset.
`Outgoing::clear_user_agent` restores the channel user-agent after a crate README interceptor prefix.
`Outgoing::clear_wait_for_ready` restores the channel wait-for-ready overlay after a crate README interceptor choice.
`Outgoing::clear_compress` then `set_compress` from `compresses_outbound` reapplies channel gzip after a crate README interceptor choice.
`Outgoing::clear_timeout` opts out of the channel timeout after a crate README interceptor choice.
`Outgoing::connected` is the live-socket snapshot on this crate README interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README interceptor Err; a local reject never opens a stream.
Distinct from a crate README handler Err: that is after the handler ran; this crate README interceptor Err is a local reject never opens a stream.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README interceptor Err is a local reject never opens a stream.
Distinct from a crate README server intercept Err: that is trailers without reading the body; this crate README interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README interceptor already ran, so a local Err never consumes that budget.
Distinct from `Server::intercept`: that runs on the inbound RPC before the handler; this crate README Channel intercept runs on the outbound call before the stream opens.
Distinct from `Channel::on_response`: that runs after a successful receive; this crate README Channel intercept runs on the outbound call before the stream opens.
Distinct from `Channel::intercept`: that runs on the outbound call before the stream opens; this crate README server intercept runs on the inbound RPC before the handler.
Distinct from `Server::on_response`: that runs after the handler returns Ok; this crate README server intercept runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README server intercept Err; those trailers reach the client without reading the body.
Distinct from a crate README handler Err: that is after the handler ran; this crate README server intercept Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README server intercept Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README server intercept Err is trailers without reading the body.
Distinct from a crate README interceptor Err: that is a local reject never opens a stream; this crate README server intercept Err is trailers without reading the body.
`Status::from_error_details` is the typed bag after this crate README Health interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README Health handler Err: that is after the handler ran; this crate README Health interceptor Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Health interceptor Err is trailers without reading the body.
Distinct from a crate README Health client interceptor Err: that is a local reject never opens a stream; this crate README Health interceptor Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Health interceptor Err is trailers without reading the body.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README Health interceptor Err is trailers without reading the body.
Distinct from a crate README Health client interceptor: that runs on the outbound call before the stream opens; this crate README Health interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README Health handler Err; those trailers reach the client.
Distinct from a crate README Health interceptor Err: that is trailers without reading the body; this crate README Health handler Err is after the handler ran.
Distinct from a crate README Health client interceptor Err: that is a local reject never opens a stream; this crate README Health handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Health handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Health handler Err is after the handler ran.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README Health handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README Health client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README Health client interceptor Err; a local reject never opens a stream.
Distinct from a crate README Health handler Err: that is after the handler ran; this crate README Health client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Health client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Health interceptor Err: that is trailers without reading the body; this crate README Health client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README Health client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README Health client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README Health interceptor: that runs on the inbound RPC before the handler; this crate README Health client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README Health StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README Health handler Err: that is after the handler ran; this crate README Health StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Health interceptor Err: that is trailers without reading the body; this crate README Health StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Health StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Health client interceptor Err: that is a local reject never opens a stream; this crate README Health StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Health StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README reflection interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README reflection handler Err: that is after the handler ran; this crate README reflection interceptor Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README reflection interceptor Err is trailers without reading the body.
Distinct from a crate README reflection client interceptor Err: that is a local reject never opens a stream; this crate README reflection interceptor Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README reflection interceptor Err is trailers without reading the body.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README reflection interceptor Err is trailers without reading the body.
Distinct from a crate README reflection client interceptor: that runs on the outbound call before the stream opens; this crate README reflection interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README reflection handler Err; those trailers reach the client.
Distinct from a crate README reflection interceptor Err: that is trailers without reading the body; this crate README reflection handler Err is after the handler ran.
Distinct from a crate README reflection client interceptor Err: that is a local reject never opens a stream; this crate README reflection handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README reflection handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README reflection handler Err is after the handler ran.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README reflection handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README reflection client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README reflection client interceptor Err; a local reject never opens a stream.
Distinct from a crate README reflection handler Err: that is after the handler ran; this crate README reflection client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README reflection client interceptor Err is a local reject never opens a stream.
Distinct from a crate README reflection interceptor Err: that is trailers without reading the body; this crate README reflection client interceptor Err is a local reject never opens a stream.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README reflection client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README reflection client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README reflection interceptor: that runs on the inbound RPC before the handler; this crate README reflection client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README reflection StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README reflection handler Err: that is after the handler ran; this crate README reflection StreamSender fail is trailers after any messages already sent.
Distinct from a crate README reflection interceptor Err: that is trailers without reading the body; this crate README reflection StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README reflection StreamSender fail is trailers after any messages already sent.
Distinct from a crate README reflection client interceptor Err: that is a local reject never opens a stream; this crate README reflection StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README reflection StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README Store interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README Store handler Err: that is after the handler ran; this crate README Store interceptor Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Store interceptor Err is trailers without reading the body.
Distinct from a crate README Store client interceptor Err: that is a local reject never opens a stream; this crate README Store interceptor Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Store interceptor Err is trailers without reading the body.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README Store interceptor Err is trailers without reading the body.
Distinct from a crate README Store client interceptor: that runs on the outbound call before the stream opens; this crate README Store interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README Store handler Err; those trailers reach the client.
Distinct from a crate README Store interceptor Err: that is trailers without reading the body; this crate README Store handler Err is after the handler ran.
Distinct from a crate README Store client interceptor Err: that is a local reject never opens a stream; this crate README Store handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Store handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Store handler Err is after the handler ran.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README Store handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README Store client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README Store client interceptor Err; a local reject never opens a stream.
Distinct from a crate README Store handler Err: that is after the handler ran; this crate README Store client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Store client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Store interceptor Err: that is trailers without reading the body; this crate README Store client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README Store client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README Store client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README Store interceptor: that runs on the inbound RPC before the handler; this crate README Store client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README Store StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README Store handler Err: that is after the handler ran; this crate README Store StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Store interceptor Err: that is trailers without reading the body; this crate README Store StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Store StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Store client interceptor Err: that is a local reject never opens a stream; this crate README Store StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Store StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README TestService interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README TestService handler Err: that is after the handler ran; this crate README TestService interceptor Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README TestService interceptor Err is trailers without reading the body.
Distinct from a crate README TestService client interceptor Err: that is a local reject never opens a stream; this crate README TestService interceptor Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README TestService interceptor Err is trailers without reading the body.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README TestService interceptor Err is trailers without reading the body.
Distinct from a crate README TestService client interceptor: that runs on the outbound call before the stream opens; this crate README TestService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README TestService handler Err; those trailers reach the client.
Distinct from a crate README TestService interceptor Err: that is trailers without reading the body; this crate README TestService handler Err is after the handler ran.
Distinct from a crate README TestService client interceptor Err: that is a local reject never opens a stream; this crate README TestService handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README TestService handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README TestService handler Err is after the handler ran.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README TestService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README TestService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README TestService client interceptor Err; a local reject never opens a stream.
Distinct from a crate README TestService handler Err: that is after the handler ran; this crate README TestService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README TestService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README TestService interceptor Err: that is trailers without reading the body; this crate README TestService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README TestService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README TestService client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README TestService interceptor: that runs on the inbound RPC before the handler; this crate README TestService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README TestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README TestService handler Err: that is after the handler ran; this crate README TestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README TestService interceptor Err: that is trailers without reading the body; this crate README TestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README TestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README TestService client interceptor Err: that is a local reject never opens a stream; this crate README TestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README TestService StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README Reverser interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README Reverser handler Err: that is after the handler ran; this crate README Reverser interceptor Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Reverser interceptor Err is trailers without reading the body.
Distinct from a crate README Reverser client interceptor Err: that is a local reject never opens a stream; this crate README Reverser interceptor Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Reverser interceptor Err is trailers without reading the body.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README Reverser interceptor Err is trailers without reading the body.
Distinct from a crate README Reverser client interceptor: that runs on the outbound call before the stream opens; this crate README Reverser interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README Reverser handler Err; those trailers reach the client.
Distinct from a crate README Reverser interceptor Err: that is trailers without reading the body; this crate README Reverser handler Err is after the handler ran.
Distinct from a crate README Reverser client interceptor Err: that is a local reject never opens a stream; this crate README Reverser handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Reverser handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Reverser handler Err is after the handler ran.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README Reverser handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README Reverser client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README Reverser client interceptor Err; a local reject never opens a stream.
Distinct from a crate README Reverser handler Err: that is after the handler ran; this crate README Reverser client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Reverser client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Reverser interceptor Err: that is trailers without reading the body; this crate README Reverser client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README Reverser client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README Reverser client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README Reverser interceptor: that runs on the inbound RPC before the handler; this crate README Reverser client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README Reverser StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README Reverser handler Err: that is after the handler ran; this crate README Reverser StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Reverser interceptor Err: that is trailers without reading the body; this crate README Reverser StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Reverser StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Reverser client interceptor Err: that is a local reject never opens a stream; this crate README Reverser StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Reverser StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README hello interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README hello handler Err: that is after the handler ran; this crate README hello interceptor Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README hello interceptor Err is trailers without reading the body.
Distinct from a crate README hello client interceptor Err: that is a local reject never opens a stream; this crate README hello interceptor Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README hello interceptor Err is trailers without reading the body.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README hello interceptor Err is trailers without reading the body.
Distinct from a crate README hello client interceptor: that runs on the outbound call before the stream opens; this crate README hello interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README hello handler Err; those trailers reach the client.
Distinct from a crate README hello interceptor Err: that is trailers without reading the body; this crate README hello handler Err is after the handler ran.
Distinct from a crate README hello client interceptor Err: that is a local reject never opens a stream; this crate README hello handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README hello handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README hello handler Err is after the handler ran.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README hello handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README hello client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README hello client interceptor Err; a local reject never opens a stream.
Distinct from a crate README hello handler Err: that is after the handler ran; this crate README hello client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README hello client interceptor Err is a local reject never opens a stream.
Distinct from a crate README hello interceptor Err: that is trailers without reading the body; this crate README hello client interceptor Err is a local reject never opens a stream.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README hello client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README hello client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README hello interceptor: that runs on the inbound RPC before the handler; this crate README hello client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README hello StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README hello handler Err: that is after the handler ran; this crate README hello StreamSender fail is trailers after any messages already sent.
Distinct from a crate README hello interceptor Err: that is trailers without reading the body; this crate README hello StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README hello StreamSender fail is trailers after any messages already sent.
Distinct from a crate README hello client interceptor Err: that is a local reject never opens a stream; this crate README hello StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README hello StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README UnimplementedService interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README UnimplementedService handler Err: that is after the handler ran; this crate README UnimplementedService interceptor Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README UnimplementedService interceptor Err is trailers without reading the body.
Distinct from a crate README UnimplementedService client interceptor Err: that is a local reject never opens a stream; this crate README UnimplementedService interceptor Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README UnimplementedService interceptor Err is trailers without reading the body.
Distinct from a crate README UnimplementedService client interceptor: that runs on the outbound call before the stream opens; this crate README UnimplementedService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README UnimplementedService handler Err; those trailers reach the client.
Distinct from a crate README UnimplementedService interceptor Err: that is trailers without reading the body; this crate README UnimplementedService handler Err is after the handler ran.
Distinct from a crate README UnimplementedService client interceptor Err: that is a local reject never opens a stream; this crate README UnimplementedService handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README UnimplementedService handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README UnimplementedService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README UnimplementedService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README UnimplementedService client interceptor Err; a local reject never opens a stream.
Distinct from a crate README UnimplementedService handler Err: that is after the handler ran; this crate README UnimplementedService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README UnimplementedService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README UnimplementedService interceptor Err: that is trailers without reading the body; this crate README UnimplementedService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README UnimplementedService client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README UnimplementedService interceptor: that runs on the inbound RPC before the handler; this crate README UnimplementedService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README InteropTestService interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README InteropTestService handler Err: that is after the handler ran; this crate README InteropTestService interceptor Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README InteropTestService interceptor Err is trailers without reading the body.
Distinct from a crate README InteropTestService client interceptor Err: that is a local reject never opens a stream; this crate README InteropTestService interceptor Err is trailers without reading the body.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README InteropTestService interceptor Err is trailers without reading the body.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README InteropTestService interceptor Err is trailers without reading the body.
Distinct from a crate README InteropTestService client interceptor: that runs on the outbound call before the stream opens; this crate README InteropTestService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README InteropTestService handler Err; those trailers reach the client.
Distinct from a crate README InteropTestService interceptor Err: that is trailers without reading the body; this crate README InteropTestService handler Err is after the handler ran.
Distinct from a crate README InteropTestService client interceptor Err: that is a local reject never opens a stream; this crate README InteropTestService handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README InteropTestService handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README InteropTestService handler Err is after the handler ran.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README InteropTestService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README InteropTestService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README InteropTestService client interceptor Err; a local reject never opens a stream.
Distinct from a crate README InteropTestService handler Err: that is after the handler ran; this crate README InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README InteropTestService interceptor Err: that is trailers without reading the body; this crate README InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README InteropTestService client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README InteropTestService interceptor: that runs on the inbound RPC before the handler; this crate README InteropTestService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README InteropTestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README InteropTestService handler Err: that is after the handler ran; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README InteropTestService interceptor Err: that is trailers without reading the body; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README InteropTestService client interceptor Err: that is a local reject never opens a stream; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
`ResponseParts::compress_is_set` is occupancy on this crate README on_response path, so a later interceptor can fill compress only when unset.
`ResponseParts::clear_compress` restores the server gzip overlay after Server on_response on this crate README on_response path.
`Status::from_error_details` is the typed bag after this crate README server on_response Err; a local reject is trailers-only after handler Ok.
Distinct from a crate README handler Err: that is after the handler ran; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README server intercept Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Health interceptor Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README reflection interceptor Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Store interceptor Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README TestService interceptor Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Reverser interceptor Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README hello interceptor Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README UnimplementedService interceptor Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README InteropTestService interceptor Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README interceptor Err: that is a local reject never opens a stream; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from `Server::intercept`: that runs on the inbound RPC before the handler; this crate README server on_response runs after the handler returns Ok.
`ResponseParts::clear_compress` drops a compress choice after Channel on_response on this crate README on_response path; a received reply has no server gzip overlay to restore.
`Status::from_error_details` is the typed bag after this crate README Channel on_response Err; a local reject fails the Call after a successful receive.
Distinct from a crate README handler Err: that is after the handler ran; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Health client interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Health interceptor Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README reflection client interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README reflection interceptor Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Store client interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Store interceptor Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README TestService client interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README TestService interceptor Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Reverser client interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Reverser interceptor Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README hello client interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README hello interceptor Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README UnimplementedService client interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README UnimplementedService interceptor Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README InteropTestService client interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README InteropTestService interceptor Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README server intercept Err: that is trailers without reading the body; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from `Channel::intercept`: that runs on the outbound call before the stream opens; this crate README Channel on_response runs after a successful receive.
`Streaming` implements
`futures_core::Stream`.

**[Guide](../docs/grpc.md)** — building services, streaming, metadata, errors,
deadlines, lazy connect, compression, interceptors, limits, tuning, testing,
and writing a service without codegen.

## Fast

Measured against tonic 0.14 over loopback on the same service and the same
protobuf codec, so the delta is transport only. Four-core Xeon; see
[benchmarks](../docs/benchmarks.md) for method, variance, and three full runs.

| Axis | Kernel | tonic 0.14 |
|---|---:|---:|
| `empty_unary` p50 | **33-54 µs** | 50-87 µs |
| `empty_unary` p99 | **42-191 µs** | 42 ms |
| `large_unary` p50 | **596-822 µs** | 1.40-1.71 ms |
| Unary QPS, 1 connection | **74k** | 2.0-2.9k |
| Unary QPS, 16 conc / 4 conns | **84-101k** | 12-27k |
| Server-stream, 1 KiB messages | **1041k/s** median | 903k/s median |

Unary latency is process-gated: `rpc-bench` exits non-zero unless the kernel
wins on both p50 and p99. Streaming is gated at 90% of tonic — the kernel leads
by 15% at the median and on five of six runs, but the per-run spread on a
contended machine is wide enough that a strict gate would fail on noise.

Against grpc-go's reference server — one kernel client, two servers in separate
processes, so the server is the only variable — the kernel is about 1.4x on
`empty_unary` p50, 1.7x on its p99, 1.5x on `large_unary` p50, and 1.8x on its
p99, with a few percent spread across rounds:

```bash
./scripts/grpc-server-bench.sh
```

## Safe

The peer is assumed hostile, and every limit is enforced before the memory it
guards is committed: a frame length is refused from the 5-byte header, and a
compressed frame inflates through a reader that stops one byte past the cap.

Defaults: 4 MiB inbound messages, 16 KiB metadata, 256 concurrent streams per
connection, 16 MiB windows. A dial that never completes HTTP/2 fails after 20 s
(`ChannelConfig::connect_timeout`); a mute client is dropped after the same
bound on the server. `tests/hostile.rs` speaks raw HTTP/2 to check them,
sending length prefixes claiming 4 GiB, gzip bombs, reserved flag values,
truncated frames, and malformed paths, then verifying the server still serves.
Property tests add what fixed cases cannot: frames survive arbitrary chunk
boundaries, arbitrary bytes never panic and never exceed the cap, and a
compressed frame never inflates past it.

Every hand-written module carries `#[forbid(unsafe_code)]`, which cannot be
relaxed from inside it. The modules that `include!` generated messages
(`hello`, `testing`, `health`, `reflection`, `pb`) are exempt, because pbrs
gencode uses `unsafe` for zeroed-message construction.

See [the threat model](../docs/grpc.md#limits-and-the-threat-model).

## Scope

h2c by default; TLS is opt-in via `ServerTls` / `ClientTls` (rustls + Graviola,
certificate verification is not optional). No load balancing. Application
retries stay at the call site; unary and server-streaming already redial once
when a connection dies after the slot looked live. See
[what is not here](../docs/grpc.md#what-is-not-here).

`pbrs` does not depend on this crate, and this crate does not depend on tonic
or `protobuf-tonic`. Use `protobuf-tonic` instead if you want to keep an
existing tonic service and only swap in pbrs message types.

## Interop

`grpc.testing.TestService` and the official test cases ship in-tree.
`scripts/grpc-interop.sh` runs them against grpc-go's reference implementation
in both directions, and CI runs the script:

```
kernel client -> kernel server   18 cases
kernel client -> Go server       14 cases
Go client     -> kernel server   14 cases
```

The four cases absent from the cross-language passes are the compression ones;
grpc-go implements `expect_compressed` and `response_compressed` in neither its
client nor its server, so they only run where both ends honour them.

```bash
./scripts/grpc-interop.sh              # all three passes
./scripts/grpc-interop.sh --self-only  # skip the Go peer
```

[`examples/greeter`](../examples/greeter) is a complete user crate: own proto,
`build.rs`, generated stubs, health, and reflection. `pbrs-grpc-hello`
exercises all four call shapes over loopback, and `tests/codegen.rs` compiles
a fresh `.proto` service the way a user's crate does, to keep the generated
`::pbrs_grpc` paths honest.
