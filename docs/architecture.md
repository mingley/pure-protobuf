# Architecture

The workspace separates protobuf messages from gRPC transport. Use `pbrs`
for messages, `protobuf-tonic` to keep a Tonic service, or `pbrs-grpc` for the
native transport. Each transport uses the same protobuf runtime.

`pbrs` handles parsing, serialization, reflection, JSON, text, and code generation.

## Crates

| Crate | Role |
|---|---|
| `pbrs` | Protobuf kernel, `protoc-gen-pbrs`, and conformance child. |
| `protobuf-tonic` | Tonic 0.14 `Codec` plus generated `FooClient` / `FooServer`. |
| `pbrs-grpc` | HTTP/2 gRPC client and server with optional Prost, Tower, and Tonic adapters. |

The core runtime requires Rust 1.85; the Tonic adapter requires Rust 1.88.
`pbrs` keeps codegen, reflection, JSON, text, and conformance helpers enabled
by default for compatibility. Runtime consumers can reduce their feature
set; see the [feature split](decisions/runtime-build-split.md) for supported
profiles. The native transport has optional Prost and Tower adapters.

Dependencies:

- The protobuf kernel has no Tonic, h2, or Hyper dependency.
- `pbrs-grpc` uses Tonic only when its optional `tonic` feature is enabled.
- `protobuf-tonic` has no `pbrs-grpc` dependency.
- A consumer can use pbrs alone, pbrs with the Tonic adapter, or pbrs with the native gRPC kernel.

The Cargo package and the library are both named `pbrs`
(`use pbrs::prelude::*`). The GitHub repository is `mingley/pure-protobuf`.

## gRPC kernel

`pbrs-grpc` speaks gRPC over prior-knowledge HTTP/2 using an embedded `h2`
backend and Tokio. TLS uses rustls with Graviola by default; gzip and
deflate use `miniz_oxide`. The [unsafe reference](unsafe-invariants.md)
describes parser, backend, and OS-helper safety requirements.

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

`Channel::connect` pools HTTP/2 connections to one authority (`Target`).
`Channel::connect_uri` opts into resolver snapshots and service-config load
balancing. Client interceptors inspect and modify `Outgoing` request context:
authority, scheme, user-agent, timeouts, wait-for-ready overlays, compression
settings, and metadata.

| Characteristic | Behavior |
|---|---|
| Addressing | Default direct authorities (`host:port`); opt-in resolver URIs (`dns:`, `passthrough:`, `ipv4:`, `ipv6:`, `unix:`, `unix-abstract:`) through `Channel::connect_uri`. `origin` can override authority without changing the TCP dial. |
| Connection recovery | Dead slots redial on the next RPC. Unary and server-streaming calls retry once transparently if disconnected before commitment. |
| Timeout management | Channel or request timeouts serialize as `grpc-timeout` headers and enforce end-to-end deadlines across hops. |
| Backpressure and limits | Channel overlays cap message encoding/decoding sizes, buffer depths, and concurrent in-flight streams. |

## Where to change performance

| Cost to investigate | Start here | Evidence needed |
|---|---|---|
| Protobuf allocation, parse, and encode | [Runtime design](design.md), `src/lazy.rs`, `src/map.rs`, `src/wire.rs`, `src/table.rs` | Equivalent generated messages, parse-and-touch work, allocations, and retained bytes. |
| Large payload copies | [Zero-copy paths](zero-copy.md), native wire framing | Copy counts plus CPU per RPC; account for gzip and TLS separately. |
| Generated code and build cost | `src/codegen/`, [layout contract](codegen-layout.md) | Output parity, generation time, clean and incremental builds, binary size. |
| Client and server overhead | `pbrs-grpc/src/client/`, `pbrs-grpc/src/server/`, `pbrs-grpc/src/transport/` | Endpoint CPU at matched load, tail latency, connection and stream limits. |
| Official Google-generated messages | `src/runtime/`, [compatibility scope](codegen-compatibility.md) | Original shared consumers and kernel-specific correctness evidence. |

The [performance plan](plan/world-class/README.md) orders this work.
Use the [comparison guide](guides/comparison.md) to choose a transport and
the [benchmark contract](benchmark-contract.md) to plan measurements.
