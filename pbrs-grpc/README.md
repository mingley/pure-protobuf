# pbrs-grpc

A pure-Rust gRPC kernel over [pbrs](../README.md). No `unsafe` in the kernel,
no C or C++ compiled into the build, no tonic. TLS uses rustls with Graviola
(rustc only; no `aws-lc-rs` or `ring`).

```toml
[dependencies]
pbrs = "0.1"
pbrs-grpc = "0.1"

[build-dependencies]
pbrs = "0.1"
```

```rust
// build.rs
pbrs::codegen::Config::new()
    .emit_kernel_stubs(true)
    .compile_protos(&["proto/hello.proto"], &["proto"])?;
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

and call it:

```rust
let client = GreeterClient::new(Channel::connect("127.0.0.1:50051").await?);
let reply = client.say_hello(Request::new(req)).await?;
```

All four call shapes, `Router` for several services, TLS (rustls + Graviola,
no C compiler) and mTLS, `grpc.health.v1`, HTTP/2 PING keepalive, graceful
drain with `GOAWAY`, per-message gzip, deadlines, cancellation, ASCII and
`-bin` metadata, and OK-path custom trailers.

**[Guide](../docs/grpc.md)** — building services, streaming, metadata, errors,
deadlines, compression, limits, tuning, testing, and writing a service without
codegen.

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
connection, 16 MiB windows. `tests/hostile.rs` speaks raw HTTP/2 to check them,
sending length prefixes claiming 4 GiB, gzip bombs, reserved flag values,
truncated frames, and malformed paths, then verifying the server still serves.
Property tests add what fixed cases cannot: frames survive arbitrary chunk
boundaries, arbitrary bytes never panic and never exceed the cap, and a
compressed frame never inflates past it.

Every hand-written module carries `#[forbid(unsafe_code)]`, which cannot be
relaxed from inside it. The two modules that `include!` generated messages are
exempt, because pbrs gencode uses `unsafe` for zeroed-message construction.

See [the threat model](../docs/grpc.md#limits-and-the-threat-model).

## Scope

h2c by default; TLS is opt-in via `ServerTls` / `ClientTls` (rustls + Graviola,
certificate verification is not optional). No load balancing, no retries —
pool with `ChannelConfig::connections`, retry at the call site, and see
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

`pbrs-grpc-hello` is a worked example exercising all four call shapes over
loopback, and `tests/codegen.rs` compiles a fresh `.proto` service the way a
user's crate does, to keep the generated `::pbrs_grpc` paths honest.
