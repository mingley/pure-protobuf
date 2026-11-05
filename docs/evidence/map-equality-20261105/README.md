# Map equality, November 5

Map equality now compares the sorted last-wins indexes directly when both
maps have them. This avoids a binary search for every wire entry, including
shadowed duplicates. Small maps keep the existing scan. Equal unique lengths
and one matching entry per key make the old reverse membership pass redundant.
The indexed comparison is outlined to keep it out of the small-map scan's
generated code. No storage layout, public API or unsafe code changed.

These measurements compare isolated overlays of
[`1c6ec8ca`](https://github.com/mingley/pure-protobuf/commit/1c6ec8cacd547cce8c3a8e335320b49d34de40b4).
They measure map equality with runtime-only pbrs, not protobuf decoding or
gRPC. The combined repository revision was not retimed by this experiment.

## Results

The final warm follow-up uses nine randomized process pairs per row. CPU
medians below exclude map construction, index construction and destruction.
String keys contain 49 bytes. A duplicate fixture has 256 wire entries and
64 distinct keys; a unique fixture has 256 distinct keys.

| Equal maps | Before CPU ns | After CPU ns | Paired new / old | All-pair range |
|---|---:|---:|---:|---:|
| Integer, unique | 4,298 | 276 | 0.0651 | 0.0627–0.0701 |
| Integer, duplicate | 2,100 | 67 | 0.0320 | 0.0315–0.0323 |
| ProtoString, unique | 34,770 | 873 | 0.0251 | 0.0235–0.0262 |
| ProtoString, duplicate | 16,073 | 222 | 0.0137 | 0.0134–0.0150 |
| LazyStr, unique | 41,619 | 1,154 | 0.0277 | 0.0273–0.0299 |
| LazyStr, duplicate | 21,672 | 292 | 0.0134 | 0.0131–0.0171 |

All 66 warm equal-map screen medians improve. Eighteen repeated equal-map
controls have paired medians of 0.0134–0.806. Small string maps still lose to
the separate-process `HashMap<String, i32>` storage reference. The warm
256-entry rows above favor the indexed map over that reference; this is not a
Prost decoding or overall performance result.

False comparisons retain losses. Four LazyStr size-mismatch controls have
nine-pair medians of 1.064–1.077, about 0.2–0.3 ns more per comparison.
An 8-entry duplicate ProtoString size mismatch has a median of 1.029,
about 1.3 ns more, with a 0.978–1.050 range. A short ProtoString size-mismatch
screen reported 1.63; the longer follow-up did not reproduce it (0.993).
All raw samples remain in the archive, including these losses and outliers.

The cold suite uses fresh, precloned maps and includes first index creation.
All 18 equal-map medians improve (new / old 0.397–0.928). One integer
16-entry missing-key row has a 1.025 median and a 0.795–1.187 range.
Cold index creation still allocates. Separate instrumented builds check 72
fixtures: all four map variants request exactly the same allocation calls
and bytes; warm comparisons allocate nothing. For two unique 256-entry
ProtoString maps, cold equality requests 516 allocations and 66,048 bytes.
HashMap equality itself allocates nothing. Construction and cloning are
outside this accounting; it does not measure retained memory or RSS.

## Variants and correctness

| Variant | Result |
|---|---|
| `baseline` | Original two-direction membership comparison |
| `candidate` | Removes the redundant reverse walk; equal-map wins, indexed searches remain, false controls retain regressions |
| `sorted` | Direct indexed pairing; large gains, but an 8-entry duplicate ProtoString size mismatch rises about 16% in its screen |
| `outlined` | Selected: keeps direct indexed pairing in a separate helper and avoids index reads on small maps; final gains and losses are above |

Fourteen map unit tests pass, including the HashMap last-wins oracle across
the index boundary, shadowed entries, reordered keys, same-cardinality key
replacement, invalid UTF-8 string keys, NaN, signed zero, and mutation after
indexes become warm. The right value still compares to the left value.
Indexed value comparisons now run in sorted-key order; evaluation order for
side-effectful custom `PartialEq` implementations is not promised.
Independent source review found no correctness blocker for lawful
`MapKey` equality, ordering and cloning.

## Reproduce and check

Run `python3 check.py` from this directory. It reads the archive without
executing its contents, reconstructs 70 selected Git inputs, checks the
source overlays and 12 builds, and recomputes every ratio, median, range,
fixture result and allocation count across 2,316 groups and 8,748 processes.
The archive includes the harnesses, lockfiles, build logs, scripts, unit-test
log and every raw sample. The recorded build scripts use their original
workspace paths; adjust `ROOT` before rebuilding elsewhere.

The compiler was Rust 1.99.1 with LLVM 23.1.1, thin LTO and one codegen unit.
CPU builds have no counting allocator. Runs used CPU 0 of a shared Linux
x86_64 VM reporting AMD EPYC 9V74; other repository builds paused during
timing. Process CPU, affinity and host-load observations are retained.
Same-executable paired controls center near one but retain variation.
Affinity and paused builds do not establish a dedicated host. Ranges are
observed pair ranges, not confidence intervals. Executables and toolchain
binaries are not retained, so their recorded hashes cannot be independently
recomputed. Selected source inventories are not a filesystem-read trace or
a hosted attestation. These local diagnostics do not qualify a ranking.
