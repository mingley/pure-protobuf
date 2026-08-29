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
credentials, transport scheme). The default copies the accept address
and does not probe `Io`. `serve_connection` leaves those fields unset.

### Dispatch

`Service::call` receives an `Rpc`. Generated `FooServer` implements
`Service`; you implement `Foo`. Consume `Rpc` with exactly one of
`unary` / `client_streaming` / `server_streaming` / `bidi_streaming` /
`unimplemented`. Interceptors run first and may inspect metadata,
deadline, `:authority` / `:scheme`, path / service / method, peer identity
/ cred, and `Rpc::limits`. `Router` splits on the service half of the path.
Generated handlers see the same facts on `Request` / `Parts`, including
path / service / method. Dumping `Rpc` prints service/method, both timeout
views, and `limits`. Dumping `Request` prints path / service / method.

### Wire

Length-prefixed protobuf frames on `h2`. Inbound decode is inline on the
handler task (`WireStream`). Outbound batches (`OutBatch`) so one DATA
frame can carry many messages. gzip is optional and never sent to a peer
that omitted it from `grpc-accept-encoding`. Caps (4 MiB inbound default,
16 KiB header list, 256 concurrent streams, rapid reset, connection
age/idle) are enforced before the memory they guard is committed.

### Client

`Channel` pools HTTP/2 connections to one authority. A client interceptor
sees `Outgoing` (path, service/method, `:authority`, `:scheme`,
`user-agent`, message caps, metadata, deadline, wait-for-ready, compression,
extensions). Unary and server-streaming retry once when the connection
dies after the stream slot looked live. `from_io` cannot redial.
`Channel::https_scheme` sends `:scheme https` on a `from_io` clone without
a TLS handshake; TCP and Unix keep the transport. `Channel::scheme` /
`FooClient::scheme` is the same string client interceptors see on
`Outgoing::scheme`.

### Interceptors

Server: `Server` / `Router` / `FooServer::intercept` and `Intercepted`.
The first registered runs first. Closures see `Rpc` (path, service/method,
metadata, both timeout views, peer, `:authority` / `:scheme`, limits).
They may only tighten the deadline. `Err(Status)` is `rpc.reject`.
Generated handlers read the same facts on `Request` / `Parts`, including
the method path.

Client: `Channel::intercept` / `FooClient::intercept`. Closures see
`Outgoing` (path, service/method, `:authority`, `:scheme`, `user-agent`,
limits, metadata, deadline, wait-for-ready, compression, extensions).
Overlays (timeout, wait-for-ready, send_compressed, message caps,
`https_scheme`) fill in before interceptors run. Bind borrowed getters
before `metadata_mut`.

Response-side interceptors are a documented omission.

### Status

`Status` is two machine words; message, metadata, and
`grpc-status-details-bin` live behind a pointer. `with_error_details` /
`from_error_details` pack the standard `google.rpc` payloads
(`ErrorInfo`, `RetryInfo`, `QuotaFailure`, `BadRequest`, ...) as
`google.rpc.Status`. `set_message` rewrites a packed protobuf whose
message still matches.

### Health and reflection

`grpc.health.v1` is an ordinary service plus `HealthReporter`
(`Check` / `Watch` only). `grpc.reflection.v1` is built from registered
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
