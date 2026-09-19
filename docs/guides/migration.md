# Migrating to pure-protobuf

This guide explains how to migrate existing Rust Protocol Buffers and gRPC applications to `pure-protobuf`, comparing trait models, code generators, and runtime expectations.

---

## 1. Migrating from Prost

The most critical architectural difference when migrating from `prost` is the trait model:

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

If you already have a codebase built on `tonic` and want to keep Tonic's routing and middleware stack, use the `protobuf-tonic` adapter:

```toml
[dependencies]
pbrs = "0.1"
protobuf-tonic = "0.1.0-alpha.1"
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

This generates `#[tonic::async_trait]` service stubs that use `pbrs` message types via `protobuf-tonic::ProtobufCodec` instead of `prost::Message`.

---

## 3. Migrating from Google upb (`protobuf` 4.x crate)

Google's official `protobuf` 4.x crate wraps the C-based `upb` kernel using FFI:

- **Build Complexity**: `protobuf` 4.x requires a C/C++ compiler toolchain. `pbrs` is 100% pure Rust and compiles cleanly with standard `cargo build`.
- **Memory Management**: `upb` allocates messages in arena arenas (`upb_Arena`). `pbrs` uses idiomatic Rust heap allocation with small-string optimizations and zero-allocation empty collections.
- **Safety**: `pbrs` eliminates C FFI boundaries, memory leaks, and segmentation faults from unsafe arena lifetimes.

---

## 4. Summary of Code Generation Differences

| Aspect | `prost-build` | `pbrs::codegen` |
|---|---|---|
| **Stub Selection** | `compile_protos` (tonic stubs optional) | Default: `pbrs-grpc` native kernel stubs; `.emit_tonic_stubs(true)` for Tonic |
| **Descriptor Format** | Bundled or system `protoc` | System `protoc` or bundled descriptor sets |
| **Well-Known Types** | Separate `prost-types` crate | Built-in WKTs in `pbrs::wkt` (`Timestamp`, `Duration`, `Any`, `Empty`, etc.) |
