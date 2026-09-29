# Tonic example ports

These examples show two ways to adopt the native `pbrs-grpc` transport:
generate Tonic-shaped handlers with `pbrs` messages, or retain Prost messages
and use native service stubs. Each executable starts its own local server,
checks calls, and exits.

Install `protoc`, then run from the repository root:

```sh
cargo run -p pbrs-grpc-example-tonic-ports --bin tonic-port-helloworld
cargo test -p pbrs-grpc-example-tonic-ports --test ports
```

The integration test exercises all ports: unary and streaming calls, RouteGuide,
interceptors, health, reflection, TLS, Unix sockets, compression, error details,
balancing, a custom JSON codec, tracing, authentication, cancellation, h2c, and
Tower middleware. Unix-socket behavior is platform-dependent. TLS examples use
public repository test keys; provide your own credentials for deployment.

To include Zstandard compression, use Rust 1.87 or newer:

```sh
cargo test -p pbrs-grpc-example-tonic-ports --features zstd --test ports
```

Read [src/lib.rs](src/lib.rs) for implementations and [build.rs](build.rs) for
both generation paths. The [migration guide](../../docs/guides/migration.md)
explains the API choices; the [comparison matrix](../../docs/guides/comparison.md)
records remaining differences. These local examples validate migration
behavior; cross-stack interop evidence is maintained separately under
[tests/interop/tonic](../../tests/interop/tonic/).
