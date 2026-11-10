# Repeated storage screens, November 10

A generic inline-singleton representation for `Repeated<T>` is **rejected**:
it reduced allocation requests in a populated Query fixture but enlarged the
heap allocation for multi-element fields with large elements. A separate
Query-only cold-vector diagnostic grew its cold struct from 32 to 96 bytes.
Unchanged control workloads shifted CPU in both directions, so neither
screen proves a general CPU gain. Both are unmerged component experiments,
not gRPC or overall performance rankings.

## What was tried

At baseline `Repeated<T>` stores an optional box containing a `Vec<T>`.
The [candidate patch](repeated-singleton-20261110/candidate.patch) instead
boxes an enum with `One(T)` and `Many(Vec<T>)` variants. Singleton creation
can then avoid a separate vector-buffer allocation. The public wrapper was
8 bytes in both builds on x86-64, but the boxed enum must be large enough for
`T` even when it stores the `Many` variant.

The [layout check](https://github.com/mingley/pure-protobuf/actions/runs/42750464938)
measured a 24-byte box payload at baseline for every tested element type.
Candidate payloads were 24 bytes for `u64`, 40 for `[u8; 32]`, 136 for
`[u8; 128]`, and 1,152 for a 1 KiB, 128-byte-aligned element. These are
observations for that target and build, not ABI guarantees. The candidate
had no element-size cutoff. Clone CPU and peak stack use were not separately
measured; their costs depend on the element and access pattern.

## Results

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42751626539)
compared baseline [`09d8ad6b`](https://github.com/mingley/pure-protobuf/commit/09d8ad6b0b1ee4fec15fe14f2521d0031fa41997)
with [candidate `7d521d29`](https://github.com/mingley/pure-protobuf/commit/7d521d29eafe6363650f319334036d3026c00501).
The [raw measurements](repeated-singleton-20261110/raw.json), [probe](repeated-singleton-20261110/probe.rs),
[driver](repeated-singleton-20261110/compare.py), and
[environment](repeated-singleton-20261110/toolchain.txt) are retained.
The candidate source blob is `e984b2f3b22e67f03b84e8cd32f4052de6cfde31`;
the patch applies to the baseline to reproduce it.

CPU cells show candidate / baseline thread CPU, with the median and observed
range of ten balanced four-process blocks. “Slower” counts blocks above 1;
lower is better. Allocation cells show **allocations + reallocations /
requested bytes** per operation, using three separate N-to-2N pairs. All
three pairs agreed for each cell. Reallocations contribute their full new
size to requested bytes; these numbers are not retained memory.

| Case | CPU median (range) | Slower blocks | Baseline requests / bytes | Candidate requests / bytes |
|---|---:|---:|---:|---:|
| Empty Query encode | 0.912 (0.887–0.920) | 0 | 0 + 0 / 0 | 0 + 0 / 0 |
| Query d4v6 construct | 0.964 (0.919–1.102) | 2 | 48 + 1 / 2656 | 38 + 0 / 2280 |
| Query d4v6 fresh encode | 0.949 (0.924–0.988) | 0 | 49 + 1 / 2949 | 39 + 0 / 2573 |
| Query d4v6 construct and mutate | 0.930 (0.910–0.962) | 0 | 48 + 2 / 2784 | 39 + 0 / 2344 |
| Query d4v6 warm encode | 0.964 (0.950–0.986) | 0 | 1 + 0 / 293 | 1 + 0 / 293 |
| Query d4v6 decode | 0.878 (0.867–1.014) | 1 | 39 + 0 / 4168 | 34 + 0 / 3080 |
| Prost d4v6 fresh control | 1.003 (0.918–1.084) | 5 | 22 + 1 / 2421 | 22 + 1 / 2421 |
| Collect one `u64` | 0.581 (0.559–0.882) | 0 | 2 + 0 / 32 | 1 + 0 / 24 |
| One `u64`, mutable proxy | 0.966 (0.825–1.017) | 1 | 2 + 0 / 32 | 2 + 0 / 40 |
| One `u64`, convert to Vec | 0.960 (0.932–1.108) | 2 | 2 + 0 / 32 | 2 + 0 / 32 |
| Collect eight `u64` values | 1.017 (0.997–1.082) | 8 | 2 + 0 / 88 | 2 + 0 / 88 |
| Collect one 1 KiB value | 0.696 (0.590–0.822) | 0 | 2 + 0 / 1048 | 1 + 0 / 1032 |
| Construct two 1 KiB values | 0.741 (0.715–0.873) | 0 | 2 + 0 / 2072 | 2 + 0 / 3080 |

Fresh Query encode includes construction, serialization, and drop; it is not
an encode-only comparison. Warm encode reuses a prepared message. The Prost
row runs the same Prost workload in both binaries as a control; its CPU ratio
is not a pbrs-versus-Prost ratio. The allocation rows show that this
candidate still made more requests than Prost for this particular fresh
fixture.

The 1 KiB multi-element example requested 1,008 more bytes despite making
the same number of requests. The `Many` variant carries the inline
singleton's size cost. A mutable singleton proxy materializes a vector and
lost the request saving. Empty Query changed CPU despite zero requests,
showing why code layout and machine noise must be considered alongside the
allocation counts.

The counter-free binaries had neither the counting allocator nor
`copy-counts`; allocation counts came from separately built binaries.
Each process warmed the input before measurement. Ten alternating ABBA/BAAB
blocks ran pinned to one logical CPU; the shortest measured process used
61.6 ms of thread CPU. Same-binary order controls and all individual
samples are in the raw file. This shared Ubuntu x86-64 runner used an AMD
EPYC 9V45, Rust 1.99.1, LLVM 23.1.1, thin LTO and one codegen unit. The
observed ranges are not confidence intervals.

## Query cold-vector diagnostic

A separate [Query-only overlay](repeated-singleton-20261110/query-cold.patch)
stored four vector headers directly inside the already-boxed cold struct.
This is a diagnostic for an allocation source, not a proposed codegen rule.
It increased the cold struct from 32 to 96 bytes on x86-64 while leaving
the outer Query at 32 bytes. The overlay explicitly uses `Default` for that
cold struct because zeroed bytes are not a valid `Vec` default. Even one
populated cold field pays for the larger box, although it can avoid a
separate vector-header box.
Requested bytes below include all allocations in each case, not retained
heap or peak memory.

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42752509555)
used the same baseline and [candidate `4ef73c60`](https://github.com/mingley/pure-protobuf/commit/4ef73c60237485b79a115c06df35ff1eb993c0e5).
The [full raw data](repeated-singleton-20261110/query-cold-raw.json),
[probe](repeated-singleton-20261110/query-cold-probe.rs), and
[driver](repeated-singleton-20261110/query-cold-compare.py) are retained.
Columns and units match the table above; each CPU cell has ten balanced
four-process blocks and each allocation cell has three N-to-2N pairs.

| Case | CPU median (range) | Slower blocks | Baseline requests / bytes | Candidate requests / bytes |
|---|---:|---:|---:|---:|
| Empty cold mutable proxy | 0.506 (0.498–0.531) | 0 | 2 + 0 / 56 | 1 + 0 / 96 |
| One predicate, fresh encode | 0.923 (0.881–0.951) | 0 | 4 + 0 / 137 | 3 + 0 / 177 |
| Ten Queries, one populated | 0.921 (0.904–0.932) | 0 | 4 + 0 / 448 | 3 + 0 / 488 |
| Query d4v6 fresh encode | 0.939 (0.925–1.093) | 1 | 49 + 1 / 2949 | 38 + 1 / 3069 |
| Query d8v6 fresh encode | 0.943 (0.915–0.977) | 0 | 85 + 1 / 4949 | 66 + 1 / 5133 |
| Query d4v6 clone and encode | 0.937 (0.906–0.948) | 0 | 40 + 0 / 2633 | 29 + 0 / 2753 |
| Query d4v6 decode | 0.818 (0.802–0.834) | 0 | 39 + 0 / 4168 | 29 + 0 / 4248 |
| Unchanged Prost fresh control | 1.048 (1.025–1.071) | 10 | 22 + 1 / 2421 | 22 + 1 / 2421 |
| Unchanged two-1 KiB repeated control | 0.914 (0.892–0.945) | 0 | 2 + 0 / 2072 | 2 + 0 / 2072 |

The mixed case constructs ten Queries, only one with a predicate; the cold
proxy case asks for a mutable empty field. Fresh encode includes message
construction and drop. Both controls run the same workload in each binary,
yet their medians were 4.8% slower and 8.6% faster, respectively; all ten
blocks moved in those directions. Binary layout and runner effects can
confound the Query ratios; these timings do not prove a CPU improvement. The allocation results demonstrate the
tradeoff: fewer requests, but more requested bytes for these sparse and
populated cases.

The [contract run](https://github.com/mingley/pure-protobuf/actions/runs/42751945155)
passed seven default, wire, unknown-field, clone and mutation tests for both
arms. The screen compared 75 generated outputs: only the marked Query blocks
changed, and all descriptors and other generated bytes matched. Negative
controls rejected unrelated source, descriptor, inventory and duplicate
Query changes. The CPU binary had no counting allocator; a separate build
collected allocations. The screen ran on one shared Ubuntu x86-64 runner;
its shortest measured process used 60.4 ms of thread CPU. The exact toolchain,
CPU metadata and sample controls are retained with the raw data. This does
not establish a general storage policy or full feature parity.

A separate [artifact check](https://github.com/mingley/pure-protobuf/actions/runs/42753142306)
recomputed 170 block ratios and 51 matched allocation blocks from all 680
CPU samples and allocation rows. The raw file SHA-256 is
`69024a6fa8e88fbb9a13348e060266c0f7b02ab02977078715cef7b7edfc39e6`;
the [original artifact](https://github.com/mingley/pure-protobuf/actions/runs/42752509555/artifacts/13733594123)
expires November 24, 2026. Scratch branches were deleted.

## Checks and limits

The [correctness preflight](https://github.com/mingley/pure-protobuf/actions/runs/42750352782)
passed five candidate-specific tests, four existing repeated-field tests and
11 codegen compatibility tests at Rust 1.85; ten stable repeated-field tests
also passed. It **failed strict Clippy** on candidate `unreachable!` and
`vec_init_then_push` lints, so the candidate did not pass the full gate.
The candidate added no unsafe code; this screen did not establish full API
or production parity.

A separate [artifact check](https://github.com/mingley/pure-protobuf/actions/runs/42752362904)
recomputed all 130 block ratios and 39 allocation pairs from the 520 CPU
samples and original allocation rows. The raw file SHA-256 is
`b31e201661a67a7cd9315b1c2548a0d2f378bdad64d8a0f768395e0747bd60b3`;
the [original artifact](https://github.com/mingley/pure-protobuf/actions/runs/42751626539/artifacts/13734121730)
expires November 24, 2026. Scratch branches were deleted.
