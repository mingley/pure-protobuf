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
after `https_scheme`). `Server::max_connection_age` / `max_connection_idle`
send GOAWAY; the next RPC of every call shape redials, including over TLS,
mTLS, and Unix. `from_io` cannot redial.
`Server::max_concurrent_connections` caps the accept loop on TCP, TLS, mTLS,
and Unix; a second dial is `UNAVAILABLE` while the cap is full.
A mute TCP, TLS, mTLS, or Unix peer that never finishes the handshake is
dropped by `handshake_timeout` so the accept loop keeps serving.
`Channel::connect_timeout` bounds the client dial the same way: a peer
that accepts and never speaks (including TLS and mTLS) is `UNAVAILABLE`;
a closed port or missing Unix path still fails immediately. `from_io` is
already connected.
`max_concurrent_rpcs` refuses extra RPCs with `RESOURCE_EXHAUSTED` before the
handler runs, on every call shape, including over TLS, mTLS, Unix, and
`from_io`. `Channel::max_concurrent_rpcs` is the client dual: extras are
`RESOURCE_EXHAUSTED` before the stream opens on those transports. Distinct
from `SETTINGS_MAX_CONCURRENT_STREAMS`, which waits.
Graceful drain finishes in-flight RPCs and refuses new connections on TLS,
mTLS, and Unix; `from_io` has no accept loop. A dead Channel slot redials
the same TCP, TLS, mTLS, or Unix address on the next RPC of every call
shape and fails fast when nothing is listening; `from_io` cannot redial.
`Incoming::peer` stamps connection facts onto every call shape on that
connection.
Compiling `ConnectionInfo` peer dumps live on the `Incoming` rustdoc.

### Dispatch

`Service::call` receives an `Rpc`. Generated `FooServer` implements
`Service`; you implement `Foo`. Consume `Rpc` with exactly one of
`unary` / `client_streaming` / `server_streaming` / `bidi_streaming` /
`unimplemented`. Interceptors run first and may inspect metadata,
deadline, `:authority` / `:scheme`, path / service / method, peer identity
/ cred, `Rpc::limits`, `accepts_gzip` / encoding, `compresses_outbound`,
`gzip_level`, `accepts_compressed`, `concurrent_rpc_limit`, `send_buffer_size`, and extensions.
`Router` splits on the service half of the path. An unmounted service, or a
method a mounted service does not have, is `UNIMPLEMENTED` on every call
shape, including over TLS, mTLS, Unix, and `from_io`. Remounting the same
service name keeps the last handler on those transports.
Generated `Foo` methods you omit answer `UNIMPLEMENTED`.
Generated handlers see the same facts on `Request` / `Parts`, including
path / service / method, `peer_timeout`, the server `rpc_timeout` overlay,
`accepts_gzip` / encoding, the
`compresses_outbound` overlay, `gzip_level`, `accepts_compressed`, `concurrent_rpc_limit`, `send_buffer_size`, `remote_addr` / `local_addr` / `peer_identity` / `peer_cred`, and `:authority` / `:scheme`, extensions, user-agent. Dumping
`Rpc` prints path / service / method, metadata, interceptor `timeout` / server `rpc_timeout` /
`peer_timeout` / `effective_timeout`, `deadline`, `accepts_gzip` /
encoding / `compresses_outbound` / `gzip_level` / `accepts_compressed` / `concurrent_rpc_limit` / `send_buffer_size`, `limits`, `remote_addr` / `local_addr` / `peer_identity` / `peer_cred`, `:authority` / `:scheme`, and extensions.
Dumping `Request` prints path / service / method, metadata, `timeout` / `rpc_timeout` /
`peer_timeout`,
`deadline`, gzip intent vs wire flag, `encoding`, `accepts_gzip`, `compresses_outbound`, `gzip_level`, `accepts_compressed`, `concurrent_rpc_limit`, `send_buffer_size`, `remote_addr` / `local_addr` / `peer_identity` / `peer_cred`,
`:authority` / `:scheme`, wait-for-ready, `limits`, cancel, extensions, and user-agent.
Dumping `Parts` prints the same facts as `Request` without the message:
path / service / method, metadata, `timeout` / `rpc_timeout` / `peer_timeout`, `deadline`,
gzip intent vs wire flag, `encoding`, `accepts_gzip`, `compresses_outbound`, `gzip_level`,
`accepts_compressed`, `concurrent_rpc_limit`, `send_buffer_size`, `remote_addr` / `local_addr` / `peer_identity` / `peer_cred`,
`:authority` / `:scheme`, wait-for-ready, `limits`, cancel, extensions, and user-agent.
Dumping `Response` prints metadata, trailers, compress intent (`compress_is_set`), received
`encoding`, path / service / method, `gzip_level` / `compresses_outbound` /
`accepts_gzip` / `accepts_compressed`, `deadline` / `timeout` / `peer_timeout` /
`rpc_timeout`, `limits`, `send_buffer_size`, and extensions.
Handlers that spawn work await `Request::cancelled` (client RST, deadline, or
after the response is written / the stream drains), including over TLS, mTLS,
Unix, and `serve_connection`. A drain waiting for the
next message sees RST and ends, so a Watch-style producer wakes without
another send.

### Wire

Length-prefixed protobuf frames on `h2`. Inbound decode is inline on the
handler task (`WireStream`). Outbound batches (`OutBatch`) so one DATA
frame can carry many messages. Encode-cap failures on a stream are producer
status (RESOURCE_EXHAUSTED trailers), not a transport reset. gzip is optional and never sent to a peer
that omitted it from `grpc-accept-encoding`. Inbound gzip is on by default;
`accept_compressed(false)` refuses it. Caps (4 MiB inbound default,
16 KiB header list, 256 concurrent streams, rapid reset, connection
age/idle) are enforced before the memory they guard is committed.

### Client

`Channel` pools HTTP/2 connections to one authority. A client interceptor
sees `Outgoing` (path, service/method, `:authority`, `:scheme`,
`user-agent` (`user_agent_is_set`), message caps, metadata, timeout / deadline Instant,
wait-for-ready (`wait_for_ready_is_set`), compression (`compress_is_set`),
channel overlays (`rpc_timeout` / `waits_for_ready` / `compresses_outbound` /
`gzip_level` / `accepts_compressed` / `concurrent_rpc_limit` / `stream_buffer_size` / `send_buffer_size` / `limits`),
extensions, `connected`). Those Outgoing getters apply to every call shape.
Dumping `Outgoing` prints path / service / method, `:authority` / `:scheme`,
`user-agent` (`user_agent_is_set`), `limits`, `rpc_timeout` / `waits_for_ready` / `compresses_outbound` /
`accepts_compressed` / `gzip_level` / `concurrent_rpc_limit` / `stream_buffer_size` /
`send_buffer_size`, metadata, timeout / deadline, wait-for-ready (`wait_for_ready_is_set`), `connected`,
compress (`compress_is_set`), and extensions.
The next RPC of every call shape redials a dead slot, including over TLS,
mTLS, and Unix. Unary and server-streaming retry once when the connection
dies after the stream slot looked live. Client-streaming and bidi retry once
when HEADERS never went out. `from_io` cannot redial.
`ChannelConfig::max_connection_idle` tears the client socket down after idle
even when keepalive PINGs still fire; the next RPC redials on TLS, mTLS, and
Unix. A long-running stream is not idle. `from_io` cannot redial after that
close. `ChannelConfig::max_connection_age` closes the client socket even while
RPCs are in flight; in-flight get grace, then the driver stops. Distinct from idle.
Keepalive PINGs do not postpone age.
`Channel::connected` is a snapshot of live sockets. Distinct from gRPC GetState.
`Outgoing::connected` is that same snapshot when a client interceptor runs.
Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Channel::https_scheme` sends `:scheme https` on a `from_io` clone without
a TLS handshake; TCP and Unix keep the transport. `Channel::scheme` /
`FooClient::scheme` is the same string client interceptors see on
`Outgoing::scheme`. `FooClient::authority` and `FooClient::grpc_user_agent`
are the same strings as `Channel::authority` / `Channel::grpc_user_agent`.
The kernel `user-agent` (and a `Channel::user_agent` prefix) is the header
the peer sees on every call shape, including over TLS, mTLS, Unix, and
`from_io`; inserting `user-agent` into metadata cannot replace it.
`Outgoing::set_user_agent` prefixes this RPC (kernel suffix stays).
`Request::set_user_agent` is the same prefix at the call site; an interceptor
that then calls `Outgoing::set_user_agent` wins.
`FooClient::rpc_timeout`, `waits_for_ready`, `compresses_outbound`,
`gzip_level`, `accepts_compressed`, `concurrent_rpc_limit`,
`stream_buffer_size`, `send_buffer_size`, and `limits` read
the same overlays as the channel (the setter names cannot collide).
`FooClient::connected` is the live-socket snapshot on a generated client. Distinct from `waits_for_ready` (overlay). Same snapshot as `Channel::connected`.
`Channel::unary` / `server_streaming` / `client_streaming` / `bidi` are
first-class for a hand-written `Service`; generated clients call the same
methods. Unknown methods on that `Service` are `UNIMPLEMENTED` on every
call shape, including over TLS, mTLS, Unix, and `from_io`.
`FooServer::rpc_timeout`, `compresses_outbound`, `gzip_level`,
`accepts_compressed`, `concurrent_rpc_limit`, `send_buffer_size`, and
`limits` (also `Server` / `Router`) read the same overlays as `server_config`.
A received `Streaming` holds the HTTP/2 driver, so dropping the `Channel`
after headers does not end the stream, including over TLS, mTLS, Unix, and
`from_io`. Dropping the `Streaming` before the end does reset it, including
bidi while the send half is still held.
OK-path custom `-bin` trailers land on `Response::trailers` (unary and
client-streaming) and `Streaming::trailers` (server-streaming and bidi,
including when called before draining messages); a `-bin` trailer must not
appear as a header, including over those transports. A non-OK trailing
`grpc-status` is `Err` from `Streaming::trailers` on those transports. A
`CallHandle` taken before await still cancels that live stream after
headers, still cancels a server-streaming or bidi call waiting for headers,
and still cancels a
client-streaming call after the sender is closed. A server-streaming or bidi
deadline RSTs the
send half before headers and after a half-close; after those headers that
deadline still RSTs the parked send half. An expired deadline is never a
clean end of stream (`DEADLINE_EXCEEDED`, not `Ok(None)`), including over
TLS, mTLS, Unix, and `from_io`. Spawned handler work
awaiting `Request::cancelled` sees that RST. A [`Call`] is fused after it yields
`Ready` (`futures_core::future::FusedFuture`). A finished [`Streaming`] is
fused after end-of-stream or error (`futures_core::stream::FusedStream`).
Client-streaming and bidi
return a `(StreamSender, Call)` pair that is `must_use`: dropping it resets
the stream.

### Interceptors

Server: `Server` / `Router` / `FooServer::intercept` and `Intercepted`.
`Intercepted` is `Clone` when the interceptor is.
The first registered runs first, including over TLS, mTLS, Unix, and
`from_io` (`FooServer::intercept`, `Router::intercept`, and
`ServiceExt::intercept`). A single `ServiceExt::intercept` wrapping a
hand-written `Service` still rejects before the handler on those
transports. A single `FooServer::intercept` (no `add_service`) still
rejects before the handler on those transports too. `FooServer::intercept` then `add_service` keeps that reject on
every mount on those transports. `FooServer::max_decoding_message_size`
then `add_service` keeps that inbound cap on every mount on those
transports too. `FooServer::max_encoding_message_size` then `add_service`
keeps that outbound cap on every mount on those transports (EmptyCall and
StreamingInputCall stay under a 16-byte encode cap). A wrapping `Service` `Rpc::reject` turns
the call away before the inner `call` on those transports too.
Interceptor extensions on `Rpc` reach handler `Request` / `Parts` on those
transports. `Response::extensions` is local typed context, not on the wire.
Distinct from metadata. A received reply starts empty. `Server::on_response` /
`Router::on_response` / `FooServer::on_response` run after the handler
returns `Ok`, before headers. Closures see `ResponseParts` and may stamp
metadata from those extensions. `Err` after the handler already ran is
trailers-only. A handler `Err` skips this hook. `Channel::on_response` /
`FooClient::on_response` run after a successful receive; a received reply
starts empty and this hook inserts typed context the peer cannot. `Err`
fails that Call (the peer already sent OK). A non-OK peer status skips
this hook. A received reply does not carry Channel overlays: `gzip_level` is not the peer's deflate effort; `compresses_outbound`, `accepts_gzip`, and `accepts_compressed` are `false`; `deadline`, `timeout`, `limits`, `peer_timeout`, `rpc_timeout`, and `send_buffer_size` are `None`.
Compiling intercept / on_response overlay dumps live on the `hello` module rustdoc (`GreeterClient` / `GreeterServer`).
`ServiceExt::on_response` / `Intercepted::on_response` is the
per-service hook and does not cover other mounts; a Server / Router hook
still runs first. Closures see `ResponseParts::path` (kernel-stamped).
Distinct from `Request::path` (inbound). Distinct from `Outgoing::path`
(before send). Closures see `ResponseParts::gzip_level` (server encode overlay).
Distinct from `compress` (on or off). Distinct from `Rpc::gzip_level`
(before the handler). Closures see `ResponseParts::compresses_outbound` (server encode overlay).
Distinct from `compress` (per-RPC). Distinct from `Rpc::compresses_outbound`
(before the handler). Closures see `ResponseParts::accepts_gzip` (peer `grpc-accept-encoding`).
Distinct from `encoding` (received). Distinct from `Rpc::accepts_gzip`
(before the handler). Closures see `ResponseParts::deadline` (kernel-stamped when writing).
Distinct from `Request::deadline` (inbound). Distinct from `Rpc::deadline`
(computed when that getter runs). Closures see `ResponseParts::timeout` (duration stamped at dispatch).
Distinct from `deadline` (Instant). Distinct from `Rpc::timeout`
(interceptor cap). Closures see `ResponseParts::limits` (encode cap when writing).
Distinct from `Request::limits` (inbound). Distinct from `Rpc::limits`
(before the handler). Closures see `ResponseParts::peer_timeout` (client `grpc-timeout`).
Distinct from `timeout` (effective). Distinct from `Rpc::peer_timeout`
(before the handler). Closures see `ResponseParts::rpc_timeout` (server overlay).
Distinct from `timeout` (effective). Distinct from `Rpc::rpc_timeout`
(before the handler). Closures see `ResponseParts::accepts_compressed` (inbound gzip overlay).
Distinct from `accepts_gzip` (peer advertisement). Distinct from `Rpc::accepts_compressed`
(before the handler). Closures see `ResponseParts::send_buffer_size` (write-time HTTP/2 send buffer overlay).
Distinct from `limits` (encode cap). Distinct from `Rpc::send_buffer_size`
(before the handler). Closures see `Rpc` (path, service/method,
metadata, interceptor `timeout`, server overlay `rpc_timeout`, `peer_timeout`,
`effective_timeout`, `deadline`, `accepts_gzip` / encoding,
`compresses_outbound`, `gzip_level`, `accepts_compressed`, `concurrent_rpc_limit`, `send_buffer_size`, `remote_addr` / `local_addr` / `peer_identity` / `peer_cred`, `:authority` / `:scheme`, limits, extensions).
Interceptors insert typed context on `Rpc::extensions_mut`. They may only tighten the deadline. `Err(Status)` is `rpc.reject`,
including `with_error_details` (those trailers reach the client).
`metadata_mut().set` / `remove` / `retain` reach the handler on every call
shape, including over TLS, mTLS, Unix, and `from_io`. Those mutations
survive `into_message_and_parts`. TLS `:authority` is
the dial `Target`, not SNI.
Generated handlers read the same facts on `Request` / `Parts`, including
the method path, the client's `grpc-timeout`, the server timeout overlay,
`accepts_gzip` / encoding, the `compresses_outbound` overlay, `gzip_level`, `accepts_compressed`, `concurrent_rpc_limit`, `send_buffer_size`, `remote_addr` / `local_addr` / `peer_identity` / `peer_cred`, and `:authority` / `:scheme`, extensions, user-agent. `Server::timeout` / `Router::timeout`
expire Slow handlers when the client omits a deadline and cap a longer client
deadline, including over TLS, mTLS, Unix, and `from_io`.
`Server::send_compressed` / `Router::send_compressed`
gzip replies when the client advertises gzip on those transports.
A default server leaves `Response::encoding` unset (identity) on those
transports. `Response::set_compress(false)` opts out of
`Server::send_compressed` on those transports too. `Request::set_compress(false)`
opts out of `Channel::send_compressed` on those transports. `Channel::send_compressed`
itself gzips unary and server-streaming payloads and `StreamSender::send`
on those transports (the handler sees the Compressed-Flag / `grpc-encoding`).
A client `grpc-timeout` is a
`Request::deadline` Instant that elapses while the handler runs, including
over TLS, mTLS, Unix, and `from_io`.

Client: `Channel::intercept` / `FooClient::intercept`. Closures see
`Outgoing` (path, service/method, `:authority`, `:scheme`, `user-agent` (`user_agent_is_set`),
limits, metadata, timeout / deadline Instant, wait-for-ready
(`wait_for_ready_is_set`), compression (`compress_is_set`), channel overlays
(`rpc_timeout` / `waits_for_ready` / `compresses_outbound` / `gzip_level` / `accepts_compressed` / `concurrent_rpc_limit` / `stream_buffer_size` / `send_buffer_size` / `limits`), extensions, `connected`).
Overlays (timeout, wait-for-ready, send_compressed, gzip_compression_level, message caps,
`https_scheme`) fill in before interceptors run; `clear_*` opts out of that
already-applied default while the overlay getters stay. `Channel::timeout` /
`ChannelConfig::timeout` fill `grpc-timeout` when the request omits one,
including over TLS, mTLS, Unix, and `from_io`; a request timeout wins over
that default. `clear_compress` then
`set_compress(compresses_outbound())` reapplies channel gzip on every call
shape. Interceptors run when the
RPC method is invoked, not when the `Call` is first polled. `Err` fails that
`Call` on poll for every call shape, including `with_error_details`; nothing
is sent. `Outgoing::set_timeout` is that Call's deadline on every call shape.
Bind borrowed getters
before `metadata_mut`.
`Outgoing::user_agent_is_set` is occupancy on this architecture interceptor path, so a later interceptor can prefix only when unset.
`Outgoing::wait_for_ready_is_set` is occupancy on this architecture interceptor path, so a later interceptor can fill wait-for-ready only when unset.
`Outgoing::compress_is_set` is occupancy on this architecture interceptor path, so a later interceptor can fill compress only when unset.
`Outgoing::clear_user_agent` restores the channel user-agent after an architecture interceptor prefix.
`Outgoing::clear_wait_for_ready` restores the channel wait-for-ready overlay after an architecture interceptor choice.
`Outgoing::clear_compress` then `set_compress` from `compresses_outbound` reapplies channel gzip after an architecture interceptor choice.
`Outgoing::clear_timeout` opts out of the channel timeout after an architecture interceptor choice.
`Outgoing::connected` is the live-socket snapshot on this architecture interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture interceptor Err; a local reject never opens a stream.
Distinct from an architecture handler Err: that is after the handler ran; this architecture interceptor Err is a local reject never opens a stream.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture interceptor Err is a local reject never opens a stream.
Distinct from an architecture server intercept Err: that is trailers without reading the body; this architecture interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture interceptor already ran, so a local Err never consumes that budget.
Distinct from `Server::intercept`: that runs on the inbound RPC before the handler; this architecture Channel intercept runs on the outbound call before the stream opens.
Distinct from `Channel::on_response`: that runs after a successful receive; this architecture Channel intercept runs on the outbound call before the stream opens.
Distinct from `Channel::intercept`: that runs on the outbound call before the stream opens; this architecture server intercept runs on the inbound RPC before the handler.
Distinct from `Server::on_response`: that runs after the handler returns Ok; this architecture server intercept runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture server intercept Err; those trailers reach the client without reading the body.
Distinct from an architecture handler Err: that is after the handler ran; this architecture server intercept Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture server intercept Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture server intercept Err is trailers without reading the body.
Distinct from an architecture interceptor Err: that is a local reject never opens a stream; this architecture server intercept Err is trailers without reading the body.
`Status::from_error_details` is the typed bag after this architecture Store interceptor Err; those trailers reach the client without reading the body.
Distinct from an architecture Store handler Err: that is after the handler ran; this architecture Store interceptor Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Store interceptor Err is trailers without reading the body.
Distinct from an architecture Store client interceptor Err: that is a local reject never opens a stream; this architecture Store interceptor Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Store interceptor Err is trailers without reading the body.
Distinct from an architecture Store StreamSender fail: that is trailers after any messages already sent; this architecture Store interceptor Err is trailers without reading the body.
Distinct from an architecture Store client interceptor: that runs on the outbound call before the stream opens; this architecture Store interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture Store handler Err; those trailers reach the client.
Distinct from an architecture Store interceptor Err: that is trailers without reading the body; this architecture Store handler Err is after the handler ran.
Distinct from an architecture Store client interceptor Err: that is a local reject never opens a stream; this architecture Store handler Err is after the handler ran.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Store handler Err is after the handler ran.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Store handler Err is after the handler ran.
Distinct from an architecture Store StreamSender fail: that is trailers after any messages already sent; this architecture Store handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this architecture Store client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture Store client interceptor Err; a local reject never opens a stream.
Distinct from an architecture Store handler Err: that is after the handler ran; this architecture Store client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Store client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Store interceptor Err: that is trailers without reading the body; this architecture Store client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Store StreamSender fail: that is trailers after any messages already sent; this architecture Store client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture Store client interceptor already ran, so a local Err never consumes that budget.
Distinct from an architecture Store interceptor: that runs on the inbound RPC before the handler; this architecture Store client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this architecture Store StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an architecture Store handler Err: that is after the handler ran; this architecture Store StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Store interceptor Err: that is trailers without reading the body; this architecture Store StreamSender fail is trailers after any messages already sent.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Store StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Store client interceptor Err: that is a local reject never opens a stream; this architecture Store StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Store StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this architecture TestService interceptor Err; those trailers reach the client without reading the body.
Distinct from an architecture TestService handler Err: that is after the handler ran; this architecture TestService interceptor Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture TestService interceptor Err is trailers without reading the body.
Distinct from an architecture TestService client interceptor Err: that is a local reject never opens a stream; this architecture TestService interceptor Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture TestService interceptor Err is trailers without reading the body.
Distinct from an architecture TestService StreamSender fail: that is trailers after any messages already sent; this architecture TestService interceptor Err is trailers without reading the body.
Distinct from an architecture TestService client interceptor: that runs on the outbound call before the stream opens; this architecture TestService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture TestService handler Err; those trailers reach the client.
Distinct from an architecture TestService interceptor Err: that is trailers without reading the body; this architecture TestService handler Err is after the handler ran.
Distinct from an architecture TestService client interceptor Err: that is a local reject never opens a stream; this architecture TestService handler Err is after the handler ran.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture TestService handler Err is after the handler ran.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture TestService handler Err is after the handler ran.
Distinct from an architecture TestService StreamSender fail: that is trailers after any messages already sent; this architecture TestService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this architecture TestService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture TestService client interceptor Err; a local reject never opens a stream.
Distinct from an architecture TestService handler Err: that is after the handler ran; this architecture TestService client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture TestService client interceptor Err is a local reject never opens a stream.
Distinct from an architecture TestService interceptor Err: that is trailers without reading the body; this architecture TestService client interceptor Err is a local reject never opens a stream.
Distinct from an architecture TestService StreamSender fail: that is trailers after any messages already sent; this architecture TestService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture TestService client interceptor already ran, so a local Err never consumes that budget.
Distinct from an architecture TestService interceptor: that runs on the inbound RPC before the handler; this architecture TestService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this architecture TestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an architecture TestService handler Err: that is after the handler ran; this architecture TestService StreamSender fail is trailers after any messages already sent.
Distinct from an architecture TestService interceptor Err: that is trailers without reading the body; this architecture TestService StreamSender fail is trailers after any messages already sent.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture TestService StreamSender fail is trailers after any messages already sent.
Distinct from an architecture TestService client interceptor Err: that is a local reject never opens a stream; this architecture TestService StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture TestService StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this architecture Reverser interceptor Err; those trailers reach the client without reading the body.
Distinct from an architecture Reverser handler Err: that is after the handler ran; this architecture Reverser interceptor Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Reverser interceptor Err is trailers without reading the body.
Distinct from an architecture Reverser client interceptor Err: that is a local reject never opens a stream; this architecture Reverser interceptor Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Reverser interceptor Err is trailers without reading the body.
Distinct from an architecture Reverser StreamSender fail: that is trailers after any messages already sent; this architecture Reverser interceptor Err is trailers without reading the body.
Distinct from an architecture Reverser client interceptor: that runs on the outbound call before the stream opens; this architecture Reverser interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture Reverser handler Err; those trailers reach the client.
Distinct from an architecture Reverser interceptor Err: that is trailers without reading the body; this architecture Reverser handler Err is after the handler ran.
Distinct from an architecture Reverser client interceptor Err: that is a local reject never opens a stream; this architecture Reverser handler Err is after the handler ran.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Reverser handler Err is after the handler ran.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Reverser handler Err is after the handler ran.
Distinct from an architecture Reverser StreamSender fail: that is trailers after any messages already sent; this architecture Reverser handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this architecture Reverser client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture Reverser client interceptor Err; a local reject never opens a stream.
Distinct from an architecture Reverser handler Err: that is after the handler ran; this architecture Reverser client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Reverser client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Reverser interceptor Err: that is trailers without reading the body; this architecture Reverser client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Reverser StreamSender fail: that is trailers after any messages already sent; this architecture Reverser client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture Reverser client interceptor already ran, so a local Err never consumes that budget.
Distinct from an architecture Reverser interceptor: that runs on the inbound RPC before the handler; this architecture Reverser client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this architecture Reverser StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an architecture Reverser handler Err: that is after the handler ran; this architecture Reverser StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Reverser interceptor Err: that is trailers without reading the body; this architecture Reverser StreamSender fail is trailers after any messages already sent.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Reverser StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Reverser client interceptor Err: that is a local reject never opens a stream; this architecture Reverser StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Reverser StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this architecture hello interceptor Err; those trailers reach the client without reading the body.
Distinct from an architecture hello handler Err: that is after the handler ran; this architecture hello interceptor Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture hello interceptor Err is trailers without reading the body.
Distinct from an architecture hello client interceptor Err: that is a local reject never opens a stream; this architecture hello interceptor Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture hello interceptor Err is trailers without reading the body.
Distinct from an architecture hello StreamSender fail: that is trailers after any messages already sent; this architecture hello interceptor Err is trailers without reading the body.
Distinct from an architecture hello client interceptor: that runs on the outbound call before the stream opens; this architecture hello interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture hello handler Err; those trailers reach the client.
Distinct from an architecture hello interceptor Err: that is trailers without reading the body; this architecture hello handler Err is after the handler ran.
Distinct from an architecture hello client interceptor Err: that is a local reject never opens a stream; this architecture hello handler Err is after the handler ran.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture hello handler Err is after the handler ran.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture hello handler Err is after the handler ran.
Distinct from an architecture hello StreamSender fail: that is trailers after any messages already sent; this architecture hello handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this architecture hello client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture hello client interceptor Err; a local reject never opens a stream.
Distinct from an architecture hello handler Err: that is after the handler ran; this architecture hello client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture hello client interceptor Err is a local reject never opens a stream.
Distinct from an architecture hello interceptor Err: that is trailers without reading the body; this architecture hello client interceptor Err is a local reject never opens a stream.
Distinct from an architecture hello StreamSender fail: that is trailers after any messages already sent; this architecture hello client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture hello client interceptor already ran, so a local Err never consumes that budget.
Distinct from an architecture hello interceptor: that runs on the inbound RPC before the handler; this architecture hello client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this architecture hello StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an architecture hello handler Err: that is after the handler ran; this architecture hello StreamSender fail is trailers after any messages already sent.
Distinct from an architecture hello interceptor Err: that is trailers without reading the body; this architecture hello StreamSender fail is trailers after any messages already sent.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture hello StreamSender fail is trailers after any messages already sent.
Distinct from an architecture hello client interceptor Err: that is a local reject never opens a stream; this architecture hello StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture hello StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this architecture UnimplementedService interceptor Err; those trailers reach the client without reading the body.
Distinct from an architecture UnimplementedService handler Err: that is after the handler ran; this architecture UnimplementedService interceptor Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture UnimplementedService interceptor Err is trailers without reading the body.
Distinct from an architecture UnimplementedService client interceptor Err: that is a local reject never opens a stream; this architecture UnimplementedService interceptor Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture UnimplementedService interceptor Err is trailers without reading the body.
Distinct from an architecture UnimplementedService client interceptor: that runs on the outbound call before the stream opens; this architecture UnimplementedService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture UnimplementedService handler Err; those trailers reach the client.
Distinct from an architecture UnimplementedService interceptor Err: that is trailers without reading the body; this architecture UnimplementedService handler Err is after the handler ran.
Distinct from an architecture UnimplementedService client interceptor Err: that is a local reject never opens a stream; this architecture UnimplementedService handler Err is after the handler ran.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture UnimplementedService handler Err is after the handler ran.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture UnimplementedService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this architecture UnimplementedService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture UnimplementedService client interceptor Err; a local reject never opens a stream.
Distinct from an architecture UnimplementedService handler Err: that is after the handler ran; this architecture UnimplementedService client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture UnimplementedService client interceptor Err is a local reject never opens a stream.
Distinct from an architecture UnimplementedService interceptor Err: that is trailers without reading the body; this architecture UnimplementedService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture UnimplementedService client interceptor already ran, so a local Err never consumes that budget.
Distinct from an architecture UnimplementedService interceptor: that runs on the inbound RPC before the handler; this architecture UnimplementedService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this architecture InteropTestService interceptor Err; those trailers reach the client without reading the body.
Distinct from an architecture InteropTestService handler Err: that is after the handler ran; this architecture InteropTestService interceptor Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture InteropTestService interceptor Err is trailers without reading the body.
Distinct from an architecture InteropTestService client interceptor Err: that is a local reject never opens a stream; this architecture InteropTestService interceptor Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture InteropTestService interceptor Err is trailers without reading the body.
Distinct from an architecture InteropTestService StreamSender fail: that is trailers after any messages already sent; this architecture InteropTestService interceptor Err is trailers without reading the body.
Distinct from an architecture InteropTestService client interceptor: that runs on the outbound call before the stream opens; this architecture InteropTestService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture InteropTestService handler Err; those trailers reach the client.
Distinct from an architecture InteropTestService interceptor Err: that is trailers without reading the body; this architecture InteropTestService handler Err is after the handler ran.
Distinct from an architecture InteropTestService client interceptor Err: that is a local reject never opens a stream; this architecture InteropTestService handler Err is after the handler ran.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture InteropTestService handler Err is after the handler ran.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture InteropTestService handler Err is after the handler ran.
Distinct from an architecture InteropTestService StreamSender fail: that is trailers after any messages already sent; this architecture InteropTestService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this architecture InteropTestService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture InteropTestService client interceptor Err; a local reject never opens a stream.
Distinct from an architecture InteropTestService handler Err: that is after the handler ran; this architecture InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from an architecture InteropTestService interceptor Err: that is trailers without reading the body; this architecture InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from an architecture InteropTestService StreamSender fail: that is trailers after any messages already sent; this architecture InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture InteropTestService client interceptor already ran, so a local Err never consumes that budget.
Distinct from an architecture InteropTestService interceptor: that runs on the inbound RPC before the handler; this architecture InteropTestService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this architecture InteropTestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an architecture InteropTestService handler Err: that is after the handler ran; this architecture InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from an architecture InteropTestService interceptor Err: that is trailers without reading the body; this architecture InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from an architecture InteropTestService client interceptor Err: that is a local reject never opens a stream; this architecture InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture InteropTestService StreamSender fail is trailers after any messages already sent.
`ResponseParts::compress_is_set` is occupancy on this architecture on_response path, so a later interceptor can fill compress only when unset.
`ResponseParts::clear_compress` restores the server gzip overlay after Server on_response on this architecture on_response path.
`Status::from_error_details` is the typed bag after this architecture server on_response Err; a local reject is trailers-only after handler Ok.
Distinct from an architecture handler Err: that is after the handler ran; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture server intercept Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture Health interceptor Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture Health StreamSender fail: that is trailers after any messages already sent; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture reflection interceptor Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture reflection StreamSender fail: that is trailers after any messages already sent; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture Store interceptor Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture Store StreamSender fail: that is trailers after any messages already sent; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture TestService interceptor Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture TestService StreamSender fail: that is trailers after any messages already sent; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture Reverser interceptor Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture Reverser StreamSender fail: that is trailers after any messages already sent; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture hello interceptor Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture hello StreamSender fail: that is trailers after any messages already sent; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture UnimplementedService interceptor Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture InteropTestService interceptor Err: that is trailers without reading the body; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture InteropTestService StreamSender fail: that is trailers after any messages already sent; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from an architecture interceptor Err: that is a local reject never opens a stream; this architecture server on_response Err is trailers-only after handler Ok.
Distinct from `Server::intercept`: that runs on the inbound RPC before the handler; this architecture server on_response runs after the handler returns Ok.
`ResponseParts::clear_compress` drops a compress choice after Channel on_response on this architecture on_response path; a received reply has no server gzip overlay to restore.
`Status::from_error_details` is the typed bag after this architecture Channel on_response Err; a local reject fails the Call after a successful receive.
Distinct from an architecture handler Err: that is after the handler ran; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Health client interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Health interceptor Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Health StreamSender fail: that is trailers after any messages already sent; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture reflection client interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture reflection interceptor Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture reflection StreamSender fail: that is trailers after any messages already sent; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Store client interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Store interceptor Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Store StreamSender fail: that is trailers after any messages already sent; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture TestService client interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture TestService interceptor Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture TestService StreamSender fail: that is trailers after any messages already sent; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Reverser client interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Reverser interceptor Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture Reverser StreamSender fail: that is trailers after any messages already sent; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture hello client interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture hello interceptor Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture hello StreamSender fail: that is trailers after any messages already sent; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture UnimplementedService client interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture UnimplementedService interceptor Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture InteropTestService client interceptor Err: that is a local reject never opens a stream; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture InteropTestService interceptor Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture InteropTestService StreamSender fail: that is trailers after any messages already sent; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture server intercept Err: that is trailers without reading the body; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Channel on_response Err fails the Call after a successful receive.
Distinct from `Channel::intercept`: that runs on the outbound call before the stream opens; this architecture Channel on_response runs after a successful receive.

### Status

`Status` is two machine words; message, metadata, and
`grpc-status-details-bin` live behind a pointer. Local I/O, a TLS handshake,
and HTTP/2 connection death attach the original error as
`std::error::Error::source`; a peer trailer has no cause.
`Status::with_cause` attaches `Error::source` onto an existing architecture status; a peer trailer has no cause.
`from_error_details` packs a typed `ErrorDetails` bag of the standard `google.rpc` payloads
(`ErrorInfo`, `RetryInfo`, `DebugInfo`, `QuotaFailure`, `PreconditionFailure`,
`BadRequest`, `RequestInfo`, `ResourceInfo`, `Help`, `LocalizedMessage`) as
`google.rpc.Status`. Distinct from `with_error_details`: that packs `Any` values, not the typed bag.
`Status::with_details` ships raw trailer bytes after this architecture with_error_details packer, Distinct from packing Anys onto a status.
`Status::with_rpc` keeps existing trailers after this architecture from_rpc constructor, Distinct from minting a fresh status.
`pb::Status::with_details` builds a packed `google.rpc.Status` after this architecture with_details constructor, Distinct from shipping raw trailer bytes.
`ErrorDetails::to_anys` returns the `Any` list after this architecture from_error_details constructor, Distinct from encoding the bag as a trailer.
`ErrorDetails::from_rpc` unpacks the `Any` list after this architecture error_details unpack, Distinct from unpacking a kernel Status trailer.
`Any::pack` packs one message into an `Any` after this architecture with_error_details packer, Distinct from packing Anys onto a status.
`Any::pack_with` takes an explicit type URL after this architecture pack constructor, Distinct from using `type.googleapis.com/<FULL_NAME>`.
`Status::set_details` ships raw trailer bytes after this architecture set_error_details packer, Distinct from packing Anys onto a status.
`Any::unpack` decodes the payload after this architecture is type-URL check, Distinct from checking the type URL.
`Any::is` is a type-URL check after this architecture unpack decode, Distinct from decoding the payload.
`ErrorDetails::new` is an empty bag after this architecture from_rpc unpack, Distinct from unpacking the `Any` list.
`ErrorDetails::with_error_info` plants packed ErrorInfo after this architecture ErrorDetails bag.
`ErrorDetails::with_retry_info` plants packed RetryInfo after this architecture ErrorDetails bag.
`ErrorDetails::with_debug_info` plants packed DebugInfo after this architecture ErrorDetails bag.
`ErrorDetails::with_quota_failure` plants packed QuotaFailure after this architecture ErrorDetails bag.
`ErrorDetails::with_precondition_failure` plants packed PreconditionFailure after this architecture ErrorDetails bag.
`ErrorDetails::with_bad_request` plants packed BadRequest after this architecture ErrorDetails bag.
`Duration::from_std` builds the protobuf from `std` after this architecture try_to_std convert, Distinct from converting this protobuf to `std`.
`Duration::try_to_std` converts this protobuf to `std` after this architecture from_std builder, Distinct from building the protobuf from `std`.
`Status::details` returns raw trailer bytes after this architecture rpc parse, Distinct from parsing a packed `google.rpc.Status`.
`Status::new` takes a code and message after this architecture from_code constructor, Distinct from being code-only.
`Status::from_code` is code-only after this architecture new constructor, Distinct from taking a code and message.
`Status::rpc` parses a packed `google.rpc.Status` after this architecture details getter, Distinct from returning raw trailer bytes.
`Status::set_code` mutates in place after this architecture with_code builder, Distinct from being the builder.
`Status::with_code` is the builder after this architecture set_code mutation, Distinct from mutating in place.
`Status::set_message` mutates in place after this architecture with_message builder, Distinct from being the builder.
`Status::with_message` is the builder after this architecture set_message mutation, Distinct from mutating in place.
`Code::from_i32` interprets a wire i32 after this architecture to_i32 emit, Distinct from emitting the wire i32.
`Code::to_i32` emits the wire i32 after this architecture from_i32 interpret, Distinct from interpreting a wire i32.
`Code::name` is the canonical name after this architecture description text, Distinct from being the one-line google.rpc.Code text.
`Code::description` is the one-line google.rpc.Code text after this architecture name spelling, Distinct from being the canonical name.
`Status::is_ok` is Code::Ok after this architecture is_retryable A6, Distinct from being UNAVAILABLE only.
`Status::code` is the ASCII `grpc-status` code after this architecture message trailer, Distinct from being the ASCII `grpc-message`.
`Status::message` is the ASCII `grpc-message` after this architecture code trailer, Distinct from being the ASCII `grpc-status` code.
`Code::is_retryable` is the A6 set on a Code after this architecture Status is_retryable, Distinct from being the same A6 set on a Status.
`Status::is_retryable` is the A6 set on a Status after this architecture Code is_retryable, Distinct from being the same A6 set on a Code.
`Status::metadata` borrows this status trailers map after this architecture metadata_mut mutation, Distinct from mutating it.
`Status::metadata_mut` mutates this status trailers map after this architecture metadata borrow, Distinct from borrowing it.
`ParseCodeError` rejects a string after this architecture from_i32 Unknown map, Distinct from mapping an unrecognised wire i32 to Unknown.
`Status::code` is the ASCII `grpc-status` trailer after this architecture packed rpc parse, Distinct from being the packed protobuf.
`Status::message` is the ASCII `grpc-message` trailer after this architecture packed rpc parse, Distinct from being the packed protobuf.
`Status::rpc` is the packed protobuf after this architecture ASCII grpc-status trailer, Distinct from being the ASCII `grpc-status` trailer.
`Status::rpc` is the packed protobuf after this architecture ASCII grpc-message trailer, Distinct from being the ASCII `grpc-message` trailer.
`Status::details` returns raw trailer bytes after this architecture ASCII grpc-status trailer, Distinct from being the ASCII `grpc-status` trailer.
`Status::details` returns raw trailer bytes after this architecture ASCII grpc-message trailer, Distinct from being the ASCII `grpc-message` trailer.
`Status::code` is the ASCII `grpc-status` trailer after this architecture raw details bytes, Distinct from returning raw trailer bytes.
`Status::message` is the ASCII `grpc-message` trailer after this architecture raw details bytes, Distinct from returning raw trailer bytes.
Unknown types stay in `ErrorDetails::unknown` so a custom detail is not dropped on a round-trip. `Status::error_info` is that packed `ErrorInfo` without
unpacking the bag. Distinct from `error_details`. Distinct from `retry_delay`
(a wait hint). `RetryInfo::with_retry_delay` builds that payload. `ErrorInfo::with_reason` builds that payload.
`ErrorInfo::with_metadata` fills a metadata pair after this architecture ErrorInfo builder.
`Status::bad_request` is packed field violations. Distinct from
`error_info`. `BadRequest::with_field` builds that payload.
`BadRequest::with_field_entry` builds an extra packed field violation after this architecture BadRequest builder.
`FieldViolation::with_field` builds a nested field path after this architecture BadRequest builder.
`FieldViolation::with_reason` builds a nested field-violation reason after this architecture FieldViolation builder.
`FieldViolation::with_localized_message` builds a nested field-violation locale after this architecture FieldViolation builder.
`Status::quota_failure`
is packed quota subjects. Distinct from `is_retryable`. Distinct from
`bad_request`. `QuotaFailure::with_violation` builds that payload.
`QuotaFailure::with_violation_entry` builds an extra packed quota violation after this architecture QuotaFailure builder.
`quota_failure::Violation::with_subject` builds a nested quota subject after this architecture QuotaFailure builder.
`quota_failure::Violation::with_api_service` builds a nested quota API service after this architecture quota subject builder.
`quota_failure::Violation::with_quota_metric` builds a nested quota metric after this architecture quota subject builder.
`quota_failure::Violation::with_quota_id` builds a nested quota id after this architecture quota subject builder.
`quota_failure::Violation::with_quota_dimension` builds a nested quota dimension pair after this architecture quota subject builder.
`quota_failure::Violation::with_quota_value` builds a nested quota value after this architecture quota subject builder.
`quota_failure::Violation::with_future_quota_value` builds a nested future quota value after this architecture quota subject builder.
`Status::precondition_failure` is packed type and subject. Distinct from
`quota_failure`. Distinct from `bad_request`. `PreconditionFailure::with_violation`
builds that payload.
`PreconditionFailure::with_violation_entry` builds an extra packed precondition violation after this architecture PreconditionFailure builder.
`precondition_failure::Violation::with_type` builds a nested precondition type after this architecture PreconditionFailure builder.
`Status::help` is packed documentation links. Distinct from
failure classifications: links can sit next to a retryable UNAVAILABLE.
`Help::with_link` builds that payload.
`Help::with_link_entry` builds an extra packed docs URL after this architecture Help builder.
`help::Link::with_url` builds a nested docs URL after this architecture Help builder.
`Status::localized_message` is packed
locale text. Distinct from the ASCII `grpc-message`. Distinct from `help`.
`LocalizedMessage::with_locale` builds that payload. `Status::request_info` is packed
request_id for logs. Distinct from `error_info`. Distinct from `help`.
`RequestInfo::with_request_id` builds that payload. `Status::resource_info` is packed
resource type and name. Distinct from `quota_failure`. Distinct from `request_info`.
`ResourceInfo::with_resource` builds that payload.
`ResourceInfo::with_description` builds a packed resource description after this architecture ResourceInfo builder.
`Status::debug_info` is packed
operator stack. Distinct from `localized_message`. Distinct from `help`.
`DebugInfo::with_stack` builds that payload.
`DebugInfo::with_stack_entry` builds an extra packed stack frame after this architecture DebugInfo builder.
`set_code` / `set_message` rewrite a packed protobuf
whose code or message still matches. `set_rpc` / `set_error_details` / `set_from_error_details` replace the protobuf without dropping trailing metadata. Handler `Err` and
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
(`Check` / `List` / `Watch`). Check of a never-set name is `NOT_FOUND`; Watch of
that name is `SERVICE_UNKNOWN`; Watch streams `set_not_serving` /
`shutdown` / `resume`; dropping Watch releases the subscription, including
over TLS, mTLS, Unix, and `from_io`. `Watch` ends when the client cancels or drops the
stream, without waiting for a later status change.
`Status::from_error_details` is the typed bag after this architecture Health interceptor Err; those trailers reach the client without reading the body.
Distinct from an architecture Health handler Err: that is after the handler ran; this architecture Health interceptor Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Health interceptor Err is trailers without reading the body.
Distinct from an architecture Health client interceptor Err: that is a local reject never opens a stream; this architecture Health interceptor Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Health interceptor Err is trailers without reading the body.
Distinct from an architecture Health StreamSender fail: that is trailers after any messages already sent; this architecture Health interceptor Err is trailers without reading the body.
Distinct from an architecture Health client interceptor: that runs on the outbound call before the stream opens; this architecture Health interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture Health handler Err; those trailers reach the client.
Distinct from an architecture Health interceptor Err: that is trailers without reading the body; this architecture Health handler Err is after the handler ran.
Distinct from an architecture Health client interceptor Err: that is a local reject never opens a stream; this architecture Health handler Err is after the handler ran.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Health handler Err is after the handler ran.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Health handler Err is after the handler ran.
Distinct from an architecture Health StreamSender fail: that is trailers after any messages already sent; this architecture Health handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this architecture Health client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture Health client interceptor Err; a local reject never opens a stream.
Distinct from an architecture Health handler Err: that is after the handler ran; this architecture Health client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Health client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Health interceptor Err: that is trailers without reading the body; this architecture Health client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Health StreamSender fail: that is trailers after any messages already sent; this architecture Health client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture Health client interceptor already ran, so a local Err never consumes that budget.
Distinct from an architecture Health interceptor: that runs on the inbound RPC before the handler; this architecture Health client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this architecture Health StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an architecture Health handler Err: that is after the handler ran; this architecture Health StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Health interceptor Err: that is trailers without reading the body; this architecture Health StreamSender fail is trailers after any messages already sent.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture Health StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Health client interceptor Err: that is a local reject never opens a stream; this architecture Health StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture Health StreamSender fail is trailers after any messages already sent.
`grpc.reflection.v1` is built from registered
`FILE_DESCRIPTOR_SET`s. `file_containing_symbol` / `file_by_filename` /
`file_containing_extension` / `all_extension_numbers_of_type` run on that
one bidi method, including over TLS, mTLS, Unix, and `from_io`.
`Status::from_error_details` is the typed bag after this architecture reflection interceptor Err; those trailers reach the client without reading the body.
Distinct from an architecture reflection handler Err: that is after the handler ran; this architecture reflection interceptor Err is trailers without reading the body.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture reflection interceptor Err is trailers without reading the body.
Distinct from an architecture reflection client interceptor Err: that is a local reject never opens a stream; this architecture reflection interceptor Err is trailers without reading the body.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture reflection interceptor Err is trailers without reading the body.
Distinct from an architecture reflection StreamSender fail: that is trailers after any messages already sent; this architecture reflection interceptor Err is trailers without reading the body.
Distinct from an architecture reflection client interceptor: that runs on the outbound call before the stream opens; this architecture reflection interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this architecture reflection handler Err; those trailers reach the client.
Distinct from an architecture reflection interceptor Err: that is trailers without reading the body; this architecture reflection handler Err is after the handler ran.
Distinct from an architecture reflection client interceptor Err: that is a local reject never opens a stream; this architecture reflection handler Err is after the handler ran.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture reflection handler Err is after the handler ran.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture reflection handler Err is after the handler ran.
Distinct from an architecture reflection StreamSender fail: that is trailers after any messages already sent; this architecture reflection handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this architecture reflection client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this architecture reflection client interceptor Err; a local reject never opens a stream.
Distinct from an architecture reflection handler Err: that is after the handler ran; this architecture reflection client interceptor Err is a local reject never opens a stream.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture reflection client interceptor Err is a local reject never opens a stream.
Distinct from an architecture reflection interceptor Err: that is trailers without reading the body; this architecture reflection client interceptor Err is a local reject never opens a stream.
Distinct from an architecture reflection StreamSender fail: that is trailers after any messages already sent; this architecture reflection client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this architecture reflection client interceptor already ran, so a local Err never consumes that budget.
Distinct from an architecture reflection interceptor: that runs on the inbound RPC before the handler; this architecture reflection client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this architecture reflection StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an architecture reflection handler Err: that is after the handler ran; this architecture reflection StreamSender fail is trailers after any messages already sent.
Distinct from an architecture reflection interceptor Err: that is trailers without reading the body; this architecture reflection StreamSender fail is trailers after any messages already sent.
Distinct from an architecture server on_response Err: that is trailers-only after handler Ok; this architecture reflection StreamSender fail is trailers after any messages already sent.
Distinct from an architecture reflection client interceptor Err: that is a local reject never opens a stream; this architecture reflection StreamSender fail is trailers after any messages already sent.
Distinct from an architecture Channel on_response Err: that fails the Call after a successful receive; this architecture reflection StreamSender fail is trailers after any messages already sent.

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
it wrote. The plugin emits `pbrs-grpc` stubs by default, same as
`compile_protos`. `PURE_PROTOBUF_STUBS=tonic` selects the tonic adapter.

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
