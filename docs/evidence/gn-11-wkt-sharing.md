# GN-11: shared imported Google protobuf definitions

Base source: `cf3eee22c6b06324e299f81b76915651471d8025`. This evidence is a
local dev-loop diagnostic. It does not establish comparative compile-time
leadership or production-corpus qualification.

The option-schema reproducer imports `google/protobuf/descriptor.proto` in
`opt_a.proto`, extends MessageOptions, and uses that option on one-field
messages in `opt_b.proto` and `opt_c.proto`. Before this change, all three
generated files contain their own complete descriptor types. The five initial
regression tests failed on the base: duplicate descriptor/WKT definitions,
the nonexistent documented `pbrs::wkt` path, silently empty mapped Google
targets, and unsupported descriptor mappings accepted without diagnostics.
The [baseline failure log](gn-11-artifacts/regression-before.log) is committed with this evidence.

Multi-input generation now adds imported Google protobuf source files to the
existing root registry once. The ordinary application files reference those
package paths instead of owning copies. Synthetic owners have only canonical
paths; explicitly requested files retain their flat compatibility aliases,
including when an imported owner has the same stem. This inspects the already
collected per-type facts once, without repeated per-target descriptor scans.

Single-input requests retain their self-contained WKT definitions and nested
`include!` compatibility. The live plugin's single-input WKT output is
byte-identical before and after, SHA-256
`b32e53bc6298407fc3e2b043b62db81ced1dbbcbcb8d8ccf80d03be1c04841bd`.
The compatibility boundary for multiple-input flat includes is documented in
the [layout contract](../codegen-layout.md#75-well-known-type-ownership-normative)
and [guide](../guides/codegen.md#shared-google-protobuf-types).

The additive runtime `pbrs::wkt` facade re-exports the existing standard
bindings and their proxy companions. It needs `conformance`, enabled by
default. It does not add generated copies or provide descriptor/tooling
schemas. Mappings to this facade validate the destination type; exact source aliases
such as `.custom.EmptyAlias` to `::pbrs::wkt::Empty` remain supported. Missing
destinations fail generation, and explicitly requesting a mapped Google source
also produces a named `extern_path` error.
Custom mappings continue to own their supplied implementations.

## Exact B3 output

Compiler/source: `libprotoc 35.1`, protobuf `v35.1`, source
`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`. Counts below include every emitted
Rust file in the raw plugin output, including the root registry. They retain
the plugin request's source information and default reflection/JSON/text
profile; they are not rustfmt-normalized or descriptor-stripped estimates.

| Metric | Base | Shared owners | Change |
|---|---:|---:|---:|
| Rust output bytes | 3,384,643 | 2,398,306 | -29.14% |
| Rust output lines | 49,018 | 31,413 | -35.92% |
| `DescriptorProto` definitions | 3 | 1 | -2 copies |
| Emitted Rust files | 4 | 5 | One shared owner added |

Each source continues to embed a full descriptor block for standalone
reflection. Adding the owner therefore adds one metadata block even though
it removes two full sets of generated descriptor implementations. Sharing
those metadata bytes is a separate compatibility decision; no such change
is hidden in these figures.

Live generation uses the pinned compiler with the same input list and flags:

```bash
protoc --plugin=protoc-gen-pbrs="$PLUGIN" --pbrs_out="$OUT" \
  -I tests/fixtures/codegen-wkt-sharing -I third_party/protobuf/src \
  tests/fixtures/codegen-wkt-sharing/opt_a.proto \
  tests/fixtures/codegen-wkt-sharing/opt_b.proto \
  tests/fixtures/codegen-wkt-sharing/opt_c.proto
```

The exact raw generated trees, including the single-input comparison, are in
[generated-output.tar.gz](gn-11-artifacts/generated-output.tar.gz). Per-file counts
are committed as [before](gn-11-artifacts/b3-before.json) and
[after](gn-11-artifacts/b3-after.json) records. The final rebuilt plugin reproduces
the after tree exactly.

## Local downstream costs (B4/B5 diagnostic)

One serialized before/after pair ran during the coordinator's quiet measurement
lease on Linux x86_64 with Rust 1.99.0 (`b940084d7`). Both consumers use the same
frozen runtime from the base revision, the same parse/serialize smoke source,
and identical dependency versions/checksums. Their Cargo locks differ only in
consumer package names. Separate empty target directories keep compiler caches
independent. Configuration: one Cargo job, default features, dev debug info off,
incremental off, offline. The build follows the two checks; it is a dev build
from checked artifacts, not a cold release build.

| Phase | Base output | Shared-owner output |
|---|---:|---:|
| Cold `cargo check` | 18.9211 s | 14.2628 s |
| Warm `cargo check` | 0.0282 s | 0.0236 s |
| Check-to-dev `cargo build` | 26.0905 s | 24.3810 s |
| Warm runtime smoke | 0.0283 s | 0.0249 s |

All eight commands exited zero. The smoke parses/serializes `OptB` and checks
`OptC`. These are one pair in fixed order, with no randomized replay or dedicated
host uncertainty calculation. The numbers support this local size/cost
comparison; they are not comparative leadership or production qualification.
Warm checks are cache diagnostics, and the sub-30 ms smoke rows are not codec
benchmarks.

The first pair is retained and excluded from those rows: the archived runtime
had no `third_party/protobuf` directory, while its build script watches that
path. The missing path forced runtime recompilation even on warm checks and
runtime smokes. The corrected pair creates a stable empty watched directory,
so both use the same committed vendored descriptor fallback; no runtime source
or runtime feature changed. The first pair nevertheless completed all eight
commands successfully. Its misleading warm timings must not be reused.

Committed records include the [corrected pair](gn-11-artifacts/cost-corrected.json),
[first setup pair](gn-11-artifacts/cost-first-invalid.json),
[raw compiler logs, manifests, locks and smoke sources](gn-11-artifacts/raw-measurements.tar.gz),
and the [exact local measurement driver](gn-11-artifacts/measurement-driver.py).
The driver retains this session's absolute scratch paths. The
[provenance manifest](gn-11-artifacts/provenance.json) records tool, source and
artifact hashes; the fixtures separately record compiler/source pin and digests.

## Validation status

The readable text logs remove trailing empty lines only; their exact original
bytes are committed in [raw-validation-logs.tar.gz](gn-11-artifacts/raw-validation-logs.tar.gz).

The first implementation run retained two failures: the facade initially
omitted proxy exports, and the test supplied an extra mutable borrow to
`CopyFrom`. Both were corrected; the subsequent five live-source contracts
passed, including compiled shared-owner and standalone consumers. The final
regressions use small pinned descriptor fixtures so ordinary CI needs neither
a downloaded source checkout nor a compiler upgrade. Their source commands
and digests are in the [fixture record](../../tests/fixtures/codegen-wkt-sharing/README.md).

The initial codegen-harness contract run had 59 passes, one skip, one failure
and one error out of 62 tests because this execution environment lacked
`/usr/bin/time`. The [prerequisite failures](gn-11-artifacts/harness-first-missing-time.log)
are retained. After the coordinator installed the official Debian GNU time
package, the unchanged checker passed 61 tests with one optional skip
([final log](gn-11-artifacts/harness-corrected.log),
[tool provenance](gn-11-artifacts/gnu-time-provenance.json)).

The required codegen suites passed: `onboarding` 15 with one compiler-specific
ignore, `pbrs_build` 32, `plugin` 46 with two compiler-specific ignores. The
pinned 35.1 onboarding parity test was then run explicitly and passed; the two
Edition 2024 source checks requiring 36.1 remain declared optional ignores.
The focused suite and documentation contracts were rerun after peer review:
six sharing/mapping cases and 25 documentation cases passed, including compiled
facade and exact-alias consumers. Logs are committed as
[required suites](gn-11-artifacts/focused-gates.log),
[post-review contracts](gn-11-artifacts/post-review-contracts.log),
[shipping contracts](gn-11-artifacts/shipping-contracts.log), and
[pinned onboarding](gn-11-artifacts/pinned-onboarding.log).

Peer review caught a source-name whitelist that rejected valid exact aliases
into existing facade types. Validation now checks resolved destination names.
The first strict Clippy run also rejected an ignored `remove_dir_all` result in
the test helper; the helper now checks that errors are only `NotFound`. The
[initial lint failure](gn-11-artifacts/clippy-first-unused-result.log) is retained.
Final scoped Clippy with `-D warnings`, rustdoc with `-D warnings`, formatting,
and diff whitespace checks passed. See the
[Clippy](gn-11-artifacts/clippy-corrected.log),
[rustdoc](gn-11-artifacts/rustdoc-final.log), and
[format](gn-11-artifacts/format-post-review.log) records.

After the validation correction, raw plugin output for the exact B3 corpus and
single-file comparison remained byte-identical. The
[post-review generated tree](gn-11-artifacts/post-review-generated-output.tar.gz)
and provenance manifest retain the final source/plugin hashes separately from
the original measured implementation. The downstream pair uses unchanged Rust
output and the frozen base runtime; it was not retimed after a mapping-only
validation change.

`CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=target/base bash scripts/regen-generated.sh
--check` passed for all 13 registered bundled bindings with the exact pinned
compiler and source. All generated-file hashes, Git diff and Git status matched
before/after, recorded in the [check log](gn-11-artifacts/regeneration-check.log)
and [nonmutation snapshot](gn-11-artifacts/regeneration-snapshot.json). No bundled
binding refresh was necessary and no generated source was hand-edited.

An extra `--no-default-features --test documentation` probe failed because Cargo
also builds the existing unconditional `protoc-gen-pbrs` binary, which references
the disabled `codegen` module. Both that binary and its Cargo declaration are
unchanged from the base; this is outside the required documentation gate.
The [optional failed probe](gn-11-artifacts/optional-minimal-docs-failure.log)
is retained, while the supported minimal library check passed
([log](gn-11-artifacts/minimal-library.log)). No broader feature/binary change is
included here.

Commands use `--locked --target-dir target/base -p pbrs`, one Cargo build job:

```bash
cargo test --test codegen_wkt_sharing --test documentation \
  --test plugin --test pbrs_build --test onboarding
cargo clippy --lib --bin protoc-gen-pbrs --test codegen_wkt_sharing \
  --test documentation -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps
cargo test --test onboarding \
  adapter_descriptor_sets_match_pinned_protoc_and_generated_output \
  -- --ignored --exact
cargo check --no-default-features --lib
cargo fmt --all --check
```

The coordinator owns integrated conformance after merging this lane; it is not
claimed here before that run.
