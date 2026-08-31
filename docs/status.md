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
  large_unary). Server-streaming and bidi ping-pong throughput are gated at
  90% of tonic 0.14 (same noise band). Client-streaming upload is gated at
  90% of tonic 0.14 the same way. QPS is reported, not gated (empty/large at
  conc=1/conns=1 and conc=16/conns=4). Nonzero RPC errors still fail
  the process. `scripts/grpc-server-bench.sh` also reports loopback bidi
  ping-pong and client-streaming upload against grpc-go (not the Xeon unary
  tables).   `Channel::connect_pool` opens independent h2 driver
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
  Unix, and `from_io`. `Health::list` returns a snapshot of every known name
  (the process `""` and names you set); unknown names are omitted, matching
  `HealthReporter::names`, including over TLS, mTLS, Unix, and `from_io`.
  Reflection
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
  Distinct from packed `google.rpc.Status`. On the receive path ASCII
  `grpc-status` / `grpc-message` win: a handler `Err` whose packed
  `google.rpc.Status` disagrees with those trailers is `code()` / `message()`
  from ASCII and `Status::rpc` as-is, on every Greeter call shape over TLS,
  mTLS, Unix, and `from_io`. Distinct from matching `with_error_details`.
  `ChannelConfig::max_encoding_message_size`
  / `max_decoding_message_size` at `connect_tls_with` / `connect_unix_with` /
  `from_io_with` is `RESOURCE_EXHAUSTED` on every call shape, distinct from
  wrapping a live Channel or generated client after connect.   A HealthClient
  `max_encoding_message_size` / `max_decoding_message_size` is
  `RESOURCE_EXHAUSTED` on Check and Watch over TLS, mTLS, Unix, and `from_io`,
  distinct from the Health server decoding cap. `HealthClient::message_limits`
  refuses the same oversize, distinct from those single-cap wrappers. A ServerReflectionClient
  `max_encoding_message_size` / `max_decoding_message_size` is
  `RESOURCE_EXHAUSTED` on the one bidi method over those transports, distinct
  from the reflection server decoding cap. `ServerReflectionClient::message_limits`
  refuses the same oversize, distinct from those single-cap wrappers. Hand-written `Channel::unary` /
  `server_streaming` / `client_streaming` / `bidi` honor those same client
  caps as `RESOURCE_EXHAUSTED` on every call shape over TLS, mTLS, Unix, and
  `from_io`, distinct from generated GreeterClient wrappers. A TestServiceClient
  `max_encoding_message_size` / `max_decoding_message_size` is
  `RESOURCE_EXHAUSTED` on UnaryCall / StreamingOutputCall / StreamingInputCall
  / FullDuplexCall over those transports, distinct from the TestService server
  add_service caps. A TestServiceClient `message_limits` /
  `ChannelConfig::message_limits` at `connect_tls_with` / `connect_unix_with` /
  `from_io_with` is `RESOURCE_EXHAUSTED` on those methods, distinct from
  wrapping the single-cap setters. `Channel::message_limits` / generated
  `FooClient::message_limits` / `ChannelConfig::message_limits` refuse
  oversize the same way as the single-cap setters over TLS, mTLS, Unix, and
  `from_io`. `Server::message_limits` / `Router::message_limits` / generated
  `FooServer::message_limits` / `ServerConfig::message_limits` refuse inbound
  or outbound oversize as `RESOURCE_EXHAUSTED` over TLS, mTLS, Unix, and
  `serve_connection`, distinct from the single-cap setters. TLS
  (rustls + Graviola), `grpc.health.v1` Check/List/Watch, and
  `grpc.reflection.v1` ship in the kernel. Unary/server-streaming that race
  a connection death after the slot looked live redial once (transparent
  retry) — proven for unary and server-streaming on h2c and TLS.
  Client-streaming and bidi retry once if HEADERS never went out.
  `Server::max_connection_age` / generated `FooServer::max_connection_age`
  name that the next RPC of every call shape redials, including over TLS, mTLS,
  and Unix (`from_io` cannot redial), and that transparent
  retry of the same in-flight RPC is unary and server-streaming after request
  bytes, client-streaming and bidi before HEADERS. Unix accept loops expose `SO_PEERCRED` on `Rpc::peer_cred`.
  Custom `Incoming` implementations stamp local_addr / mTLS identity /
  Unix credentials / transport scheme via `Incoming::peer` and
  `ConnectionInfo`. Compiling `ConnectionInfo` peer dumps live on the `Incoming` rustdoc. TLS `:scheme https` and mTLS `peer_identity` apply to
  every call shape. HTTP/2 PING keepalive still serves every Greeter shape
  after PINGs fire on h2c, TLS (including mTLS), Unix, and `from_io`. TCP
  `SO_KEEPALIVE` is TCP-only and still serves every Greeter shape on h2c, TLS,
  and mTLS. `Server::max_concurrent_connections` refuses a second TCP, TLS,
  mTLS, or Unix dial with `UNAVAILABLE` while the cap is full (`from_io` is
  not an accept loop).   A `ChannelConfig::connections` pool larger than that
  cap fails the whole dial as `UNAVAILABLE` on TLS, mTLS, and Unix (`from_io`
  cannot pool), including Health Check/List/Watch and reflection
  `ServerReflectionInfo`.   Oversize metadata against `Server::max_header_list_size` /
  `Router::max_header_list_size` / `ServerConfig::max_header_list_size` /
  generated `FooServer::max_header_list_size` is refused over TLS, mTLS, Unix,
  and `serve_connection`, distinct from a raw HTTP/2 peer and from wrapping
  only the generated Greeter server setter. Health Check/List/Watch and reflection
  `ServerReflectionInfo` refuse the same flood then keep serving a healthy
  client. Official TestService EmptyCall / StreamingOutputCall /
  StreamingInputCall / FullDuplexCall refuse the same flood then keep serving
  a healthy client. Hand-written Reverser `Channel` APIs refuse the same flood
  then keep serving Reverse / Server / Client / Bidi on a healthy client.
  `ChannelConfig::max_header_list_size`
  refuses oversize response headers or trailers as `UNAVAILABLE` over TLS, mTLS,
  Unix, and `from_io`, distinct from the server inbound cap.
  `header_table_size` is HTTP/2 `SETTINGS_HEADER_TABLE_SIZE` (HPACK dynamic
  table, default 4096). Distinct from `max_header_list_size`. Handshake-only
  on the client. A well-behaved peer still completes every call shape at a
  smaller table (including 0).
  `data_frame_budget` is the small-DATA framing budget (default 25600).
  Distinct from the connection window (flow-control bytes). h2 Auto (half
  the window) is not exposed. Handshake-only on the client. A well-behaved
  peer still completes every call shape at this framing budget.
  `max_concurrent_reset_streams` is remembered locally-reset HTTP/2 stream
  IDs (default 50). Distinct from pending-reset and protocol-error RST
  (those GOAWAY). Handshake-only on the client. Exceeding this evicts the
  oldest ID, not `ENHANCE_YOUR_CALM`. Frames on a purged ID are a
  connection `PROTOCOL_ERROR`. A well-behaved peer still completes every
  call shape at this memory cap.
  `reset_stream_duration` is how long those IDs are remembered (default
  1 s). Distinct from the count cap. Handshake-only on the client. After
  that duration the ID is forgotten, not `ENHANCE_YOUR_CALM`.
  `Server::max_concurrent_streams` / `Router::max_concurrent_streams` /
  generated `FooServer::max_concurrent_streams` /
  `ServerConfig::max_concurrent_streams` serialize extra RPCs on the same
  HTTP/2 connection (a well-behaved client waits; both still complete) over
  TLS, mTLS, Unix, and `serve_connection`, distinct from wrapping only the
  generated Greeter setter and from `max_concurrent_rpcs` which refuses extras
  as `RESOURCE_EXHAUSTED`. `ChannelConfig::max_concurrent_streams` advertises
  client SETTINGS and does not serialize Slow RPCs (push is disabled) over TLS,
  mTLS, Unix, and `from_io`. `Server::max_frame_size` / `Router::max_frame_size` /
  generated `FooServer::max_frame_size` / `ServerConfig::max_frame_size` still
  serve every Greeter and Store shape at the HTTP/2 16 KiB SETTINGS minimum over
  TLS, mTLS, Unix, and `serve_connection`, distinct from wrapping only the
  generated Greeter setter, from header-list refuse, and from stream-cap
  serialize. `ChannelConfig::max_frame_size` advertises client SETTINGS and
  still serves every Greeter shape when a well-behaved server splits DATA over
  TLS, mTLS, Unix, and `from_io`. `Server::initial_stream_window_size` /
  `Router::initial_stream_window_size` / generated
  `FooServer::initial_stream_window_size` / `ServerConfig::initial_stream_window_size`
  (and the matching connection-window setters) still serve every Greeter and
  Store shape at a 64 KiB stream / 128 KiB connection window over TLS, mTLS, Unix,
  and `serve_connection`, distinct from wrapping only the generated Greeter
  setter, from frame-size still-serves, and from stream-cap serialize.
  `ChannelConfig::initial_stream_window_size` /
  `ChannelConfig::initial_connection_window_size` advertise client windows and
  still serve every Greeter shape when a well-behaved server completes over TLS,
  mTLS, Unix, and `from_io`. [`HealthServer::max_frame_size`] still serves Check
  and Watch, and [`ServerReflectionServer::max_frame_size`] still serves
  `ServerReflectionInfo`, at the HTTP/2 16 KiB SETTINGS minimum over TLS, mTLS,
  Unix, and `serve_connection`, distinct from wrapping only GreeterServer.
  [`TestServiceServer::max_frame_size`] still serves EmptyCall /
  StreamingOutputCall / StreamingInputCall / FullDuplexCall, and hand-written
  Reverser `Channel` APIs still serve Reverse / Server / Client / Bidi, at the
  HTTP/2 16 KiB SETTINGS minimum over TLS, mTLS, Unix, and `serve_connection`,
  distinct from wrapping only GreeterServer. `Server::max_send_buffer_size` /
  `Router::max_send_buffer_size` / generated `FooServer::max_send_buffer_size` /
  `ServerConfig::max_send_buffer_size` still serve every Greeter and Store shape
  at a 16 KiB send buffer over TLS, mTLS, Unix, and `serve_connection`, distinct
  from wrapping only the generated Greeter setter, from frame-size still-serves,
  and from window still-serves. `ChannelConfig::max_send_buffer_size` applies
  client outbound backpressure and still serves every Greeter shape when a
  well-behaved server completes over TLS, mTLS, Unix, and `from_io`. A
  `TestServiceClient` pool larger than `TestServiceServer::max_concurrent_connections`
  fails the whole dial as `UNAVAILABLE` on TLS, mTLS, and Unix
  (`from_io_with` cannot pool). Hand-written Reverser `Server::max_concurrent_connections`
  refuses a `ChannelConfig::connections` pool the same way, distinct from wrapping
  only GreeterServer. Generated `FooServer::max_pending_accept_reset_streams`
  still serves every Greeter shape at a pending-reset cap of 1 over TLS, mTLS,
  Unix, and `serve_connection`; a well-behaved client never fills that queue,
  distinct from a raw HTTP/2 RST flood. `Server::max_pending_accept_reset_streams`
  / `Router::max_pending_accept_reset_streams` / generated
  `FooServer::max_pending_accept_reset_streams` /
  `ServerConfig::max_pending_accept_reset_streams` still serve every Greeter and
  Store shape at that cap, distinct from wrapping only the generated Greeter
  setter. `ChannelConfig::max_pending_accept_reset_streams` caps the client's
  accept queue and still serves every Greeter shape when a well-behaved server
  never fills it, over TLS, mTLS, Unix, and `from_io`.   A raw HTTP/2 RST flood
  that exceeds `max_pending_accept_reset_streams` drops that connection
  (`ENHANCE_YOUR_CALM`); the accept loop still serves a well-behaved client.
  Distinct from wrap still-serves. h2c-only (`RawPeer`; no TLS raw peer).
  `ServerConfig::max_local_error_reset_streams` (default 1024) caps RSTs we
  send after an invalid frame. Exceeding it is `ENHANCE_YOUR_CALM`; the
  accept loop still serves. Distinct from rapid reset (remotely-reset
  streams). h2's `None` disable is not exposed. A raw PRIORITY
  self-dependency flood is h2c-only (`RawH2`). `Server` / `Router` /
  generated `FooServer` / `ChannelConfig` still serve at that cap on h2c
  and `from_io`.
  `ServerConfig::max_concurrent_reset_streams` (default 50) remembers
  locally-reset stream IDs after we RST. Exceeding it evicts the oldest
  ID; it is not `ENHANCE_YOUR_CALM`. Distinct from rapid-reset GOAWAY and
  protocol-error RST GOAWAY. `Server` / `Router` / generated `FooServer` /
  `ChannelConfig` still serve at that memory cap on h2c and `from_io`.
  `ServerConfig::reset_stream_duration` (default 1 s) is how long those
  IDs stay in memory. Distinct from the count cap. After that duration
  the ID is forgotten, not `ENHANCE_YOUR_CALM`.
  The gRPC guide Distincts that well-behaved still-serves from the raw flood,
  and Distincts `ChannelConfig::max_pending_accept_reset_streams` as the
  client accept queue, not the server cap.
  `HealthServer::max_pending_accept_reset_streams` still serves Check, List, and Watch
  at a pending-reset cap of 1 over TLS, mTLS, Unix, and `serve_connection`.
  `ServerReflectionServer::max_pending_accept_reset_streams` still serves the
  one bidi method at that cap on those transports.
  Official TestService and hand-written Reverser still serve every shape at a
  pending-reset cap of 1 over TLS, mTLS, Unix, and `from_io` (mTLS Reverser
  uses `Reverser::mtls` with the same leaf).
  `HealthServer::max_send_buffer_size` still serves Check, List, and Watch at a 16 KiB
  send buffer over TLS, mTLS, Unix, and `serve_connection`.
  `ServerReflectionServer::max_send_buffer_size` still serves the one bidi
  method at that buffer on those transports. Official TestService and
  hand-written Reverser still serve every shape at a 16 KiB send buffer over
  TLS, mTLS, Unix, and `from_io` (mTLS Reverser uses `Reverser::mtls` with the
  same leaf). `HealthServer` HTTP/2 windows still serve Check, List, and Watch at
  64 KiB / 128 KiB over TLS, mTLS, Unix, and `serve_connection`.
  `ServerReflectionServer` windows still serve the one bidi method at those
  sizes on those transports. Official TestService and hand-written Reverser
  still serve every shape at 64 KiB / 128 KiB windows over TLS, mTLS, Unix,
  and `from_io` (mTLS Reverser uses `Reverser::mtls` with the same leaf). A mute TCP, TLS, mTLS, or Unix peer that never finishes
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
  `ChannelConfig::max_connection_age` closes the client socket even while RPCs
  are in flight; in-flight get grace, then the driver stops. Distinct from idle.
  Keepalive PINGs do not postpone age.
  `Channel::connected` is a snapshot of live sockets. Distinct from gRPC GetState.
  `Outgoing::connected` is that same snapshot when a client interceptor runs.
  Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
  `Channel::https_scheme` sends `:scheme https` on a
  `from_io` clone (no TLS handshake; no-op on TCP/Unix);
  `Channel::scheme` / generated `FooClient::scheme` / `FooClient::authority` /
  `FooClient::grpc_user_agent` read that overlay and the other interceptor-visible
  channel facts. Interceptors run when the RPC method is invoked (all four
  shapes) on h2c, TLS (including mTLS), Unix, and `from_io`, including official
  TestService methods, hand-written Reverser `Channel` APIs, generated Store,
  Health Check/List/Watch, and reflection `ServerReflectionInfo`, not on first poll of the `Call`. Interceptors and generated
  handlers see `MessageLimits` on `Rpc::limits` / `Request::limits` /
  `Parts::limits`, including over TLS, mTLS, Unix, and `from_io`, the
  method path on `Rpc::path` / `Request::path` / `Parts::path`, including
  over those transports, `accepts_gzip` / encoding (unary Compressed-Flag on
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
  call shape over TLS, mTLS, Unix, and `from_io`). `Response::extensions` is
  local typed context, not on the wire. Distinct from metadata. A received
  reply starts empty.   `Server::send_compressed` / `Router::send_compressed`
  gzip every call shape when the client advertises gzip (`Response::compressed`
  and `encoding()` are gzip), including over TLS, mTLS, Unix, and `from_io`.
  `gzip_compression_level` is deflate effort (default 1). Distinct from
  `send_compressed`, which is on or off. 0 stores; 9 is best.
  `Outgoing::gzip_level` is that overlay in a client interceptor. Distinct from
  `compresses_outbound` (on or off). An interceptor cannot change it.
  `Rpc::gzip_level` is that overlay in a server interceptor. Distinct from
  `Rpc::compresses_outbound` (on or off). An interceptor cannot change it.
  `Channel::gzip_level` reads the deflate overlay without colliding with `gzip_compression_level`. Same overlay as `Outgoing::gzip_level`.
  `Server::gzip_level` reads the deflate overlay without colliding with `gzip_compression_level`. Same overlay as `Rpc::gzip_level`.
  `Outgoing::compresses_outbound` is that overlay in a client interceptor. Distinct from `compress` (per-RPC). An interceptor cannot change it.
  `Rpc::compresses_outbound` is that overlay in a server interceptor. Distinct from `Outgoing::compresses_outbound` (client).
  `Channel::compresses_outbound` reads the outbound gzip overlay without colliding with `send_compressed`. Same overlay as `Outgoing::compresses_outbound`.
  `Server::compresses_outbound` reads the outbound gzip overlay without colliding with `send_compressed`. Same overlay as `Rpc::compresses_outbound`.
  `Outgoing::accepts_compressed` is that overlay in a client interceptor. Distinct from `gzip_level` (deflate effort). An interceptor cannot change it.
  `Rpc::accepts_compressed` is that overlay in a server interceptor. Distinct from `Rpc::accepts_gzip` (peer advertisement). An interceptor cannot change it.
  `Channel::accepts_compressed` reads the inbound gzip overlay without colliding with `accept_compressed`. Same overlay as `Outgoing::accepts_compressed`.
  `Server::accepts_compressed` reads the inbound gzip overlay without colliding with `accept_compressed`. Same overlay as `Rpc::accepts_compressed`.
  `Outgoing::concurrent_rpc_limit` is that overlay in a client interceptor. Distinct from `waits_for_ready` (connection). An interceptor cannot change it.
  `Rpc::concurrent_rpc_limit` is that overlay in a server interceptor. Distinct from HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS` (waits).
  `Channel::concurrent_rpc_limit` reads the RPC-cap overlay without colliding with `max_concurrent_rpcs`. Same overlay as `Outgoing::concurrent_rpc_limit`.
  `Server::concurrent_rpc_limit` reads the RPC-cap overlay without colliding with `max_concurrent_rpcs`. Same overlay as `Rpc::concurrent_rpc_limit`.
  `Outgoing::waits_for_ready` is that overlay in a client interceptor. Distinct from `connected` (live snapshot). An interceptor cannot change it.
  `Channel::waits_for_ready` reads the wait-for-ready overlay without colliding with `wait_for_ready`. Same overlay as `Outgoing::waits_for_ready`.
  `Outgoing::connected` is the live-socket snapshot in a client interceptor. Distinct from `waits_for_ready` (overlay).
  `Channel::connected` is that same snapshot without an interceptor.
  `FooClient::connected` is the live-socket snapshot on a generated client. Distinct from `waits_for_ready` (overlay). Same snapshot as `Channel::connected`.
  `Outgoing::rpc_timeout` is that overlay in a client interceptor. Distinct from `timeout` (per-RPC). An interceptor cannot change it.
  `Rpc::rpc_timeout` is that overlay in a server interceptor. Distinct from `Rpc::timeout` (interceptor cap).
  `Channel::rpc_timeout` reads the deadline overlay without colliding with `timeout`. Same overlay as `Outgoing::rpc_timeout`.
  `Server::rpc_timeout` reads the deadline overlay without colliding with `timeout`. Same overlay as `Rpc::rpc_timeout`.
  `Outgoing::stream_buffer_size` is that overlay in a client interceptor. Distinct from `limits` (message size). Applies to client-streaming and bidi. An interceptor cannot change it.
  `Channel::stream_buffer_size` reads the stream-queue overlay without colliding with `stream_buffer`. Same overlay as `Outgoing::stream_buffer_size`.
  `Outgoing::limits` is that overlay in a client interceptor. Distinct from `send_buffer_size` (HTTP/2). An interceptor cannot change it.
  `Rpc::limits` is that overlay in a server interceptor. Distinct from `Outgoing::limits` (client).
  `Channel::limits` reads the message-cap overlay without colliding with `message_limits`. Same overlay as `Outgoing::limits`.
  `Server::limits` reads the message-cap overlay without colliding with `message_limits`. Same overlay as `Rpc::limits`.
  `Channel::send_buffer_size` reads the HTTP/2 send buffer overlay without colliding with `max_send_buffer_size`. Same overlay as `Outgoing::send_buffer_size`.
  `Server::send_buffer_size` reads the HTTP/2 send buffer overlay without colliding with `max_send_buffer_size`. Same overlay as `Rpc::send_buffer_size`.
  `Outgoing::send_buffer_size` is that overlay in a client interceptor. Distinct from `stream_buffer_size` (queue depth). An interceptor cannot change it.
  `Rpc::send_buffer_size` is that overlay in a server interceptor. Distinct from `Outgoing::send_buffer_size` (client). An interceptor cannot change it.
  `Response::send_buffer_size` is that overlay in a response interceptor. Distinct from `Request::send_buffer_size` (inbound). Distinct from `Rpc::send_buffer_size` (before the handler). An interceptor cannot change it.
  `Response::path` is kernel-stamped after `Ok` (server) and after a successful receive (client). Distinct from `Request::path` (inbound). Distinct from `Outgoing::path` (before send). An interceptor cannot change it.
  `Response::gzip_level` is that overlay in a response interceptor. Distinct from `compress` (on or off). Distinct from `Rpc::gzip_level` (before the handler). An interceptor cannot change it.
  `Response::compresses_outbound` is that overlay in a response interceptor. Distinct from `compress` (per-RPC). Distinct from `Rpc::compresses_outbound` (before the handler). An interceptor cannot change it.
  `Response::accepts_gzip` is the peer advertisement in a response interceptor. Distinct from `encoding` (received). Distinct from `Rpc::accepts_gzip` (before the handler). An interceptor cannot change it.
  `Response::deadline` is kernel-stamped after `Ok`, when writing. Distinct from `Request::deadline` (inbound). Distinct from `Rpc::deadline` (computed when that getter runs). An interceptor cannot change it.
  `Response::timeout` is the duration stamped at dispatch, in a response interceptor. Distinct from `deadline` (Instant). Distinct from `Rpc::timeout` (interceptor cap). An interceptor cannot change it.
  `Response::limits` is the encode cap in a response interceptor. Distinct from `Request::limits` (inbound). Distinct from `Rpc::limits` (before the handler). An interceptor cannot change it.
  `Response::peer_timeout` is the client's `grpc-timeout` in a response interceptor. Distinct from `timeout` (effective). Distinct from `Rpc::peer_timeout` (before the handler). An interceptor cannot change it.
  `Response::rpc_timeout` is the server overlay in a response interceptor. Distinct from `timeout` (effective). Distinct from `Rpc::rpc_timeout` (before the handler). An interceptor cannot change it.
  `Response::accepts_compressed` is the inbound overlay in a response interceptor. Distinct from `accepts_gzip` (peer advertisement). Distinct from `Rpc::accepts_compressed` (before the handler). An interceptor cannot change it.
  Inbound gzip is on by default; `Server::accept_compressed(false)` /
  `Channel::accept_compressed(false)` refuse `grpc-encoding: gzip` as
  `UNIMPLEMENTED` and advertise `identity` only (distinct from tonic's
  opt-in `accept_compressed`).
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
  methods, generated Store Get / Watch / PutAll / Sync, Health Check, List, and Watch
  and reflection `ServerReflectionInfo`. `clear_compress` then `set_compress(compresses_outbound())`
  reapplies channel gzip on those transports plus `from_io`, including official
  TestService methods, hand-written Reverser `Channel` APIs, generated Store
  Get / Watch / PutAll / Sync, Health Check, List, and Watch, and reflection
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
  deadline apply on those Store dialers too. Health Check, List, and Watch
  retry until listen on the same dialers. A client interceptor
  `set_wait_for_ready(true)` retries Check, List, and Watch until listen on those dialers too. Opt-out and a
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
  Store, Health Check/List/Watch, and reflection `ServerReflectionInfo`; nothing is sent. A reserved
  `grpc-*` or hop-by-hop interceptor `insert` is `INVALID_ARGUMENT` on those same
  paths. A packed `google.rpc.Status` on that
  local `Err` is `Status::rpc` / `Status::error_details` on the Call.
  Outgoing getters apply to every call shape. Kernel `user-agent` (and a
  `Channel::user_agent` prefix) is sent on every shape, including over h2c, TLS
  (including mTLS), Unix, and `from_io`; inserting `user-agent`
  into metadata cannot override it. `Request::set_user_agent` prefixes this RPC
  (kernel suffix stays); an interceptor `Outgoing::set_user_agent` that runs
  after the call site wins.
  `Outgoing::user_agent_is_set` distinguishes that override from the channel value on this packed-status interceptor path.
  `Outgoing::clear_user_agent` restores the channel user-agent after a packed-status interceptor prefix.
  `Outgoing::clear_wait_for_ready` restores the channel wait-for-ready overlay after a packed-status interceptor choice.
  `Outgoing::clear_compress` then `set_compress` from `compresses_outbound` reapplies channel gzip after a packed-status interceptor choice.
  `Outgoing::clear_timeout` opts out of the channel timeout after a packed-status interceptor choice.
  `Outgoing::wait_for_ready_is_set` distinguishes an unset wait-for-ready from an explicit `false` on this packed-status interceptor path.
  `Outgoing::compress_is_set` distinguishes unset compress from an explicit `false` on this packed-status interceptor path.
  `Status::from_error_details` is the typed bag after this packed-status interceptor Err; a local reject never opens a stream.
  `Status::from_error_details` is the typed bag after this packed-status server intercept Err; those trailers reach the client without reading the body.
  `ResponseParts::compress_is_set` is occupancy on this packed-status on_response path, so a later interceptor can fill compress only when unset.
  `ResponseParts::clear_compress` restores the server gzip overlay after Server on_response on this packed-status on_response path.
  `Status::from_error_details` is the typed bag after this packed-status server on_response Err; a local reject is trailers-only after handler Ok.
  `ResponseParts::clear_compress` drops a compress choice after Channel on_response on this packed-status on_response path; a received reply has no server gzip overlay to restore.
  `Status::from_error_details` is the typed bag after this packed-status Channel on_response Err; a local reject fails the Call after a successful receive.
  Caller extensions on `Request::extensions_mut`
  and channel `MessageLimits` on `Outgoing::limits` are visible to a client
  interceptor on those transports plus `from_io`, including official TestService
  methods and hand-written Reverser `Channel` APIs, generated Store Get / Watch /
  PutAll / Sync, Health Check, List, and Watch, and reflection `ServerReflectionInfo`. A `Channel::user_agent` prefix
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
  `FooServer::on_response` / `Server::on_response` / `Router::on_response`
  run after the handler returns `Ok`. `Response::extensions` is local;
  stamp metadata to send a header. `Err` after the handler already ran
  is trailers-only (including `with_error_details`). A handler `Err`
  skips this hook. First registered runs first. Applies to every call
  shape, including over TLS, mTLS, Unix, and `from_io`.
  `Channel::on_response` / `FooClient::on_response` run after a successful
  receive. A received reply starts empty; this hook inserts typed context
  the peer cannot. `Err` fails that Call (the peer already sent OK). A
  non-OK peer status skips this hook. First registered runs first. Applies
  to every call shape, including over TLS, mTLS, Unix, and `from_io`.
  A received reply does not carry Channel overlays: `gzip_level` is not the peer's deflate effort; `compresses_outbound`, `accepts_gzip`, and `accepts_compressed` are `false`; `deadline`, `timeout`, `limits`, `peer_timeout`, `rpc_timeout`, and `send_buffer_size` are `None`.
  Compiling intercept / on_response overlay dumps live on the `hello` module rustdoc (`GreeterClient` / `GreeterServer`).
  `ServiceExt::on_response` / `Intercepted::on_response` is per-service and
  does not cover other mounts; a Server / Router hook still runs first.
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
  `Status::error_details` on every call shape. Unknown types stay in `ErrorDetails::unknown` so a custom detail is not dropped on a round-trip. `Status::error_info` is the
  packed `ErrorInfo` without unpacking the bag. Distinct from `error_details`.
  Distinct from `retry_delay` (a wait hint). `RetryInfo::with_retry_delay` builds that payload. `ErrorInfo::with_reason` builds that payload. `Status::bad_request` is packed
  field violations.   Distinct from `error_info`. `BadRequest::with_field` builds
  that payload. `Status::quota_failure` is packed quota subjects.
  Distinct from `is_retryable` (`RESOURCE_EXHAUSTED` is never A6-retryable)
  and from `bad_request`. `QuotaFailure::with_violation` builds that payload.
  `Status::precondition_failure` is packed type and subject.
  Distinct from `quota_failure` (`FAILED_PRECONDITION` is never A6-retryable)
  and from `bad_request`. `PreconditionFailure::with_violation` builds that payload.
  `Status::help` is packed documentation links. Distinct from failure
  classifications: links can sit next to a retryable UNAVAILABLE.
  `Help::with_link` builds that payload.
  `Status::localized_message` is packed locale text. Distinct from the ASCII
  `grpc-message`. Distinct from `help` (a docs URL).
  `LocalizedMessage::with_locale` builds that payload.
  `Status::request_info` is packed request_id for logs. Distinct from
  `error_info` (a metadata map). Distinct from `help` (a docs URL).
  `RequestInfo::with_request_id` builds that payload.
  `Status::resource_info` is packed resource type and name. Distinct from
  `quota_failure` (a quota subject). Distinct from `request_info` (a request_id).
  `ResourceInfo::with_resource` builds that payload.
  `Status::debug_info` is packed operator stack. Distinct from
  `localized_message` (a locale). Distinct from `help` (a docs URL).
  `DebugInfo::with_stack` builds that payload. A
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
  RPCs finish inside the grace window on every Greeter call shape, including
  over TLS, mTLS, Unix, and `serve_connection`. Server `max_connection_idle`
  does not arm while Slow is in flight on every Greeter call shape on those
  transports.   Client `ChannelConfig::max_connection_idle` leaves those same
  in-flight Slow shapes alone. `ChannelConfig::max_connection_age` still
  lets those in-flight Slow shapes finish inside the grace window. Graceful drain finishes in-flight Slow on
  every Greeter call shape on those transports. A `CallHandle` taken before await still cancels that
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
  window, including over TLS, mTLS, Unix, and `from_io`.   `max_concurrent_rpcs` refuses extra RPCs with
  `RESOURCE_EXHAUSTED` before the handler runs, on every call shape, including
  over TLS, mTLS, Unix, and `from_io`.
  `Channel::max_concurrent_rpcs` is the client dual: extras are
  `RESOURCE_EXHAUSTED` before the stream opens on those transports, including
  `from_io`. Distinct from `SETTINGS_MAX_CONCURRENT_STREAMS`, which waits.
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
  shape, including over those transports. `add_optional_service` mounts when
  `Some`; `None` is a no-op. Distinct from `add_service`, which always mounts.
  A hand-written
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
  `send_compressed` gzips Check, List, and Watch, including over TLS, mTLS, Unix, and
  `from_io`; reflection `send_compressed` gzips the bidi `list_services`
  method, including over TLS, mTLS, Unix, and `from_io`. A client interceptor
  sees Outgoing path / service / method / authority / scheme on Health
  Check/List/Watch, the reflection bidi method, and generated Store Get / Watch
  / PutAll / Sync, including over TLS, mTLS, Unix, and `from_io`. A packed `google.rpc.Status` from interceptor
  `Err(with_error_details)` unpacks on those Store, Health, and reflection
  methods the same way, including over TLS, mTLS, Unix, and `from_io`.
  `Status::from_error_details` is the typed bag after this packed-status Health interceptor Err; those trailers reach the client without reading the body.
  `Status::from_error_details` is the typed bag after this packed-status reflection interceptor Err; those trailers reach the client without reading the body.
  `Status::from_error_details` is the typed bag after this packed-status Store interceptor Err; those trailers reach the client without reading the body.
  A generated Store handler `Err(with_error_details)`
  unpacks on Get / Watch / PutAll / Sync too, including over TLS, mTLS, Unix, and `from_io`.
  `Status::from_error_details` is the typed bag after this packed-status Store handler Err; those trailers reach the client.
  `Status::from_error_details` is the typed bag after this packed-status Store client interceptor Err; a local reject never opens a stream.
  A Health handler `Err(with_error_details)` unpacks on Check, List, and Watch too, including over TLS, mTLS, Unix, and `from_io`.
  `Status::from_error_details` is the typed bag after this packed-status Health handler Err; those trailers reach the client.
  `Status::from_error_details` is the typed bag after this packed-status Health client interceptor Err; a local reject never opens a stream.
  A reflection handler `Err(with_error_details)` unpacks on the bidi `list_services` method too, including over TLS, mTLS, Unix,
  and `from_io`.
  `Status::from_error_details` is the typed bag after this packed-status reflection handler Err; those trailers reach the client.
  `Status::from_error_details` is the typed bag after this packed-status reflection client interceptor Err; a local reject never opens a stream.
  A wrapping `Service`
  interceptor `Err(with_error_details)` unpacks on every hand-written
  Reverser Channel API, including over TLS, mTLS, Unix, and `from_io`, a wrapping
  `Service` handler `Err(with_error_details)` unpacks on those APIs the same way,
  and a client interceptor stamps Outgoing path facts on those APIs, including over TLS,
  mTLS, Unix, and `from_io`.
  `Status::from_error_details` is the typed bag after this packed-status Reverser interceptor Err; those trailers reach the client without reading the body.
  `Status::from_error_details` is the typed bag after this packed-status Reverser handler Err; those trailers reach the client.
  Official `TestService` interceptor `Err(with_error_details)` unpacks on EmptyCall /
  StreamingOutputCall / StreamingInputCall / FullDuplexCall, including over
  TLS, mTLS, Unix, and `from_io`, a generated TestService handler
  `Err(with_error_details)` unpacks on those methods too, including over TLS, mTLS,
  Unix, and `from_io`, and a client interceptor stamps Outgoing path facts
  on those methods, including over TLS, mTLS, Unix, and `from_io`.
  `Status::from_error_details` is the typed bag after this packed-status TestService interceptor Err; those trailers reach the client without reading the body.
  `Status::from_error_details` is the typed bag after this packed-status TestService handler Err; those trailers reach the client.
  `Status::from_error_details` is the typed bag after this packed-status TestService client interceptor Err; a local reject never opens a stream.
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
