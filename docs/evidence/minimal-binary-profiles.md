# Minimal binary feature profiles

Base: GN-11 source/evidence head `4f4c87d4`. The optional minimal documentation
probe failed because Cargo built `protoc-gen-pbrs` while `codegen` was disabled.
The binary references `pbrs::codegen`, whose library module is feature gated.
The [before failure](minimal-binary-profiles/before-codegen-disabled.log) is retained.

Adding the codegen binary gate exposed the same existing declaration problem in
`conformance`: it imports `pbrs::gencode`, which needs the `conformance` feature.
The intermediate [check](minimal-binary-profiles/intermediate-conformance-disabled-check.log)
and [documentation](minimal-binary-profiles/intermediate-conformance-disabled-docs.log)
failures are retained. Both binary declarations now name their required feature:

| Binary | Required feature |
|---|---|
| `protoc-gen-pbrs` | `codegen` |
| `conformance` | `conformance` |

No source, library feature definition, dependency, version or default changed.
Cargo now skips disabled binaries in minimal profiles. Explicitly enabling each
feature continues to build its corresponding executable.

Validation used Rust 1.99.0, pinned libprotoc 35.1, one Cargo job and
`--locked --target-dir target/base -p pbrs`:

- `cargo check --no-default-features` passed ([log](minimal-binary-profiles/minimal-check.log)).
- `cargo test --no-default-features --test documentation` passed all 24 applicable
  contracts ([log](minimal-binary-profiles/minimal-docs.log)). The facade proof is
  guarded by `codegen`, as its generation API requires that feature.
- `cargo build --bin protoc-gen-pbrs` passed with defaults
  ([log](minimal-binary-profiles/default-cli-build.log)); the pinned compiler ran
  the plugin successfully on the checked `aliases.proto` fixture.
- `cargo build --no-default-features --features codegen --bin protoc-gen-pbrs`
  passed ([log](minimal-binary-profiles/codegen-only-cli-build.log)); the same
  compiler request ran successfully and emitted byte-identical Rust files.
- `cargo build --no-default-features --features conformance --bin conformance`
  passed ([log](minimal-binary-profiles/conformance-enabled-build.log)); an empty
  stdin EOF exited zero with no output ([record](minimal-binary-profiles/conformance-eof-smoke.json)).
  This is a CLI smoke; integrated conformance remains the coordinator's gate.

The [provenance record](minimal-binary-profiles/provenance.json) contains manifest,
fixture, output and artifact hashes and verifies unchanged feature/package/
dependency definitions. Readable logs remove trailing empty lines only; exact
original logs are in [raw-logs.tar.gz](minimal-binary-profiles/raw-logs.tar.gz).
This manifest follow-up is separate from GN-11's measured-source evidence.
