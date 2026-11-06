# Alloc-only messages

The unreleased 0.2.1 candidate can generate native binary messages for
`thumbv7em-none-eabi`. Generation runs on a host with `std`; the target runtime
uses `core` and `alloc`.

The target needs a heap allocator, pointer compare-and-swap and 32-bit atomics.
Your firmware supplies the allocator and startup code. This profile does not
supply a gRPC transport, reflection, JSON/text, or the `rust_out` compatibility
runtime.

Use a repository checkout while this version is unpublished. These paths assume
your application sits next to the `pure-protobuf` checkout:

```toml
[workspace]
resolver = "2"

[dependencies]
pbrs = { path = "../pure-protobuf", default-features = false, features = ["alloc-only"] }

[build-dependencies]
pbrs = { path = "../pure-protobuf", default-features = false, features = ["codegen"] }
```

Resolver 2 keeps the host generator's std features separate from the target
runtime. If your application belongs to a workspace, set its resolver at the
workspace root.

In `build.rs`, select alloc-only output:

```rust
fn main() -> Result<(), pbrs::codegen::CodegenError> {
    pbrs::codegen::Config::new()
        .alloc_only(true)
        .compile_protos(&["message.proto"], &["proto"])?;
    Ok(())
}
```

Put your schema in `proto/message.proto`. Include the generated modules from
your library:

```rust
#![no_std]
extern crate alloc;

pub mod messages {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}
```

Install the target and build:

```sh
rustup target add thumbv7em-none-eabi
cargo build --target thumbv7em-none-eabi
```

`alloc_only(true)` disables reflection, JSON/text helpers, service stubs and
shared descriptors by default. Explicit incompatible output options return a
configuration error. Ordinary generated output stays unchanged when this
option is absent or false.

Disabling Cargo default features alone preserves the existing std runtime.
Select `alloc-only` explicitly. If Cargo enables both `std` and `alloc-only` in
the same runtime dependency context, `std` wins. Pure alloc-only builds on OS
hosts are currently rejected. Existing std profiles retain their std APIs and
SIMD runtime dispatch.

The alloc backend publishes lazy cache values through pointer atomics. Racing
initializers may construct multiple candidates; one is retained and the others
are dropped. Each candidate needs a box allocation. Std profiles keep
`std::sync::OnceLock`. Performance for the alloc backend remains unmeasured.

The [generated consumer proof](../../tests/proof/alloc_runtime/README.md) builds
concrete Cortex messages and matching Prost messages, and runs functional
checks on a host with std explicitly enabled. It covers nested defaults,
collections, groups, selected storage and cache fallbacks. Firmware execution
and other embedded targets remain unverified.
