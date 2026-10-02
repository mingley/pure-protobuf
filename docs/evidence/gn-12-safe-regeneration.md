# GN-12: safe bundled regeneration

Base: `03e2201d`. This is a correctness and reproducibility repair, not a
performance change. The coordinator approved the focused
`tests/test_regen_generated.py` write-scope extension on 2026-10-02.

The old command passed each proto directly into `src/generated`. Current
codegen emits a root registry and hierarchical copies as well as a flat
compatibility file. Each request therefore replaced the handwritten public
registry and left unrelated output trees in the checkout. The old command
also ignored `--check`, rewriting bindings instead of checking them.

The repaired command checks the immutable protobuf source SHA, rejects
modified tracked source and untracked proto inputs, verifies the existing
conformance build stamp, and verifies `libprotoc 35.1`. It invokes
`target/conformance-build/protoc` directly; a compiler on `PATH` cannot
substitute for that build. The Rust plugin is built from this checkout with
the locked dependency graph and selected from Cargo's current executable
artifact record. `CARGO_TARGET_DIR` may relocate the Rust build
without changing the pinned C++ compiler location.

All 13 input protos retain the previous per-file, shared-pool profile,
including FieldMask. Their flat outputs are checked against the handwritten
module registry before generation. Ambient `PURE_PROTOBUF_*` consumer
settings are cleared. The complete output is generated in a temporary
directory and the registered flat files are formatted with Rust Edition 2021
and the repository's rustfmt configuration. Missing outputs or destination
symlinks fail before any binding is copied. Generated hierarchy and registry
files remain temporary. Only changed registered files are copied; public
`src/generated/mod.rs` and unrelated files are preserved.

The required CI conformance job explicitly installs rustfmt and now runs
`./scripts/regen-generated.sh --check` after `scripts/conformance.sh`
provisions the source, compiler and provenance stamp. The test job also runs
the focused regeneration contracts without needing Rust builds or protoc.

## Regression proof

Before changing the script, the six initial fake-tool contract tests failed
nine assertions: registry replacement, hierarchical output, a mutating
`--check`, failure to reject drift, incomplete-output partial updates,
source SHA/cleanliness mismatches, compiler-version/build-stamp mismatches,
and unchecked registry coverage. The fake compiler emits both flat and
hierarchical outputs and a generated registry, reproducing the destructive
behavior without downloading tools or compiling Rust.

After the repair:

```bash
python3 -B -m unittest tests.test_regen_generated -q
bash -n scripts/regen-generated.sh
git diff --check
```

All eleven tests passed, as did shell syntax and whitespace validation. They
check every registered binding, byte-identical repeated generation without
mtime changes, successful and failing non-mutating checks, untracked files
(including a Rust source and an existing hierarchy), source/compiler
provenance, incomplete output, registry coverage, a custom Cargo target with
an unusable `PATH` compiler, protection against symlink writes, and invalid
command arguments.

Independent review reproduced an additional provenance defect in the initial
repair: with `CARGO_BUILD_TARGET` set, Cargo built a target-specific current
plugin while the script selected a stale default-directory executable. Two
new tests failed before the follow-up repair, demonstrating stale selection
and fallback when no executable artifact was reported. The repaired script
parses Cargo's JSON `compiler-artifact` record for this manifest's
`protoc-gen-pbrs` binary and rejects missing or ambiguous executable records.
The target-specific test verifies all 13 generation requests use the current
reported artifact while leaving the stale default executable untouched.

## Pinned generation and final gates

Upstream source/compiler pin: `v35.1`,
`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`. Regenerated output and final
acceptance gates are recorded after the GN-14 enum-option correction and
GN-13 generated implementation-lint repair are integrated. The coordinator
owns final conformance, package-consumer and public-semver results; the
script contracts alone do not qualify those gates.
