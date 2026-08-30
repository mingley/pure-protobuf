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
reports `:scheme` `http`. `Incoming::accept` still yields
`(Io, Option<SocketAddr>)`. Other connection facts go on
`Incoming::peer` as a `ConnectionInfo` (local address, identity,
credentials, transport scheme). Those facts are copied onto every call
shape on that connection. TLS reports `:scheme` `https` and, on mTLS, the
verified client chain on every call shape. The default copies the accept address
and does not probe `Io`. `serve_connection` leaves those fields unset
on `Rpc`, and generated handlers see the same empty facts on `Request`
and `Parts` (the peer's `:scheme` / `:authority` still apply, including
after `https_scheme`).

### Dispatch

`Service::call` receives an `Rpc`. Generated `FooServer` implements
`Service`; you implement `Foo`. Consume `Rpc` with exactly one of
`unary` / `client_streaming` / `server_streaming` / `bidi_streaming` /
`unimplemented`. Interceptors run first and may inspect metadata,
deadline, `:authority` / `:scheme`, path / service / method, peer identity
/ cred, `Rpc::limits`, gzip accept/encoding, and `compresses_outbound`.
`Router` splits on the service half of the path. An unmounted service, or a
method a mounted service does not have, is `UNIMPLEMENTED` on every call
shape, including over TLS, mTLS, Unix, and `from_io`.
Generated `Foo` methods you omit answer `UNIMPLEMENTED`.
Generated handlers see the same facts on `Request` / `Parts`, including
path / service / method, `peer_timeout`, the server `rpc_timeout` overlay,
gzip accept/encoding, and the
`compresses_outbound` overlay. Dumping
`Rpc` prints service/method, interceptor `timeout` / server `rpc_timeout` /
`peer_timeout` / `effective_timeout`, `deadline`, gzip accept /
encoding / `compresses_outbound`, and `limits`.
Dumping `Request` prints path / service / method, `timeout` / `rpc_timeout` /
`peer_timeout`,
`deadline`, gzip intent vs wire flag, `encoding`, `compresses_outbound`, peer,
`:authority` / `:scheme`, wait-for-ready, `limits`, and cancel.
Dumping `Response` prints metadata, trailers, compress intent, and received
`encoding`.
Handlers that spawn work await `Request::cancelled` (client RST, deadline, or
after the response is written / the stream drains). A drain waiting for the
next message sees RST and ends, so a Watch-style producer wakes without
another send.

### Wire

Length-prefixed protobuf frames on `h2`. Inbound decode is inline on the
handler task (`WireStream`). Outbound batches (`OutBatch`) so one DATA
frame can carry many messages. Encode-cap failures on a stream are producer
status (RESOURCE_EXHAUSTED trailers), not a transport reset. gzip is optional and never sent to a peer
that omitted it from `grpc-accept-encoding`. Caps (4 MiB inbound default,
16 KiB header list, 256 concurrent streams, rapid reset, connection
age/idle) are enforced before the memory they guard is committed.

### Client

`Channel` pools HTTP/2 connections to one authority. A client interceptor
sees `Outgoing` (path, service/method, `:authority`, `:scheme`,
`user-agent`, message caps, metadata, timeout / deadline Instant,
wait-for-ready (`wait_for_ready_is_set`), compression (`compress_is_set`),
channel overlays (`rpc_timeout` / `waits_for_ready` / `compresses_outbound`),
extensions). Those Outgoing getters apply to every call shape. Unary and server-streaming retry once when the connection
dies after the stream slot looked live. `from_io` cannot redial.
`Channel::https_scheme` sends `:scheme https` on a `from_io` clone without
a TLS handshake; TCP and Unix keep the transport. `Channel::scheme` /
`FooClient::scheme` is the same string client interceptors see on
`Outgoing::scheme`. `FooClient::authority` and `FooClient::grpc_user_agent`
are the same strings as `Channel::authority` / `Channel::grpc_user_agent`.
`FooClient::rpc_timeout`, `waits_for_ready`, and `compresses_outbound` read
the same overlays as the channel (the setter names cannot collide).
`Channel::unary` / `server_streaming` / `client_streaming` / `bidi` are
first-class for a hand-written `Service`; generated clients call the same
methods.
`FooServer::rpc_timeout` and `compresses_outbound` (also `Server` / `Router`)
read the same overlays as `server_config`.
A received `Streaming` holds the HTTP/2 driver, so dropping the `Channel`
after headers does not end the stream. Dropping the `Streaming` before the
end does reset it, including bidi while the send half is still held. A
`CallHandle` taken before await still cancels that live stream after
headers, still cancels a server-streaming or bidi call waiting for headers,
and still cancels a
client-streaming call after the sender is closed. A server-streaming or bidi
deadline RSTs the
send half before headers and after a half-close; after those headers that
deadline still RSTs the parked send half. Spawned handler work
awaiting `Request::cancelled` sees that RST. A [`Call`] is fused after it yields
`Ready` (`futures_core::future::FusedFuture`). Client-streaming and bidi
return a `(StreamSender, Call)` pair that is `must_use`: dropping it resets
the stream.

### Interceptors

Server: `Server` / `Router` / `FooServer::intercept` and `Intercepted`.
`Intercepted` is `Clone` when the interceptor is.
The first registered runs first, including over TLS, mTLS, Unix, and
`from_io` (`FooServer::intercept`, `Router::intercept`, and
`ServiceExt::intercept`). `FooServer::intercept` then `add_service` keeps that reject on
every mount on those transports. `FooServer::max_decoding_message_size`
then `add_service` keeps that inbound cap on every mount on those
transports too. `FooServer::max_encoding_message_size` then `add_service`
keeps that outbound cap on every mount on those transports (EmptyCall and
StreamingInputCall stay under a 16-byte encode cap). A wrapping `Service` `Rpc::reject` turns
the call away before the inner `call` on those transports too.
Interceptor extensions on `Rpc` reach handler `Request` / `Parts` on those
transports. Closures see `Rpc` (path, service/method,
metadata, interceptor `timeout`, server overlay `rpc_timeout`, `peer_timeout`,
`effective_timeout`, `deadline`, gzip accept/encoding,
`compresses_outbound`, peer, `:authority` / `:scheme`, limits).
They may only tighten the deadline. `Err(Status)` is `rpc.reject`,
including `with_error_details` (those trailers reach the client).
`metadata_mut().set` / `remove` / `retain` reach the handler on every call
shape, including over TLS, mTLS, Unix, and `from_io`. Those mutations
survive `into_message_and_parts`. TLS `:authority` is
the dial `Target`, not SNI.
Generated handlers read the same facts on `Request` / `Parts`, including
the method path, the client's `grpc-timeout`, the server timeout overlay,
gzip, and the
`compresses_outbound` overlay. A client `grpc-timeout` is a
`Request::deadline` Instant that elapses while the handler runs, including
over TLS, mTLS, Unix, and `from_io`.

Client: `Channel::intercept` / `FooClient::intercept`. Closures see
`Outgoing` (path, service/method, `:authority`, `:scheme`, `user-agent`,
limits, metadata, timeout / deadline Instant, wait-for-ready
(`wait_for_ready_is_set`), compression (`compress_is_set`), channel overlays
(`rpc_timeout` / `waits_for_ready` / `compresses_outbound`), extensions).
Overlays (timeout, wait-for-ready, send_compressed, message caps,
`https_scheme`) fill in before interceptors run; `clear_*` opts out of that
already-applied default while the overlay getters stay. `clear_compress` then
`set_compress(compresses_outbound())` reapplies channel gzip on every call
shape. Interceptors run when the
RPC method is invoked, not when the `Call` is first polled. `Err` fails that
`Call` on poll for every call shape, including `with_error_details`; nothing
is sent. `Outgoing::set_timeout` is that Call's deadline on every call shape.
Bind borrowed getters
before `metadata_mut`.

Response-side interceptors are a documented omission.

### Status

`Status` is two machine words; message, metadata, and
`grpc-status-details-bin` live behind a pointer. `with_error_details` /
`from_error_details` pack the standard `google.rpc` payloads
(`ErrorInfo`, `RetryInfo`, `DebugInfo`, `QuotaFailure`, `PreconditionFailure`,
`BadRequest`, `RequestInfo`, `ResourceInfo`, `Help`, `LocalizedMessage`) as
`google.rpc.Status`. `set_code` / `set_message` rewrite a packed protobuf
whose code or message still matches. `set_rpc` / `set_error_details`
replace the protobuf without dropping trailing metadata. Handler `Err` and
`StreamSender::fail` after headers both put that protobuf on trailing
`grpc-status-details-bin` for a server response stream. A client request
`fail` resets CANCEL; a client-streaming `Call`, or a bidi `Call` that has
not yet seen headers, resolves with the status, not `UNAVAILABLE` from the
reset. After bidi headers the received `Streaming` sees `CANCELLED`, not
that status.
Received ASCII
`grpc-status` / `grpc-message` are independent of the packed protobuf;
`rpc()` does not overwrite one from the other.

### Health and reflection

`grpc.health.v1` is an ordinary service plus `HealthReporter`
(`Check` / `Watch` only). `Watch` ends when the client cancels or drops the
stream, without waiting for a later status change. `grpc.reflection.v1` is built from registered
`FILE_DESCRIPTOR_SET`s.

## Parse / encode

1. Bytes enter through `Parse::parse`.
2. Generated `merge_inner` matches tags, with depth at most 100.
3. Values land in field storage: scalars, `LazyStr`, `Packed`, `LazyMsg`,
   `Map`, `Repeated`.
4. Getters materialize lazy slots on first access.

Encode is the reverse. `CachedSize` is filled first, then `write_to` writes
into a `Vec<u8>`. Nested and packed fields write in place
(`encode_len_header` + `write_to`). There is no scratch `Vec` per
submessage.

Generated proto3 JSON and text are field-wise for messages whose fields
are proto3 scalars (int32, int64, uint32, uint64, sint32, sint64,
fixed32, fixed64, sfixed32, sfixed64, bool, float, double, string,
bytes), open proto3 enums, repeated and scalar maps of those types,
nested messages of that set, real oneofs of that set (`Person`,
`hello`, `ExtraScalars`, `OneofHole`), or `google.protobuf.Timestamp`
/ `Duration` / `Empty` / proto3 wrappers (official proto3 JSON:
Timestamp / Duration strings; Empty is `{}`; wrappers are the
wrapped value, not an object; text is the existing field mapping).
Map-of-enum is skipped (map-entry descriptors lack enum names at
codegen). Other WKT (Struct, Value, ListValue, Any, FieldMask) and
TAT still serialize to bytes and transcode through `DynamicMessage`.
TAT is not closed.

## Codegen

`protoc-gen-pbrs` is a normal protoc plugin (`--pbrs_out`).
`./scripts/gen.sh` finds or builds it, runs protoc, and rustfmts the `.rs`
it wrote.

Generated messages are field-wise Rust structs plus `impl_typed_message!`.
They are not `DynamicMessage` wrappers and not Google `OwnedMessageInner`.

A same-crate `build.rs` cannot invoke the plugin binary. Conformance
TestAllTypes lives in `src/generated/` and is re-exported from
`pbrs::gencode`.

## Modules

| module | job |
|---|---|
| `rt` | `CachedSize`, `OptBool`, packed aliases, wire helpers |
| `lazy` | `Wire`, `LazyStr`, `LazyBytes`, `LazyMsg` |
| `packed` | packed scalars; memcpy only for fixed-width |
| `repeated` / `map` | 8-byte empty (`Option<Box<Vec<_>>>`) |
| `dynamic` | `DescriptorPool`, `DynamicMessage` |
| `json` / `text` | WKT + spec codecs on dynamic messages; field-wise generated helpers for Person-shaped proto3, extra proto3 scalars, real oneofs of that set, Timestamp / Duration, Empty, and proto3 wrappers |
| `codegen` | plugin + FileDescriptorSet |
| `gen_support` | `impl_typed_message!`, default instances |

## Conformance process

`src/bin/conformance.rs` speaks the official runner protocol. The runner
is C++ (`conformance_test_runner` at protobuf v35.1). Fetch it with
`./scripts/fetch-protobuf.sh`. The protobuf tree is gitignored (~115 MiB).
Pin and rust_upb skip lists live in `vendor/google/` (~304 KiB).
