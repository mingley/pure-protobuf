# Dynamic parse table memo

A dynamic parse needs at least one descriptor table. It used to allocate a
vector to remember the first table, even if the message never resolved a
second descriptor. The parse context now stores that entry inline and creates
the vector when another descriptor is needed. Recursive values can reuse the
first table. Descriptor identity, lookup order, and table ownership are the same.
[Change](https://github.com/mingley/pure-protobuf/commit/72722c85f80579337479009c27ce72c888887891)

| Valid parse and drop | Allocation events | Requested bytes | CPU ratio, candidate/baseline | Baseline duplicate-run range |
| --- | ---: | ---: | ---: | ---: |
| Empty, warmed pool | 1 → 0 | 64 → 0 | 0.825 (0.757–0.861) | 0.816–1.084 |
| Scalar fields, warmed pool | 2 → 1 | 736 → 672 | 0.970 (0.871–1.162) | 0.610–1.281 |
| Scalar fields, no pool | 9 → 8 | 1428 → 1364 | 0.963 (0.948–0.977) | 0.991–1.056 |
| Eight nodes of one recursive type | 10 → 9 | 5568 → 5504 | 0.978 (0.970–0.990) | 0.985–1.014 |
| Eight children of one distinct type | 14 → 14 | 6968 → 6968 | 0.998 (0.963–1.012) | 0.940–1.023 |
| Eight distinct child types | 14 → 13 | 6880 → 6624 | 1.004 (0.984–1.021) | 0.960–1.021 |

The empty message with a warmed pool saves its only allocation. The scalar and
same-type recursive cases each save one allocation. The repeated-child case
still needs two tables. The wide case saves one vector growth event.

These are component measurements on a shared x86-64 Linux runner pinned to one
CPU. The Rust 1.85 release binaries use `reflect` with default features off.
Each row has six alternating ABBA/BAAB blocks with separate baseline and
candidate processes. Ratios use process CPU. The report also retains timed-loop
elapsed time. Baseline duplicate runs show substantial variation for the small
pooled scalar case. The repeated-child and wide timings are within the control
variation. Separate counting binaries measure allocations and reallocations
using three independent 64/128-iteration pairs. Requested bytes are allocation
requests over the operation. Each process verifies round-trip wire equality
outside its timed loop.
[Raw samples](results.json) · [Run](https://github.com/mingley/pure-protobuf/actions/runs/44347575883) ·
[Artifact](https://github.com/mingley/pure-protobuf/actions/runs/44347575883/artifacts/14446717122)

The candidate passed 26 table tests, 36 dynamic tests, 16 required-merge tests,
strict stable Clippy and a Rust 1.85 reflection-only build. The initial
measurement job stopped because the standalone counter needed the repository's
benchmark allocator lint allowance. The corrected job reused the candidate
and made the same allowance on both sides.
[Correctness run](https://github.com/mingley/pure-protobuf/actions/runs/44347112961)

The archived workflow builds the two source revisions with the same probe,
schema and driver. To reproduce locally, use the same compiler, create separate
worktrees at the two source revisions, and follow the workflow's build and
measurement commands with a temporary output directory.
[Workflow](screen.yml) · [Probe](probe.rs) · [Schema](fixture.proto) ·
[Driver](driver.py) · [Source pins](source-pins.json) · [Source patch](candidate.patch)
