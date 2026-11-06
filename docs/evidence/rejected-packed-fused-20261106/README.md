# Rejected fused packed-wire prototype

**Rejected and unshipped.** Fusing normalization into the original Vec
cache improves fresh encoding and noncanonical input, but cached TAT and
encode-then-get regress in all nine pairs. Large owned and small cached
controls also lose. This does not meet the performance goal.

Full TAT fresh encoding remains slower than the best actual Prost API in
the candidate binary: pbrs 379.64 versus `encode_to_vec` 357.02 ns/op.
Prost encoding into an initially empty Vec takes 458.26 ns/op there.
Both APIs, and their results in both frozen binaries, remain visible.
These are shared-host diagnostics, not a verified-hardware ranking.

## Every measured row

CPU values are per-variant medians. Ratios are medians of nine paired
candidate/baseline ratios; above one means slower. Ranges include every
pair, so a ratio need not equal the ratio of the rounded CPU medians.
All 432 CPU and 48 clock-free allocation processes passed. No slow row,
sample, failure status, or reference API was dropped; no CPU rerun or
source tuning followed the freeze.

| Case | CPU ns/op, baseline → candidate | Paired CPU ratio (range) | Allocation calls/op | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| `tat.fresh` | 500.17 → 379.64 | 0.7723 (0.7260–0.8745) | 7 → 3 | 0/9 |
| `tat.cached` | 111.94 → 132.42 | 1.1894 (1.1470–1.2942) | 1 → 1 | 9/9 |
| `tat.encode_get` | 505.69 → 551.81 | 1.1001 (1.0500–1.5508) | 7 → 7 | 9/9 |
| `i32.canonical.8.fresh` | 85.94 → 42.63 | 0.4995 (0.4682–0.5530) | 4 → 2 | 0/9 |
| `i32.canonical.8.cached` | 18.10 → 19.33 | 1.0603 (1.0175–1.2457) | 1 → 1 | 9/9 |
| `i32.canonical.8.encode_get` | 90.62 → 91.13 | 1.0101 (0.7842–1.0866) | 4 → 4 | 6/9 |
| `i32.canonical.8.get_encode` | 92.42 → 89.94 | 0.9738 (0.8064–1.0090) | 4 → 4 | 1/9 |
| `i32.canonical.8.owned` | 43.61 → 45.41 | 1.0337 (0.9155–1.2356) | 2 → 2 | 6/9 |
| `i32.canonical.4096.fresh` | 8678.84 → 4660.40 | 0.5249 (0.4886–0.5505) | 20 → 2 | 0/9 |
| `i32.canonical.4096.cached` | 74.91 → 76.04 | 1.0137 (0.9460–1.3005) | 1 → 1 | 6/9 |
| `i32.canonical.4096.encode_get` | 9285.95 → 8642.22 | 0.9338 (0.8140–1.0182) | 20 → 11 | 1/9 |
| `i32.canonical.4096.get_encode` | 9079.40 → 8782.29 | 0.9714 (0.9164–1.3669) | 20 → 11 | 2/9 |
| `i32.canonical.4096.owned` | 5207.87 → 6137.93 | 1.1729 (0.9617–1.3242) | 11 → 11 | 7/9 |
| `i32.early.8.fresh` | 87.32 → 43.86 | 0.5047 (0.3952–0.7009) | 4 → 2 | 0/9 |
| `i32.late.4096.fresh` | 8971.76 → 4686.64 | 0.5179 (0.4767–0.5567) | 20 → 2 | 0/9 |
| `bool.normalize.8.fresh` | 67.22 → 41.60 | 0.6149 (0.5735–0.6499) | 3 → 2 | 0/9 |
| `fixed32.canonical.4096.fresh` | 3505.59 → 3515.80 | 1.0133 (0.9814–1.0236) | 1 → 1 | 5/9 |
| `fixed32.canonical.4096.cached` | 3084.44 → 3070.13 | 0.9876 (0.7119–1.1086) | 1 → 1 | 3/9 |

Unchanged Prost implementations shift by 2.0–6.9% in their median paired
ratios between the linked binaries. No reference-based adjustment was
applied to pbrs. Host variation and changed compiler/linker output remain
confounds; the experiment does not isolate every timing difference.

| Case | CPU ns/op, baseline → candidate | Paired CPU ratio (range) | Allocation calls/op | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| `tat.fresh / prost-grow` | 442.57 → 458.26 | 1.0502 (0.9966–1.1045) | 5 → 5 | 8/9 |
| `tat.fresh / prost-reserve` | 335.63 → 357.02 | 1.0690 (0.9622–1.0964) | 1 → 1 | 8/9 |
| `tat.cached / prost-grow` | 450.78 → 478.36 | 1.0434 (0.9997–1.2913) | 5 → 5 | 8/9 |
| `tat.cached / prost-reserve` | 316.56 → 319.35 | 1.0202 (0.9305–1.0463) | 1 → 1 | 6/9 |
| `tat.encode_get / prost-grow` | 434.36 → 465.60 | 1.0648 (0.9860–1.1554) | 5 → 5 | 8/9 |
| `tat.encode_get / prost-reserve` | 335.20 → 352.16 | 1.0509 (1.0330–1.1355) | 1 → 1 | 9/9 |

## Design and tradeoffs

The baseline is Git `e72629e4` plus the independent `take_vec` cache-recovery
guard. The candidate keeps the original `OnceLock<Vec<u8>>`, layouts,
empty/fixed-width prelude, and owned/default-custom fallback body. An
optional function-pointer constant defaults to None; only built-in
varint, zigzag, and bool codecs opt in. Their normalizer scans a canonical
prefix, allocates its output Vec, copies that prefix once, and normalizes
the remaining scalars directly. Entirely canonical wire is copied once
after scanning. No typed value Vec or separate predicate/fallback pass is
needed for a cold retained-wire encode. Signed int32 expansion can grow
the output Vec beyond its initial wire-sized capacity.

Fresh TAT allocations fall from seven to three, requesting 199 → 103
bytes; encode-then-get remains seven calls and 199 requested bytes.
Cached TAT stays at one call. Late noncanonical 4096-value encoding falls
from twenty allocation calls to two. Owned and fixed-width counts stay
unchanged. Requested bytes for every cell remain in the raw records.
Prost `encode_to_vec` still needs only one allocation for this TAT fixture.

Keeping a Vec cache does not preserve identical machine instructions.
The unchanged `is_empty` implementation follows different branches when
encoding has not populated the decoded cache. Selected disassembly also
shows the candidate materializing its encoder function address in a stack
slot before the cache check, including paths that return cached bytes or
fall back to owned values. The selected i32 accessor is 173 bytes in the
baseline and 222 in the candidate; the latter has two retained copies.
The whole executables are 2,005,952 and 2,026,392 bytes. These are static
observations, not proof that the stack slot caused every regression.
No implementation change followed this inspection or these results.

## Workload and source controls

The full bundled pbrs TAT message and full generated Prost 0.13.5 message
are used, with the reference source and provenance archived by value.
pbrs emits 87 bytes and Prost 83: pbrs includes default map-entry scalar
tags that Prost omits. Actual output sizes are recorded correctly from
the outset. Every full-message output is decoded and compared outside
the measured window; separate probes preserve unknown fields and lazy
child-message bytes. Current Prost 0.14, RPC, and codegen are not measured.

Other rows encode one packed field with exact output reservation. Parsed
fresh objects have no encoded cache and are prepared in batches of 32
before measurement. Sixteen independent warmup objects are discarded;
none is reused for measured fresh encoding. Cached rows use a separately
warmed object. Getter-order rows include field access in the window;
owned values are prepared beforehand. Parsing, construction, output
validation, and destruction are excluded. These small pools may fit CPU
caches; this is not a complete message lifecycle comparison.

CPU binaries have no allocation counters; allocation binaries have no
measurement clocks. Both variants were built sequentially at one new
canonical source/harness path with identical package IDs, crate metadata,
profile, compiler, manifests, and lockfiles. Only `src/packed.rs` differs
between the measured source inventories. The sparse workspace is reduced
to `["."]` and dependencies resolved offline; exact adaptations are
archived. Thin LTO, one codegen unit, and zero debug info are retained in
the build receipts. Package-path differences are controlled within this
experiment; changed source/code size and linked placement are not.
Do not treat raw times from the earlier rejected marker experiment's
different root as a matched comparison with this experiment.

The coordinator reported no active local Cargo, Miri, packaging, or
benchmark process for the short authorized slot. That statement is
attributed in `quiet-slot.json`; it does not establish exclusive access
to the underlying shared hardware. No CPU affinity pinning is claimed.

## Correctness and verification

Candidate source is `5a1cf98c…72ab8e`; the guarded baseline is
`3995c4a2…996d308`. Nine shared golden tests pass on the baseline; the
candidate passes 117 minimal-runtime tests and strict Clippy. Twenty
targeted strict-provenance Miri executions cover the actual normalizer,
malformed windows, signed expansion, cache lifetimes/layout, factory
Err/panic, custom callback order, concurrency, and recovery: eight fused
tests plus two recovery tests on each of 64-bit and interpreted i686.
Interpreted i686 is not native 32-bit execution. Private/public sizes stay
104/8 bytes on 64-bit and 52/4 on interpreted 32-bit. No new unsafe code
is introduced. Independent source review is retained.

The prior custom-decode recovery bug is already fixed in the measured
baseline and is not counted as an optimization gain. Initial lint and
inspection-label stages are archived without adding their checks to the
final-source counts. The separate earlier predicate/marker rejection
remains frozen; this candidate neither replaces it nor changes main.

Run `python3 docs/evidence/rejected-packed-fused-20261106/check.py` in a
checkout containing the named Git base. The checker locates the actual
checkout, checks all archived hashes, reconstructs unchanged Git inputs,
binds source/build/measurement records, recalculates every ratio and
allocation total, and rejects six deliberate accounting errors. It does
not execute archived code, replay tests/timings, or qualify the host.
The archive contains text sources and records, no executables/full trees.
