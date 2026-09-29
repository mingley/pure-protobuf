# Migrating to pure-protobuf

Use this guide when moving an existing Rust Protocol Buffers or gRPC codebase
to `pure-protobuf`. You should know whether your current stack uses `prost`,
Tonic, or Google's `protobuf` 4.x crate. Bottom line: the main migration work
is switching message traits and choosing the right generated service stubs.

---

## 1. Migrating from Prost

Start with the trait model. `pbrs` message types use the Google Protobuf v4
application API traits, not `prost::Message`.

> **Important**: `pbrs` message types implement the official Google Protobuf v4 application API traits (`Parse`, `Serialize`, `Clear`, `Message`), **not** `prost::Message`.

### Trait Comparison
| Feature | Prost (`prost`) | pure-protobuf (`pbrs`) |
|---|---|---|
| **Core Trait** | `prost::Message` | `pbrs::Message` |
| **Parsing** | `Message::decode(bytes)` | `Parse::parse(&bytes)` |
| **Serialization** | `Message::encode(&mut buf)` | `Serialize::serialize(&self) -> Result<Vec<u8>, ...>` |
| **Field Presence** | `Option<T>` for optional / message | Accessors: `has_foo()`, `foo()`, `clear_foo()` |
| **Repeated Fields** | `Vec<T>` | `RepeatedView<T>` / `Vec<T>` |
| **String Fields** | Standard `String` | Small-string optimized (SSO <= 23 bytes) |
| **C/C++ Dependencies** | None (pure Rust) | None (pure Rust) |

### Code Migration Example

Replace `prost::Message::decode` / `encode` calls with the `Parse` and
`Serialize` traits from `pbrs::prelude`.

```rust
// Prost pattern:
// use prost::Message;
// let msg = MyMessage::decode(&bytes[..])?;
// let mut out = Vec::new();
// msg.encode(&mut out)?;

// pbrs pattern:
use pbrs::prelude::*;

let msg = MyMessage::parse(&bytes)?;
let out = msg.serialize()?;
```

---

## 2. Using pbrs with Existing Tonic Services

Keep Tonic when you want its routing, transport, and middleware stack. Generate
Tonic-shaped service stubs and use the `protobuf-tonic` codec adapter.

```toml
[dependencies]
pbrs = "0.2"
protobuf-tonic = "0.1.0-alpha.2"
tonic = { version = "0.14", default-features = false, features = ["transport", "codegen"] }
```

In `build.rs`, explicitly configure Tonic stub generation:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(true)
        .compile_protos(&["proto/service.proto"], &["proto"])?;
    Ok(())
}
```

This generates `#[tonic::async_trait]` service stubs. They use `pbrs` message
types through `protobuf-tonic::ProtobufCodec` instead of `prost::Message`.

### Tonic-shaped API over the native transport

If you want Tonic-shaped handlers but not Tonic's transport stack, generate the
compat mode instead:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::Config::new()
        .tonic_compat(true)
        .compile_protos(&["proto/service.proto"], &["proto"])?;
    Ok(())
}
```

The generated client/server names stay familiar, but they run on
`pbrs-grpc::Channel`, `Server`, and `Router`. The mode uses
`pbrs_grpc::compat::{Request, Response, Status, Streaming}` and accepts
`impl IntoRequest<T>` / `impl IntoStreamingRequest<T>` on client methods.

Mechanical rewrite table:

| Tonic code | Native compat rewrite |
|---|---|
| `use tonic::{Request, Response, Status};` | `use pbrs_grpc::compat::{Request, Response, Status};` |
| `tonic::Streaming<T>` | `pbrs_grpc::compat::Streaming<T>` |
| `tonic::transport::Server::builder().add_service(FooServer::new(svc)).serve(addr)` | `FooServer::new(svc).serve(addr)` or mount on `pbrs_grpc::Server` / `Router` |
| `FooClient::connect("http://host:port").await?` | `FooClient::connect("host:port").await?` |
| `client.unary(Request::new(msg)).await?` | unchanged, with compat `Request` |
| `client.client_stream(tokio_stream::iter(items)).await?` | `client.client_stream(pbrs_grpc::compat::iter(items)).await?` or pass any `futures_core::Stream<Item = T> + Send + 'static` |
| `Response::new(stream)` where `stream: Stream<Item = Result<T, Status>>` | unchanged; generated server stubs convert that stream to native `Streaming<T>` |

The repository keeps self-contained ports of Tonic 0.14's examples in
`pbrs-grpc/tests/compat_fixtures.rs`, with copied protos under
`pbrs-grpc/tests/fixtures/compat/`. The copied proto files retain the upstream
gRPC Apache-2.0 headers, and the adapted Rust fixtures retain the Tonic MIT
attribution.

| Upstream Tonic example sources | Compat fixture | Upstream lines | Fixture lines | Diff count |
|---|---|---:|---:|---:|
| `examples/src/helloworld/{server,client}.rs` | `helloworld_compat.rs` | 62 | 57 | +31 / -31 |
| `examples/src/routeguide/{server,client}.rs` | `routeguide_compat.rs` | 315 | 246 | +135 / -173 |

---

## 3. Migrating from Google upb (`protobuf` 4.x crate)

Google's official `protobuf` 4.x crate wraps the C-based `upb` kernel through
foreign function interface (FFI). `pbrs` keeps the runtime in Rust.

- **Build complexity**: `protobuf` 4.x requires a C/C++ compiler toolchain.
  `pbrs` is 100% pure Rust and builds with standard `cargo build`.
- **Memory management**: `upb` allocates messages in arenas (`upb_Arena`).
  `pbrs` uses Rust heap allocation with small-string optimizations and
  zero-allocation empty collections.
- **Safety**: `pbrs` removes the C FFI boundary, memory leaks, and
  segmentation faults from unsafe arena lifetimes.

---

## 4. Summary of Code Generation Differences

Choose the generator mode that matches the service stack you want to keep.

| Aspect | `prost-build` | `pbrs::codegen` |
|---|---|---|
| **Stub Selection** | `compile_protos` (tonic stubs optional) | Default: `pbrs-grpc` native kernel stubs; `.emit_tonic_stubs(true)` for Tonic |
| **Descriptor Format** | Bundled or system `protoc` | System `protoc` or bundled descriptor sets |
| **Well-Known Types** | Separate `prost-types` crate | Built-in WKTs in `pbrs::wkt` (`Timestamp`, `Duration`, `Any`, `Empty`, etc.) |
