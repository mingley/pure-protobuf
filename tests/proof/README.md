# Parity and performance checks

These independent consumers test pbrs against pinned tonic/prost releases.
They preserve counterexamples. A passing audit means the recorded behavior
was reproduced; it does not mean full parity or a performance ranking.

| Check | Run | What it establishes |
|---|---|---|
| Protobuf wire behavior | `cargo run --locked --manifest-path tests/proof/prost_parity/Cargo.toml` | Compares parse and merge results. Strict mode fails on any recorded difference. |
| Known wire differences | Add `-- --check-known-differences` to that command | Requires the exact known counterexamples and agreement on the remaining corpus. |
| Owned-profile wire behavior | Add `--features owned-messages` before `--` in either protobuf command | Runs the same corpus with nested children constructed during parsing; known differences remain explicit. |
| Owned-message storage | `cargo run --locked --manifest-path tests/proof/owned_messages/Cargo.toml` | Checks merge, validation, shared-byte ownership, clone/mutation and default-profile isolation against independent controls. |
| Lazy materialization | `cargo run --locked --manifest-path tests/proof/validated_lazy/Cargo.toml` | Checks single-pass descendant validation, partial-read allocation bounds, malformed input, clone/thread safety and old-code fallback. |
| Generated forbid-unsafe policy | `python3 tests/proof/safe_defaults/run.py --out work/parity-safe-defaults` | Compiles lazy, owned and boxed consumers under `forbid(unsafe_code)` and requires the ordinary-profile negative control; runtime dependencies retain their own unsafe code. |
| Generated consumers and embedded support | `python3 tests/proof/codegen_parity/run.py --output work/parity-codegen` | Runs valid roundtrips, build-configuration probes and an alloc-only target control. |
| Field view accessor names | See [field view consumers](field_views/README.md) | Checks singular message/group view methods, existing field APIs, and wire values against Prost across four storage/default profiles. |
| Generated namespaces | `python3 tests/proof/namespace_symbols/run.py --out work/parity-namespaces` | Checks framework symbol shadows, runtime aliases and preserved user paths across four storage/default profiles. |
| RPC service namespaces | `python3 tests/proof/rpc_names/run.py --out work/parity-rpc-names` | Compiles 55 service names in four stub modes, strict aliases and a pinned Tonic reference against actual runtime artifacts. |
| gRPC message presence | `cargo test --locked --manifest-path tests/proof/tonic_parity/Cargo.toml -- --nocapture` | Compares missing and framed-empty messages against tonic, including peer-error precedence. |
| Codec costs | See [codec benchmark](codec_benchmark/README.md) | Keeps parse-only and reading every field separate; saves all comparative results. |
| RPC CPU claim preflight | `python3 scripts/verify-performance-claim.py --help` | Checks supplied campaign evidence and recomputes intervals; cannot certify a world ranking. |

Install Rust, Python 3 and `protoc`. For the codegen audit, also run:

```sh
rustup target add thumbv7em-none-eabi
cargo fetch --locked --manifest-path tests/proof/codegen_parity/Cargo.toml
cargo fetch --locked --manifest-path tests/proof/namespace_symbols/Cargo.toml
cargo fetch --locked --manifest-path tests/proof/codegen_parity/no_std/prost/Cargo.toml
cargo fetch --locked --manifest-path tests/proof/codegen_parity/no_std/pbrs/Cargo.toml
```

Before running the safe-default consumer, cache its dependencies too:

```sh
cargo fetch --locked --manifest-path tests/proof/safe_defaults/Cargo.toml
```

Both Python runners use the downloaded dependencies offline. The embedded
control checks compilation; it does not execute on embedded hardware.
The configured consumer also checks type names, type URLs, `Any` conversions
and retained enum prefixes against Prost. Three boxed-field consumers check
selected, default and owned storage against the same reference wire values.
The [selected bytes controls](codegen_parity/bytes.md) add real Bytes-backed
storage, ownership and codec checks; run them alone with `--only bytes`.
The remaining API compile-fail probe requests `btree_map`. A separate
compile-fail control records the reflection-off complex-codec limit, and a
runtime counterexample retains duplicate-key removal differences. Use the
runner or a named consumer rather than building every binary in that package.

The packages have independent lockfiles and stay outside the root workspace.
Read each result's source revision, compiler versions, raw inputs and scope
before using it as evidence.
