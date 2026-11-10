# Lean codegen instruction profile, November 10

[Benchmarks](../benchmarks.md)

A single Callgrind trace of pbrs code generation on source
[`5fe70efd`](https://github.com/mingley/pure-protobuf/commit/5fe70efdc798031badefd1a3847d8866dde352f5)
counted 666,229,445 user-space guest instructions. It is an attribution
diagnostic for one workload, not a native CPU measurement or a comparison
with Prost.

| Direct call edge | Inclusive guest instructions | Share of trace | Calls |
|---|---:|---:|---:|
| Generation closure → `emit_message` | 303,495,994 | 45.55% | 3,000 |
| `emit_message` → `emit_codec` | 175,051,493 | 26.27% | 3,000 |
| Generation closure → `FileMatcher::matches` | 48,423,578 | 7.27% | 60,000 |

The trace includes process setup, two warmup generations and one measured
generation. Call counts cover all three generations. The edges are inclusive:
`emit_codec` is inside `emit_message`, so the rows overlap and must not be
added. Debug information and inlining can also produce duplicate symbol rows;
the table uses the named direct caller edges. The retained source-line report
did not yield line-level attribution.

## Workload and controls

The prepared, in-memory lean request has 1,000 messages in 20 proto files.
Lean disables reflection, JSON and text generation. The checks compared all
20 proto inputs and the descriptor set byte for byte with the earlier retained
corpus. The base and debug-line builds each generated 21 files; all 42 files
matched the corresponding retained output bytes and names, with 12,925,901
bytes per build.

The [profile run](https://github.com/mingley/pure-protobuf/actions/runs/42783012988)
used one pinned guest CPU on a shared AMD EPYC 9V74, Rust 1.99.1, thin LTO,
one codegen unit and Valgrind 3.22.0. The debug-line build and a base build
were separately compared in five alternating native CPU blocks, with four
measured generations per sample after warmup. Debug/base CPU ratios had
median 0.9825 and range 0.8851–1.1154; same-binary control ranges were
0.9652–1.0612 for base and 0.9727–1.0440 for debug. These short samples and
overlapping controls do not establish a native speed difference.

The traced process took 4.399 seconds of wall time. Three native debug-line
processes took 86.6–89.6 ms, making the traced run 49.5 times the native
median. Callgrind instrumentation changes execution substantially; its
instruction shares are not CPU-time shares.

## Data and verification

The [compact data](codegen-instruction-profile-20261110.json) keeps the raw
native blocks and controls, output manifests, trace total and selected caller
edges. The [independent verification](https://github.com/mingley/pure-protobuf/actions/runs/42783928347)
checked the original input archives, output bytes, binary hashes, native timing
arithmetic and trace total. Its
[verified bundle](https://github.com/mingley/pure-protobuf/actions/runs/42783928347/artifacts/13747138008)
expires February 8, 2027; the bundle ZIP SHA-256 is
`7c658aa30930c29b887a0d225275d6dc9579965663b535f03f4ff7737bbb0e3c`.
It contains the original profile archive and raw trace.

From the repository root, the following checks the contained original archive,
reruns the source-pinned verifier, regenerates the caller tree from the trace,
and compares it with the compact data. It requires `gh`, Python 3 and
`callgrind_annotate`.

```sh
(
  set -eu
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  gh run download 42783928347 --repo mingley/pure-protobuf \
    --name verified-lean-codegen-instruction-profile-5fe --dir "$temp_dir"
  python3 docs/benchmarks/codegen-instruction-profile-20261110-verify.py \
    "$temp_dir" --check docs/benchmarks/codegen-instruction-profile-20261110.json
)
```
