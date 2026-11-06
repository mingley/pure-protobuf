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
| Generated consumers and legacy target boundary | `python3 tests/proof/codegen_parity/run.py --output work/parity-codegen` | Runs roundtrips, build-configuration probes and the std-required legacy no-default target control. |
| Explicit alloc-only messages | `python3 tests/proof/alloc_runtime/run.py --out work/parity-alloc-runtime` | Builds concrete native and Prost Cortex messages with separate std host codegen, then runs std-backed host checks; see [scope](alloc_runtime/README.md). |
| Field view accessor names | See [field view consumers](field_views/README.md) | Checks singular message/group view methods, existing field APIs, and wire values against Prost across four storage/default profiles. |
| Selective source comments | `python3 tests/proof/comment_selectors/run.py --out work/parity-comments` | [15 compiled consumers](comment_selectors/README.md) compare selectors and wire values with Prost 0.14.4; three RPC modes have source-only comment checks. |
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
cargo fetch --locked --manifest-path tests/proof/comment_selectors/Cargo.toml
cargo fetch --locked --manifest-path tests/proof/codegen_parity/no_std/prost/Cargo.toml
cargo fetch --locked --manifest-path tests/proof/codegen_parity/no_std/pbrs/Cargo.toml
cargo fetch --locked --manifest-path tests/proof/alloc_runtime/Cargo.toml
cargo fetch --locked --manifest-path tests/proof/alloc_runtime/legacy_std/Cargo.toml
```

Before running the safe-default consumer, cache its dependencies too:

```sh
cargo fetch --locked --manifest-path tests/proof/safe_defaults/Cargo.toml
```

The Python runners use the downloaded dependencies offline. Cortex checks
establish target compilation, not firmware execution. The legacy no-default
control preserves pbrs std behavior; the explicit alloc-only consumer qualifies
its separate target profile.
The configured consumer also checks type names, type URLs, `Any` conversions
and retained enum prefixes against Prost. Three boxed-field consumers check
selected, default and owned storage against the same reference wire values.
The [selected bytes controls](codegen_parity/bytes.md) add real Bytes-backed
storage, ownership and codec checks; run them alone with `--only bytes`.
The [ordered-map controls](ordered_maps/README.md) add real unique tree storage,
duplicate replacement, key order, mutation and codec checks; run them alone
with `--only ordered`. A separate compile-fail control records the
reflection-off complex-codec limit, and a
runtime counterexample retains duplicate-key removal differences. Use the
runner or a named consumer rather than building every binary in that package.

The packages have independent lockfiles and stay outside the root workspace.
Read each result's source revision, compiler versions, raw inputs and scope
before using it as evidence.
