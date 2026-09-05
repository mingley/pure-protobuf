# pbrs

[![Crates.io](https://img.shields.io/crates/v/pbrs.svg)](https://crates.io/crates/pbrs)
[![Documentation](https://docs.rs/pbrs/badge.svg)](https://docs.rs/pbrs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

A high-performance, pure-Rust Protocol Buffers kernel with the Google protobuf v4 application API.

`pbrs` provides full support for Protocol Buffers (proto2, proto3, and Editions 2023) with **zero C/C++ dependencies**, **zero `unsafe` in application code**, and **100% official Google conformance**.

---

## Why pbrs?

- **Pure Rust**: No C or C++ compiler required. Unlike crates.io `protobuf` 4.x (which wraps Google's `upb` C library via FFI), `pbrs` compiles entirely with `rustc`.
- **Google Protobuf v4 API**: Matches the official Google Rust application API traits and shapes (`Parse`, `Serialize`, `Clear`, `proto!`, `ProtoStr`, `RepeatedView`, `DynamicMessage`).
- **100% Conformance**: Passes Google's official `conformance_test_runner` v35.1 (5,631 binary + JSON tests, 909 text tests, 0 unexpected failures, 0 skips).
- **Prost Alternative**: Unlike Prost, which uses custom derive traits and layout conventions, `pbrs` gives you the standard protobuf v4 API, exact proto3 presence semantics, WKT support, dynamic reflection, and field-wise JSON/Text formatting.
- **Fast & Memory Efficient**: Single-pass parsing with small-string optimization (SSO), zero-allocation empty collections, lazy sub-message parsing, and vectorized packed-scalar decoding.

---

## Workspace Crates

| Crate | Description | crates.io Status |
|---|---|---|
| [`pbrs`](.) | The core Protocol Buffers kernel: parser, serializer, codegen (`protoc-gen-pbrs`), dynamic messages, WKT, and JSON/text format. | `0.1.0` (Production Ready) |
| [`pbrs-grpc`](pbrs-grpc) | A standalone, pure-Rust HTTP/2 gRPC client and server kernel (no C, no tonic). Built on `rustls` + `Graviola`. | `0.1.0-alpha.1` (Pre-release / Preview) |
| [`protobuf-tonic`](protobuf-tonic) | A tonic 0.14+ `Codec` adapter allowing tonic servers and clients to use `pbrs` message types. | `0.1.0-alpha.1` (Pre-release / Preview) |
| [`examples/greeter`](examples/greeter) | A complete working example showing generated stubs, health checks, and server reflection. | Example only (`publish = false`) |

---

## Installation

Add `pbrs` to your `Cargo.toml`:

```toml
[dependencies]
pbrs = "0.1"
```

*(Until published to crates.io, you can point to the git repository:)*
```toml
[dependencies]
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
```

---

## Code Generation

`protoc` must be installed and available on your `PATH`.

### Option A: Using `build.rs` (Recommended)

Add `pbrs` as a build dependency in your `Cargo.toml`:

```toml
[build-dependencies]
pbrs = "0.1"
```

In your `build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])?;
    Ok(())
}
```

Then include the generated code in your `src/lib.rs` or `src/main.rs`:

```rust
include!(concat!(env!("OUT_DIR"), "/helloworld.rs"));
```

### Option B: Using `protoc-gen-pbrs` Plugin

Install or build the plugin binary:

```bash
cargo install --path . --bin protoc-gen-pbrs
```

Run `protoc` with the `--pbrs_out` flag:

```bash
protoc --pbrs_out=./gen --proto_path=./proto ./proto/hello.proto
```

Or using the helper script:

```bash
./scripts/gen.sh -I proto -o gen proto/your.proto
```

---

## Quickstart Example

Given a proto file (`proto/person.proto`):

```protobuf
syntax = "proto3";
package tutorial;

message Person {
  string name = 1;
  int32 id = 2;
  string email = 3;
  repeated string phones = 4;
}
```

Using the generated struct:

```rust
use pbrs::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create and populate a message
    let mut person = Person::new();
    person.set_name("Ada Lovelace");
    person.set_id(42);
    person.set_email("ada@example.com");
    person.phones_mut().push("555-0100");

    // 2. Serialize to wire format
    let bytes: Vec<u8> = person.serialize()?;

    // 3. Parse back from bytes
    let decoded = Person::parse(&bytes)?;
    assert_eq!(decoded.name(), "Ada Lovelace");
    assert_eq!(decoded.id(), 42);

    // 4. Inspect or clear
    println!("Parsed: {} (ID: {})", decoded.name(), decoded.id());
    
    Ok(())
}
```

---

## gRPC Integration

You can use `pbrs` messages with either gRPC stack:

1. **`pbrs-grpc` (Pure-Rust Kernel)**: If you want a lightweight, pure-Rust gRPC stack without `tonic`, C dependencies, or `unsafe` in the transport layer. See the [gRPC Guide](docs/grpc.md) and [`pbrs-grpc/README.md`](pbrs-grpc/README.md).
2. **`protobuf-tonic` (Tonic Adapter)**: If you have an existing `tonic` 0.14+ service and want to drop in `pbrs` messages instead of `prost`. See [`protobuf-tonic/README.md`](protobuf-tonic/README.md).

For a complete end-to-end example with service stubs, gRPC health checking (`grpc.health.v1`), and server reflection (`grpc.reflection.v1`), check out [`examples/greeter`](examples/greeter).

---

## Comparison with Alternatives

| Feature | `pbrs` | `protobuf` 4.x (`upb`) | `prost` | `pb-rs` |
|---|---|---|---|---|
| **Pure Rust** | Yes (No C/C++) | No (wraps C library via FFI) | Yes | Yes |
| **API Shape** | Google Protobuf v4 | Google Protobuf v4 | Custom Prost traits | Custom traits |
| **Official Conformance** | 100% (5,631 binary/JSON + 909 text) | Passes with skip list | Partial (binary only) | Partial |
| **Proto Editions** | proto2, proto3, Edition 2023 | proto2, proto3, Edition 2023 | proto2, proto3 | proto2, proto3 |
| **Well-Known Types (WKT)** | Built-in (Timestamp, Duration, Any, etc.) | Built-in | `prost-types` | Minimal |
| **JSON & Text Format** | Field-wise & Dynamic conformance | C++ / upb implementation | Optional third-party | No |
| **Dynamic Messages** | Full descriptor reflection | Full reflection | None | None |
| **gRPC Options** | Native `pbrs-grpc` OR `tonic` | Google gRPC | `tonic-prost` | None |

---

## Conformance & Testing

`pbrs` is tested against Google's official protobuf test suite:

- **Official `conformance_test_runner` v35.1**:
  - **Required tests (×2)**: 5,631 binary + JSON, 0 unexpected failures.
  - **Text format tests**: 909 text tests, 0 unexpected failures.
  - **Recommended tests (`--enforce_recommended`)**: Passed with 0 unexpected failures.
  - **Skip list**: None. Unlike `upb`, which maintains a skip list for proto2 UTF-8, `pbrs` passes all tests without exclusions.

Run the conformance suite locally:

```bash
./scripts/fetch-protobuf.sh
./scripts/conformance.sh
```

---

## Documentation

- [Architecture Overview](docs/architecture.md) — Module organization and crate boundaries.
- [Design & Internals](docs/design.md) — Memory layout, parsing strategy, and optimization techniques.
- [Relative to upb](docs/upb.md) — Detailed comparison with Google's C-based upb kernel.
- [Benchmarks & Performance](docs/benchmarks.md) — Unary, streaming, and throughput measurements.
- [Implementation Status](docs/status.md) — Supported features, conformance breakdown, and roadmap.
- [Native gRPC Kernel Guide](docs/grpc.md) — In-depth guide to building microservices with `pbrs-grpc`.
- [Tonic Adapter Guide](protobuf-tonic/README.md) — Using `pbrs` with tonic 0.14+.
- [Release Policy & Publishing](docs/RELEASE.md) — Automated crates.io publication workflows and tag conventions.
- [Production Readiness Roadmap](docs/ROADMAP.md) — Phased ramp to production GA (1.0.0).
- [TODO & Task Tracker](TODO.md) — Prioritized feature gap checklist.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

