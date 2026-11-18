# Dev-loop codec preparation and allocation losses

The Rust 1.100.0 main-branch run at `7b34d68c` completed 1,634 matched cells.
It retained allocation and wall-time diagnostics without hardware instruction
counts. The advisory comparison flagged regressions.
[Run and artifact](https://github.com/mingley/pure-protobuf/actions/runs/44360934662/artifacts/14463202293)

The historical allocation tables below use the complete recovery baseline at
`cc502ed2`. The next two attempts on that earlier harness produced invalid
instruction pairs and zero measured cells. [Recovery baseline](https://github.com/mingley/pure-protobuf/actions/runs/44036005116)

## Where to look first

The November 17 run compared base `2f27343e23770571c3ab40a15038c6dcb2ed5217`
with head `cc502ed27ac4970fa2c8fbc08d846c6904da87ce`. The tables compare
implementations within that head using Prost 0.14.4 and tonic 0.14.6.
Matching iterations, repeats, units and prepared-input fingerprints were
verified on all pairs. RPC pairs also match prepared input lengths. Each
operation covers 64 specimens. [Source lockfile](https://github.com/mingley/pure-protobuf/blob/cc502ed27ac4970fa2c8fbc08d846c6904da87ce/bench/devloop/Cargo.lock)

| Operation | Metric | pbrs higher / equal / lower | Largest excess, pbrs vs Prost |
|---|---|---:|---|
| Fresh construction and encode | Requests | 64 / 0 / 0 | EntityList 1,000 (38,002 vs 27,335) |
| Fresh construction and encode | Requested bytes | 64 / 0 / 0 | EntityList 1,000 (3,064,494 vs 2,096,502) |
| Owned decode | Requests | 57 / 1 / 6 | Query depth 8 variants 0 and 5 (67 vs 36) |
| Owned decode | Requested bytes | 62 / 0 / 2 | EntityList 1,000 (2,278,848 vs 2,032,926) |
| Decode and read every field | Requests | 57 / 1 / 6 | Query depth 8 variants 0 and 5 (77 vs 36) |
| Decode and read every field | Requested bytes | 63 / 0 / 1 | EntityList 1,000 (2,918,848 vs 2,791,410) |
| Clone a fully read message | Requests | 58 / 0 / 6 | All eight depth 8 Query variants (+29) |
| Clone a fully read message | Requested bytes | 58 / 0 / 6 | Query depth 7 variants 0, 1, 4 and 5 (+1,794) |

Sparse variant 5 has the highest proportional excess for owned decode and
read-all (976 vs 75 requested bytes) and for cloning (832 vs 59). These differences are
901 and 773 bytes per operation respectively. The 1,000 entity specimen
requests fewer allocations for decode, read-all and clone. All adoption cells
perform complete value checks in preparation. [Operation definitions](../../bench/devloop/ADOPTION.md)

| Combined client/server RPC | Metric | Native higher / equal / lower | Largest excess, native vs tonic |
|---|---|---:|---|
| Native pbrs vs tonic Prost unary | Requests | 48 / 0 / 16 | Query depth 8 variant 0 (235.030 vs 167.010) |
| Native pbrs vs tonic Prost unary | Requested bytes | 0 / 0 / 64 | — |
| Native pbrs vs tonic Prost server stream | Requests | 48 / 0 / 16 | Query depth 8 variant 0 (729.000 vs 414.980) |
| Native pbrs vs tonic Prost server stream | Requested bytes | 11 / 0 / 53 | Maps 512 (2,454,993.60 vs 2,286,166.63) |
| Both using Prost, unary | Requests | 0 / 0 / 64 | — |
| Both using Prost, unary | Requested bytes | 62 / 0 / 2 | EntityList 1,000 (14,683,469.28 vs 12,918,108.96) |
| Both using Prost, server stream | Requests | 0 / 0 / 64 | — |
| Both using Prost, server stream | Requested bytes | 59 / 0 / 5 | EntityList 1,000 (32,320,504.66 vs 28,345,954.985) |

With native transport on both sides, Query depth 8 variant 0 still requests
235.030 allocations per unary RPC with pbrs and 122.005 with Prost. The audit
summary includes all operation totals and tied maxima for this comparison.

Codec counts are exact in the timed loop. The RPC figures are process-wide
medians from TCP loopback on a shared runner. Requested sizes include
reallocations. Timing, process-wide futex and syscall values, allocation
counts, and requested bytes answer different questions. Qualification requires
the dedicated hosts, paired repetitions and uncertainty intervals in the
[benchmark contract](../benchmark-contract.md).

## Recent main-branch runs

| Head | Run | Evidence |
|---|---|---|
| [`7b34d68c`](https://github.com/mingley/pure-protobuf/commit/7b34d68c2ad6c8d0b12efe418106e53e0d6c454f) | [44360934662](https://github.com/mingley/pure-protobuf/actions/runs/44360934662) | 1,634 common cells on Rust 1.100.0. Instruction counts unavailable. Advisory comparison exit 1. |
| [`9061b8ee`](https://github.com/mingley/pure-protobuf/commit/9061b8ee0a75d32acb19ebf7090e4a95660012db) | [44054498584](https://github.com/mingley/pure-protobuf/actions/runs/44054498584) | Base and head instruction pairs decreased at 2N. [Error archive](https://github.com/mingley/pure-protobuf/actions/runs/44054498584/artifacts/14314894366) |
| [`111e7b88`](https://github.com/mingley/pure-protobuf/commit/111e7b88d70a5bcbb5137f225932330f7171e721) | [44045742672](https://github.com/mingley/pure-protobuf/actions/runs/44045742672) | Base and head instruction pairs decreased at 2N. [Error archive](https://github.com/mingley/pure-protobuf/actions/runs/44045742672/artifacts/14313857493) |
| [`fe56a183`](https://github.com/mingley/pure-protobuf/commit/fe56a1836f47bec7554c688b988fa6fe638ad59e) | [44040440896](https://github.com/mingley/pure-protobuf/actions/runs/44040440896) | Cancelled before any job or artifact was recorded. |
| [`cc502ed2`](https://github.com/mingley/pure-protobuf/commit/cc502ed27ac4970fa2c8fbc08d846c6904da87ce) | [44036005116](https://github.com/mingley/pure-protobuf/actions/runs/44036005116) | 1,634 common cells, advisory comparison exit 1. [Complete archive](https://github.com/mingley/pure-protobuf/actions/runs/44036005116/artifacts/14312479823) |

The archived successful and failed comparisons pin the same RPC fixture source
SHA-256 `78d7b32b538f8610c9d7a6ee49d670407d8106948b7251eafe127f38eaaea806`.
No legacy RPC overlay was applied. Each failed artifact retains empty-cell
error reports and the log with the offending raw counts. The complete run's
advisory comparison flagged syscall and futex-family rows. Its allocation
observations in the tables come from cross-implementation comparisons at the
head.

## Instruction counter recovery

The ordinary codec preparation allocated three independent fresh-encode pools
for every codec cell, including the two-byte ID encode and the empty decode.
With N=2,000, each paired child prepared 4,000 full TestAllTypes objects per
codec. Work outside the timed loop dwarfed the incremental instructions in
the two tiny operations. At [`9061b8ee`](https://github.com/mingley/pure-protobuf/commit/9061b8ee0a75d32acb19ebf7090e4a95660012db), the base empty-encode pair decreased from
135,126,396 to 135,006,408 instructions. The head ID-encode pair decreased
from 135,318,055 to 135,281,023.

[`36c58fec`](https://github.com/mingley/pure-protobuf/commit/36c58fec9fcded7da355f1b46e26aa16504c3ed7) prepares only the pool used by the selected fresh-encode cell.
The independent warmup objects, fixture checks, measured loop, iteration
counts and invalid-counter failure gate are preserved.

On the original preparation, the detecting test found four pbrs fresh objects
for a tiny empty encode that consumes none. The corrected harness passed
36 unit tests and 18 comparison diagnostics. Both source pins built release
binaries. Direct N and 2N runs matched allocations, requested bytes, copy
counts and reported input metadata in all 36 pairs across pbrs, Prost and
protobuf v4. [Detecting control](https://github.com/mingley/pure-protobuf/actions/runs/44348605076/artifacts/14448495027)

The shared runner used Callgrind 3.26.0. Eight cells on each source ran six
batches at N=2,000 and 2N, with two separate N samples per batch. All 48
differential pairs per source were monotonic. The four cells that triggered the
official hardware-counter failures show the following instruction-reference
counts. A/A is the largest absolute difference between two same-N samples.

| Cell | N median, original → corrected | A/A, original → corrected | N-to-2N median delta, original → corrected |
|---|---:|---:|---:|
| pbrs empty encode | 100,770,411.5 → 973,330 | 549 → 17 | 214,116.5 → 213,996 |
| pbrs ID encode | 100,831,787.5 → 1,034,365 | 280 → 28 | 271,825 → 271,990.5 |
| pbrs ID decode | 100,826,998.5 → 1,030,009 | 411 → 15 | 268,114.5 → 267,997.5 |
| Prost empty decode | 100,862,793 → 1,065,766 | 526 → 15 | 302,008.5 → 301,993 |

The probe replayed the original decreasing counts through a synthetic counter
and confirmed that the corrected collector rejects the pair before writing
a report. The standard eight-cell aggregate stopped at its first child because
the scratch PATH shim resolved the Valgrind launcher without its sibling
`valgrind.bin`. The raw replay invoked the installed Valgrind directly. The
fresh-encode controls also show that changing preparation can shift a
whole-process differential. These results cover setup and measurement
diagnostics. Hardware-counter recovery for the full hosted matrix remains
open. [Raw trials and controls](https://github.com/mingley/pure-protobuf/actions/runs/44349454651/artifacts/14449156597)

The raw trial ZIP has SHA-256
`5fb4644e87f28c054f97b83cd47e36baacf1db4fc425b057beda094c551cec84`
and expires February 16, 2027 at 16:37:55 UTC.

The first automatic comparison after integration may still build a base
revision with the original preparation. The workflow pins each revision
independently. A decreasing counter pair still fails evidence validation
for that source, and a full base/head run requires valid captures on both
sides. Later source pairs can include the preparation repair on both sides.

## Reproduce the summary

The development-loop entry point can measure an individual pair. With the
pinned compiler on PATH, run this from the repository root. [Build and run instructions](../../bench/devloop/README.md)

~~~sh
bench_output=$(mktemp -d)
cells=codec.adoption.pbrs.query.d8.v0.read_all,codec.adoption.prost.query.d8.v0.read_all
scripts/devloop.sh --cells "$cells" --out "$bench_output/query-read.json"
~~~

For the exact retained allocation table, download the pinned source probe and
original official archives into a temporary directory. The audit bundle contains the reducer, fixture hashes,
the complete run's summary and the two original error reports. Its ZIP has
SHA-256 `32db6b30133f79b0dc599ce487c14d9aa1c8ef8883e97715780a661be87ebb19`
and expires February 16, 2027 at 16:28:34 UTC. [Audit bundle](https://github.com/mingley/pure-protobuf/actions/runs/44347653961/artifacts/14447915394)

~~~sh
bench_output=$(mktemp -d)
gh run download 44347653961 --repo mingley/pure-protobuf \
  --name nov18-devloop-prep --dir "$bench_output/probe"
mkdir -p "$bench_output/devloop-prior-artifacts" "$bench_output/devloop-prep-evidence"
for artifact in 14314894366 14313857493 14312479823; do
  gh api "repos/mingley/pure-protobuf/actions/artifacts/$artifact/zip" \
    > "$bench_output/devloop-prior-artifacts/$artifact.zip"
done
RUNNER_TEMP="$bench_output" python3 "$bench_output/probe/nov18-devloop-prep.py" recover
cmp "$bench_output/probe/prior-allocation-summary.json" \
  "$bench_output/devloop-prep-evidence/prior-allocation-summary.json"
~~~

The successful original ZIP has SHA-256 `9d50147e3df32fa98bdba86782b52e0ae4b12c6cefab3b078fa91ce9c0db2069` and expires
February 15, 2027 at 07:05:21 UTC. The compact summary retains exact ties and
the greatest absolute and relative excess for each operation and metric.
