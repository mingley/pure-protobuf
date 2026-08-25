# protobuf-tonic

This crate is the tonic 0.14 `Codec` and plugin-generated stubs over pbrs
(`Parse` / `Serialize`).

It is not `tonic-prost`. These types do not implement `prost::Message`.
The kernel does not depend on tonic. tonic 0.12 and 0.13 are unsupported.
MSRV is 1.88. This crate depends on `pbrs` by path (git until `pbrs` is
on crates.io); `cargo publish -p protobuf-tonic` cannot succeed until that
registry version exists.

`protoc-gen-pbrs` (and `pbrs::codegen::generate_from_file_descriptor_set`)
emit `FooClient` / `FooServer` / a `Foo` trait for each `.proto` service.
Stubs use `ProtobufCodec`, not prost. `build.rs` in this crate generates
`hello.rs` from `proto/hello.proto`.

```rust
impl Greeter for Echo {
    async fn say_hello(&self, request: Request<HelloRequest>) -> Result<Response<HelloReply>, Status> {
        /* ... */
    }
    type StreamHelloStream = ReceiverStream<Result<HelloReply, Status>>;
    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        /* ... */
    }
}

Server::builder().add_service(GreeterServer::new(Echo));
let mut client = GreeterClient::new(channel);
let resp = client.say_hello(Request::new(req)).await?;
let stream = client.stream_hello(Request::new(inbound)).await?;
```

`ProtobufCodec<Encode, Decode>` takes the encode type first and the decode
type second. `tonic-bench` Codec survey vs prost and v4 upb lives in
`docs/benchmarks.md`. Typical unary `rpc_mixed` is already ~2× prost.
`name_4kib` combined beats prost (process-gated). `rpc_sparse` decode
and `tags_32` decode vs v4 are also gated. Flatten (#39) tried and
discarded. See
`docs/status.md` Remaining. Not kernel `./bench`.

Generated `FooClient` / `FooServer` expose tonic `send_compressed` /
`accept_compressed`, `with_interceptor`, and `max_decoding_message_size` /
`max_encoding_message_size`. `tests/gzip.rs` runs unary `say_hello` with gzip.
`tests/interceptor_size.rs` runs a unary RPC through a request interceptor
and through encode/decode size bounds.
`tests/health_reflection.rs` serves gRPC health (SERVING) and server
reflection that lists `helloworld.Greeter`. Health and reflection are
tonic's crates, not a second stack.

`proto/hello.proto` has all four Greeter RPCs. `tests/unary.rs` is the
unary happy path. `tests/streaming.rs` covers client-stream, server-stream,
and bidi. `tests/status.rs` asserts non-OK `Status` code+message
(`NotFound`) on all four RPCs. Server-stream fails before a stream.
Bidi fails after the first inbound name (same path as client-stream;
client sees it on the call `Result`). `tests/trailers.rs` splits initial
`Response` metadata (headers) from `Status` metadata sent as HTTP/2
trailers on unary, client-stream, server-stream, and bidi.
Server-stream trailers still fail before a stream. Client-stream
headers need the reply `Response`. `tests/interop.rs` is same-process
analogues of official gRPC interop names (`unimplemented_method`,
`unimplemented_service`,
`special_status_message`, `empty_unary`, `large_unary`, `empty_stream`,
`cancel_after_begin`, `cancel_after_first_response`,
`timeout_on_sleeping_server`, `custom_metadata`). `large_unary` uses
`hello.proto` string-field sizes (271828 / 314159), not official
`SimpleRequest.payload.body` / `response_size`. `empty_stream` is
StreamHello open + half-close with no messages; client sees OK and zero
replies. Cancel analogues abort the client future (`JoinError::Cancelled`,
not a `Status`). `timeout_on_sleeping_server` is unary
`Request::set_timeout` → `Code::Cancelled` / "Timeout expired", not
`DeadlineExceeded`. `custom_metadata` (unary SayHello): client sends
`x-grpc-test-echo-initial` and `x-grpc-test-echo-trailing-bin`; ascii
echo is `Response.metadata` (headers). tonic 0.14 has no first-class
OK-path custom trailers (`Response` has no `trailers()`);
`x-grpc-test-echo-trailing-bin` is absent on the OK path. That bag is
not trailers. Not official interop. Not a Google peer.

## License

MIT OR Apache-2.0, same as `pbrs`.
