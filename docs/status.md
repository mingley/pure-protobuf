# Status

## Verified

- `cargo fmt --check`, `clippy --all-targets --all-features -- -D warnings`,
  and `cargo test --workspace` pass.
- CI on `main` runs fmt, clippy, tests, and official conformance
  (`./scripts/conformance.sh`: required ×2 and recommended, v35.1, cmake
  protoc from the pin, not system, no skip list). The `test` job still
  apt-installs `protobuf-compiler` for the plugin and protobuf-tonic
  `build.rs`.
- Conformance v35.1 (`--maximum_edition 2023`, `protoc` hidden / vendored
  FDS): required ×2: 5631 binary+JSON + 909 text, 0 unexpected.
  `--enforce_recommended`: same. No skip list. Empty-FDS hole was closed
  in #6: `build.rs` used to write `[]` when `protoc` was missing; #6 ships
  `vendor/google/conformance_fds.bin` and falls back to it (that was the
  2090 JsonOutput / `missing desc` cluster). CI printed the same totals.
- Official `protoc --rust_out` (4.35.1-release, `kernel=upb`) for
  `proto/person.proto` links against this crate as `protobuf` and
  parse→serialize→parse roundtrips (`rust_out_person/`).
- `rust_out_shared` runs official `rust/test/shared` googletest files
  (19 crates, 0 failed) against `protoc --rust_out kernel=upb`. Skips are
  only the files listed below.
- In-tree fuzz: `tests/fuzz_parse.rs` (empty / truncated / Person / TAT).
- grpc 0.9 unary remap (`grpc_remap/`): `ok name=ada message=Hello ada`
  through `protobuf-shim` → pbrs, not protobuf-tonic.
- 38 `google_shared` tests cover a plugin-generated subset of
  `rust/test/shared`.
- Plugin round-trip works, including `./scripts/gen.sh`.
- `protobuf-tonic` on this tonic 0.14 stack covers all four RPC shapes
  (unary, client-stream, server-stream, bidi) including `Status`
  code+message. Initial `Response` metadata is headers; `Status` metadata
  is HTTP/2 trailers. Client-stream, server-stream, and bidi carry both
  (same split as unary). Server-stream `Status` trailers also work when
  the handler returns `Ok(Response(stream))` and the stream errors
  before any item (empty stream + error, or first item `Err`): headers
  stay on `Response.metadata`, trailers on `Status.metadata`. Handler
  `Err(Status)` before opening a stream still lands on the call
  `Result`. Client-stream headers need the reply `Response`.
  `tests/interop.rs` has same-process analogues of
  official interop names (`unimplemented_method`, `unimplemented_service`,
  `special_status_message`, `empty_unary`, `large_unary`, `empty_stream`,
  `cancel_after_begin`, `cancel_after_first_response`,
  `timeout_on_sleeping_server`, `custom_metadata`). `large_unary` sizes
  (271828 / 314159) are `hello.proto` string fields (`name` / `message`),
  not official `SimpleRequest.payload.body` / `response_size`. Cancel
  analogues abort the client future (`JoinError::Cancelled`, not a
  `Status`). `timeout_on_sleeping_server` is unary `Request::set_timeout`
  → `Code::Cancelled` / "Timeout expired", not `DeadlineExceeded`.
  `custom_metadata` (unary SayHello): client sends
  `x-grpc-test-echo-initial` and `x-grpc-test-echo-trailing-bin`; ascii
  echo is `Response.metadata` (headers). Kernel
  `pbrs_grpc::Response::trailers()` carries OK-path custom trailers;
  `Streaming::trailers()` waits for end-of-stream. `protobuf-tonic` uses
  tonic's `Response`, which has no `trailers()`, so
  `x-grpc-test-echo-trailing-bin` is absent on that adapter's OK path.
  Same-process tonic, not official interop, no Google peer. Gzip is covered
  (`tests/gzip.rs`). Generated stubs expose
  `with_interceptor` and `max_decoding_message_size` /
  `max_encoding_message_size` (`tests/interceptor_size.rs`). Codec survey
  (`tonic-bench`, `proto/codec_cases.proto`) vs prost and v4 upb is in
  `docs/benchmarks.md`. Typical unary `rpc_mixed` is already ~2× prost
  and beats v4. `name_4kib` combined beats prost (gated). `tags_32`
  decode beats v4 (gated). Not kernel `./bench`. Not in CI.
- `./bench` fails the process if a gated case loses encode or owned decode
  to prost, v4, or buffa owned. Twelve cases: empty, person, tat_populated,
  packed_256, map_64, nested_8, strings, unpacked_256, packed_fixed_256,
  packed_fixed64_256, packed_float_256, repeated_nested_8. View is gated
  except `tat_populated`, `person`, and packed-fixed rows.
- File, enum, method, message, and field custom options survive
  FileDescriptorSet parse (`custom_option(n)`; file options on
  `FileDescriptor` / `DescriptorPool::get_file`).
- `pbrs-grpc` is a native HTTP/2 gRPC kernel over pbrs. It is not tonic.
  Official `grpc.testing.TestService` interop binaries
  (`pbrs-grpc-interop-server` / `pbrs-grpc-interop-client`) pass the
  shared uncompressed `_TEST_CASES` against Go `interop/client` and
  `interop/server` (`--use_tls=false`) and the four gzip cases
  kernel-vs-kernel. Loopback `rpc-bench` latency is process-gated
  (kernel median ns strictly below tonic 0.14 on empty_unary and
  large_unary). QPS is reported, not gated (empty/large at
  conc=1/conns=1 and conc=16/conns=4). Nonzero RPC errors still fail
  the process.   `Channel::connect_pool` opens independent h2 driver
  tasks.   `Channel`, `GreeterClient` / `TestServiceClient`, and
  `GreeterServer` / `TestServiceServer` expose
  `max_decoding_message_size` / `max_encoding_message_size`
  (default 4 MiB inbound, unlimited outbound). Oversize encode or decode is
  `RESOURCE_EXHAUSTED` on every call shape (`pbrs-grpc/tests/message_size.rs`).
  Not a
  latency or QPS win. `protobuf-tonic` stays the tonic adapter.

## Remaining

See `docs/upb.md`. Short list:

- Native gRPC is `pbrs-grpc`. Official `grpc.testing` TestService interop
  (`empty_unary` … `unimplemented_service`, plus the four gzip cases,
  including mixed `server_compressed_streaming`) is locked against
  `InteropTestService` over h2c, TLS, mTLS, Unix, and `from_io`. Greeter
  OK-path custom `-bin` trailers land on `Response::trailers` (unary and
  client-streaming) and `Streaming::trailers` (server-streaming and bidi,
  including when called before draining messages); a `-bin` trailer must not
  appear as a header, including over those transports. A non-OK trailing
  `grpc-status` is `Err` from `Streaming::trailers` on those transports. Health
  Check of a never-set name is `NOT_FOUND`, Watch of that name is
  `SERVICE_UNKNOWN`, Watch streams `set_not_serving` / `shutdown` / `resume`,
  and dropping a Watch releases the subscription, including over TLS, mTLS,
  Unix, and `from_io`. There is still no Health `List`. Reflection
  `file_containing_symbol` / `file_by_filename` / `file_containing_extension`
  / `all_extension_numbers_of_type` run on the one bidi method, including
  over TLS, mTLS, Unix, and `from_io`. An inbound Health Check or Watch over
  the decoding cap is `RESOURCE_EXHAUSTED` on those transports.
  `Router::message_limits` / `HealthServer::message_limits` refuse the same
  oversize, distinct from `max_decoding_message_size`. A HealthClient
  `ChannelConfig::message_limits` at `connect_tls_with` / `connect_unix_with` /
  `from_io_with` is `RESOURCE_EXHAUSTED` on Check and Watch, distinct from
  wrapping a live client. An inbound
  reflection message over the decoding cap fails that bidi stream as
  `RESOURCE_EXHAUSTED` trailers on those transports.
  `Router::message_limits` / `ServerReflectionServer::message_limits` refuse
  the same oversize, distinct from `max_decoding_message_size`. A
  ServerReflectionClient `ChannelConfig::message_limits` at those dialers is
  `RESOURCE_EXHAUSTED` on the one bidi method, distinct from wrapping a live
  client. Generated Store
  `max_decoding_message_size` is `RESOURCE_EXHAUSTED` on Get / Watch / PutAll
  / Sync over TLS, mTLS, Unix, and `from_io`. A Greeter client
  `max_encoding_message_size` / `max_decoding_message_size` is
  `RESOURCE_EXHAUSTED` on every call shape over those transports, distinct
  from the server caps. A generated Store client
  `max_encoding_message_size` / `max_decoding_message_size` is
  `RESOURCE_EXHAUSTED` on Get / Watch / PutAll / Sync over TLS, mTLS, Unix,
  and `from_io`, distinct from the Store server decode cap and from Greeter
  client caps. `ChannelConfig::connections` opens independent HTTP/2
  drivers on TLS, mTLS, and Unix (`connect_tls_with` / `connect_unix_with`);
  `from_io` cannot pool. A handler `Status::set_details` blob round-trips as
  `Status::details()` with trailing metadata on every call shape over TLS,
  mTLS, Unix, and `from_io`; `grpc-status-details-bin` is not a metadata key.
  Distinct from packed `google.rpc.Status`. `ChannelConfig::max_encoding_message_size`
  / `max_decoding_message_size` at `connect_tls_with` / `connect_unix_with` /
  `from_io_with` is `RESOURCE_EXHAUSTED` on every call shape, distinct from
  wrapping a live Channel or generated client after connect. A HealthClient
  `max_encoding_message_size` / `max_decoding_message_size` is
  `RESOURCE_EXHAUSTED` on Check and Watch over TLS, mTLS, Unix, and `from_io`,
  distinct from the Health server decoding cap. A ServerReflectionClient
  `max_encoding_message_size` / `max_decoding_message_size` is
  `RESOURCE_EXHAUSTED` on the one bidi method over those transports, distinct
  from the reflection server decoding cap. Hand-written `Channel::unary` /
  `server_streaming` / `client_streaming` / `bidi` honor those same client
  caps as `RESOURCE_EXHAUSTED` on every call shape over TLS, mTLS, Unix, and
  `from_io`, distinct from generated GreeterClient wrappers. A TestServiceClient
  `max_encoding_message_size` / `max_decoding_message_size` is
  `RESOURCE_EXHAUSTED` on UnaryCall / StreamingOutputCall / StreamingInputCall
  / FullDuplexCall over those transports, distinct from the TestService server
  add_service caps. `Channel::message_limits` / generated
  `FooClient::message_limits` / `ChannelConfig::message_limits` refuse
  oversize the same way as the single-cap setters over TLS, mTLS, Unix, and
  `from_io`. `Server::message_limits` / `Router::message_limits` / generated
  `FooServer::message_limits` / `ServerConfig::message_limits` refuse inbound
  or outbound oversize as `RESOURCE_EXHAUSTED` over TLS, mTLS, Unix, and
  `serve_connection`, distinct from the single-cap setters. TLS
  (rustls + Graviola), `grpc.health.v1` Check/Watch, and
  `grpc.reflection.v1` ship in the kernel. Unary/server-streaming that race
  a connection death after the slot looked live redial once (transparent
  retry) — proven for unary and server-streaming on h2c and TLS.
  `Server::max_connection_age` / generated `FooServer::max_connection_age`
  name that the next RPC of every call shape redials, including over TLS, mTLS,
  and Unix (`from_io` cannot redial), and that transparent
  retry of the same in-flight RPC is unary and server-streaming only. Unix accept loops expose `SO_PEERCRED` on `Rpc::peer_cred`.
  Custom `Incoming` implementations stamp local_addr / mTLS identity /
  Unix credentials / transport scheme via `Incoming::peer` and
  `ConnectionInfo`. TLS `:scheme https` and mTLS `peer_identity` apply to
  every call shape. HTTP/2 PING keepalive still serves every Greeter shape
  after PINGs fire on h2c, TLS (including mTLS), Unix, and `from_io`. TCP
  `SO_KEEPALIVE` is TCP-only and still serves every Greeter shape on h2c, TLS,
  and mTLS. `Server::max_concurrent_connections` refuses a second TCP, TLS,
  mTLS, or Unix dial with `UNAVAILABLE` while the cap is full (`from_io` is
  not an accept loop). A `ChannelConfig::connections` pool larger than that
  cap fails the whole dial as `UNAVAILABLE` on TLS, mTLS, and Unix (`from_io`
  cannot pool). Oversize metadata against `Server::max_header_list_size` is
  refused over TLS, mTLS, Unix, and `serve_connection`, distinct from a raw
  HTTP/2 peer. A mute TCP, TLS, mTLS, or Unix peer that never finishes
  the handshake is dropped by `handshake_timeout` so the accept loop keeps
  serving. Graceful drain finishes in-flight RPCs and refuses new connections
  on TLS, mTLS, and Unix (`from_io` has no accept loop). A dead Channel slot
  redials the same TCP, TLS, mTLS, or Unix address on the next RPC of every
  call shape and fails fast when nothing is listening, including
  `connect_tls_lazy` / `connect_unix_lazy`. `from_io` cannot redial.
  `Channel::connect_timeout` fails with `UNAVAILABLE` when a TCP, TLS, mTLS,
  or Unix peer accepts and never speaks, and still fails immediately on a
  closed port or missing Unix path. `from_io` is already connected.
  `ChannelConfig::max_connection_idle` tears down the client HTTP/2 driver
  after idle even when keepalive PINGs still fire; the next RPC of every call
  shape redials on TLS, mTLS, and Unix. `from_io` cannot redial after that
  close. A long-running server stream is not idle.
  `Channel::https_scheme` sends `:scheme https` on a
  `from_io` clone (no TLS handshake; no-op on TCP/Unix);
  `Channel::scheme` / generated `FooClient::scheme` / `FooClient::authority` /
  `FooClient::grpc_user_agent` read that overlay and the other interceptor-visible
  channel facts. Interceptors run when the RPC method is invoked (all four
  shapes) on h2c, TLS (including mTLS), Unix, and `from_io`, including official
  TestService methods, hand-written Reverser `Channel` APIs, generated Store,
  Health Check/Watch, and reflection `ServerReflectionInfo`, not on first poll of the `Call`. Interceptors and generated
  handlers see `MessageLimits` on `Rpc::limits` / `Request::limits` /
  `Parts::limits`, including over TLS, mTLS, Unix, and `from_io`, the
  method path on `Rpc::path` / `Request::path` / `Parts::path`, including
  over those transports, gzip accept/encoding (unary Compressed-Flag on
  identity vs gzip, including over TLS, mTLS, Unix, and `from_io`), and
  the server overlays on `Rpc::compresses_outbound` /
  `Request::compresses_outbound` and `Rpc::rpc_timeout` /
  `Request::rpc_timeout` (the `Server::timeout` cap, distinct from the
  interceptor `set_timeout` and the client's `peer_timeout`); a `Server::timeout`
  / `Router::timeout` overlay expires Slow handlers when the client omits a
  deadline and caps a longer client deadline, including over TLS, mTLS, Unix,
  and `from_io`; received replies
  surface `grpc-encoding` on
  `Response::encoding` (`None` for identity, including an explicit
  `identity` token, and a default server with no `send_compressed` on every
  call shape over TLS, mTLS, Unix, and `from_io`). `Server::send_compressed` / `Router::send_compressed`
  gzip every call shape when the client advertises gzip (`Response::compressed`
  and `encoding()` are gzip), including over TLS, mTLS, Unix, and `from_io`.
  `Server::send_compressed` / `Response::set_compress(false)`
  opt-out apply to every call shape, including over TLS, mTLS, Unix, and
  `from_io`. `Request::set_compress(false)` opts out of
  `Channel::send_compressed` on those transports too. `Channel::send_compressed`
  itself gzips unary and server-streaming payloads and `StreamSender::send`
  on those transports (the handler sees the Compressed-Flag / `grpc-encoding`).
  Client interceptors see the channel overlay
  on `Outgoing::limits` plus a deadline Instant, fill-if-unset
  wait-for-ready / compress (a client interceptor `set_compress(true)` gzips, and
  `set_compress(false)` opts out of `send_compressed`, on
  h2c, TLS including mTLS, Unix, and `from_io`; `set_compress(true)` also stamps
  `StreamSender::compress` on client-streaming and bidi — unary and
  server-streaming have no request `StreamSender` — on those transports plus
  `from_io`), and the channel overlays
  (`Outgoing::rpc_timeout` / `waits_for_ready` / `compresses_outbound`)
  after `clear_*` on h2c, TLS including mTLS, and Unix lazy dialers (fail-fast
  after `clear_wait_for_ready`) and on `from_io` (already connected; the RPC
  still runs), including official TestService EmptyCall / StreamingOutputCall /
  StreamingInputCall / FullDuplexCall, hand-written Reverser `Channel`
  methods, generated Store Get / Watch / PutAll / Sync, Health Check and Watch
  (no List), and reflection `ServerReflectionInfo`. `clear_compress` then `set_compress(compresses_outbound())`
  reapplies channel gzip on those transports plus `from_io`, including official
  TestService methods, hand-written Reverser `Channel` APIs, generated Store
  Get / Watch / PutAll / Sync, Health Check and Watch, and reflection
  `ServerReflectionInfo`. Wait-for-ready completes on h2c, TLS (`connect_tls_lazy`,
  including mTLS), and Unix (`connect_unix_lazy`) on every call shape, including the channel
  overlay, a client interceptor `set_wait_for_ready(true)`, per-RPC opt-out, a client interceptor `set_wait_for_ready(false)`, and a waiting Call's deadline, including mTLS.
  Official TestService EmptyCall / StreamingOutputCall /
  StreamingInputCall / FullDuplexCall and hand-written Reverser `Channel`
  methods retry that interceptor fill on those dialers too.
  `set_wait_for_ready(false)` in an interceptor opts out of a channel default
  on those TestService, Reverser, Store, Health, and reflection dialers too.
  Generated `StoreClient::connect_lazy` / `connect_tls_lazy` / `connect_unix_lazy`
  retry Get / Watch / PutAll / Sync until listen on those transports, from either
  the request flag, `FooClient::wait_for_ready`, or a client interceptor `set_wait_for_ready(true)`; opt-out and a waiting Call's
  deadline apply on those Store dialers too. Health Check and Watch
  retry until listen on the same dialers; Health has no List. A client interceptor
  `set_wait_for_ready(true)` retries Check and Watch until listen on those dialers too. Opt-out and a
  waiting Call's deadline apply on those Health dialers too, including mTLS.
  Reflection
  `ServerReflectionInfo` retries until listen on those dialers; reflection
  is one bidi method. A client interceptor `set_wait_for_ready(true)` retries
  that method until listen on those dialers too. Opt-out and a waiting Call's deadline apply on those
  reflection dialers too, including mTLS. Official TestService EmptyCall / StreamingOutputCall /
  StreamingInputCall / FullDuplexCall retry until listen on those dialers.
  Opt-out and a waiting Call's deadline apply on those TestService dialers
  too, including mTLS. Hand-written Reverser `Channel` methods retry until
  listen on those dialers. Opt-out and a waiting Call's deadline apply on
  those Reverser dialers too, including mTLS. A client interceptor `Err` fails the `Call` on poll for
  every call shape, including `with_error_details` and a local fail-before-open
  without details on h2c, TLS (including mTLS), Unix, and `from_io`, including
  official TestService methods, hand-written Reverser `Channel` APIs, generated
  Store, Health Check/Watch, and reflection `ServerReflectionInfo`; nothing is sent. A reserved
  `grpc-*` or hop-by-hop interceptor `insert` is `INVALID_ARGUMENT` on those same
  paths. A packed `google.rpc.Status` on that
  local `Err` is `Status::rpc` / `Status::error_details` on the Call.
  Outgoing getters apply to every call shape. Kernel `user-agent` (and a
  `Channel::user_agent` prefix) is sent on every shape, including over h2c, TLS
  (including mTLS), Unix, and `from_io`; inserting `user-agent`
  into metadata cannot override it.   Caller extensions on `Request::extensions_mut`
  and channel `MessageLimits` on `Outgoing::limits` are visible to a client
  interceptor on those transports plus `from_io`, including official TestService
  methods and hand-written Reverser `Channel` APIs, generated Store Get / Watch /
  PutAll / Sync, Health Check and Watch, and reflection `ServerReflectionInfo`. A `Channel::user_agent` prefix
  is `Outgoing::user_agent` on those same paths.   Server interceptor `set` / `remove` /
  `retain` reach the handler on every shape, including over TLS, mTLS, Unix,
  and `from_io` (`set` replaces a peer-smuggled hop, `remove` strips before
  the handler, `retain` keeps a subset including `-bin` keys; `Request` and
  `Parts` after `into_message_and_parts` see the same mutation). TLS
  (including mTLS) interceptor `:authority` is the dial `Target`
  (`SocketAddr` is `127.0.0.1:port`), not SNI `localhost`; Unix interceptor
  `:authority` is `localhost` on both sides even after `https_scheme`.
  `Outgoing::set_timeout` is that Call's deadline on every call shape, including when
  a client interceptor stamps it over h2c, TLS (including mTLS), Unix, and `from_io`.
  `Outgoing::clear_timeout` opts out of a channel timeout on those transports plus
  `from_io`. `Channel::timeout` / `ChannelConfig::timeout` expire the RPC when
  the request omits a deadline, on those transports; a request timeout wins
  over the channel default.
  A wrapping `Service` `Rpc::reject` turns the call away before the inner
  `call` on every call shape, including over TLS, mTLS, Unix, and `from_io`.
  Generated `FooServer::intercept`, `Router::intercept`, and
  `ServiceExt::intercept` reject before the
  body is read and stack in declaration order on every call shape, including
  over those transports (a later hop without the first interceptor's required
  metadata is `INVALID_ARGUMENT`, not `UNAUTHENTICATED`). A single
  `ServiceExt::intercept` on a hand-written `Service` still rejects before
  the handler (`UNAUTHENTICATED` without a token; the handler never runs)
  on those transports too. A single `FooServer::intercept` (no `add_service`)
  still rejects before the handler the same way on those transports. Interceptor extensions on a
  wrapping `Service` reach the handler `Request` and `Parts` on every call
  shape, including over TLS, mTLS, Unix, and `from_io`. `FooServer::intercept` then
  `add_service` keeps that reject on every mount and every call shape,
  including over TLS, mTLS, Unix, and `from_io`.
  The same `add_service` keeps `max_decoding_message_size` on every mount
  and every call shape, including over TLS, mTLS, Unix, and `from_io`.
  `max_encoding_message_size` then `add_service` keeps that outbound cap on
  every mount too: oversize encode is `RESOURCE_EXHAUSTED` on every Greeter
  call shape and on TestService UnaryCall / StreamingOutputCall /
  FullDuplexCall (EmptyCall and StreamingInputCall stay under a 16-byte
  cap), including over those transports.
  Generated handlers see
  `:authority` / `:scheme` / `Request` parts, a deadline Instant that
  elapses, TCP local/remote, Unix `peer_cred`, and `Incoming::peer`
  stamps on every call shape. `serve_connection` / `from_io` leave peer
  addrs, identity, and `peer_cred` unset on `Request` and `Parts`; `:scheme`
  follows the peer, including after `https_scheme`. That client `grpc-timeout` Instant elapses
  while the handler runs, including over TLS, mTLS, Unix, and `from_io`. A server interceptor `set_timeout` is the
  handler `Request` / `Parts` timeout and deadline Instant, not the client
  `peer_timeout`, including over TLS, mTLS, Unix, and `from_io`. Stacked
  server interceptors can only tighten that cap, on those transports too. Handler `Err` (nonzero `grpc-status` and
  custom details) is that status on every call shape. A packed
  `google.rpc.Status` from `with_error_details` is `Status::rpc` /
  `Status::error_details` on every call shape. A
  server interceptor `Err` ships those trailers the same way a handler
  `Err` does. `Status::set_rpc` / `set_code` keep trailing
  metadata. `StreamSender::fail` after headers ships those trailers and
  a packed `google.rpc.Status` the same way a handler `Err` does on a
  server response stream, including after a streamed DATA frame on
  server-streaming and bidi over h2c, TLS (including mTLS), Unix, and
  `from_io` (unary and client-streaming have no response DATA then trailers),
  including official TestService StreamingOutputCall / FullDuplexCall,
  generated Store Watch / Sync, Health Watch (Check is unary),
  reflection `ServerReflectionInfo`, and hand-written Reverser Channel
  Server / Bidi.
  On a client request sender it resets CANCEL
  (no request-side `grpc-status`); a client-streaming `Call`, or a bidi
  `Call` that has not yet seen headers, resolves with that status, not
  `UNAVAILABLE` from the reset, including over TLS, mTLS, Unix, and
  `from_io`. After bidi headers the received `Streaming`
  sees `CANCELLED`, not that status, including over those transports. A `Call`
  is fused after `Ready`. Client-streaming and bidi
  `(StreamSender, Call)` pairs are `must_use`. `Health::watch` ends when the
  client leaves, without waiting for the next status change. A server-streaming
  drain waiting for the next message ends on client RST.   Dropping a received
  `Streaming` before the end resets that RPC, including bidi while the send
  half is still held, over h2c, TLS (including mTLS), Unix, and `from_io`.
  Dropping the last `Channel` clone after headers still lets that received
  `Streaming` drain on those transports. A spawned server-streaming producer
  stays live until that drain; `Request::cancelled` does not fire when the
  handler returns, on those transports. An expired deadline is never a clean
  end of stream (`DEADLINE_EXCEEDED`, not `Ok(None)`), including over those
  transports. Server `max_connection_age` GOAWAY still lets in-flight Slow
  unary RPCs finish inside the grace window, including over TLS, mTLS, Unix,
  and `serve_connection`. Server `max_connection_idle` does not arm while an
  RPC is in flight on those transports. A `CallHandle` taken before await still cancels that
  live stream after headers, still cancels a server-streaming or bidi call
  waiting for headers, and a client-streaming handle still cancels
  after the sender is closed while the unary response is pending (dropping
  the `Call` or hitting the deadline after that half-close does the same).
  Official `cancel_after_begin` (cancel a client-streaming `Call` before any
  request message, while still holding the sender) is `CANCELLED`, not OK
  from a half-close, including over TLS, mTLS, Unix, and `from_io`.
  Greeter OK-path custom `-bin` trailers land on `Response::trailers` on
  unary and client-streaming and on `Streaming::trailers` on server-streaming
  and bidi, including when `trailers()` is called before draining messages;
  a `-bin` trailer must not appear as a header, including over TLS, mTLS,
  Unix, and `from_io`. A non-OK trailing `grpc-status` is `Err` from
  `Streaming::trailers` on those transports.
  CallHandle cancel of a live server-streaming or bidi stream after headers,
  a bidi Call waiting for headers, and client-streaming after the sender
  closes also run over TLS, mTLS, Unix, and `from_io`.
  `CallHandle` cancel also drops a hanging handler on every call shape
  before it runs to completion. Dropping the `Call` or cancelling it with a
  `CallHandle` drops a hanging handler on every call shape over TLS, mTLS,
  Unix, and `from_io` too. A handler that ignores its inbound request
  stream still answers on client-streaming and bidi rather than stalling the
  window, including over TLS, mTLS, Unix, and `from_io`. `max_concurrent_rpcs` refuses extra RPCs with
  `RESOURCE_EXHAUSTED` before the handler runs, on every call shape, including
  over TLS, mTLS, Unix, and `from_io`.
  A server-streaming or bidi deadline RSTs the send half before headers and
  after a half-close;
  after those headers that deadline still RSTs the parked
  send half, including over TLS, mTLS, Unix, and `from_io`. A request deadline
  before headers also fires on unary and client-streaming on those transports. Spawned handler work awaiting `Request::cancelled` sees the RST, including
  when the server deadline wins (signalled before trailers), including when a
  server interceptor `set_timeout` wins, over TLS, mTLS, Unix, and `from_io`. CallHandle cancel
  of spawned work on every call shape also runs over TLS, mTLS, Unix, and
  `from_io`. Spawned work also observes `Request::cancelled` when the RPC
  completes on those transports. Generated trait
  rustdoc names `Request::cancelled` on every call shape (and
  `StreamSender::closed` on server-streaming); unary `Channel` / generated
  client methods name `CallHandle`. Generated client-streaming and bidi
  methods name `StreamSender::fail`; server-streaming and bidi methods name
  `CallHandle` before and after headers, and deadline RST before and after
  headers.
  Generated method rustdoc names
  inbound/received `encoding` and interceptor timing. Methods omitted on generated traits answer `UNIMPLEMENTED`.
  `Router::new().add_service` path dispatch is `UNIMPLEMENTED` for an unmounted
  service and for a method a mounted service does not have, on every call
  shape, including over TLS, mTLS, Unix, and `from_io`. Remounting the same
  service name keeps the last handler; that last mount serves on every call
  shape, including over those transports. A hand-written
  `Service` served with `Server::new` answers the same `UNIMPLEMENTED` for
  methods it does not implement, on every call shape and those transports.
  Generated `FooClient::connect_tls_with` / `connect_lazy_with` /
  `connect_tls_lazy` / `connect_tls_lazy_with` / `connect_unix_lazy_with`
  and `Channel::connect_tls_with` apply to every call shape. Generated
  Store TLS (`serve_tls_with_shutdown` / `connect_tls_with` /
  `connect_tls_lazy_with`) and `send_compressed` gzip every Store shape,
  including gzip over TLS, mTLS, and Unix. `from_io` / `serve_connection` gzip
  those Store shapes the same way. Greeter `send_compressed` gzips every
  call shape over TLS, including over mTLS, Unix, and `from_io`. A TLS, mTLS, Unix,
  or `from_io` interceptor `Err(with_error_details)` unpacks on every Greeter
  shape. A client interceptor sees Outgoing path / service / method / authority /
  scheme on every Greeter shape, including over TLS, mTLS, Unix, and `from_io`.
  A generated Greeter handler `Err(with_error_details)` unpacks on every call
  shape, including over TLS, mTLS, Unix, and `from_io`. Official `TestService` `send_compressed` gzips EmptyCall /
  StreamingOutputCall / StreamingInputCall / FullDuplexCall, including over
  TLS, mTLS, Unix, and `from_io`. A wrapping `Service` `send_compressed` gzips every
  hand-written Reverser Channel API, including over TLS, mTLS, Unix, and `from_io`. Health
  `send_compressed` gzips Check and Watch, including over TLS, mTLS, Unix, and
  `from_io`; reflection `send_compressed` gzips the bidi `list_services`
  method, including over TLS, mTLS, Unix, and `from_io`. A client interceptor
  sees Outgoing path / service / method / authority / scheme on Health
  Check/Watch, the reflection bidi method, and generated Store Get / Watch
  / PutAll / Sync, including over TLS, mTLS, Unix, and `from_io`. A packed `google.rpc.Status` from interceptor
  `Err(with_error_details)` unpacks on those Store, Health, and reflection
  methods the same way, including over TLS, mTLS, Unix, and `from_io`. A generated Store handler `Err(with_error_details)`
  unpacks on Get / Watch / PutAll / Sync too, including over TLS, mTLS, Unix, and `from_io`. A Health handler
  `Err(with_error_details)` unpacks on Check and Watch too, including over TLS, mTLS, Unix, and `from_io`. A reflection
  handler `Err(with_error_details)` unpacks on the bidi `list_services` method too, including over TLS, mTLS, Unix,
  and `from_io`. A wrapping `Service`
  interceptor `Err(with_error_details)` unpacks on every hand-written
  Reverser Channel API, including over TLS, mTLS, Unix, and `from_io`, a wrapping
  `Service` handler `Err(with_error_details)` unpacks on those APIs the same way,
  and a client interceptor stamps Outgoing path facts on those APIs, including over TLS,
  mTLS, Unix, and `from_io`.
  Official `TestService` interceptor `Err(with_error_details)` unpacks on EmptyCall /
  StreamingOutputCall / StreamingInputCall / FullDuplexCall, including over
  TLS, mTLS, Unix, and `from_io`, a generated TestService handler
  `Err(with_error_details)` unpacks on those methods too, including over TLS, mTLS,
  Unix, and `from_io`, and a client interceptor stamps Outgoing path facts
  on those methods, including over TLS, mTLS, Unix, and `from_io`.
  GCP-auth and ORCA stay out; load balancing, application retries, and
  hedging are documented omissions. The tonic adapter still covers
  health/gzip/reflection via tonic crates for stacks that stay on tonic.
- There are no arena views.
- Generated `google.protobuf.Timestamp`, `Duration`, `Empty`, and the
  proto3 wrappers (BoolValue, Int32Value, Int64Value, UInt32Value,
  UInt64Value, FloatValue, DoubleValue, StringValue, BytesValue)
  JSON / text are field-wise. Timestamp / Duration use the official
  proto3 JSON string mapping (text is `seconds` / `nanos`). Empty
  JSON is `{}`. Wrappers encode as the wrapped JSON value, not an
  object. Text for Empty / wrappers is the existing field mapping.
  Other WKT (Struct, Value, ListValue, Any, FieldMask) still go
  through `DynamicMessage`; a field-wise object for those would
  disagree with the official mapping. Person-shaped proto3, the
  extra proto3 scalars (bool, int64, uint32, uint64, sint32,
  sint64, fixed32, fixed64, sfixed32, sfixed64, float, double,
  bytes, open proto3 enums, plus repeated and scalar maps of those
  types), real oneofs of that set (`OneofHole`), and messages whose
  only WKT fields are Timestamp / Duration / Empty / wrappers are
  field-wise. Map-of-enum is skipped: map-entry descriptors used at
  codegen do not carry enum names, so names would be a guess. TAT
  is not closed (it still has the other WKT). Remaining is not
  closed.
- Edition 2024 extensions, CORD / cpp VIEW, and gtest matchers are missing.
- Maps are last-wins `Vec` plus a lazy index (not a codec win).
- `name_4kib` Codec combined beats prost (gated). `blob_4kib` still
  wins. `rpc_sparse` decode and `tags_32` decode vs v4 are gated.
  Leftover unary item is `name_80` combined (still a loss). A draft
  heap-copy try (#57) shrank same-host leftover; leftover is
  `merge_inner`. The cut is not on main. Flatten `merge_inner`
  (#39) stays discarded. Survey: `docs/benchmarks.md`. Closed
  inventories (notes + harnesses, not merged as wins):
  `docs/inventory/`.
  [#27](https://github.com/mingley/pure-protobuf/pull/27) rust_out 234
  errors is superseded by #42.

## Skipped rust/test/shared files

- `ctype_cord_test.rs`
- `gtest_matchers_test.rs`
- `no_internal_access_test.rs` (`__internal` is a module)
- `package_disambiguation_test.rs` (empty)
- `extensions_test.rs` (edition 2024 proto)
- edition2023 `str_view` cpp VIEW (ordinary string)
- `proto!` `#[cfg(bzl)]` qualified paths

## Publish

`pbrs` is registry-ready (`cargo publish -p pbrs --dry-run` on
crates.io). Live upload is not done from this tree. Do not publish as
`protobuf`. Nearby name `pb-rs` is quick-protobuf. `protobuf-tonic`
keeps a path (git until a registry version exists) dependency on `pbrs`;
`cargo publish -p protobuf-tonic` cannot succeed until that version
exists.
