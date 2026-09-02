# pbrs-grpc

A pure-Rust gRPC kernel over [pbrs](../README.md). No `unsafe` in the kernel,
no C or C++ compiled into the build, no tonic. TLS uses rustls with Graviola
(rustc only; no `aws-lc-rs` or `ring`).

```toml
[dependencies]
# until these crates are on crates.io:
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
pbrs-grpc = { git = "https://github.com/mingley/pure-protobuf" }

[build-dependencies]
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
```

```rust
// build.rs
pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])?;
```

That generates a service trait, a server, and a client for every `service` in
your `.proto`. Implement the trait:

```rust
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {}", request.get_ref().name()));
        Ok(Response::new(reply))
    }
}

GreeterServer::new(MyGreeter).serve(addr).await?;
```

Methods you omit answer `UNIMPLEMENTED`.

and call it:

```rust
let client = GreeterClient::connect("127.0.0.1:50051").await?;
let reply = client.say_hello(Request::new(req)).await?;
```

All four call shapes, `Router` for several services, TLS (rustls + Graviola,
no C compiler) and mTLS, `grpc.health.v1`, `grpc.reflection.v1`, interceptors
(server `Rpc`/`Request` metadata/timeout/deadline/`peer_timeout`/`rpc_timeout`/`:authority`/`:scheme`/path/service/method/`local_addr`/`peer_identity`/`peer_cred`/`limits`/`accepts_gzip`/`encoding`/`compresses_outbound`/extensions, client `Outgoing` with path/service/method, `:authority`, `:scheme`, `user-agent` (`user_agent_is_set`), message caps, timeout/deadline Instant (`set_timeout` is the `Call` deadline on every shape), wait-for-ready (`wait_for_ready_is_set`), compression (`compress_is_set`), channel overlays (`rpc_timeout` / `waits_for_ready` / `compresses_outbound` / `gzip_level` / `accepts_compressed` / `concurrent_rpc_limit` / `stream_buffer_size` / `send_buffer_size` / `limits`; `clear_*` opts out of the already-applied default), inbound gzip (`accepts_compressed`; default on), caller and stacked-interceptor extensions; `Err` with `with_error_details` fails the `Call` on every shape and nothing is sent),
received `Response::encoding` (`None` for identity, including an explicit `identity` token; `Some("gzip")` when the peer advertised gzip),
typed `google.rpc.Status` / `ErrorDetails` (`ErrorInfo` / `RetryInfo` / `DebugInfo` / `QuotaFailure` / `PreconditionFailure` / `BadRequest` / `RequestInfo` / `ResourceInfo` / `Help` / `LocalizedMessage`) on `grpc-status-details-bin`,
`Code::is_retryable` / `Status::is_retryable` (gRPC A6: `UNAVAILABLE` only), `Status::error_info` / `ErrorInfo::with_reason` / `ErrorInfo::with_metadata`, `Status::bad_request` / `BadRequest::with_field` builds packed field violations on this crate README, `FieldViolation::with_field` builds a nested field path on this crate README, `Status::quota_failure` / `QuotaFailure::with_violation` builds packed quota subjects on this crate README, `quota_failure::Violation::with_subject` builds a nested quota subject on this crate README, `Status::precondition_failure` / `PreconditionFailure::with_violation` builds packed type and subject on this crate README, `precondition_failure::Violation::with_type` builds a nested precondition type on this crate README, `Status::help` / `Help::with_link` builds packed documentation links on this crate README, `help::Link::with_url` builds a nested docs URL on this crate README, `Status::localized_message` / `LocalizedMessage::with_locale` builds packed locale text on this crate README, `Status::request_info` / `RequestInfo::with_request_id` builds packed request_id on this crate README, `Status::resource_info` / `ResourceInfo::with_resource` builds packed resource type and name on this crate README, `Status::debug_info` / `DebugInfo::with_stack` builds packed operator stack on this crate README, `Status::retry_delay` / `RetryInfo::with_retry_delay`, `Status::from_error` wrapping local errors, `Status::with_cause` attaching `Error::source` onto an existing status, `Status::set_error_details` / `Status::set_from_error_details` replace the protobuf without dropping trailing metadata on this crate README, `Status::with_details` ships raw trailer bytes on this crate README, Distinct from `with_error_details` packing Anys onto a status. `Status::with_rpc` keeps existing trailers on this crate README, Distinct from `from_rpc` minting a fresh status. `pb::Status::with_details` builds a packed `google.rpc.Status` on this crate README, Distinct from `Status::with_details` shipping raw trailer bytes. `ErrorDetails::to_anys` returns the `Any` list on this crate README, Distinct from `from_error_details` encoding the bag as a trailer. `ErrorDetails::from_rpc` unpacks the `Any` list on this crate README, Distinct from `Status::error_details` unpacking a kernel Status trailer. `Any::pack` packs one message into an `Any` on this crate README, Distinct from `with_error_details` packing Anys onto a status. `Any::pack_with` takes an explicit type URL on this crate README, Distinct from `pack` using `type.googleapis.com/<FULL_NAME>`. `Status::set_details` ships raw trailer bytes on this crate README, Distinct from `set_error_details` packing Anys onto a status. `Any::unpack` decodes the payload on this crate README, Distinct from `is` checking the type URL. `Any::is` is a type-URL check on this crate README, Distinct from `unpack` decoding the payload. `ErrorDetails::new` is an empty bag on this crate README, Distinct from `from_rpc` unpacking the `Any` list. `Duration::from_std` builds the protobuf from `std` on this crate README, Distinct from `try_to_std` converting this protobuf to `std`. `Duration::try_to_std` converts this protobuf to `std` on this crate README, Distinct from `from_std` building the protobuf from `std`. `Status::details` returns raw trailer bytes on this crate README, Distinct from `rpc` parsing a packed `google.rpc.Status`. `Status::new` takes a code and message on this crate README, Distinct from `from_code` being code-only. `Status::from_code` is code-only on this crate README, Distinct from `new` taking a code and message. `Status::rpc` parses a packed `google.rpc.Status` on this crate README, Distinct from `details` returning raw trailer bytes. `Status::set_code` mutates in place on this crate README, Distinct from `with_code` being the builder. `Status::with_code` is the builder on this crate README, Distinct from `set_code` mutating in place. `Status::set_message` mutates in place on this crate README, Distinct from `with_message` being the builder. `Status::with_message` is the builder on this crate README, Distinct from `set_message` mutating in place. `Code::from_i32` interprets a wire i32 on this crate README, Distinct from `to_i32` emitting the wire i32. `Code::to_i32` emits the wire i32 on this crate README, Distinct from `from_i32` interpreting a wire i32. `Code::name` is the canonical name on this crate README, Distinct from `description` being the one-line google.rpc.Code text. `Code::description` is the one-line google.rpc.Code text on this crate README, Distinct from `name` being the canonical name. `Status::is_ok` is Code::Ok on this crate README, Distinct from `is_retryable` being UNAVAILABLE only. `Status::code` is the ASCII `grpc-status` code on this crate README, Distinct from `message` being the ASCII `grpc-message`. `Status::message` is the ASCII `grpc-message` on this crate README, Distinct from `code` being the ASCII `grpc-status` code. `Code::is_retryable` is the A6 set on a Code on this crate README, Distinct from `Status::is_retryable` being the same A6 set on a Status. `Status::is_retryable` is the A6 set on a Status on this crate README, Distinct from `Code::is_retryable` being the same A6 set on a Code. `Status::metadata` borrows this status trailers map on this crate README, Distinct from `metadata_mut` mutating it. `Status::metadata_mut` mutates this status trailers map on this crate README, Distinct from `metadata` borrowing it. `ParseCodeError` rejects a string on this crate README, Distinct from `Code::from_i32` mapping an unrecognised wire i32 to `Unknown`. `Status::code` is the ASCII `grpc-status` trailer on this crate README, Distinct from `rpc` being the packed protobuf. `Status::message` is the ASCII `grpc-message` trailer on this crate README, Distinct from `rpc` being the packed protobuf. `Status::rpc` is the packed protobuf on this crate README, Distinct from `code` being the ASCII `grpc-status` trailer. `Status::rpc` is the packed protobuf on this crate README, Distinct from `message` being the ASCII `grpc-message` trailer. `Status::details` returns raw trailer bytes on this crate README, Distinct from `code` being the ASCII `grpc-status` trailer. `Status::details` returns raw trailer bytes on this crate README, Distinct from `message` being the ASCII `grpc-message` trailer. `Status::code` is the ASCII `grpc-status` trailer on this crate README, Distinct from `details` returning raw trailer bytes. `Status::message` is the ASCII `grpc-message` trailer on this crate README, Distinct from `details` returning raw trailer bytes.
HTTP/2 PING keepalive, TCP `SO_KEEPALIVE`, max connection age (jittered ±10%) and idle, automatic
redial of a dead connection, lazy connect with wait-for-ready, in-process
`Channel::from_io` / `Server::serve_connection`, Unix domain
sockets (h2c; `serve_unix_unlink` after a crash, without stealing a live listener), graceful drain with `GOAWAY`, per-message gzip, deadlines,
cancellation (dropping a `Call` or a received `Streaming` resets the stream; a `CallHandle` taken before await still cancels while waiting for server-streaming or bidi headers, after streaming headers, and after a client-streaming sender is closed; `StreamSender::fail` on a client request sender resets CANCEL and resolves a client-streaming or pre-headers bidi `Call` with that status, not `UNAVAILABLE` from the reset; after bidi headers the received `Streaming` sees `CANCELLED`, not that status; `Request::cancelled` for spawned work), ASCII and `-bin` metadata, OK-path custom trailers,
mTLS client certificates on `Rpc::peer_identity`, Unix `SO_PEERCRED` on `Rpc::peer_cred`,
`Incoming::peer` / `ConnectionInfo` for custom acceptors, `Channel::https_scheme`
for already-encrypted `from_io` streams. Outbound
RPCs send `user-agent: pbrs-grpc/<version>`; prefix it with `Channel::user_agent`, `Request::set_user_agent`, or `Outgoing::set_user_agent`.
`Outgoing::user_agent_is_set` is occupancy on this crate README interceptor path, so a later interceptor can prefix only when unset.
`Outgoing::wait_for_ready_is_set` is occupancy on this crate README interceptor path, so a later interceptor can fill wait-for-ready only when unset.
`Outgoing::compress_is_set` is occupancy on this crate README interceptor path, so a later interceptor can fill compress only when unset.
`Outgoing::clear_user_agent` restores the channel user-agent after a crate README interceptor prefix.
`Outgoing::clear_wait_for_ready` restores the channel wait-for-ready overlay after a crate README interceptor choice.
`Outgoing::clear_compress` then `set_compress` from `compresses_outbound` reapplies channel gzip after a crate README interceptor choice.
`Outgoing::clear_timeout` opts out of the channel timeout after a crate README interceptor choice.
`Outgoing::connected` is the live-socket snapshot on this crate README interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README interceptor Err; a local reject never opens a stream.
Distinct from a crate README handler Err: that is after the handler ran; this crate README interceptor Err is a local reject never opens a stream.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README interceptor Err is a local reject never opens a stream.
Distinct from a crate README server intercept Err: that is trailers without reading the body; this crate README interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README interceptor already ran, so a local Err never consumes that budget.
Distinct from `Server::intercept`: that runs on the inbound RPC before the handler; this crate README Channel intercept runs on the outbound call before the stream opens.
Distinct from `Channel::on_response`: that runs after a successful receive; this crate README Channel intercept runs on the outbound call before the stream opens.
Distinct from `Channel::intercept`: that runs on the outbound call before the stream opens; this crate README server intercept runs on the inbound RPC before the handler.
Distinct from `Server::on_response`: that runs after the handler returns Ok; this crate README server intercept runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README server intercept Err; those trailers reach the client without reading the body.
Distinct from a crate README handler Err: that is after the handler ran; this crate README server intercept Err is trailers without reading the body.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README server intercept Err is trailers without reading the body.
Distinct from a crate README interceptor Err: that is a local reject never opens a stream; this crate README server intercept Err is trailers without reading the body.
`Status::from_error_details` is the typed bag after this crate README Health interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README Health handler Err: that is after the handler ran; this crate README Health interceptor Err is trailers without reading the body.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README Health interceptor Err is trailers without reading the body.
Distinct from a crate README Health client interceptor: that runs on the outbound call before the stream opens; this crate README Health interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README Health handler Err; those trailers reach the client.
Distinct from a crate README Health interceptor Err: that is trailers without reading the body; this crate README Health handler Err is after the handler ran.
Distinct from a crate README Health client interceptor Err: that is a local reject never opens a stream; this crate README Health handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Health handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Health handler Err is after the handler ran.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README Health handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README Health client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README Health client interceptor Err; a local reject never opens a stream.
Distinct from a crate README Health handler Err: that is after the handler ran; this crate README Health client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README Health client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README Health client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README Health interceptor: that runs on the inbound RPC before the handler; this crate README Health client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README Health StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README Health handler Err: that is after the handler ran; this crate README Health StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Health interceptor Err: that is trailers without reading the body; this crate README Health StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Health StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Health client interceptor Err: that is a local reject never opens a stream; this crate README Health StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Health StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README reflection interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README reflection handler Err: that is after the handler ran; this crate README reflection interceptor Err is trailers without reading the body.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README reflection interceptor Err is trailers without reading the body.
Distinct from a crate README reflection client interceptor: that runs on the outbound call before the stream opens; this crate README reflection interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README reflection handler Err; those trailers reach the client.
Distinct from a crate README reflection interceptor Err: that is trailers without reading the body; this crate README reflection handler Err is after the handler ran.
Distinct from a crate README reflection client interceptor Err: that is a local reject never opens a stream; this crate README reflection handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README reflection handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README reflection handler Err is after the handler ran.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README reflection handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README reflection client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README reflection client interceptor Err; a local reject never opens a stream.
Distinct from a crate README reflection handler Err: that is after the handler ran; this crate README reflection client interceptor Err is a local reject never opens a stream.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README reflection client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README reflection client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README reflection interceptor: that runs on the inbound RPC before the handler; this crate README reflection client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README reflection StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README reflection handler Err: that is after the handler ran; this crate README reflection StreamSender fail is trailers after any messages already sent.
Distinct from a crate README reflection interceptor Err: that is trailers without reading the body; this crate README reflection StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README reflection StreamSender fail is trailers after any messages already sent.
Distinct from a crate README reflection client interceptor Err: that is a local reject never opens a stream; this crate README reflection StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README reflection StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README Store interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README Store handler Err: that is after the handler ran; this crate README Store interceptor Err is trailers without reading the body.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README Store interceptor Err is trailers without reading the body.
Distinct from a crate README Store client interceptor: that runs on the outbound call before the stream opens; this crate README Store interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README Store handler Err; those trailers reach the client.
Distinct from a crate README Store interceptor Err: that is trailers without reading the body; this crate README Store handler Err is after the handler ran.
Distinct from a crate README Store client interceptor Err: that is a local reject never opens a stream; this crate README Store handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Store handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Store handler Err is after the handler ran.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README Store handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README Store client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README Store client interceptor Err; a local reject never opens a stream.
Distinct from a crate README Store handler Err: that is after the handler ran; this crate README Store client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README Store client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README Store client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README Store interceptor: that runs on the inbound RPC before the handler; this crate README Store client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README Store StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README Store handler Err: that is after the handler ran; this crate README Store StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Store interceptor Err: that is trailers without reading the body; this crate README Store StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Store StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Store client interceptor Err: that is a local reject never opens a stream; this crate README Store StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Store StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README TestService interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README TestService handler Err: that is after the handler ran; this crate README TestService interceptor Err is trailers without reading the body.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README TestService interceptor Err is trailers without reading the body.
Distinct from a crate README TestService client interceptor: that runs on the outbound call before the stream opens; this crate README TestService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README TestService handler Err; those trailers reach the client.
Distinct from a crate README TestService interceptor Err: that is trailers without reading the body; this crate README TestService handler Err is after the handler ran.
Distinct from a crate README TestService client interceptor Err: that is a local reject never opens a stream; this crate README TestService handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README TestService handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README TestService handler Err is after the handler ran.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README TestService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README TestService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README TestService client interceptor Err; a local reject never opens a stream.
Distinct from a crate README TestService handler Err: that is after the handler ran; this crate README TestService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README TestService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README TestService client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README TestService interceptor: that runs on the inbound RPC before the handler; this crate README TestService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README TestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README TestService handler Err: that is after the handler ran; this crate README TestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README TestService interceptor Err: that is trailers without reading the body; this crate README TestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README TestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README TestService client interceptor Err: that is a local reject never opens a stream; this crate README TestService StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README Reverser interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README Reverser handler Err: that is after the handler ran; this crate README Reverser interceptor Err is trailers without reading the body.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README Reverser interceptor Err is trailers without reading the body.
Distinct from a crate README Reverser client interceptor: that runs on the outbound call before the stream opens; this crate README Reverser interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README Reverser handler Err; those trailers reach the client.
Distinct from a crate README Reverser interceptor Err: that is trailers without reading the body; this crate README Reverser handler Err is after the handler ran.
Distinct from a crate README Reverser client interceptor Err: that is a local reject never opens a stream; this crate README Reverser handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Reverser handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README Reverser handler Err is after the handler ran.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README Reverser handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README Reverser client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README Reverser client interceptor Err; a local reject never opens a stream.
Distinct from a crate README Reverser handler Err: that is after the handler ran; this crate README Reverser client interceptor Err is a local reject never opens a stream.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README Reverser client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README Reverser client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README Reverser interceptor: that runs on the inbound RPC before the handler; this crate README Reverser client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README Reverser StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README Reverser handler Err: that is after the handler ran; this crate README Reverser StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Reverser interceptor Err: that is trailers without reading the body; this crate README Reverser StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README Reverser StreamSender fail is trailers after any messages already sent.
Distinct from a crate README Reverser client interceptor Err: that is a local reject never opens a stream; this crate README Reverser StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README hello interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README hello handler Err: that is after the handler ran; this crate README hello interceptor Err is trailers without reading the body.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README hello interceptor Err is trailers without reading the body.
Distinct from a crate README hello client interceptor: that runs on the outbound call before the stream opens; this crate README hello interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README hello handler Err; those trailers reach the client.
Distinct from a crate README hello interceptor Err: that is trailers without reading the body; this crate README hello handler Err is after the handler ran.
Distinct from a crate README hello client interceptor Err: that is a local reject never opens a stream; this crate README hello handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README hello handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README hello handler Err is after the handler ran.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README hello handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README hello client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README hello client interceptor Err; a local reject never opens a stream.
Distinct from a crate README hello handler Err: that is after the handler ran; this crate README hello client interceptor Err is a local reject never opens a stream.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README hello client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README hello client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README hello interceptor: that runs on the inbound RPC before the handler; this crate README hello client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README hello StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README hello handler Err: that is after the handler ran; this crate README hello StreamSender fail is trailers after any messages already sent.
Distinct from a crate README hello interceptor Err: that is trailers without reading the body; this crate README hello StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README hello StreamSender fail is trailers after any messages already sent.
Distinct from a crate README hello client interceptor Err: that is a local reject never opens a stream; this crate README hello StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this crate README UnimplementedService interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README UnimplementedService handler Err: that is after the handler ran; this crate README UnimplementedService interceptor Err is trailers without reading the body.
Distinct from a crate README UnimplementedService client interceptor: that runs on the outbound call before the stream opens; this crate README UnimplementedService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README UnimplementedService handler Err; those trailers reach the client.
Distinct from a crate README UnimplementedService interceptor Err: that is trailers without reading the body; this crate README UnimplementedService handler Err is after the handler ran.
Distinct from a crate README UnimplementedService client interceptor Err: that is a local reject never opens a stream; this crate README UnimplementedService handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README UnimplementedService handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README UnimplementedService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README UnimplementedService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README UnimplementedService client interceptor Err; a local reject never opens a stream.
Distinct from a crate README UnimplementedService handler Err: that is after the handler ran; this crate README UnimplementedService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README UnimplementedService client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README UnimplementedService interceptor: that runs on the inbound RPC before the handler; this crate README UnimplementedService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README InteropTestService interceptor Err; those trailers reach the client without reading the body.
Distinct from a crate README InteropTestService handler Err: that is after the handler ran; this crate README InteropTestService interceptor Err is trailers without reading the body.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README InteropTestService interceptor Err is trailers without reading the body.
Distinct from a crate README InteropTestService client interceptor: that runs on the outbound call before the stream opens; this crate README InteropTestService interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this crate README InteropTestService handler Err; those trailers reach the client.
Distinct from a crate README InteropTestService interceptor Err: that is trailers without reading the body; this crate README InteropTestService handler Err is after the handler ran.
Distinct from a crate README InteropTestService client interceptor Err: that is a local reject never opens a stream; this crate README InteropTestService handler Err is after the handler ran.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README InteropTestService handler Err is after the handler ran.
Distinct from a crate README Channel on_response Err: that fails the Call after a successful receive; this crate README InteropTestService handler Err is after the handler ran.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README InteropTestService handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this crate README InteropTestService client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this crate README InteropTestService client interceptor Err; a local reject never opens a stream.
Distinct from a crate README InteropTestService handler Err: that is after the handler ran; this crate README InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README InteropTestService client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this crate README InteropTestService client interceptor already ran, so a local Err never consumes that budget.
Distinct from a crate README InteropTestService interceptor: that runs on the inbound RPC before the handler; this crate README InteropTestService client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this crate README InteropTestService StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from a crate README InteropTestService handler Err: that is after the handler ran; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README InteropTestService interceptor Err: that is trailers without reading the body; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README server on_response Err: that is trailers-only after handler Ok; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
Distinct from a crate README InteropTestService client interceptor Err: that is a local reject never opens a stream; this crate README InteropTestService StreamSender fail is trailers after any messages already sent.
`ResponseParts::compress_is_set` is occupancy on this crate README on_response path, so a later interceptor can fill compress only when unset.
`ResponseParts::clear_compress` restores the server gzip overlay after Server on_response on this crate README on_response path.
`Status::from_error_details` is the typed bag after this crate README server on_response Err; a local reject is trailers-only after handler Ok.
Distinct from a crate README handler Err: that is after the handler ran; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README server intercept Err: that is trailers without reading the body; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README TestService StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README Reverser StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README hello StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from a crate README InteropTestService StreamSender fail: that is trailers after any messages already sent; this crate README server on_response Err is trailers-only after handler Ok.
Distinct from `Server::intercept`: that runs on the inbound RPC before the handler; this crate README server on_response runs after the handler returns Ok.
`ResponseParts::clear_compress` drops a compress choice after Channel on_response on this crate README on_response path; a received reply has no server gzip overlay to restore.
`Status::from_error_details` is the typed bag after this crate README Channel on_response Err; a local reject fails the Call after a successful receive.
Distinct from a crate README handler Err: that is after the handler ran; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README interceptor Err: that is a local reject never opens a stream; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Health StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README reflection StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from a crate README Store StreamSender fail: that is trailers after any messages already sent; this crate README Channel on_response Err fails the Call after a successful receive.
Distinct from `Channel::intercept`: that runs on the outbound call before the stream opens; this crate README Channel on_response runs after a successful receive.
`Streaming` implements
`futures_core::Stream`.

**[Guide](../docs/grpc.md)** — building services, streaming, metadata, errors,
deadlines, lazy connect, compression, interceptors, limits, tuning, testing,
and writing a service without codegen.

## Fast

Measured against tonic 0.14 over loopback on the same service and the same
protobuf codec, so the delta is transport only. Four-core Xeon; see
[benchmarks](../docs/benchmarks.md) for method, variance, and three full runs.

| Axis | Kernel | tonic 0.14 |
|---|---:|---:|
| `empty_unary` p50 | **33-54 µs** | 50-87 µs |
| `empty_unary` p99 | **42-191 µs** | 42 ms |
| `large_unary` p50 | **596-822 µs** | 1.40-1.71 ms |
| Unary QPS, 1 connection | **74k** | 2.0-2.9k |
| Unary QPS, 16 conc / 4 conns | **84-101k** | 12-27k |
| Server-stream, 1 KiB messages | **1041k/s** median | 903k/s median |

Unary latency is process-gated: `rpc-bench` exits non-zero unless the kernel
wins on both p50 and p99. Streaming is gated at 90% of tonic — the kernel leads
by 15% at the median and on five of six runs, but the per-run spread on a
contended machine is wide enough that a strict gate would fail on noise.

Against grpc-go's reference server — one kernel client, two servers in separate
processes, so the server is the only variable — the kernel is about 1.4x on
`empty_unary` p50, 1.7x on its p99, 1.5x on `large_unary` p50, and 1.8x on its
p99, with a few percent spread across rounds:

```bash
./scripts/grpc-server-bench.sh
```

## Safe

The peer is assumed hostile, and every limit is enforced before the memory it
guards is committed: a frame length is refused from the 5-byte header, and a
compressed frame inflates through a reader that stops one byte past the cap.

Defaults: 4 MiB inbound messages, 16 KiB metadata, 256 concurrent streams per
connection, 16 MiB windows. A dial that never completes HTTP/2 fails after 20 s
(`ChannelConfig::connect_timeout`); a mute client is dropped after the same
bound on the server. `tests/hostile.rs` speaks raw HTTP/2 to check them,
sending length prefixes claiming 4 GiB, gzip bombs, reserved flag values,
truncated frames, and malformed paths, then verifying the server still serves.
Property tests add what fixed cases cannot: frames survive arbitrary chunk
boundaries, arbitrary bytes never panic and never exceed the cap, and a
compressed frame never inflates past it.

Every hand-written module carries `#[forbid(unsafe_code)]`, which cannot be
relaxed from inside it. The modules that `include!` generated messages
(`hello`, `testing`, `health`, `reflection`, `pb`) are exempt, because pbrs
gencode uses `unsafe` for zeroed-message construction.

See [the threat model](../docs/grpc.md#limits-and-the-threat-model).

## Scope

h2c by default; TLS is opt-in via `ServerTls` / `ClientTls` (rustls + Graviola,
certificate verification is not optional). No load balancing. Application
retries stay at the call site; unary and server-streaming already redial once
when a connection dies after the slot looked live. See
[what is not here](../docs/grpc.md#what-is-not-here).

`pbrs` does not depend on this crate, and this crate does not depend on tonic
or `protobuf-tonic`. Use `protobuf-tonic` instead if you want to keep an
existing tonic service and only swap in pbrs message types.

## Interop

`grpc.testing.TestService` and the official test cases ship in-tree.
`scripts/grpc-interop.sh` runs them against grpc-go's reference implementation
in both directions, and CI runs the script:

```
kernel client -> kernel server   18 cases
kernel client -> Go server       14 cases
Go client     -> kernel server   14 cases
```

The four cases absent from the cross-language passes are the compression ones;
grpc-go implements `expect_compressed` and `response_compressed` in neither its
client nor its server, so they only run where both ends honour them.

```bash
./scripts/grpc-interop.sh              # all three passes
./scripts/grpc-interop.sh --self-only  # skip the Go peer
```

[`examples/greeter`](../examples/greeter) is a complete user crate: own proto,
`build.rs`, generated stubs, health, and reflection. `pbrs-grpc-hello`
exercises all four call shapes over loopback, and `tests/codegen.rs` compiles
a fresh `.proto` service the way a user's crate does, to keep the generated
`::pbrs_grpc` paths honest.
