# pbrs

[![Crates.io](https://img.shields.io/crates/v/pbrs.svg)](https://crates.io/crates/pbrs)
[![Documentation](https://docs.rs/pbrs/badge.svg)](https://docs.rs/pbrs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

`pbrs` is a Protocol Buffers runtime and code generator with the Google
protobuf v4 application API. The workspace also includes `pbrs-grpc` for
gRPC over HTTP/2 and `protobuf-tonic` for using pbrs messages with tonic 0.14+.

**Status:** conformance is tested through Protocol Buffers Edition 2023.
Edition 2024 generation supports a [defined subset](docs/edition-2024.md).
The gRPC crates are previews; see the [support policy](docs/support-policy.md)
for supported versions and deployment guidance.
For contributors, start with [what to work on next](TODO.md) and the
[2026-09-29 repository audit](docs/audit-2026-09-29.md).

<a id="why-use-pbrs"></a>
## Features

- **Google protobuf v4 API:** generated types use `Parse`, `Serialize`, `Clear`, `proto!`,
  `ProtoStr`, `RepeatedView`, and `DynamicMessage`.
- **Recorded conformance:** Google's `conformance_test_runner` v35.1 passes
  5,631 binary + JSON cases and 909 text cases with no unexpected results.
- **Formats and reflection:** proto2, proto3, Edition 2023, well-known types
  (WKT), dynamic reflection, JSON, and text format.
- **Storage:** small-string optimization, zero-allocation
  empty collections, lazy materialization after wire validation, and specialized
  packed-scalar handling are built into the runtime.

`pbrs` is an application API alternative, not a drop-in replacement for
official generated internals or `prost::Message`. See the
[upb comparison](docs/upb.md) and [implementation status](docs/status.md) for
the tested boundaries.

## Quick start: install, generate, use

This quick start generates Rust from `proto/person.proto` during your Cargo
build. Direct `.proto` generation requires `protoc` on your `PATH`; see
[code generation options](#code-generation-options) if you want checked-in
descriptors instead.

1. Add `pbrs` as both a runtime and build dependency:

   ```toml
   [dependencies]
   pbrs = "0.2"

   [build-dependencies]
   pbrs = "0.2"
   ```

2. Add a proto file at `proto/person.proto`:

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

3. Generate Rust in `build.rs`:

   ```rust
   fn main() -> Result<(), Box<dyn std::error::Error>> {
       pbrs::codegen::compile_protos(&["proto/person.proto"], &["proto"])?;
       Ok(())
   }
   ```

4. Include and use the generated type:

   ```rust
   use pbrs::prelude::*;

   include!(concat!(env!("OUT_DIR"), "/person.rs"));

   fn main() -> Result<(), Box<dyn std::error::Error>> {
       let mut person = Person::new();
       person.set_name("Ada Lovelace");
       person.set_id(42);
       person.set_email("ada@example.com");
       person.phones_mut().push("555-0100");

       let bytes: Vec<u8> = person.serialize()?;
       let decoded = Person::parse(&bytes)?;

       assert_eq!(decoded.name(), "Ada Lovelace");
       assert_eq!(decoded.id(), 42);

       println!("Parsed: {} (ID: {})", decoded.name(), decoded.id());
       Ok(())
   }
   ```

For protos that define services, `compile_protos` emits native `pbrs-grpc`
stubs by default. tonic users must opt in with
[`Config::emit_tonic_stubs(true)`](protobuf-tonic/README.md).

## Which crate do I need?

<a id="workspace-crates"></a>

| Need | Use | Version |
|---|---|---|
| Protocol Buffers messages, reflection, JSON/text, or code generation | [`pbrs`](.) | `0.2.0` |
| Native gRPC client and server | [`pbrs-grpc`](pbrs-grpc) | `0.1.0-alpha.2` (Preview) |
| tonic 0.14+ with `pbrs` messages | [`protobuf-tonic`](protobuf-tonic) | `0.1.0-alpha.2` (Preview) |
| Runnable service with health and reflection | [`examples/greeter`](examples/greeter) | Example |

## Code generation options

Generating directly from `.proto` files needs `protoc`. Generating from a
previously compiled descriptor set does not run `protoc` during the application
build.

| Task | Needs `protoc`? | Notes |
|---|---|---|
| Build the core `pbrs` crate | No | The crate uses a bundled `FileDescriptorSet`. |
| Build `pbrs-grpc` and `protobuf-tonic` (`0.1.0-alpha.2` and later, or this checkout) | No | They build from checked descriptor sets. |
| Build the older `pbrs-grpc` / `protobuf-tonic` `0.1.0-alpha.1` crates | Yes | Those alphas predate the checked-descriptor change. |
| Run `compile_protos` or `protoc-gen-pbrs` on application `.proto` files | Yes | `protoc` must be on `PATH`. |
| Run `Config::compile_descriptor_set` from a checked descriptor set | No during the app build | Creating or updating the descriptor set still needs `protoc`. |

There is no enforced universal `protoc` version; see the
[support matrix](#support-matrix).

### Direct `.proto` generation with `build.rs`

Use this when your build environment has `protoc` available:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/person.proto"], &["proto"])?;
    Ok(())
}
```

Then include the generated file in `src/lib.rs` or `src/main.rs`:

```rust
include!(concat!(env!("OUT_DIR"), "/person.rs"));
```

### Checked descriptor set generation

Use this when you want application builds to avoid running `protoc`. First
create and check in a descriptor set with imports included:

```bash
protoc -I proto --include_imports --descriptor_set_out=proto/person.fds proto/person.proto
```

Then compile from that descriptor set:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::Config::new()
        .out_dir(std::env::var("OUT_DIR")?)
        .compile_descriptor_set("proto/person.fds", &["person.proto"], &["proto"])?;
    Ok(())
}
```

Descriptor targets use include-relative names such as `person.proto`. The
descriptor and available source imports are tracked for rebuilds. See the
[codegen guide](docs/guides/codegen.md) for native stubs, tonic stubs, and
messages-only generation.

Against this checkout, a consumer using a checked descriptor set can cold-build
messages, native stubs, and tonic stubs without `protoc` on its build `PATH`.
The older `0.1.0-alpha.1` adapter archives still require it.

### `protoc-gen-pbrs` plugin

Install or build the plugin binary:

```bash
cargo install --path . --bin protoc-gen-pbrs
```

Run `protoc` with the `--pbrs_out` flag:

```bash
protoc --pbrs_out=./gen --proto_path=./proto ./proto/hello.proto
```

Or use the helper script:

```bash
./scripts/gen.sh -I proto -o gen proto/your.proto
```

## gRPC choices

| Stack | Choose it when | Read more |
|---|---|---|
| `pbrs-grpc` native kernel | You want the native gRPC client and server. | [gRPC guide](docs/grpc.md), [`pbrs-grpc/README.md`](pbrs-grpc/README.md) |
| `protobuf-tonic` adapter | You already use tonic 0.14+ and can use regenerated `pbrs` stubs instead of `prost` messages. Middleware must accept those message traits. | [`protobuf-tonic/README.md`](protobuf-tonic/README.md) |

For a complete service with generated stubs, health checking, and server
reflection, see [`examples/greeter`](examples/greeter).

## Support matrix

The minimum supported Rust versions (MSRV) below are checked in CI.
The tested column includes earlier runs with Rust 1.98. Releases follow
the [release guide](docs/RELEASE.md); pushes to `main` do not publish crates.

The maintained crates and tools use **Rust language Edition 2024**, available
from rustc 1.85. Frozen reference/comparator and discarded-experiment
manifests keep their original edition for reproducibility. This is independent
of **Protocol Buffers Edition 2024**.

| Crate | Declared MSRV | Tested | `protoc` | Stub default |
|---|---|---|---|---|
| [`pbrs`](.) | 1.85 | rustc 1.98 (this host); CI `msrv-core` 1.85 `--lib`, stable Linux + macOS | Not required to **build** the crate (bundled FileDescriptorSet). Required for `compile_protos` / `protoc-gen-pbrs`. | Messages; `.proto` `service` blocks emit native `pbrs-grpc` stubs |
| [`pbrs-grpc`](pbrs-grpc) | 1.85 | rustc 1.98 (this host); CI `msrv-core` 1.85 `--lib` (incl. `tcp::tests`), stable Linux + macOS | **Current source:** no compiler needed to build from checked FileDescriptorSets; required to regenerate descriptors or compile application `.proto`. **Older `0.1.0-alpha.1`:** still requires `protoc`. | Native kernel (`compile_protos` default) |
| [`protobuf-tonic`](protobuf-tonic) | 1.88 | rustc 1.98 (this host); CI `msrv-tonic` 1.88 | **Current source:** no compiler needed to build from the checked FileDescriptorSet. **Older `0.1.0-alpha.1`:** still requires `protoc`. Direct `.proto` compilation needs it in either version. | Must call [`Config::emit_tonic_stubs(true)`](protobuf-tonic/README.md); not a `prost::Message` drop-in |
| [`examples/greeter`](examples/greeter) | 1.85 | rustc 1.98 (this host); CI stable Linux (`--workspace`) + macOS onboarding | Required | Native kernel default |

**Untested / unsupported** (not a support commitment):

- tonic 0.12 and 0.13 are **unsupported**.
- Protocol Buffers Edition 2024 has descriptor and generated-consumer coverage
  for a [defined subset](docs/edition-2024.md); complete conformance beyond
  Edition 2023 is **not qualified**.
- Windows CI is **untested**.

## Performance and stack selection

The [benchmark report](docs/benchmarks.md) compares workloads with prost,
the Google Rust/upb wrapper, buffa, tonic, and grpc-go. Each result lists its
versions, workload, and measurement limits. The
[roadmap scorecard](docs/ROADMAP.md#scorecard) tracks the remaining comparisons.

For large payloads and copy behavior, also see
[Large payloads / zero-copy](docs/zero-copy.md).

## Conformance and testing

`pbrs` is tested against Google's official protobuf test suite:

| Suite | Recorded result | Scope |
|---|---:|---|
| Official `conformance_test_runner` v35.1 required tests (×2) | 5,631 binary + JSON, 0 unexpected failures | Pinned runner through Edition 2023 |
| Text format tests | 909 text tests, 0 unexpected failures | Pinned runner through Edition 2023 |
| Recommended tests (`--enforce_recommended`) | Passed with 0 unexpected failures | Does not imply every upstream suite |

The separate [`rust/test/shared` coverage](docs/status.md#skipped-rusttestshared-files)
lists its exclusions. The [enum-map repair](docs/evidence/shared-map-recovery.md)
and [closed-enum tests](docs/evidence/closed-enum-recovery.md) record the later
compatibility runs.

Run the conformance suite locally:

```bash
./scripts/fetch-protobuf.sh
./scripts/conformance.sh
```

## Documentation

The [documentation map](docs/documentation-map.md) helps you choose a guide.
For task status, use `python3 scripts/plan-status.py --ready`; for API details,
run `cargo doc --workspace --no-deps --open`.


| Topic | Start here |
|---|---|
| Architecture and crate boundaries | [Architecture overview](docs/architecture.md) |
| Runtime design and storage choices | [Design and internals](docs/design.md) |
| API and representation differences from Google's C-based upb kernel | [Relative to upb](docs/upb.md) |
| Measurements and known losses | [Benchmarks](docs/benchmarks.md) |
| Large payloads and copy behavior | [Large payloads / zero-copy](docs/zero-copy.md) |
| Supported features and conformance breakdown | [Implementation status](docs/status.md) |
| Native `pbrs-grpc` services | [Native gRPC kernel guide](docs/grpc.md) |
| Code generation, RPC shapes, interceptors, and operations recipes | [Task guides and recipes](docs/guides/rpc-shapes.md) |
| tonic 0.14+ integration | [Tonic adapter guide](protobuf-tonic/README.md) |
| Toolchain and `protoc` requirements | [Support matrix](#support-matrix) |
| Publishing policy | [Release policy](docs/RELEASE.md) |
| Roadmap and remaining work | [Implementation plan and scorecard](docs/ROADMAP.md) |
| Next contribution and audit findings | [Execution queue](TODO.md), [repository audit](docs/audit-2026-09-29.md), [task contracts](docs/plan/README.md) |
| gRPC performance work | [Performance plan](docs/plan/world-class/README.md) and [task cards](docs/plan/world-class/tasks.json) |
| Adopting pbrs in existing prost/tonic systems | [Adoption program](docs/plan/adoption/README.md) |
| Every-cell comparison against tonic and prost | [Dominance program](docs/plan/dominance/README.md) |

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
