# protobuf-tonic

This crate is the tonic 0.14 `Codec` and plugin-generated stubs over pbrs
(`Parse` / `Serialize`).

It is not `tonic-prost`. These types do not implement `prost::Message`.
The kernel does not depend on tonic. tonic 0.12 and 0.13 are unsupported.
MSRV is 1.88.

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
type second.

See `tests/unary.rs` and `tests/streaming.rs`.

## License

MIT OR Apache-2.0, same as `pbrs`.
