# pbrs-grpc

HTTP/2 gRPC kernel over pbrs. It is not tonic. `protobuf-tonic` remains
the adapter for existing tonic 0.14 services.

`pbrs` has no dependency on this crate. This crate has no dependency on
tonic or on `protobuf-tonic`.

Cleartext prior-knowledge HTTP/2 only (no TLS). Identity framing (no
gzip). `helloworld.Greeter` is the in-tree service: unary, client-stream,
server-stream, and bidi.

```rust
let listener = TcpListener::bind("127.0.0.1:0").await?;
let addr = listener.local_addr()?;
tokio::spawn(GreeterServer::new(Echo).serve_listener(listener));
let client = GreeterClient::new(Channel::connect(addr).await?);
let resp = client.say_hello(Request::new(req)).await?;
```

OK-path custom trailers (including `-bin`) are first-class.
`grpc-timeout` maps to `DEADLINE_EXCEEDED`. Client cancel maps to
`CANCELLED`.
