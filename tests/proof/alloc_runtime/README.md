# Alloc-only generated messages

This proof builds concrete native messages and Prost 0.14.4 messages for
`thumbv7em-none-eabi`. Its host build dependency uses pbrs code generation with
`std`; the target dependency selects only `alloc-only`. Resolver 2 keeps their
feature sets separate. Compiler artifact receipts record both contexts.

Install Rust, Python 3, `protoc` and the target, then fetch the locked dependencies:

```sh
rustup target add thumbv7em-none-eabi
cargo fetch --locked --manifest-path tests/proof/alloc_runtime/Cargo.toml
cargo fetch --locked --manifest-path tests/proof/alloc_runtime/legacy_std/Cargo.toml
python3 tests/proof/alloc_runtime/run.py --out work/alloc-runtime-proof
```

The runner builds actual encode/decode/mutation calls for every scalar kind,
strings and bytes, optional and oneof fields, packed and unpacked collections,
maps, nested messages and proto2 required fields/groups/defaults. Five native
profiles cover lazy, owned, selected bytes/boxes/ordered maps, safe defaults with
a runtime alias, and an external type path containing `std`. Child default
statics, `Send + Sync` bounds and the 32-bit size-cache fallback are exercised.
A separate native Edition 2024 control covers the supported singular int32
extension API; it has a fixed wire expectation, not a Prost extension oracle.

Host tests execute the same message assertions against generated Prost with
pbrs `std` explicitly enabled. Another consumer retains the existing no-default
std APIs and Bytes integration; its artifact receipt must retain dependency std
features, including SIMD runtime dispatch. Pure host alloc-only is rejected.
The code-generation profile rejects reflection, JSON/text, service stubs and
shared descriptor output. It preserves user literals and external paths.

Use `--baseline-plugin /path/to/protoc-gen-pbrs` to compare ordinary output bytes
against a frozen previous generator. The fixture also compares an explicit
`alloc_only=false` reset with ordinary output and matches descriptor/plugin API
entry points. A dedicated `--target-dir` can share the team's bounded cache.
Commands, failures, source hashes and compiler feature receipts are retained;
source drift, a 440 MiB target directory or less than 1 GiB free space fails the
run. The target cap reserves room for sources and logs within a 500 MiB team budget.

Cortex compilation proves the selected target graph does not need `std`.
Host execution uses the std cache backend. This proof does not execute firmware,
claim support for every embedded target, provide a no_std gRPC transport or
reflection/formats/rust_out ABI, or measure performance.
