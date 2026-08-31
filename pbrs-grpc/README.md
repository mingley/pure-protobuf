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
`Code::is_retryable` / `Status::is_retryable` (gRPC A6: `UNAVAILABLE` only), `Status::retry_delay` from packed `RetryInfo`, `Status::from_error` wrapping local errors,
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
