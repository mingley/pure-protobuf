# greeter

This example shows `pbrs-grpc` the way an application uses it: local `.proto`,
`build.rs`, generated service trait, server, client, health, and reflection.
Start here when you want a small working service before reading the larger
guides.

## Run it

```bash
cargo run -p pbrs-grpc-example-greeter
# prints: hello world
```

## What it covers

- Own proto plus `compile_protos`; native kernel stubs are the default.
- Generated `Greeter` trait, server, and client.
- All four gRPC shapes: `SayHello`, `ClientHello`, `ServerHello`, and `StreamHello`.
- Built-in `grpc.health.v1` and `grpc.reflection.v1` services.
- `src/lib.rs` as the whole service implementation.

Tests cover every call shape, health `Check`, health `Watch` (dropping the
stream ends the subscription), and reflection `list_services`.

## Error Details & Interceptor Invariants

These sentences are intentionally exact. The example tests use them as a
contract for where `Status::from_error_details` and interceptor failures land.

`Status::from_error_details` is the typed bag after this example README greeter interceptor Err; those trailers reach the client without reading the body.
Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter interceptor Err is trailers without reading the body.
Distinct from an example README greeter server on_response Err: that is trailers-only after handler Ok; this example README greeter interceptor Err is trailers without reading the body.
Distinct from an example README greeter client interceptor Err: that is a local reject never opens a stream; this example README greeter interceptor Err is trailers without reading the body.
Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter interceptor Err is trailers without reading the body.
Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter interceptor Err is trailers without reading the body.
Distinct from an example README greeter client interceptor: that runs on the outbound call before the stream opens; this example README greeter interceptor runs on the inbound RPC before the handler.
`Status::from_error_details` is the typed bag after this example README greeter handler Err; those trailers reach the client.
Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter handler Err is after the handler ran.
Distinct from an example README greeter client interceptor Err: that is a local reject never opens a stream; this example README greeter handler Err is after the handler ran.
Distinct from an example README greeter server on_response Err: that is trailers-only after handler Ok; this example README greeter handler Err is after the handler ran.
Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter handler Err is after the handler ran.
Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter handler Err is after the handler ran.
`Outgoing::connected` is the live-socket snapshot on this example README greeter client interceptor path (`Channel::connected`), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
`Status::from_error_details` is the typed bag after this example README greeter client interceptor Err; a local reject never opens a stream.
Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter client interceptor Err is a local reject never opens a stream.
Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter client interceptor Err is a local reject never opens a stream.
Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter client interceptor Err is a local reject never opens a stream.
Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter client interceptor Err is a local reject never opens a stream.
Distinct from `Channel::max_concurrent_rpcs`: that takes a slot when the `Call` is polled; this example README greeter client interceptor already ran, so a local Err never consumes that budget.
Distinct from an example README greeter interceptor: that runs on the inbound RPC before the handler; this example README greeter client interceptor runs on the outbound call before the stream opens.
`Status::from_error_details` is the typed bag after this example README greeter StreamSender fail on a server response producer; those trailers ship after any messages already sent.
Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter StreamSender fail is trailers after any messages already sent.
Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter StreamSender fail is trailers after any messages already sent.
Distinct from an example README greeter server on_response Err: that is trailers-only after handler Ok; this example README greeter StreamSender fail is trailers after any messages already sent.
Distinct from an example README greeter client interceptor Err: that is a local reject never opens a stream; this example README greeter StreamSender fail is trailers after any messages already sent.
Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter StreamSender fail is trailers after any messages already sent.
`Status::from_error_details` is the typed bag after this example README greeter server on_response Err; a local reject is trailers-only after handler Ok.
Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter server on_response Err is trailers-only after handler Ok.
Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter server on_response Err is trailers-only after handler Ok.
Distinct from an example README greeter client on_response Err: that fails the Call after a successful receive; this example README greeter server on_response Err is trailers-only after handler Ok.
Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter server on_response Err is trailers-only after handler Ok.
`Status::from_error_details` is the typed bag after this example README greeter client on_response Err; a local reject fails the Call after a successful receive.
Distinct from an example README greeter handler Err: that is after the handler ran; this example README greeter client on_response Err fails the Call after a successful receive.
Distinct from an example README greeter interceptor Err: that is trailers without reading the body; this example README greeter client on_response Err fails the Call after a successful receive.
Distinct from an example README greeter client interceptor Err: that is a local reject never opens a stream; this example README greeter client on_response Err fails the Call after a successful receive.
Distinct from an example README greeter server on_response Err: that is trailers-only after handler Ok; this example README greeter client on_response Err fails the Call after a successful receive.
Distinct from an example README greeter StreamSender fail: that is trailers after any messages already sent; this example README greeter client on_response Err fails the Call after a successful receive.
