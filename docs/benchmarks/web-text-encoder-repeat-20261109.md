# gRPC-Web text encoder repeat, November 9

The no-cutover encoder reduced allocation requests in carried segments, but
all three variants were slower than baseline in every observed pair for the
large segment starting without carry and the fully aligned stream. **No
runtime variant was merged.** These are isolated encoder screens, not gRPC
calls or comparisons with tonic or Prost.

[Run 42708113781](https://github.com/mingley/pure-protobuf/actions/runs/42708113781)
passed and deleted its scratch branch. The
[artifact](https://github.com/mingley/pure-protobuf/actions/runs/42708113781/artifacts/13715114424)
has SHA-256 `293fdc6d8edfc3017a2908fec8d37a0e16950870793c513cd9627cb7eecf66aa`
and expires November 23, 2026. The
[retained comparisons](web-text-encoder-repeat-20261109.json) are reconstructed
from the validated job log; raw samples, source fixtures, build records and
disassembly are in the artifact. The earlier
[buffered holdout](web-text-encoder-buffered-20261109.md) used five pairs.

## Compared sources and checks

The production baseline is commit
[`25fabced`](https://github.com/mingley/pure-protobuf/commit/25fabced4f44ef3386d301d999f3928f6c188efb),
with `pbrs-grpc/src/web/frame.rs` blob
`d23508bb5f843f238387b9a433778aa52934de30`. The
[scratch commit](https://github.com/mingley/pure-protobuf/commit/cd82e06149493408d5fa1d8c97d8297c65c1fafa)
contains the fixtures and probe. They share the same tests and differ only
in the private encoder. Fixture blobs are listed before the probe is appended.

| Fixture | Git blob | Change |
|---|---|---|
| Baseline | `33233fa6f7560d640ef6b5989e3f7995f8df14bd` | Existing encoder |
| No-cut | `07668e8186a7ae8c4d3f5e50223c268078870733` | Encode carried segments directly at every length and reuse the small carry buffer |
| Cutover | `34f998ee4f56b84db62520f43ea44a40ff4924e3` | Direct encoding only for carried segments of at least 32 bytes |
| Stack | `d6213a813b8ca36e253c6ed075627aee00483831` | Stack carry and direct output buffer |

The Ubuntu 24.04 x86-64 runner used Rust 1.99.1 and LLVM 23.1.1 for release
screens. Rust 1.85.0 ran the correctness tests. Every fixture passed four
boundary/carry tests and one deadline test. Three deliberate mutations were
caught: corrupting carried bytes, losing deadline carry and mishandling an
incomplete final flush. The no-cut variant also passed the public Web, terminal
accounting and text-ingress suites. Formatting and strict Clippy passed.

## Workloads

Each sequence creates an encoder, feeds prebuilt, cloned `Bytes` segments
and drops outputs immediately. The final segment ends the stream. Short
sequences are `[1, 2]` and `[2, 2]`; empty cases are `[1, 0, 2]`,
`[1, 0]`, `[2, 0]` and `[1, 0, 1, 0]`. Small sequences are
`[297, 23]`, `[298, 23]` and `[299, 23]`. The cutover controls are
`[1, 31, 23]`, `[1, 32, 23]` and `[1, 33, 23]`.

The large cases are `[6, 1048576, 23]`, `[4, 1048576, 23]` and
`[5, 1048576, 23]`, for carry 0, 1 and 2 before the 1 MiB segment.
`large_carry0` leaves one byte of carry for its final segment.
`stream_aligned` uses 64 segments of 1,029 bytes and a final 23;
`stream_carry` uses 64 segments of 1,030 bytes and a final 23.

Every process checked the full output against the base64 reference and
checked carry and output presence at each segment before measurement.
Measured iterations checked output length, not every output byte.

## Counter-free CPU

Cells are candidate / baseline process-CPU ratios: median and observed
minimum–maximum across 12 paired N/2N samples. ↓ means every ratio was below
1, ↑ means every ratio was above 1, and ~ means the range crossed 1. These are
descriptive ranges, not confidence intervals. Smaller is faster.

| Case | No-cut | Cutover | Stack |
|---|---:|---:|---:|
| `empty_between` | 0.681 (0.671–0.708) ↓ | 1.009 (0.991–1.051) ~ | 0.630 (0.581–0.641) ↓ |
| `empty_flush1` | 0.964 (0.860–1.023) ~ | 1.009 (0.969–1.053) ~ | 0.719 (0.698–0.937) ↓ |
| `empty_flush2` | 0.941 (0.871–0.963) ↓ | 0.996 (0.933–1.250) ~ | 0.727 (0.695–0.892) ↓ |
| `large_carry0` | 1.047 (1.002–1.533) ↑ | 1.048 (1.010–1.071) ↑ | 1.044 (1.009–1.069) ↑ |
| `large_carry1` | 0.909 (0.885–0.970) ↓ | 0.907 (0.877–0.969) ↓ | 0.905 (0.897–0.972) ↓ |
| `large_carry2` | 0.911 (0.900–0.928) ↓ | 0.914 (0.902–0.937) ↓ | 0.910 (0.889–0.938) ↓ |
| `short_carry1` | 0.871 (0.833–1.846) ~ | 1.000 (0.968–1.806) ~ | 0.726 (0.708–1.341) ~ |
| `short_carry2` | 0.895 (0.844–1.029) ~ | 1.007 (0.979–1.169) ~ | 0.719 (0.618–0.749) ↓ |
| `short_incomplete` | 0.635 (0.565–0.695) ↓ | 1.008 (0.956–1.170) ~ | 0.598 (0.534–0.640) ↓ |
| `small_carry0` | 1.009 (0.988–1.178) ~ | 1.002 (0.991–1.021) ~ | 0.992 (0.877–1.002) ~ |
| `small_carry1` | 0.947 (0.814–1.001) ~ | 0.993 (0.885–1.136) ~ | 0.898 (0.793–1.094) ~ |
| `small_carry2` | 0.913 (0.897–1.027) ~ | 1.004 (0.984–1.150) ~ | 0.872 (0.845–1.106) ~ |
| `stream_aligned` | 1.038 (1.014–1.051) ↑ | 1.035 (1.022–1.108) ↑ | 1.037 (1.027–1.070) ↑ |
| `stream_carry` | 0.898 (0.863–0.926) ↓ | 0.917 (0.903–1.094) ~ | 0.913 (0.879–0.929) ↓ |
| `threshold_above` | 0.695 (0.675–0.709) ↓ | 0.819 (0.787–0.971) ↓ | 0.651 (0.611–0.660) ↓ |
| `threshold_at` | 0.863 (0.757–1.085) ~ | 0.821 (0.724–1.011) ~ | 0.784 (0.662–1.200) ~ |
| `threshold_below` | 0.675 (0.627–0.695) ↓ | 1.001 (0.912–1.356) ~ | 0.634 (0.599–0.691) ↓ |

All three variants lost every observed `large_carry0` and
`stream_aligned` pair. No-cut improved every observed `stream_carry`
pair. Some short-case ranges crossed 1 despite lower medians; the results do
not establish a win for every case or the cause of the aligned losses.

Samples used separate processes pinned to one allowed CPU, alternating
variant and N/2N order; CPU N was four times the earlier holdout. The
counter-free binaries excluded the allocation wrapper and `copy-counts`.
Process CPU can include other test threads. Affinity does not control shared
host load or frequency, and `black_box` is best effort. The hardware
instruction counter was unavailable.

## Allocation requests

Cells are **allocation calls / reallocations / requested bytes** per sequence,
from `(2N − N) / N`. The first column is baseline; the three variant columns
are changes from baseline. Allocation calls include successful reallocations.
Three N/2N repeats agreed for each variant. The counting allocator can affect
optimization and count other threads; requested bytes are not live or retained
memory.

| Case | Baseline | No-cut change | Cutover change | Stack change |
|---|---:|---:|---:|---:|
| `empty_between` | 5 / 0 / 68 | -3 / 0 / -56 | 0 / 0 / 0 | -4 / 0 / -64 |
| `empty_flush1` | 3 / 0 / 36 | -1 / 0 / -24 | 0 / 0 / 0 | -2 / 0 / -32 |
| `empty_flush2` | 3 / 0 / 36 | -1 / 0 / -24 | 0 / 0 / 0 | -2 / 0 / -32 |
| `large_carry0` | 5 / 1 / 1,398,172 | -1 / -1 / -24 | 0 / 0 / 0 | -2 / -1 / -32 |
| `large_carry1` | 7 / 2 / 2,446,758 | -3 / -2 / -1,048,610 | -1 / -1 / -1,048,577 | -4 / -2 / -1,048,618 |
| `large_carry2` | 5 / 1 / 2,446,726 | -1 / -1 / -1,048,578 | -1 / -1 / -1,048,578 | -2 / -1 / -1,048,586 |
| `short_carry1` | 3 / 0 / 36 | -1 / 0 / -24 | 0 / 0 / 0 | -2 / 0 / -32 |
| `short_carry2` | 3 / 0 / 40 | -1 / 0 / -24 | 0 / 0 / 0 | -2 / 0 / -32 |
| `short_incomplete` | 7 / 0 / 100 | -5 / 0 / -88 | 0 / 0 / 0 | -6 / 0 / -96 |
| `small_carry0` | 2 / 0 / 428 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| `small_carry1` | 4 / 1 / 460 | -1 / -1 / -24 | 0 / 0 / 0 | -2 / -1 / -32 |
| `small_carry2` | 4 / 1 / 465 | -1 / -1 / -25 | 0 / 0 / 0 | -2 / -1 / -33 |
| `stream_aligned` | 65 / 0 / 87,840 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| `stream_carry` | 151 / 43 / 131,615 | -85 / -43 / -43,683 | -42 / -42 / -43,323 | -86 / -43 / -43,691 |
| `threshold_above` | 6 / 2 / 150 | -3 / -2 / -66 | -1 / -1 / -34 | -4 / -2 / -74 |
| `threshold_at` | 4 / 1 / 117 | -1 / -1 / -33 | -1 / -1 / -33 | -2 / -1 / -41 |
| `threshold_below` | 6 / 2 / 149 | -3 / -2 / -65 | 0 / 0 / 0 | -4 / -2 / -73 |

In the carried stream, no-cut reduced requests from 151 to 66 and
reallocations from 43 to zero. It retains a small carry buffer between writes;
this screen does not measure concurrent-stream memory. It also excludes frame
construction, transport, backpressure, TLS and RPC scheduling.
