# CG-19 codegen cost diagnostic

From the repository root, run `./scripts/codegen-bench.sh --case small` for a
smoke cell, or omit `--case` for the seeded 6-, 100-, and 1,000-message
multi-file matrix. This is an **unqualified** local diagnostic, not a
compile-time leadership claim. Results, corpus inputs, an explicit
`summary.json` and raw per-phase logs go in a new `target/codegen-bench/` run
directory. No existing evidence is deleted. See
[`docs/benchmarks.md`](../../docs/benchmarks.md#codegen-and-downstream-compilation-cg-19-diagnostic)
for measured phases, RSS limitations and reference qualification boundaries.

An **explicit opt-in** pairs one default-seed corpus with the genuine pinned
upstream Rust generator. Specify exactly one of `--case small`, `--case 100`,
or `--case 1000`; `--case all` and an omitted case are rejected in reference
mode to avoid unexpectedly running three cold comparison builds:

```sh
CARGO_BUILD_JOBS=2 ./scripts/codegen-bench.sh --case small \
  --reference-protoc "$PWD/target/pinned-protoc-build/protoc" --jobs 2
```

The harness requires `libprotoc 35.1` with SHA-256
`e2b116ef44d4b7f3246945ceb1938c72f04e16040020e321ac601869135ab940`,
the checked and clean upstream source at `vendor/google/SHA`
(`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`), and an offline Cargo
lockfile pinning the **independent** `protobuf` and `protobuf-macros`
`4.35.1-release` registry crates by checksum. There is no `pbrs` dependency
or ABI shim in the reference consumer. Its `kernel=upb` runtime builds C
code, so the report records the selected C compiler as well as Cargo, Rust,
protoc, flags, lockfile hashes and source state. Missing/mismatched pins fail
closed. Opt-in needs Python 3.11+ for standard-library lockfile verification
and requires one explicit case with `--seed 190019 --jobs 2`. Running
`--case 100` or `--case 1000` triggers a separate cold pbrs and C/upb consumer
build, which is substantially heavier than generation alone. No timed
100/1,000-message reference consumer run has been performed or qualified.
For a bounded **generation-only** check against an already built pinned
compiler, use
`CG19_PINNED_PROTOC="$PWD/target/pinned-protoc-build/protoc" python3 -B -m unittest discover -s bench/codegen -p 'test_*.py'`.
This verifies all 5/20 generated Rust modules and their pinned byte hashes,
but does not compile them or measure runtime performance.

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
