# Transport controls

TCP tuning, HTTP/2 keepalive and task scheduling are independent choices.
Configure them before connecting or starting a server.

The endpoint-origin APIs below are additions in the unreleased `pbrs-grpc`
0.1.0-alpha.3 candidate.

## Client configuration

```rust
use std::time::Duration;
use pbrs_grpc::{Channel, ChannelConfig, Endpoint, Status};

async fn connect() -> Result<Channel, Status> {
    Endpoint::from_static("http://127.0.0.1:50051")?
        .with_config(
            ChannelConfig::default()
                .tcp_nodelay(true)
                .keep_alive_interval(Duration::from_secs(30))
                .keep_alive_timeout(Duration::from_secs(10))
                .keep_alive_while_idle(false),
        )
        .connect()
        .await
}
```

| Setting | Default | Effect |
|---|---|---|
| `tcp_nodelay` | `true` | Disables Nagle's algorithm on native TCP connections, including TLS. |
| `keep_alive_interval` | Disabled | Sends HTTP/2 PINGs at the configured interval. |
| `keep_alive_timeout` | 20 seconds | Closes a connection whose PING is not acknowledged in time. |
| `keep_alive_while_idle` | `true` | Allows interval PINGs without an active RPC. |

Idle PINGs default to `true` to preserve existing pbrs behavior; tonic defaults
to `false`. Setting this flag does not enable keepalive by itself. With `false`,
an outstanding response stream still counts as active until it completes or is
dropped. A PING already sent keeps its original acknowledgement deadline even
if the RPC ends.

The idle policy also applies when `adaptive_window(true)` is enabled. Separate
BDP probes triggered by received DATA remain enabled. TCP operating-system
keepalive uses the separate `tcp_keepalive*` settings.

## Request origin and dial target

`Endpoint::origin(uri)` sets the request URI's scheme and authority without
changing dialing, TLS or certificate server-name choices. It takes an
`http::Uri`, supports any valid scheme, and ignores the origin's path and
query so the RPC keeps its method path. An origin missing a scheme or
authority is rejected when an RPC starts, before hooks, encoding or headers.

`Channel::request_scheme()` reports that logical wire scheme.
`Channel::scheme()` and client interceptor security context keep their
transport scheme. A later `Channel::origin(authority)` replaces only the
authority on that clone and preserves the logical scheme. Static balance
helpers reject endpoints with configured origins.

The [endpoint-origin tests](../../pbrs-grpc/tests/endpoint_origin.rs) cover all
four call shapes, eager/lazy/custom connections, TLS/SNI, clone overrides and
Tonic comparisons.

## Server TCP behavior

`ServerConfig::tcp_nodelay(bool)`, `Server::tcp_nodelay(bool)` and
`Router::tcp_nodelay(bool)` control accepted TCP sockets. Generated native server
wrappers forward the same setting. TLS uses the configured socket option too.
Unix sockets and caller-supplied I/O do not receive native TCP tuning.

Server HTTP/2 keepalive retains its existing interval and timeout controls;
`keep_alive_while_idle` is a client setting.

## Connection executors

Use `Endpoint::executor(handle)` to run connection drivers on a chosen Tokio
runtime handle. The selection persists through eager and lazy connections,
reconnections and custom connectors. Connected I/O can use
`Channel::from_io_with_executor`.

Servers, routers and generated native server wrappers also expose
`executor(...)`. Server handlers run inside their connection driver. A custom
scheduler implements the public `Executor` trait and receives an
`ExecutorFuture` to poll or drop. The executor and its Tokio runtime must remain
usable for the whole connection lifetime, including responses that outlive the
last channel handle. A custom executor also overrides connection placement
when using per-core serving; accept loops keep their existing placement.

This selects connection scheduling. Socket I/O, timers and retry, upload,
resolver and keepalive helper tasks still require Tokio. `serve_connection`
remains polled by its caller; an executor setter does not move that
caller-owned future.

`Endpoint::balance_list` and `balance_channel` retain each endpoint's executor.
Their plaintext IP endpoint lists use the full URI as the connection key:
different URIs can dial the same socket with different executors. The last
entry for an identical URI wins. The helpers' `config` argument supplies the
shared connection settings.

The watched helper processes full snapshots directly. Unchanged endpoint
clones retain their connections; a changed executor for the same URI replaces
its connection lazily. Existing RPCs drain within `max_connection_age_grace`.
An empty update removes every peer. An invalid update leaves the previous
valid list active and emits a channelz warning. This differs from tonic's
keyed `Change` API: tonic may use a previous ready entry once while its
replacement becomes ready; native snapshots stop selecting that entry when
the update is processed.

These helpers still use a channel-level request authority and plaintext HTTP
scheme. `Channel::origin` overrides authority for the whole balanced channel;
the selected URI does not supply it as it does in tonic. Per-endpoint origins,
TLS and hostname lookup remain unsupported by these helpers. Use resolver
channels for the existing DNS, Unix and TLS routing APIs.

See the [executor tests](../../pbrs-grpc/tests/custom_executor.rs),
[balanced executor tests](../../pbrs-grpc/tests/balance_executors.rs), and
[transport policy tests](../../pbrs-grpc/tests/transport_policies.rs) for working
examples and lifecycle checks. The [comparison guide](comparison.md) lists the
remaining tonic migration differences.
