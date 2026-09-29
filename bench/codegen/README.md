# CG-19 codegen cost diagnostic

This harness measures generator time, generated output size, downstream
`cargo check`, release build, binary size, and resident set size (RSS) for
generated Rust consumers. From the repository root, run
`./scripts/codegen-bench.sh --case small` for a smoke cell, or omit `--case`
for the seeded 6-, 100-, and 1,000-message multi-file matrix. Bottom line:
these runs are **unqualified** local diagnostics, not compile-time leadership
claims.

Each run writes corpus inputs, `summary.json`, and raw per-phase logs to a new
`target/codegen-bench/` directory. No existing evidence is deleted. See
[`docs/benchmarks.md`](../../docs/benchmarks.md#codegen-and-downstream-compilation-cg-19-diagnostic)
for measured phases, RSS limits, and reference qualification boundaries.

## Reference opt-in

An **explicit opt-in** pairs one default-seed corpus with the genuine pinned
upstream Rust generator. Specify exactly one of `--case small`, `--case 100`,
or `--case 1000`; `--case all` and an omitted case are rejected in reference
mode to avoid unexpectedly running three cold comparison builds:

```sh
CARGO_BUILD_JOBS=2 ./scripts/codegen-bench.sh --case small \
  --reference-protoc "$PWD/target/pinned-protoc-build/protoc" --jobs 2
```

The reference path is deliberately strict:

- `libprotoc 35.1` must have SHA-256
  `e2b116ef44d4b7f3246945ceb1938c72f04e16040020e321ac601869135ab940`.
- The upstream source must match `vendor/google/SHA`
  (`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`) and be checked and clean.
- The offline Cargo lockfile must pin the independent `protobuf` and
  `protobuf-macros` `4.35.1-release` registry crates by checksum.
- The reference consumer has no `pbrs` dependency and no ABI shim.

The reference `kernel=upb` runtime builds C code, so the report records the
selected C compiler as well as Cargo, Rust, protoc, flags, lockfile hashes and
source state. Missing or mismatched pins fail closed. Opt-in needs Python 3.11+
for standard-library lockfile verification and requires one explicit case with
`--seed 190019 --jobs 2`. Running
`--case 100` or `--case 1000` triggers a separate cold pbrs and C/upb consumer
build, which is substantially heavier than generation alone. No timed
100/1,000-message reference consumer run has been performed or qualified.
For a bounded **generation-only** check against an already built pinned
compiler, use
`CG19_PINNED_PROTOC="$PWD/target/pinned-protoc-build/protoc" python3 -B -m unittest discover -s bench/codegen -p 'test_*.py'`.
This verifies all 5/20 generated Rust modules and their pinned byte hashes,
but does not compile them or measure runtime performance.

## What is and is not timed

Bootstrap is **excluded** from generation and consumer check timings. Its
driver builds offline against the compatible shared
`target/integration-consumers` Cargo target used by nested consumer tests,
with `CARGO_BUILD_JOBS` capped at `min(--jobs, 2)`; it does not force a
different `CARGO_INCREMENTAL` setting. The built driver is copied into
`<run>/bin/cg19-generator`, checked against the shared binary's SHA-256
before and after copying, then hashed and invoked only from that per-run
path. Replacing the shared executable during a concurrent run cannot
replace the copied one.

In contrast, **each corpus** uses a distinct, initially nonexistent
`<run>/cases/<case>/target` for its cold `cargo check`, followed by an
incremental check and release build in that same target. Bootstrap cache
warmth is never reported as a clean consumer check. The report records both
target paths, the bootstrap job cap, inherited compiler/cache settings, and
the hashes of the wrapper, driver, Rust source and lockfiles. A missing
reference peer in the **default** pbrs-only mode remains
`reference.status: missing`, `qualification.qualified: false` and
`comparison.losing_cells: null`.

## What reference mode compares

In opt-in mode, pbrs generation uses the **same pinned protoc** to compile
descriptors; upstream uses its built-in `--rust_out` with
`experimental-codegen=enabled,kernel=upb`. Both use default reflection
metadata, the same input files and equivalent `Parse`/`Serialize` work
across every generated message in the selected corpus, but
their generated Rust module layouts differ (`mod.rs` versus `generated.rs`).
Both generated file sets and byte hashes are recorded. Pbrs must preserve all
`.rs` bytes and mtimes on repeat generation; upstream must preserve the
bytes, and its mtime rewrites are reported rather than hidden. Each consumer
gets a **separate empty target** and identical Cargo profile, while the
local registry, wrapper caches and fixed pbrs-first order may be warm. Cold
checks include each runtime's real transitive compilation, including C/upb
for the reference; they are not generator-only or Rust-only comparisons.
Only default message values run; nonempty-field behavior is not qualified here.
Bootstrapping/lockfile resolution is excluded from both measured generation
and consumer builds. Cargo invocations are serial and limited to two jobs.
After each release build and binary hash, `release_smoke` executes that binary
once from its own consumer directory with a maximum 15-second timeout (or the
smaller `--timeout-seconds`). It fails closed unless exit status is zero,
**raw** stdout is exactly `1\n`, and raw stderr is empty. Each phase retains
its command, cwd, timeout, exit status, output hashes and
`release-smoke.{stdout,stderr}.log` on success. Failures retain status and
log paths (plus exit code if the child exited) and stop the run.
The smoke does not use `/usr/bin/time` (which would contaminate program stderr)
and adds **no timing/RSS values** to the 12 comparison metrics.
`comparison.metrics` records raw paired values for all 12 measured metrics
and `comparison.losing_cells` contains every metric where pbrs is larger.
Reference errors leave losses `null` unless a paired cell actually completed.
Even a complete local pair has `qualification.qualified: false`;
`--require-qualified` fails before any compiler work.

The corrected six-message pair is retained at
`target/codegen-bench/20260924T211609Z-63179/summary.json`, with **40 raw
stdout/stderr logs**, including `logs/small/release-smoke.*.log` and
`logs/small/reference/release-smoke.*.log`. Both smoke phases passed; all eight
observed pbrs-losing metrics (the prior seven plus generation RSS) are
reported without changing the unqualified status.

Run the no-compiler tests with
`python3 -B -m unittest discover -s bench/codegen -p 'test_*.py'`.

# SB-09 codegen comparator matrix

SB-09 extends this harness with peer generators on the seeded corpora
(`small`, `100`, `1000`), service-stub corpora (`svc-small`, `svc-100`),
and realistic corpora (`otlp`, `googleapis`, `envoy-core`,
`envoy-discovery`):

```sh
# One realistic case, all message generators, one repeat (smoke).
./scripts/codegen-bench.sh --case googleapis \
  --generators pbrs,prost,buffa,v4 --repeats 1
# B1/B2 only: validate generated bytes and unchanged generation, but skip
# downstream cargo check/build phases.
./scripts/codegen-bench.sh --case all \
  --generators pbrs,prost,v4 --repeats 5 --generation-only
# Service-stub comparison on the seeded service corpus.
./scripts/codegen-bench.sh --case svc-small \
  --stub-generators pbrs-native,pbrs-tonic,tonic-build --repeats 1
```

Message generators (`--generators`, default `pbrs`): `pbrs` keeps the
exact CG-19 flow; `prost` (prost-build 0.14.4), `buffa` (buffa-build
0.9.1), and `v4` (pinned protoc 35.1 `--rust_out`, kernel=upb) run the
same phases on the same corpora under `cases/<case>/gen/<name>/`. Each
consumer does equivalent work — construct, serialize, parse, serialize
— for every message, with per-generator module wiring: pbrs/buffa
include `mod.rs`, prost gets a rendered package tree (prost emits one
file per package and extern-maps WKT to `prost-types`), v4 includes its
single `generated.rs` entrypoint. Every consumer binary must exit 0 in
`release_smoke`.

Stub generators (`--stub-generators`, default `pbrs-native`) run only on
the `svc-*` corpora (one unary + streaming service per file) and are
compared separately: `pbrs-native`, `pbrs-tonic`, `tonic-build`. Stub
consumers roundtrip every message and construct each service's server
plus a lazy client.

`--repeats N` (default 5) executes every cell N times in seeded-random
order (`--seed`, default 190019); `summary.json#matrix` reports median,
min/max, and standard deviation per metric plus the pbrs-vs-peer loss
list. Case/generator exclusions (with reasons) are recorded under
`summary.json#excluded` instead of failing.

`--generation-only` records `generation`, `generation_unchanged`, generated
Rust bytes, output hashes, and the usual matrix medians for the metrics that
exist. Use it for generator B1/B2 evidence; do not use it for B3-B8
compile/build claims.

Realistic corpora fetch 27 hash-pinned `.proto` files (OTLP v1.7.0,
googleapis, Envoy v1.39.1, udpa, xds, protoc-gen-validate v1.3.3) from
`raw.githubusercontent.com` into `target/codegen-bench/vendor/` on first
use; cached files are re-verified by SHA-256 on every run and any
mismatch aborts the run. WKT support files come from the repo's pinned
`third_party/protobuf` checkout, never the network. Every generator
compiles the full file closure as inputs (pbrs/buffa/v4 need support
messages for cross-file paths; prost compiles the closure on its own),
while consumers roundtrip the subset's top-level messages only — nested
types are out of scope. Package and message lists are baked into
`run.py` and verified against the fetched bytes; generated-output sets
are validated per generator (exact for pbrs/prost/v4, core-plus-aux for
buffa). `envoy-discovery` excludes `v4`: its flat namespace cannot hold
the `PackageVersionStatus` enum defined by both `udpa/annotations` and
`xds/annotations/v3` (E0659). Only default message values run; all
results stay `qualification.qualified: false`.

The `v4` matrix generator uses `target/pinned-protoc-build/protoc`, built by
`scripts/build-pinned-protoc.sh`. The harness verifies `libprotoc 35.1`,
`--rust_out` support, and the pinned protobuf source revision, then records the
local binary SHA-256 in `summary.json#setup.v4_protoc_sha256`. The exact binary
hash can vary across local CMake/linker environments; the stricter fixed binary
hash check still applies to the legacy `--reference-protoc` opt-in flow above.
