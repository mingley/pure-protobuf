# Target-file match cache screen sources

[Benchmark record](../../benchmarks/codegen-target-cache-20261110.md)

The scratch candidate was based on
`5fe70efdc798031badefd1a3847d8866dde352f5`. Apply `candidate.patch`
to that source to reproduce
`src/codegen/descriptors.rs` Git blob
`8c2ead035dbac53426fc67f5a465f7c76eda6069`. It was not merged.

- `probe.rs`, `corpus.py`, `compare.py` and `witness.py` are the
  exact probe, corpus extension, balanced controller and counter/mutant
  controller used in the screen. The original requests come from the
  [file-memo corpus](../codegen-file-memo-20261110/); the screen also checks
  the original artifact hash before extending them.
- `base-trace.rs` and `candidate-trace.rs` add an atomic call counter
  for diagnostic runs only. `mutant.rs` disables cache invalidation to
  check the interleaved fixture. None supplied timed measurements.
- `Cargo.lock` is the original probe lockfile. `workflow.yml` records
  the exact build and run commands; it is a historical scratch workflow,
  not active CI. Adapt the scratch paths and ref cleanup to reproduce it.
- `verify-archive.py` rechecks the retained screen inputs, output bytes,
  manifests, counters, mutant, CPU blocks and allocations. The smaller
  verifier beside the benchmark note checks the extracted data.

The raw artifact retains all request bytes, timings and manifests, plus actual
output files for selected cases and the counter/mutant checks. Its expiration
and digest are in the benchmark record.
