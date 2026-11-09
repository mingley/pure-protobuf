# gRPC-Web text encoder screen, November 9

Three proposed encoder variants cut allocation requests on several carried
segment cases. Each also showed slower CPU samples on at least one control.
**None of these variants was merged.** The screen tests a synchronous encoder,
not an HTTP/2 call or a comparison with tonic or Prost.

[Run 42689169987](https://github.com/mingley/pure-protobuf/actions/runs/42689169987)
passed its checks and deleted its scratch branch. Its
[artifact](https://github.com/mingley/pure-protobuf/actions/runs/42689169987/artifacts/13707372718)
has SHA-256
`c7c2327b19b39d86227f143332450810d6a1f22f2468804b87ebd5b052a3b373`
and expires November 23, 2026. The
[compact comparison records](web-text-encoder-20261109.json) are retained in
the repository. They are reconstructed from the validated job log; they are
not the complete raw samples, binaries, or disassembly in the artifact.

## Compared sources

The baseline is commit
[`4a2d8e26`](https://github.com/mingley/pure-protobuf/commit/4a2d8e26134a41eaf71e449ab29f5f50890933c1),
with production `frame.rs` blob `d23508bb5f843f238387b9a433778aa52934de30`.
Scratch commit `f924f2f6512ff6d7270582c7200c148da56500f4` contains the
source-pinned fixtures, which differ in the private encoder and share the same
added tests:

| Variant | Fixture Git blob | Change from baseline |
|---|---|---|
| Baseline | `1b8d0298a0a249051785a16d69fd803c99aaef22` | Existing encoder with the same new tests |
| Chunked | `27aee798f0deedb33c626131709f383042140c3e` | Stack carry and base64 `encode_string` |
| Direct | `b120dfbb309f5edef7015adc5703e7f2268f1b2f` | Stack carry and direct `encode_slice` into an output buffer |
| Fast | `df909925a1d2c8acc89304cc7afc5054d5c94cb7` | Direct variant that skips resetting an empty carry |

The hosted Ubuntu 24.04 x86-64 runner used Rust 1.99.1 and LLVM 23.1.1 for
the release screens. Rust 1.85.0 ran the correctness tests. The recorded
compiler and system `protoc 3.21.12` hashes, lockfile, feature tree,
fixtures, binaries, and disassembly are in the artifact.

## What was measured

Each operation creates a `TextEncoder`, feeds a fixed sequence of prebuilt
`Bytes` segments, and immediately drops each result. Inputs are cloned
inside the operation and were promoted before measurement. This is a
synchronous encoder lifecycle; it excludes frame construction, network,
HTTP/2 backpressure, transport scheduling, RPCs, and retained output.
`black_box` is a best-effort optimization barrier.

The short sequences use segment lengths `[1, 2]` or `[2, 2]`; the small
ones use `[297, 23]`, `[298, 23]`, or `[299, 23]`. Large cases use
`[6, 1048576, 23]`, `[4, 1048576, 23]`, or `[5, 1048576, 23]`, in
carry-0/1/2 order. In `large_carry0` the large segment starts without carry,
but it leaves one byte for the final segment. Streaming cases use 64
segments of 1,029 or 1,030 bytes followed by 23 bytes.

Every test process checked the complete accumulated base64 bytes against
`STANDARD` and checked carry and output presence after each segment before
measurement. The measured loop checked its total output length; it did not
compare every output byte on every iteration. FNV values identify fixtures,
not cryptographic proofs of equality. Separate Rust 1.85 unit tests covered
small boundaries, empty final flushes, large segments, and deadline carry for
all four variants. The public gRPC-Web integration suites ran on the fast
variant separately.

## Allocation requests

The table reports **allocation calls / reallocations / requested bytes** per
sequence from `(2N − N) / N`. Allocation calls include successful
reallocations, whose full new size is counted. The process-wide System allocator wrapper can affect optimization, and its
counters can include other threads. Requested bytes are neither live nor retained memory.
Three repeated N/2N samples agreed. All three alternatives had the same
allocation observations in these cases.

| Sequence | Baseline | Each alternative |
|---|---:|---:|
| `short_carry1` | 3 / 0 / 36 | 1 / 0 / 4 |
| `short_carry2` | 3 / 0 / 40 | 1 / 0 / 8 |
| `small_carry0` | 2 / 0 / 428 | 2 / 0 / 428 |
| `small_carry1` | 4 / 1 / 460 | 2 / 0 / 428 |
| `small_carry2` | 4 / 1 / 465 | 2 / 0 / 432 |
| `large_carry0` | 5 / 1 / 1,398,172 | 3 / 0 / 1,398,140 |
| `large_carry1` | 7 / 2 / 2,446,758 | 3 / 0 / 1,398,140 |
| `large_carry2` | 5 / 1 / 2,446,726 | 3 / 0 / 1,398,140 |
| `stream_aligned` | 65 / 0 / 87,840 | 65 / 0 / 87,840 |
| `stream_carry` | 151 / 43 / 131,615 | 65 / 0 / 87,924 |

## Counter-free CPU screen

The following are candidate / baseline process-CPU ratios from five paired
N/2N samples. Cells show the median and the minimum–maximum observed pair:
↓ means all five ratios were below 1, ↑ means all five were above 1, and ~
means the range crossed 1. These ranges describe this run, not confidence
intervals or a guarantee. Smaller is faster.

| Sequence | Chunked | Direct | Fast |
|---|---:|---:|---:|
| `short_carry1` | 0.969 (0.915–0.985) ↓ | 0.734 (0.670–0.746) ↓ | 0.741 (0.690–0.772) ↓ |
| `short_carry2` | 1.030 (0.842–1.080) ~ | 0.678 (0.596–0.741) ↓ | 0.712 (0.568–0.727) ↓ |
| `small_carry0` | 0.975 (0.881–0.990) ↓ | 0.989 (0.952–1.007) ~ | 0.985 (0.958–1.002) ~ |
| `small_carry1` | 0.923 (0.909–0.960) ↓ | 0.916 (0.897–0.925) ↓ | 0.910 (0.901–0.919) ↓ |
| `small_carry2` | 0.873 (0.725–0.882) ↓ | 0.870 (0.723–0.871) ↓ | 0.865 (0.716–0.952) ↓ |
| `large_carry0` | 1.047 (1.034–1.064) ↑ | 1.056 (1.029–1.067) ↑ | 1.056 (1.040–1.062) ↑ |
| `large_carry1` | 1.018 (1.014–1.030) ↑ | 0.909 (0.895–0.910) ↓ | 0.905 (0.897–0.920) ↓ |
| `large_carry2` | 1.018 (1.014–1.026) ↑ | 0.911 (0.899–0.925) ↓ | 0.908 (0.907–0.923) ↓ |
| `stream_aligned` | 1.046 (0.964–1.049) ~ | 1.037 (1.007–1.048) ↑ | 1.035 (0.970–1.039) ~ |
| `stream_carry` | 1.018 (1.015–1.089) ↑ | 0.922 (0.910–0.983) ↓ | 0.923 (0.919–0.988) ↓ |

The fast and direct variants improved the observed carried large and streaming
cases, but all five `large_carry0` pairs favored the baseline. The direct
variant also lost all five aligned-stream pairs; the fast variant's aligned
range crossed 1. This warrants further controls before using the change.

The timing binaries excluded the counting allocator marker and the feature
graph did not enable `copy-counts`. Each sample was a separate test process
on one allowed CPU. The process CPU clock can include work by other test
threads, and affinity does not control the shared host or CPU frequency.
N/2N and variant order alternated; a CPU delta below five milliseconds would
be marked inconclusive. This screen does not establish no regression, an RPC
improvement, production qualification, or a global performance ranking.
