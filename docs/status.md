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
  (`empty_unary` … `timeout_on_sleeping_server`, plus the four gzip cases,
  including mixed `server_compressed_streaming`) is implemented. TLS
  (rustls + Graviola), `grpc.health.v1` Check/Watch, and
  `grpc.reflection.v1` ship in the kernel. Unary/server-streaming that race
  a connection death after the slot looked live redial once (transparent
  retry) — proven for unary and server-streaming on h2c and TLS. Unix accept loops expose `SO_PEERCRED` on `Rpc::peer_cred`.
  Custom `Incoming` implementations stamp local_addr / mTLS identity /
  Unix credentials / transport scheme via `Incoming::peer` and
  `ConnectionInfo`. TLS `:scheme https` and mTLS `peer_identity` apply to
  every call shape. `Channel::https_scheme` sends `:scheme https` on a
  `from_io` clone (no TLS handshake; no-op on TCP/Unix);
  `Channel::scheme` / generated `FooClient::scheme` / `FooClient::authority` /
  `FooClient::grpc_user_agent` read that overlay and the other interceptor-visible
  channel facts. Interceptors run when the RPC method is invoked (all four
  shapes), not on first poll of the `Call`. Interceptors and generated
  handlers see `MessageLimits` on `Rpc::limits` / `Request::limits`, the
  method path on `Rpc::path` / `Request::path`, gzip accept/encoding, and
  the server overlays on `Rpc::compresses_outbound` /
  `Request::compresses_outbound` and `Rpc::rpc_timeout` /
  `Request::rpc_timeout` (the `Server::timeout` cap, distinct from the
  interceptor `set_timeout` and the client's `peer_timeout`); received replies
  surface `grpc-encoding` on
  `Response::encoding` (`None` for identity, including an explicit
  `identity` token). `Server::send_compressed` / `Response::set_compress(false)`
  opt-out apply to every call shape. Client interceptors see the channel overlay
  on `Outgoing::limits` plus a deadline Instant, fill-if-unset
  wait-for-ready / compress, and the channel overlays
  (`Outgoing::rpc_timeout` / `waits_for_ready` / `compresses_outbound`)
  after `clear_*`. `clear_compress` then `set_compress(compresses_outbound())`
  reapplies channel gzip on every call shape. A client interceptor `Err` fails the `Call` on poll for
  every call shape, including `with_error_details` and a local fail-before-open
  without details; nothing is sent. A packed `google.rpc.Status` on that
  local `Err` is `Status::rpc` / `Status::error_details` on the Call.
  Outgoing getters apply to every call shape. Kernel `user-agent` (and a
  `Channel::user_agent` prefix) is sent on every shape; inserting `user-agent`
  into metadata cannot override it. Server interceptor `set` / `remove` /
  `retain` reach the handler on every shape.
  `Outgoing::set_timeout` is that Call's deadline on every call shape. A
  wrapping `Service`, generated `FooServer::intercept`, and
  `Router::intercept` reject before the body is read and stack in
  declaration order on every call shape. Interceptor extensions on a
  wrapping `Service` reach the handler `Request` on every call shape. `FooServer::intercept` then
  `add_service` keeps that reject on every mount and every call shape.
  The same `add_service` keeps `max_decoding_message_size` on every mount
  and every call shape. Generated handlers see
  `:authority` / `:scheme` / `Request` parts, a deadline Instant that
  elapses, TCP local/remote, Unix `peer_cred`, and `Incoming::peer`
  stamps on every call shape. Handler `Err` (nonzero `grpc-status` and
  custom details) is that status on every call shape. A packed
  `google.rpc.Status` from `with_error_details` is `Status::rpc` /
  `Status::error_details` on every call shape. A
  server interceptor `Err` ships those trailers the same way a handler
  `Err` does. `Status::set_rpc` / `set_code` keep trailing
  metadata. `StreamSender::fail` after headers ships those trailers and
  a packed `google.rpc.Status` the same way a handler `Err` does on a
  server response stream. On a client request sender it resets CANCEL
  (no request-side `grpc-status`); a client-streaming `Call`, or a bidi
  `Call` that has not yet seen headers, resolves with that status, not
  `UNAVAILABLE` from the reset. After bidi headers the received `Streaming`
  sees `CANCELLED`, not that status. A `Call`
  is fused after `Ready`. Client-streaming and bidi
  `(StreamSender, Call)` pairs are `must_use`. `Health::watch` ends when the
  client leaves, without waiting for the next status change. A server-streaming
  drain waiting for the next message ends on client RST. Dropping a received
  `Streaming` before the end resets that RPC, including bidi while the send
  half is still held. A `CallHandle` taken before await still cancels that
  live stream after headers, still cancels a server-streaming or bidi call
  waiting for headers, and a client-streaming handle still cancels
  after the sender is closed while the unary response is pending (dropping
  the `Call` or hitting the deadline after that half-close does the same).
  `CallHandle` cancel also drops a hanging handler on every call shape
  before it runs to completion. A handler that ignores its inbound request
  stream still answers on client-streaming and bidi rather than stalling the
  window. `max_concurrent_rpcs` refuses extra RPCs on streaming the same as
  unary.
  A server-streaming or bidi deadline RSTs the send half before headers and
  after a half-close;
  after those headers that deadline still RSTs the parked
  send half. Spawned handler work awaiting `Request::cancelled` sees the RST, including
  when the server deadline wins (signalled before trailers). Generated trait
  rustdoc names `Request::cancelled` on every call shape (and
  `StreamSender::closed` on server-streaming); unary `Channel` / generated
  client methods name `CallHandle`. Generated client-streaming and bidi
  methods name `StreamSender::fail`; server-streaming and bidi methods name
  `CallHandle` before and after headers, and deadline RST before and after
  headers.
  Generated method rustdoc names
  inbound/received `encoding` and interceptor timing. Methods omitted on generated traits answer `UNIMPLEMENTED`.
  Generated `FooClient::connect_tls_with` / `connect_lazy_with` /
  `connect_tls_lazy` / `connect_tls_lazy_with` / `connect_unix_lazy_with`
  and `Channel::connect_tls_with` apply to every call shape. Generated
  Store TLS (`serve_tls_with_shutdown` / `connect_tls_with` /
  `connect_tls_lazy_with`) and `send_compressed` gzip every Store shape,
  including gzip over TLS and Unix. `from_io` / `serve_connection` gzip
  those Store shapes the same way. Greeter `send_compressed` gzips every
  call shape over TLS, including over mTLS, Unix, and `from_io`. A TLS, Unix,
  or `from_io` interceptor `Err(with_error_details)` unpacks on every Greeter
  shape. Official `TestService` `send_compressed` gzips EmptyCall /
  StreamingOutputCall / StreamingInputCall / FullDuplexCall, including over
  TLS, Unix, and `from_io`. A wrapping `Service` `send_compressed` gzips every
  hand-written Reverser Channel API, including over TLS, Unix, and `from_io`. Health
  `send_compressed` gzips Check and Watch, including over TLS, Unix, and
  `from_io`; reflection `send_compressed` gzips the bidi `list_services`
  method, including over TLS, Unix, and `from_io`. A client interceptor
  sees Outgoing path / service / method / authority / scheme on Health
  Check/Watch, the reflection bidi method, and generated Store Get / Watch
  / PutAll / Sync, including over TLS, Unix, and `from_io`. A packed `google.rpc.Status` from interceptor
  `Err(with_error_details)` unpacks on those Store, Health, and reflection
  methods the same way, including over TLS, Unix, and `from_io`. A generated Store handler `Err(with_error_details)`
  unpacks on Get / Watch / PutAll / Sync too. A wrapping `Service`
  interceptor `Err(with_error_details)` unpacks on every hand-written
  Reverser Channel API, including over TLS, Unix, and `from_io`, and a client
  interceptor stamps Outgoing path facts on those APIs, including over TLS,
  Unix, and `from_io`.
  Official `TestService` interceptor `Err(with_error_details)` unpacks on EmptyCall /
  StreamingOutputCall / StreamingInputCall / FullDuplexCall, including over
  TLS, Unix, and `from_io`, and a client interceptor stamps Outgoing path facts
  on those methods, including over TLS, Unix, and `from_io`.
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
