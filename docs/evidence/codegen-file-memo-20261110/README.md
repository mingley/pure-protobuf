# File-memo codegen screen sources

[Benchmark record](../../benchmarks/codegen-file-memo-20261110.md)

These are the exact scratch sources used in the November 10 screens:

- `probe.rs` measures repeated in-memory requests and, in a separate build,
  counts allocation requests.
- `corpus.py` builds the 13 requests using `protoc` and the baseline's
  `bench/codegen/run.py`. `compare.py` runs the initial three-version screen;
  `repeat.py` runs the longer two-version screen.
- `borrowed.patch` and `contains.patch` apply independently to baseline
  `5fe70efdc798031badefd1a3847d8866dde352f5`. The resulting
  `src/codegen/descriptors.rs` Git blob IDs are
  `398dc509f8cae124ac8fc395893438078a3aa8a4` and
  `8bf430983aeb4a93b6febcbc7ef0e0f3ef54c12b`, respectively.
- `Cargo.lock` is the probe lockfile recovered from the original artifact.
- `initial-workflow.yml` and `repeat-workflow.yml` record the exact build,
  request, pinning and comparison commands. They are historical scratch
  workflows, not active CI. They assume the original scratch paths and refs;
  adapt those paths and ref-cleanup steps when reproducing.
- `initial-verify.py` and `repeat-verify.py` checked the downloaded
  artifacts as described in their respective verification runs. The portable
  verifier alongside the benchmark note checks the smaller extracted data.

The raw artifacts include the probe lockfile, request bytes, build logs and
output manifests. They are retained until the dates in the benchmark note;
the individual generated output files were compared during collection and
then removed.
