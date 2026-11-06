# Full hosted benchmark record, 2026-11-06

This packet retains the complete artifact from
[GitHub Actions run 42163543503](https://github.com/mingley/pure-protobuf/actions/runs/42163543503).
It compares [`b6eb8c7b`](https://github.com/mingley/pure-protobuf/commit/b6eb8c7b3c137e5173538baba14a192ab6b4dc65)
with [`05a3cca5`](https://github.com/mingley/pure-protobuf/commit/05a3cca50e7708f5be1facce6d27a7ed6b87006b).
Later RPC, balancer and JSON changes are outside this record.

All 1,634 cells appear on both sides. Allocations, allocated bytes, syscalls,
futex calls and wall time have finite measured values for every cell.
Instructions were **NOT_RUN** because the runner had neither perf nor
Valgrind. Base and head report matching host metadata.

The three canonical fresh encoders use `independent-warmup-v1` on both sides:
warmup objects are separate from measured fresh objects. Validation reports
`qualified_for_noise=true`, with no errors. This qualifies the measurement
inputs and available metrics for comparison. It does not establish a
performance improvement, victory in every benchmark, or a world ranking.

## Selected head rows

These rows compare pbrs with Prost at **05a3cca5**, rather than with the base
revision. Lower is better. Wall time is secondary evidence from a shared
hosted runner; the full record includes both gains and losses.

TAT encoding and owned decode use Prost 0.13.5. The TAT encoders call
`Message::encode` into a new growable `Vec` for each operation. Name decode
and mixed-blob encoding use Prost 0.14.4; mixed-blob calls `encode_to_vec`
and constructs payloads inside the measured operation. The comparisons apply
to those APIs and versions.

<!-- selected-head-table:start -->
| Operation | pbrs ns/op | Prost ns/op | pbrs allocs/op | Prost allocs/op |
|---|---:|---:|---:|---:|
| Fresh encode | 584.30 | 526.55 | 7 | 5 |
| Cached encode | 105.97 | 372.13 | 1 | 5 |
| Owned decode | 378.64 | 677.42 | 7 | 10 |
| 80-byte name decode | 53.80 | 40.40 | 1 | 1 |
| Mixed blob encode | 1955.60 | 1383.04 | 19 | 10 |
<!-- selected-head-table:end -->

## Inspect or verify

- [official-artifact.zip](official-artifact.zip) contains all eight original
  members: both JSON records and logs, `validation.json`, `summary.md`,
  `compare.txt`, and the base RPC fixture overlay report. The archive bytes
  are unchanged from official artifact **13480729136**.
- [manifest.json](manifest.json) records the official ZIP digest, member
  digests, source pair, coverage and exact selected values.
- [check.py](check.py) reads these files with the Python standard library.
  It checks the hashes, all cell coverage, metrics, lifecycle metadata and
  selected table, including the losing rows. It performs no network access,
  remeasurement, source reconstruction or execution of archive contents.

```sh
python3 -B docs/evidence/hosted-devloop-20261106/check.py
python3 -B docs/evidence/hosted-devloop-20261106/check.py --negative-controls
```

The negative controls alter data only in memory. They check rejection of
missing or nonfinite metrics, incompatible fresh profiles, wrong sources,
missing coverage, fabricated instructions, changed official identity and
edited or omitted losing rows.
