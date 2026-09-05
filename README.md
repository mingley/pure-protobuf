# pbrs

[![Crates.io](https://img.shields.io/crates/v/pbrs.svg)](https://crates.io/crates/pbrs)
[![Documentation](https://docs.rs/pbrs/badge.svg)](https://docs.rs/pbrs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

A high-performance, pure-Rust Protocol Buffers kernel with the Google protobuf v4 application API.

`pbrs` implements proto2, proto3, and Edition 2023 behavior with a Rust runtime
and code generator. See the [compatibility boundaries](docs/upb.md) and
[recorded conformance results](docs/status.md) for the tested scope.
Published versions are available for evaluation; production qualification and
performance leadership are tracked in the [implementation plan](docs/ROADMAP.md).

---

## Why pbrs?

- **Pure Rust**: No C or C++ compiler required. Unlike crates.io `protobuf` 4.x (which wraps Google's `upb` C library via FFI), `pbrs` compiles entirely with `rustc`.
- **Google Protobuf v4 API**: Matches the official Google Rust application API traits and shapes (`Parse`, `Serialize`, `Clear`, `proto!`, `ProtoStr`, `RepeatedView`, `DynamicMessage`).
- **Recorded Conformance**: Google's `conformance_test_runner` v35.1, through Edition 2023: 5,631 binary + JSON cases and 909 text cases, with no unexpected results.
- **Protobuf API Alternative**: Application-level v4-shaped traits, presence semantics, WKT support, dynamic reflection, and JSON/text formatting. Official generated internals and `prost::Message` are not drop-in compatible.
- **Performance-oriented Storage**: Small-string optimization, zero-allocation empty collections, lazy materialization after wire validation, and specialized packed-scalar handling. [Benchmarks](docs/benchmarks.md) report workload-specific results and losses.

---

## Workspace Crates

| Crate | Description | crates.io Status |
|---|---|---|
| [`pbrs`](.) | The core Protocol Buffers kernel: parser, serializer, codegen (`protoc-gen-pbrs`), dynamic messages, WKT, and JSON/text format. | `0.1.0` (Published; qualification in progress) |
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

---

## Code Generation

Generating code from `.proto` files requires `protoc` on your `PATH`.
Building the core crate alone uses a bundled descriptor set and does not
require it. The gRPC crates currently run code generation in their own build
scripts, so their builds also require `protoc`. There is no enforced
universal `protoc` version; see the [support matrix](#support-matrix).

### Option A: Using `build.rs` (Recommended)

Add `pbrs` as a build dependency in your `Cargo.toml`:

```toml
[build-dependencies]
pbrs = "0.1"
```

In your `build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/person.proto"], &["proto"])?;
    Ok(())
}
```

Then include the generated code in your `src/lib.rs` or `src/main.rs`:

```rust
include!(concat!(env!("OUT_DIR"), "/person.rs"));
```

This generates the message used in the quickstart below. For protos defining
services, the default is native `pbrs-grpc` stubs; tonic users must explicitly
select `Config::emit_tonic_stubs(true)`.

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

Using the `build.rs` above, put this in `src/main.rs`:

```rust
use pbrs::prelude::*;
include!(concat!(env!("OUT_DIR"), "/person.rs"));

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

1. **`pbrs-grpc` (Native Kernel)**: A Rust gRPC stack independent of `tonic`. See the [gRPC Guide](docs/grpc.md) and [`pbrs-grpc/README.md`](pbrs-grpc/README.md).
2. **`protobuf-tonic` (Tonic Adapter)**: For `tonic` 0.14 services using regenerated `pbrs` stubs rather than `prost` messages. Middleware must accept those message traits. See [`protobuf-tonic/README.md`](protobuf-tonic/README.md).

For a complete end-to-end example with service stubs, gRPC health checking (`grpc.health.v1`), and server reflection (`grpc.reflection.v1`), check out [`examples/greeter`](examples/greeter).

---

## Support matrix

Recorded against this repository on stable `rustc` 1.98. Declared
`rust-version` is the CI MSRV job, not this host's toolchain. There is no
claimed universal minimum `protoc`. Releases: [release guide](docs/RELEASE.md)
(tag/dispatch only; `main` pushes do not publish).

| Crate | Declared MSRV | Tested | `protoc` | Stub default |
|---|---|---|---|---|
| [`pbrs`](.) | 1.85 | rustc 1.98 (this host); CI `msrv-core` 1.85 `--lib`, stable Linux + macOS | Not required to **build** the crate (bundled FileDescriptorSet). Required for `compile_protos` / `protoc-gen-pbrs`. | Messages; `.proto` `service` blocks emit native `pbrs-grpc` stubs |
| [`pbrs-grpc`](pbrs-grpc) | 1.85 | rustc 1.98 (this host); CI `msrv-core` 1.85 `--lib` (incl. `tcp::tests`), stable Linux + macOS | Required (`build.rs` calls `compile_protos`) | Native kernel (`compile_protos` default) |
| [`protobuf-tonic`](protobuf-tonic) | 1.88 | rustc 1.98 (this host); CI `msrv-tonic` 1.88 | Required (`build.rs` calls `compile_protos`) | Must call [`Config::emit_tonic_stubs(true)`](protobuf-tonic/README.md); not a `prost::Message` drop-in |
| [`examples/greeter`](examples/greeter) | 1.85 | rustc 1.98 (this host); CI stable Linux (`--workspace`) + macOS onboarding | Required | Native kernel default |

**Untested / unsupported** (not a support commitment):

- tonic 0.12 and 0.13 are **unsupported**.
- Edition 2024 is **untested**.
- Windows CI is **untested**.

## Choosing a Stack

Use the [upb comparison](docs/upb.md) for API and representation differences,
and the [benchmark report](docs/benchmarks.md) for versioned, workload-specific
comparisons with prost, the Google Rust/upb wrapper, buffa, tonic and grpc-go.
Those results do not establish universal feature parity or superiority.
The [roadmap scorecard](docs/ROADMAP.md#scorecard) defines the evidence needed
to make stronger claims.

---

## Conformance & Testing

`pbrs` is tested against Google's official protobuf test suite:

- **Official `conformance_test_runner` v35.1**:
  - **Required tests (×2)**: 5,631 binary + JSON, 0 unexpected failures.
  - **Text format tests**: 909 text tests, 0 unexpected failures.
  - **Recommended tests (`--enforce_recommended`)**: Passed with 0 unexpected failures.
  - **Scope**: These recorded results concern this pinned conformance runner, not every upstream suite. The separate [`rust/test/shared` coverage](docs/status.md#skipped-rusttestshared-files) has documented exclusions.

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
- [Tonic Adapter Guide](protobuf-tonic/README.md) — Using `pbrs` with tonic 0.14+ (`emit_tonic_stubs(true)`).
- [Support matrix](#support-matrix) — Declared MSRV vs tested toolchain, `protoc` requirements, stub defaults, untested/unsupported cases.
- [Release Policy & Publishing](docs/RELEASE.md) — Tag/dispatch crates.io publisher, required CI, `CRATES_IO_TOKEN` (not Trusted Publishing).
- [Implementation Plan & Scorecard](docs/ROADMAP.md) — Ordered work packages for compatibility, reliability, operational readiness and measurable performance leadership.
- [Execution Queue](TODO.md) — First PRs, dependencies and evidence required to close each item.

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
