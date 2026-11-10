# Lazy-message cell seeding, November 10

[Benchmarks](../benchmarks.md)

A seeded-cell experiment reduced CPU for the populated Query on Rust 1.99.1.
It remains **unmerged**: the Rust 1.85 screens showed no clear improvement,
and some unchanged controls moved. The results do not establish a general
ranking or an RPC improvement.

The [baseline](https://github.com/mingley/pure-protobuf/commit/e521462b360892232d8e0c8cac72760617fa561d)
constructs an empty `MemoCell` and calls `set` for an already-owned or
already-parsed child. [Candidate eac5ffd5](https://github.com/mingley/pure-protobuf/commit/eac5ffd54507595a55e4eb5c50a514fee20bdd4c)
constructs the cell with `MemoCell::from(value)`. The std and alloc-only
cells both provide that API. [Candidate 05e42378](https://github.com/mingley/pure-protobuf/commit/05e42378fcd29ac372b214e38570e0c7ecca2bdf)
adds two behavior tests to the same production change. [Candidate 4bb89cb7](https://github.com/mingley/pure-protobuf/commit/4bb89cb7864be35374d3c1f5633c3b60434081ea)
adds `#[inline]` to the two constructors; it was tested separately.

## Results

Cells are candidate / baseline thread CPU: median (range), followed by
the number of slower blocks out of ten. Lower is better. “Fresh encode”
includes construction, serialization and drop. The Prost and eight-u64 rows
run unchanged workloads in both binaries; they are controls, not paired
pbrs-versus-Prost measurements.

| Workload | [eac, Rust 1.99 #1](https://github.com/mingley/pure-protobuf/actions/runs/42775074195) | [eac, Rust 1.99 #2](https://github.com/mingley/pure-protobuf/actions/runs/42775883171) | [05e, Rust 1.99](https://github.com/mingley/pure-protobuf/actions/runs/42776002159) | [inline, Rust 1.99](https://github.com/mingley/pure-protobuf/actions/runs/42776796175) |
|---|---:|---:|---:|---:|
| d4 construct | 0.7930 (0.7837–0.8026), 0/10 | 0.8342 (0.8193–0.8570), 0/10 | 0.7997 (0.7423–0.8964), 0/10 | 0.8004 (0.7870–0.8100), 0/10 |
| d4 fresh encode | 0.8384 (0.8195–0.8512), 0/10 | 0.8656 (0.8241–0.8998), 0/10 | 0.8361 (0.8059–0.9626), 0/10 | 0.8527 (0.8186–0.8775), 0/10 |
| d4 decode | 1.0059 (0.9900–1.0326), 7/10 | 1.0257 (0.9864–1.0455), 9/10 | 0.9894 (0.9204–1.0151), 4/10 | 1.0057 (0.9743–1.0213), 6/10 |
| Empty Query | 0.9993 (0.9963–1.0185), 4/10 | 1.0007 (0.9994–1.0023), 8/10 | 1.0477 (1.0213–1.0733), 10/10 | 0.9995 (0.9874–1.0172), 4/10 |
| Operator only | 1.0158 (1.0077–1.0259), 10/10 | 1.0004 (0.9895–1.0065), 6/10 | 1.0086 (0.9786–1.1738), 7/10 | 1.0179 (0.9829–1.0236), 9/10 |
| Ten Queries, one populated | 0.9962 (0.9821–1.0169), 4/10 | 0.9976 (0.9869–1.0138), 5/10 | 1.0272 (1.0016–1.0817), 10/10 | 0.9965 (0.9825–1.0059), 4/10 |
| Eight u64s, unchanged | 1.0013 (0.9985–1.0063), 9/10 | 1.0011 (0.9974–1.0071), 6/10 | 1.0289 (1.0096–1.0713), 10/10 | 0.9981 (0.9098–1.0073), 3/10 |
| Prost fresh, unchanged | 0.9731 (0.9519–1.0364), 3/10 | 0.9895 (0.9573–1.1447), 2/10 | 1.0168 (0.9795–1.1047), 7/10 | 0.9689 (0.9605–1.0165), 1/10 |

| Workload | [eac, Rust 1.85 #1](https://github.com/mingley/pure-protobuf/actions/runs/42775599055) | [eac, Rust 1.85 #2](https://github.com/mingley/pure-protobuf/actions/runs/42776474950) | [inline, Rust 1.85](https://github.com/mingley/pure-protobuf/actions/runs/42776552744) |
|---|---:|---:|---:|
| d4 construct | 1.0300 (1.0191–1.0424), 10/10 | 1.0094 (0.9673–1.0470), 8/10 | 1.0149 (0.9936–1.1673), 9/10 |
| d4 fresh encode | 1.0412 (1.0145–1.0826), 10/10 | 1.0058 (0.9961–1.0206), 9/10 | 1.0279 (0.9302–1.0346), 9/10 |
| d4 decode | 1.0003 (0.9879–1.0154), 7/10 | 1.0088 (1.0019–1.0179), 10/10 | 0.9931 (0.9718–1.0285), 4/10 |
| Empty Query | 1.0005 (0.9980–1.0467), 7/10 | 1.0101 (0.9951–1.0296), 8/10 | 1.1000 (1.0788–1.1534), 10/10 |
| Operator only | 0.9998 (0.9843–1.4182), 5/10 | 1.0002 (0.8965–1.0752), 5/10 | 1.0110 (0.9398–1.0394), 7/10 |
| Ten Queries, one populated | 1.0013 (0.9874–1.0385), 6/10 | 0.9762 (0.9555–1.0071), 1/10 | 1.0029 (0.9900–1.0324), 7/10 |
| Eight u64s, unchanged | 1.0002 (0.9354–1.0472), 6/10 | 0.9858 (0.9611–1.0226), 3/10 | 0.9949 (0.9713–1.0322), 4/10 |
| Prost fresh, unchanged | 0.9987 (0.9370–1.0105), 4/10 | 0.9827 (0.9580–1.0008), 1/10 | 1.0442 (1.0303–1.0573), 10/10 |

The first 1.85 d4 fresh result was slower in all ten blocks; its repeat
was near one and included a result below one. The inline 1.85 run also
shifted the unchanged Prost control. On 1.99, the tested-source run shifted
the empty and eight-u64 controls even though they do not exercise a seeded
child; other runs did not reproduce the same shifts. This limits attribution
of the small changes. None of the seven screens changed allocation requests:
d4 fresh requested 49 allocations plus one reallocation and 2,949 bytes for
both pbrs variants; the separate Prost control requested 22 allocations plus
one reallocation and 2,421 bytes. Requested bytes are not live or peak memory.

The inline screen's `size` output measured 554,781 → 554,989 text bytes
on 1.85 and 514,811 → 513,983 on 1.99 for these benchmark executables.
Those are fixture binary sizes, not packaged-crate sizes.

## Method and checks

Each screen built the baseline and its candidate sequentially at one canonical
package path with release optimization, thin LTO and one codegen unit. Ten
ABBA/BAAB blocks used fresh processes pinned to one logical CPU on shared
Linux runners; each process targeted about 120 ms of thread CPU, with a
20-million-iteration cap. Same-binary controls, each individual sample and
separate N/2N allocation measurements are in the [raw data](lazy-cell-seeding-20261110.json).
Observed ranges are not confidence intervals.

The adoption driver normally requires Rust 1.88. For the 1.85 screens, the
same measurement-only overlay in both arms lowers its declared toolchain
floor and replaces its `is_multiple_of` expressions with equivalent remainder
checks. The [retry workflow](https://github.com/mingley/pure-protobuf/blob/bed9db1aac9b17c99be49e703c1b81f7a927aefc/.github/workflows/codex-query-seeded-cell-msrv-retry-20261110.yml)
records that overlay. An [earlier 1.85 scratch run](https://github.com/mingley/pure-protobuf/actions/runs/42775513068)
failed before measurement because its overlay regex was incorrectly escaped;
it is excluded from the tables. The 1.99 and 1.85 runs used different shared hosts, so
their absolute timings cannot be compared directly. The prepared d4 input
is 293 bytes; the probes check generated Query values, round trips and
matching `adoption.rs` output digests.

On Rust 1.99.1, `OnceLock::from` constructs a completed cell; in Rust 1.85
it uses `set`. That source difference is consistent with the observed
populated-Query direction, but these screens do not isolate machine code or
prove the cause.

The [correctness run](https://github.com/mingley/pure-protobuf/actions/runs/42775418162)
passed its Rust 1.85 seeded-cell and alloc-only target checks and three
strict-provenance Miri tests. Its stable job failed because an offline
compatibility consumer lacked a fetched dependency. A
[first retry](https://github.com/mingley/pure-protobuf/actions/runs/42775840095)
passed the behavioral checks but failed strict Clippy on a redundant test
expression. The later
[stable retry](https://github.com/mingley/pure-protobuf/actions/runs/42776279496)
passed 21 lazy tests, 11 compatibility tests, a generated lazy consumer and
strict Clippy on the corrected test candidate. A
[binary comparison](https://github.com/mingley/pure-protobuf/actions/runs/42776347025)
found the 05e and corrected test-candidate release binaries differed; results
above apply only to their named sources. No candidate was merged.

The [independent evidence check](https://github.com/mingley/pure-protobuf/actions/runs/42777614262)
verified the seven original ZIP hashes and recomputed all 119 case summaries,
including controls and allocation differences. The retained JSON contains
every recorded block and identifies the source runs and original artifacts.
From the repository root, check the record with:

```sh
python3 docs/benchmarks/lazy-cell-seeding-20261110-verify.py
```

This verifies recorded arithmetic and table rows; it does not rerun the
benchmarks or qualify the runners.
