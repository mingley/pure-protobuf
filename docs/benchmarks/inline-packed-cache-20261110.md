# Inline packed-cache experiment, November 10

Two **unmerged** packed-cache candidates saved one allocation for each fresh
short packed payload in this fixture, but both slowed warm serialization in
the longer screen. The early-fallback candidate also slowed a prepared
10-Entity clone-and-encode case. The changes were not selected.

The candidates store an encoded run of up to eight bytes inline in the existing
boxed packed-field state. The second candidate skips the inline attempt when a
value already needs more than eight bytes. Both fall back to a `Vec` for longer
runs or custom codecs. A debug layout probe on this build measured the outer
`Packed` at 8 bytes, the populated `PackedInner` at 104 bytes and its encoded
cell at 32 bytes in all three versions.

| Workload | Inline / baseline CPU: median (range) | Early fallback / baseline CPU: median (range) |
|---|---:|---:|
| Fresh empty EntityList | 1.0029 (0.9850–1.0099) | 0.9988 (0.9823–1.0158) |
| Fresh three-value Payload | 0.9606 (0.9235–0.9768) | 0.9768 (0.9400–0.9838) |
| Warm three-value Payload | 1.0745 (1.0301–1.0932) | 1.0674 (1.0292–1.1190) |
| Fresh 1,000-Entity list | 1.0176 (1.0088–1.0248) | 0.9961 (0.9818–1.0209) |
| Prepared 10-Entity clone and encode | 1.0078 (0.9986–1.0586) | 1.0724 (1.0607–1.1375) |
| Prepared 1,000-Entity clone and encode | 1.0063 (0.9902–1.0511) | 1.0032 (0.9835–1.0469) |
| 1,000-Entity decode and full read | 0.9983 (0.9794–1.0005) | 0.9947 (0.9815–1.0036) |
| 1,000-Entity decode and encode | 1.0037 (0.9761–1.0217) | 0.9974 (0.9685–1.1148) |

Lower is better. Both candidates were slower in all ten warm-Payload blocks.
The early-fallback candidate was slower in all ten 10-Entity clone blocks.
Within those warm-Payload blocks, the second/first same-binary CPU ratios
ranged from 0.9640–1.0379 for baseline, 0.9783–1.1030 for inline, and
0.9207–1.0445 for early fallback. These are observations on one shared host;
they do not identify a cause or establish a general ranking.

Separate allocation-instrumented binaries measured request counts and requested
bytes using three N-to-2N pairs. All pairs agreed for these cases:

| Workload | Baseline | Either candidate |
|---|---:|---:|
| Fresh three-value Payload | 8 allocations, 752 bytes | 7 allocations, 744 bytes |
| Fresh 1,000-Entity list | 32,002 allocations, 2,923,166 bytes | 31,002 allocations, 2,915,166 bytes |
| Warm three-value Payload | 1 allocation, 256 bytes | 1 allocation, 256 bytes |
| Prepared 10-Entity clone and encode | 139 allocations, 14,490 bytes | 139 allocations, 14,490 bytes |

The 1,000-Entity case also made 7,000 reallocations in each version. Requested
bytes include allocation and reallocation requests; they are not peak or
retained memory. The fresh Entity fixture constructs and drops the tree and
serializes each nested Payload into `Any` inside the measured operation.
Prepared Entity fixtures already contain serialized `Any` payloads. The
observed allocation difference therefore does not measure encoding alone.

The [long screen](https://github.com/mingley/pure-protobuf/actions/runs/42736628978)
compared baseline [`8a84230d`](https://github.com/mingley/pure-protobuf/commit/8a84230db1ba1b4e3fc28911149744a50e2381f6),
inline [`714a547d`](https://github.com/mingley/pure-protobuf/commit/714a547d9d3658ccb2ed7d0b5e1644173a80a209)
and early fallback [`9897d1de`](https://github.com/mingley/pure-protobuf/commit/9897d1de813e6b193a6f3e99e031a4623d0fe426).
Their `src/packed.rs` blob IDs are respectively
`311968f6fd59fe432ff037f9aec6f5464571e294`,
`4aa75725f67af85f4ce64cef07853f748f44cd31` and
`8eba007cb236d092cc743121d663c897be05e943`. The scratch
[probe and workflow](https://github.com/mingley/pure-protobuf/commit/15ae39d93ce049908cc431b4864d72b8c3945f48)
were applied identically to the three builds.

Rust 1.99.1 release binaries ran on one pinned logical CPU of a shared AMD EPYC
7763 runner. For each case, the baseline calibrated the iteration count, then
ten six-process blocks rotated the three variants and reversed their order.
Each ratio compares the sum of two candidate thread-CPU samples with the sum
of two baseline samples in its block; the same-binary control is second divided
by first within that block. Each process built and checked fixture wire outside
the sample and ran two warmup iterations. The shortest measured process used
141.712 ms of thread CPU. Allocation counters were disabled in timing builds;
runtime synchronization was not removed. No RPC, TLS or tonic comparison was
measured.

The earlier [short screen](https://github.com/mingley/pure-protobuf/actions/runs/42735600706)
used a different [inline revision](https://github.com/mingley/pure-protobuf/commit/6bece9073a1c2b01208ed54242b4c45320fd69f9)
and had a 3.336 ms minimum for warm Payload. It is retained separately and
should not be pooled with the longer measurements. Its warm-Payload median
was 1.0403, with all ten pairs slower; its extreme-value median was 1.1712
with a 1.970 ms minimum. The longer screen did not repeat that extreme-value
result for its later revisions.

The [retained data](inline-packed-cache-20261110.json) contains all 14 cases,
140 timing blocks, within-block controls and 42 allocation pairs. It is a
compact JSON rendering of the downloaded `raw.json`; the original file's
SHA-256 is `fd3744112ccdd056d8341752e92521647924c0aee6c90b09903bfd3e5d675619`.
The [long-screen artifact](https://github.com/mingley/pure-protobuf/actions/runs/42736628978/artifacts/13727557843)
has GitHub-reported SHA-256
`8f16be31a334da04356117f84e884ec52a2f27a759699737d6b7987adfc3cad3`.
The [short-screen artifact](https://github.com/mingley/pure-protobuf/actions/runs/42735600706/artifacts/13726334427)
has GitHub-reported SHA-256
`166c0a599e9388b1e92d89efcaee90d6b91a8a1c95d71515271b6e7f4db78310`.
Both expire November 24, 2026.
