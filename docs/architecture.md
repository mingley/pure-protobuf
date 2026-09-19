# Architecture

pbrs is a protobuf kernel: parse, serialize, reflection, JSON, text, and
plugin codegen. There is no upb, no libprotobuf, and no C.

## Crates

| crate | role |
|---|---|
| `pbrs` | protobuf kernel, `protoc-gen-pbrs`, conformance child |
| `protobuf-tonic` | tonic 0.14 `Codec` and generated `FooClient` / `FooServer` |
| `pbrs-grpc` | HTTP/2 gRPC kernel over pbrs (not tonic) |

The protobuf kernel has no tonic, h2, or hyper dependency. `pbrs-grpc` has no tonic dependency. `protobuf-tonic` has no `pbrs-grpc` dependency. A consumer can use pbrs alone, pbrs plus the tonic adapter, or pbrs plus the gRPC kernel.

The Cargo package and the library are both named `pbrs`
(`use pbrs::prelude::*`). The GitHub repo is `mingley/pure-protobuf`.

## gRPC kernel

`pbrs-grpc` speaks gRPC over prior-knowledge HTTP/2. Hand-written modules
forbid `unsafe`. Generated messages still use pbrs `unsafe` for zeroed
construction. There is no C compiler in the build (TLS is rustls +
Graviola; gzip is `miniz_oxide`).

### Accept

TCP (`serve` / `serve_listener`), TLS (`serve_tls*`), Unix (`serve_unix*`),
a single already-accepted stream (`serve_connection`), or a custom
`Incoming`. The TCP/TLS loops apply `TCP_NODELAY`, optional
`SO_KEEPALIVE`, and — on mTLS — the verified client chain on
`Rpc::peer_identity`. Unix fills `SO_PEERCRED` on `Rpc::peer_cred` and
reports `:scheme` `http`. `Incoming::accept` yields
`(Io, Option<SocketAddr>)`. Other connection facts go on
`Incoming::peer` as a `ConnectionInfo` (local address, identity,
credentials, transport scheme). Those facts are copied onto every call
shape on that connection. TLS reports `:scheme` `https` and, on mTLS, the
verified client chain on every call shape. The default copies the accept address
and does not probe `Io`. `serve_connection` leaves those fields unset
on `Rpc`, and generated handlers see the same empty facts on `Request`
and `Parts` (the peer's `:scheme` / `:authority` still apply, including
after `https_scheme`).

Connection lifecycle is governed by explicit configuration:
- `Server::max_connection_age` and `max_connection_idle` send `GOAWAY`; the next RPC redials.
- `Server::max_concurrent_connections` caps active connection count.
- `ServerConfig::handshake_timeout` drops mute peers that fail to complete TLS or the HTTP/2 preface.
- `ServerConfig::max_concurrent_rpcs` guards handler dispatch, returning `RESOURCE_EXHAUSTED` when capacity is full.
- `Server::serve_with_shutdown` coordinates graceful drain, allowing active RPCs to finish before closing the listener.

### Dispatch

`Service::call` receives an `Rpc`. Generated `FooServer` implements
`Service`; application code implements the `Foo` trait. Consume `Rpc` with
exactly one of `unary`, `client_streaming`, `server_streaming`,
`bidi_streaming`, or `unimplemented`.

Interceptors execute prior to service logic and can inspect:
- Path, service name, and method name
- Metadata headers and binary trailers
- Deadlines and timeouts
- Peer identity and credentials (`peer_identity`, `peer_cred`)
- Compression and message size limits
- Request extensions for sharing typed context

`Router` routes based on the service name component of the path. Unregistered
services answer `Code::Unimplemented`. Generated methods that are omitted
similarly return `Code::Unimplemented`.

Handlers that perform asynchronous work can monitor `Request::cancelled()`
to react immediately to client disconnects, timeouts, or stream resets.

### Wire

Length-prefixed protobuf frames on `h2`. Inbound decode is inline on the
handler task (`WireStream`). Outbound batches (`OutBatch`) so one DATA
frame can carry many messages. Encode-cap failures on a stream are producer
status (`RESOURCE_EXHAUSTED` trailers), not a transport reset. gzip is optional
and never sent to a peer that omitted it from `grpc-accept-encoding`. Inbound
gzip is on by default; `accept_compressed(false)` refuses it. Caps (4 MiB
inbound default, 16 KiB header list, 256 concurrent streams, rapid reset,
connection age/idle) are enforced before the memory they guard is committed.

### Client

`Channel` pools HTTP/2 connections to one authority (`Target`). Client interceptors
inspect and modify `Outgoing` request context (authority, scheme, user-agent,
timeouts, wait-for-ready overlays, compression settings, and metadata).

Key client architectural characteristics:
- **Addressing**: Dials direct authorities (`host:port`); authority can be overridden via `origin` without altering the TCP dial.
- **Connection Recovery**: Dead connection slots automatically redial on the next RPC. Unary and server-streaming calls retry once transparently if disconnected prior to commitment.
- **Timeout Management**: Timeouts set on channels or requests serialize as `grpc-timeout` headers, enforcing end-to-end deadlines across hops.
- **Backpressure & Limits**: Channel overlays cap message encoding/decoding sizes, buffer depths, and concurrent in-flight streams.

---

## Architectural Comparison with Tonic and gRPC-Go

| Architecture Domain | `pbrs-grpc` | Tonic | gRPC-Go |
|---|---|---|---|
| **Core Transport** | Direct `h2` HTTP/2 driver | Hyper HTTP/2 + Tower | Internal Go HTTP/2 stack |
| **Addressing Model** | `host:port` string / `Target` | `http://` or `https://` URIs | Resolver URIs (`dns:///`, `passthrough:///`) |
| **Concurrency Limiting** | Strict process-wide RPC cap | Tower `ConcurrencyLimitLayer` | Worker pool / goroutine dispatch |
| **Memory Allocation** | Bounded buffers with early caps | Configurable Tower buffers | Shared transport write buffers |
| **TLS & Security** | rustls + Graviola (ALPN `h2`) | rustls or native-tls | Go crypto/tls |
| **Service Resolution** | Immutable `Router` | Tower service multiplexing | Handler registration tables |

For complete invariant comparisons across all transports, middleware, and operational controls, refer to [docs/guides/comparison.md](guides/comparison.md).
