# pbrs-grpc

HTTP/2 gRPC kernel over pbrs. It is not tonic. `protobuf-tonic` remains
the adapter for existing tonic 0.14 services.

`pbrs` has no dependency on this crate. This crate has no dependency on
tonic or on `protobuf-tonic`.

Cleartext prior-knowledge HTTP/2 only (no TLS). Identity framing by
default; gzip via `grpc-encoding` and the Compressed-Flag.
`helloworld.Greeter` is the in-tree service: unary, client-stream,
server-stream, and bidi.

```rust
let listener = TcpListener::bind("127.0.0.1:0").await?;
let addr = listener.local_addr()?;
tokio::spawn(GreeterServer::new(Echo).serve_listener(listener));
let client = GreeterClient::new(Channel::connect(addr).await?);
// Channel::connect_pool(addr, n) for independent h2 driver tasks.
let resp = client.say_hello(Request::new(req)).await?;
```

OK-path custom trailers (including `-bin`) are first-class.
`grpc-timeout` maps to `DEADLINE_EXCEEDED`. Client cancel maps to
`CANCELLED`. Gzip is supported (`grpc-encoding` / Compressed-Flag).

Official interop: `pbrs-grpc-interop-server --port N` and
`pbrs-grpc-interop-client --server_host H --server_port N --test_case=empty_unary`.
The Go peer is `google.golang.org/grpc/interop/{client,server}` with
`-use_tls=false`. Loopback empty_unary / large_unary vs tonic 0.14 is
`rpc-bench` (excluded crate; latency process-gated, QPS reported).
