# CG-19 codegen cost diagnostic

From the repository root, run `./scripts/codegen-bench.sh --case small` for a
smoke cell, or omit `--case` for the seeded 6-, 100-, and 1,000-message
multi-file matrix. This is an **unqualified** local diagnostic, not a
compile-time leadership claim. Results, corpus inputs, an explicit
`summary.json` and raw per-phase logs go in a new `target/codegen-bench/` run
directory. No existing evidence is deleted. See
[`docs/benchmarks.md`](../../docs/benchmarks.md#codegen-and-downstream-compilation-cg-19-diagnostic)
for measured phases, RSS limitations and missing reference qualification.

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
reference peer remains `reference.status: missing`,
`qualification.qualified: false` and `comparison.losing_cells: null`.

Run the no-compiler tests with
`python3 -B -m unittest discover -s bench/codegen -p 'test_*.py'`.
