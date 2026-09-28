# Architecture

This page explains how the workspace is split and how the native gRPC kernel moves bytes. It is for Rust developers who want the big picture before reading implementation details. The bottom line: `pbrs`, `protobuf-tonic`, and `pbrs-grpc` are separate crates in `mingley/pure-protobuf`, and only the selected adapter pulls in a gRPC stack.

`pbrs` is a protobuf kernel for parse, serialize, reflection, JSON, text, and
plugin codegen. It does not use upb, libprotobuf, or C.

## Crates

| Crate | Role |
|---|---|
| `pbrs` | Protobuf kernel, `protoc-gen-pbrs`, and conformance child. |
| `protobuf-tonic` | Tonic 0.14 `Codec` plus generated `FooClient` / `FooServer`. |
| `pbrs-grpc` | HTTP/2 gRPC kernel over pbrs; it is not Tonic. |

Dependency boundaries are intentional:

- The protobuf kernel has no Tonic, h2, or Hyper dependency.
- `pbrs-grpc` has no Tonic dependency.
- `protobuf-tonic` has no `pbrs-grpc` dependency.
- A consumer can use pbrs alone, pbrs with the Tonic adapter, or pbrs with the native gRPC kernel.

The Cargo package and the library are both named `pbrs`
(`use pbrs::prelude::*`). The GitHub repository is `mingley/pure-protobuf`.

## gRPC kernel

`pbrs-grpc` speaks gRPC over prior-knowledge HTTP/2. Hand-written modules
forbid `unsafe`. Generated messages still use pbrs `unsafe` for zeroed
construction. There is no C compiler in the build: TLS uses rustls + Graviola,
and gzip uses `miniz_oxide`.

### Accept

Servers can accept:

- TCP (`serve` / `serve_listener`)
- TLS (`serve_tls*`)
- Unix Domain Sockets (`serve_unix*`)
- one already-accepted stream (`serve_connection`)
- a custom `Incoming`

The TCP/TLS loops apply `TCP_NODELAY`, optional `SO_KEEPALIVE`, and, on mTLS,
the verified client chain on `Rpc::peer_identity`.

Unix fills `SO_PEERCRED` on `Rpc::peer_cred` and reports `:scheme` `http`.
`Incoming::accept` yields `(Io, Option<SocketAddr>)`. Other connection facts go
on `Incoming::peer` as `ConnectionInfo`: local address, identity, credentials,
and transport scheme. Those facts are copied onto every call shape on that
connection.

TLS reports `:scheme` `https` and, on mTLS, the verified client chain on every
call shape. The default copies the accept address and does not probe `Io`.
`serve_connection` leaves those fields unset on `Rpc`. Generated handlers see
the same empty facts on `Request` and `Parts`. The peer's `:scheme` and
`:authority` still apply, including after `https_scheme`.

Connection lifecycle is explicit:

| Setting | Effect |
|---|---|
| `Server::max_connection_age` / `max_connection_idle` | Sends `GOAWAY`; the next RPC redials. |
| `Server::max_concurrent_connections` | Caps active connections. |
| `ServerConfig::handshake_timeout` | Drops mute peers that do not complete TLS or the HTTP/2 preface. |
| `ServerConfig::max_concurrent_rpcs` | Guards handler dispatch and returns `RESOURCE_EXHAUSTED` at capacity. |
| `Server::serve_with_shutdown` | Drains gracefully and lets active RPCs finish before the listener closes. |

### Dispatch

`Service::call` receives an `Rpc`. Generated `FooServer` implements `Service`;
application code implements the `Foo` trait. Consume each `Rpc` with exactly
one of:

- `unary`
- `client_streaming`
- `server_streaming`
- `bidi_streaming`
- `unimplemented`

Interceptors run before service logic and can inspect:

- path, service name, and method name
- metadata headers and binary trailers
- deadlines and timeouts
- peer identity and credentials (`peer_identity`, `peer_cred`)
- compression and message size limits
- request extensions for sharing typed context

`Router` routes on the service name in the path. Unregistered services return
`Code::Unimplemented`. Generated methods that are omitted return the same code.

Handlers that do asynchronous work can monitor `Request::cancelled()` to react
to client disconnects, timeouts, or stream resets.

### Wire

Messages are length-prefixed protobuf frames on `h2`. Inbound decode happens
inline on the handler task (`WireStream`). Outbound writes use batches
(`OutBatch`) so one DATA frame can carry many messages.

Encode-cap failures on a stream become producer status
(`RESOURCE_EXHAUSTED` trailers), not a transport reset.

gzip is optional and is never sent to a peer that omitted it from
`grpc-accept-encoding`. Inbound gzip is on by default; `accept_compressed(false)`
refuses it.

The kernel enforces caps before committing the memory they guard:

- 4 MiB inbound message default
- 16 KiB header list
- 256 concurrent streams
- rapid reset defense
- connection age and idle limits

### Client

`Channel` pools HTTP/2 connections to one authority (`Target`). Client
interceptors inspect and modify `Outgoing` request context: authority, scheme,
user-agent, timeouts, wait-for-ready overlays, compression settings, and
metadata.

| Characteristic | Behavior |
|---|---|
| Addressing | Dials direct authorities (`host:port`). `origin` can override authority without changing the TCP dial. |
| Connection recovery | Dead slots redial on the next RPC. Unary and server-streaming calls retry once transparently if disconnected before commitment. |
| Timeout management | Channel or request timeouts serialize as `grpc-timeout` headers and enforce end-to-end deadlines across hops. |
| Backpressure and limits | Channel overlays cap message encoding/decoding sizes, buffer depths, and concurrent in-flight streams. |

## Architectural Comparison with Tonic and gRPC-Go

| Architecture domain | `pbrs-grpc` | Tonic | gRPC-Go |
|---|---|---|---|
| Core transport | Direct `h2` HTTP/2 driver | Hyper HTTP/2 + Tower | Internal Go HTTP/2 stack |
| Addressing model | `host:port` string / `Target` | `http://` or `https://` URIs | Resolver URIs (`dns:///`, `passthrough:///`) |
| Concurrency limiting | Strict process-wide RPC cap | Tower `ConcurrencyLimitLayer` | Worker pool / goroutine dispatch |
| Memory allocation | Bounded buffers with early caps | Configurable Tower buffers | Shared transport write buffers |
| TLS and security | rustls + Graviola with ALPN `h2` | rustls or native-tls | Go crypto/tls |
| Service resolution | Immutable `Router` | Tower service multiplexing | Handler registration tables |

For full invariant comparisons across transports, middleware, and operational
controls, see [docs/guides/comparison.md](guides/comparison.md).
