# SB-23: trustworthy performance CI

The repaired workflow measures exact base/head revisions and preserves failures.
Performance thresholds remain advisory. A green upload is not a benchmark result;
only validated, comparable measurements can enter SB-20's noise study.
SB-23 is complete: a corrected-parser CI pair and a rejected failed-build pair
are retained below. SB-20's calibration remains open.

## Delivered

- Relevant pushes to `main` and explicit full-SHA dispatches run the comparison.
- Base and head build from isolated source trees and locked dependencies on the
  same runner. Both receive the verified pinned protobuf schemas and compiler.
- Reports retain source, host, compiler, tool availability, sample counts, units,
  measurements and explicit reasons for unavailable metrics.
- The evidence validator rejects empty/error reports, wrong source revisions,
  incompatible hosts/units/methods and malformed measurements. Missing optional
  counters are excluded rather than treated as zero.
- Build failures retain error JSON and logs. A local regression comparison still
  exits nonzero without overwriting its measured report. CI thresholds are advisory;
  broken measurement infrastructure fails the job.
- Counter parsers now read the actual `perf stat -x,` and `strace -c` formats.
  Partial repeats stay `not_run`. The legacy `locks` field is correctly labeled
  futex-family syscalls, including wakes, not blocking waits. This unit change
  deliberately prevents mixing old and new series.

## Real failed-build proof

[Run 36614240614](https://github.com/mingley/pure-protobuf/actions/runs/36614240614)
compared `deea5e3f91f9271dd7c2926375369c1e367d1287` with
`9fe6478a33941104d72c640890d409eff7f60330` on Linux x86_64
(AMD EPYC 7763, rustc 1.98.1). The base failed because a Git worktree does not
inherit the ignored `third_party/protobuf` checkout needed by `v4_tat`.
Commit `7b716301` fixed that by sharing only verified, identical pinned schemas.

The head produced 88 measured cells, but the base had none. The workflow retained
[both reports and logs](perf-ci/36614240614/) and correctly marked
[`qualified_for_noise: false`](perf-ci/36614240614/validation.json).
This run contributes **zero** qualifying comparisons. Its old instruction/syscall
parsers also missed available output; the later parser repair is `9a04f82b`.

The retained files are byte-identical to the downloaded `devloop-36614240614`
artifact. SHA-256 of `base.json` is
`d27a1ba955bc53b519b833a2f390ef000bcb9feb490b2f79999121f90481fb28`;
`head.json` is
`3c5aa1febc241a02e8336860054a5fecab7a72b411916c5941141dc57be4bc80`;
`validation.json` is
`2a6e361f3086f5fd58aaa77798ae2ea6be9f940231f8096007fc449b8e8039f7`.

## First successful pair

[Run 36615248270](https://github.com/mingley/pure-protobuf/actions/runs/36615248270)
compared `9fe6478a33941104d72c640890d409eff7f60330` with
`c89608bd2073f2fa7e7d30d7299b1fd08466904c` on the same Linux x86_64 runner.
Both reports contain 88 cells, with matching host/compiler metadata and three
repeats per cell. The [retained artifact](perf-ci/36615248270/) includes the
logs, comparison and validator result: no evidence errors, 88 common cells.

This demonstrates the workflow and pinned-schema repair executing a real pair.
Both revisions still used the older counter parsers: instructions and syscalls
are `not_run` in every cell, and the old `locks` label is misleading. Exclude
those metrics. Allocation counts/bytes and wall time remain available, but this
pair does not qualify the corrected counter series. A new run compares
`9a04f82b` with `d9c34c06` using the repaired parsers.

## Corrected-parser comparison

[Run 36617638958](https://github.com/mingley/pure-protobuf/actions/runs/36617638958)
successfully compared `9a04f82b87d932954437cdb1446d673e7aee3899` with
`d9c34c067d9b8115651904fd5778298e6cd5fdfe` on Linux x86_64, Intel Xeon
Platinum 8370C, rustc 1.98.1. Both [retained reports](perf-ci/36617638958/)
contain 88 cells with three repeats each. The evidence validator found no errors,
88 common cells, and `qualified_for_noise: true`. Revalidating the downloaded
reports reproduced `validation.json` byte for byte.

All 88 cells on both revisions measured allocations, allocated bytes, syscalls,
futex-family syscalls and wall time. Instruction output was missing or invalid
in one or more repeats for every cell, despite `perf` being present. Instructions
remain `not_run` and are absent from all eligible metric sets; executable
availability alone does not establish an instruction measurement. Syscalls now
parse correctly, and futex calls use the corrected unit. No zero values were
substituted for missing counters.

The advisory comparison reported syscall/futex variation and picker budget
exceedances; its exit status is retained in the summary. Those old zero-lock
budgets cannot be interpreted as zero futex-family calls, which include process
setup and wakes. This is one usable pair for the available metric series, not a
speed win or an instruction-noise sample. Do not combine it with the old parser
series or a different CPU cohort when calibrating thresholds.

## Latest source checkpoint

[Run 36622048330](https://github.com/mingley/pure-protobuf/actions/runs/36622048330)
successfully compared `d9c34c067d9b8115651904fd5778298e6cd5fdfe` with
`3a7aa12857e8d9f7b3569ec3847ccd9b8963ef1e` on Linux x86_64, AMD EPYC 7763,
rustc 1.98.1. Both [retained reports](perf-ci/36622048330/) have 88 cells and
three repeats each. Independent revalidation reproduced `validation.json`
byte for byte: no errors, 88 common cells, `qualified_for_noise: true`.

Allocations, allocated bytes, syscalls, futex-family syscalls and wall time are
eligible in every cell. Instructions remain `not_run` in every cell on both
revisions and are excluded. The advisory comparison exits one; this is valid
measurement infrastructure, not a speed-win verdict.

The retained files match the downloaded artifact. SHA-256 of `base.json` is
`ecd2ed13869296ba440b30942707b6108b4a05d4bb3ac678ba3c431628f0513f`;
`head.json` is
`74c1915307aa54aeace12091bd17d37cd956954cb6ed4416379f242d70f34914`;
`validation.json` is
`f7a66f6158e5f8e0512ac5e202376579eb1a3164c82bd136c8ea5a3fa96f1797`.

There are now two verified corrected-parser pairs, but one is Intel and one
AMD. They are separate host cohorts, with one retained pair in each; neither
has the 30 comparable measurements needed for threshold calibration.

## Standalone lockfile recovery — 2026-09-30

[Run 36744553856](https://github.com/mingley/pure-protobuf/actions/runs/36744553856)
failed both builds because the standalone dev-loop lockfile still referenced
the removed native dependency on `protobuf-tonic`. Its
[retained validation](perf-ci/36744553856/validation.json) rejects both empty
reports with `qualified_for_noise: false`. The RPC benchmark, tonic benchmark
and tonic peer also had stale path dependency edges. SB-30 refreshes those
four lockfiles and checks their locked resolution in ordinary CI.

[Run 36746154592](https://github.com/mingley/pure-protobuf/actions/runs/36746154592)
then built and measured exact source
`59ea5ef7f7c9e62e1436346bb70525f09b0302aa` twice, in isolated worktrees with
the pinned compiler. Both [retained reports](perf-ci/36746154592/) contain
98 cells on Linux x86_64, AMD EPYC 9V74, rustc 1.98.1. Independent revalidation
reproduces the downloaded validator result byte for byte: no errors,
98 common cells, `qualified_for_noise: true`.

This identical-source pair verifies pipeline recovery; it does not measure a
code improvement. Allocations, allocated bytes, syscalls, futex-family calls
and wall time are eligible. Instructions remain `not_run` and excluded.
The 98-cell matrix and new CPU are a separate cohort from the older 88-cell
runs. Neither this pair nor a green workflow establishes calibrated thresholds.

## Qualification still required

SB-20 needs at least 30 comparable successful measurements, grouped by available
metrics and compatible methodology. Hosted-runner wall time is a noisy diagnostic,
not dedicated-host gRPC leadership evidence. Nor can unmatched dev-loop RPC
workloads establish a win over another stack; see the
[harness limitations](../../bench/devloop/README.md#cell-families).

Local validation passed five counter-parser tests and 12 report/wrapper tests,
including failure and malformed-artifact cases. Full workspace Clippy with
warnings denied also passed after the guide and TLS-benchmark changes.
