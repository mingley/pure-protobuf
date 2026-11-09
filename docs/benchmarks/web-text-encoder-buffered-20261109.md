# Buffered gRPC-Web text encoder holdout, November 9

The buffered-carry candidates reduced CPU and allocation requests in some
carried-segment cases. Direct and fast were slower in all five `short_carry1`
pairs. **None of the variants was merged.** This is an isolated encoder
screen, not an RPC benchmark or a comparison with tonic or Prost.

[Run 42702010056](https://github.com/mingley/pure-protobuf/actions/runs/42702010056)
passed its checks and deleted its scratch branch. Its
[artifact](https://github.com/mingley/pure-protobuf/actions/runs/42702010056/artifacts/13712553011)
has SHA-256
`44423a960ee1a9e3e721f5bdded3f9484e09b2db88e21d12f243376afdd657c0`
and expires November 23, 2026. The
[compact comparison records](web-text-encoder-buffered-20261109.json)
are reconstructed from the two validated job-log records; they are not the
complete raw samples, binaries, or disassembly in the artifact.

## Compared sources

The parent is commit
[`4037304c`](https://github.com/mingley/pure-protobuf/commit/4037304c7730588e0e3c12a25ee77a4f7c84c0a1),
with production `frame.rs` blob `d23508bb5f843f238387b9a433778aa52934de30`.
Scratch commit
[`b439a83a`](https://github.com/mingley/pure-protobuf/commit/b439a83adbb77690a2483eb88ece0e1e35155103)
contains four fixtures with the same added tests:

| Variant | Fixture Git blob | Change from baseline |
|---|---|---|
| Baseline | `33233fa6f7560d640ef6b5989e3f7995f8df14bd` | Existing encoder plus tests |
| Chunked | `dc292c1e52c08e765c6730054252768707520147` | Fixed array carry and two base64 `encode_string` calls |
| Direct | `627c4257392e10d490ea4272ead53c3bac23c320` | Keeps `BytesMut` carry; for a carried segment of at least 32 bytes, encodes a stack quantum and tail into one output |
| Fast | `34f998ee4f56b84db62520f43ea44a40ff4924e3` | Direct variant with a `#[cold]` carried helper |

Direct and fast preserve the baseline's field layout and no-carry source
branch. Different code layout can still affect those controls. The probe is
blob `56a0ed61f88a163c6e278ece96df81d8994be4f0`, the driver is
`a7f1fdba66aa86e43f2efe202a67b14e5c7f2661`, the public integration tests
are `98eec8e64540353f3b893f8f9eae9e92d37f84cc`, and the workflow is
`e7402731df45a0ab0c9d32e5221a33f6bf48cfd9`.

The hosted Ubuntu 24.04 x86-64 runner used Rust 1.99.1 and LLVM 23.1.1
for release screens and Rust 1.85.0 for correctness tests. The artifact
records compiler and `protoc 3.21.12` hashes, lockfile, feature tree, source,
binary hashes, individual captures, and available disassembly.

## What was measured

Each operation creates a `TextEncoder`, feeds fixed prebuilt `Bytes`
segments, and immediately drops each output. Inputs are promoted before
measurement and cloned inside the operation. This excludes frame construction,
HTTP/2, network, TLS, RPC scheduling, backpressure and retained output.
`black_box` is a best-effort optimization barrier.

The short cases use `[1, 2]` and `[2, 2]`; the small cases use
`[297, 23]`, `[298, 23]` and `[299, 23]`. Threshold cases use
`[1, 31, 23]`, `[1, 32, 23]` and `[1, 33, 23]`. The large cases
use `[6, 1048576, 23]`, `[4, 1048576, 23]` and `[5, 1048576, 23]`
in carry-0/1/2 order. In `large_carry0`, the large segment starts with no
carry but leaves one byte for the final segment. Streaming cases use 64
segments of 1,029 or 1,030 bytes followed by 23 bytes.

Each test process checked accumulated base64 bytes against `STANDARD`, plus
carry and output presence after every segment, before measurement. The measured
loop checked total output length, not every output byte on every iteration.
The fixture fingerprints identify inputs and outputs; they are not a
cryptographic equality proof.

Each variant passed four focused encoder tests and one deadline-carry test on
Rust 1.85. The focused tests include the 31/32/33 cutover and a deadline after
a large carried segment. Two deliberate mutants, corrupting a carried quantum
and losing deadline carry, failed their targeted tests as expected. The fast
variant separately passed all 26 tests across three public gRPC-Web suites,
including streaming across the cutover. Stable formatting and Clippy also
passed. These checks cover the stated paths, not complete production readiness.

## Allocation requests

The table reports **allocation calls / reallocations / requested bytes** per
sequence from `(2N − N) / N`. Calls include successful reallocations, whose
full new size is counted. The process-wide allocator wrapper can affect
optimization and include other threads; requested bytes are neither live nor
retained memory. Three repeated N/2N samples agreed. Direct and fast had
identical allocation observations in these cases.

| Sequence | Baseline | Chunked | Direct and fast |
|---|---:|---:|---:|
| `short_carry1` | 3 / 0 / 36 | 1 / 0 / 4 | 3 / 0 / 36 |
| `short_carry2` | 3 / 0 / 40 | 1 / 0 / 8 | 3 / 0 / 40 |
| `small_carry0` | 2 / 0 / 428 | 2 / 0 / 428 | 2 / 0 / 428 |
| `small_carry1` | 4 / 1 / 460 | 2 / 0 / 428 | 4 / 1 / 460 |
| `small_carry2` | 4 / 1 / 465 | 2 / 0 / 432 | 4 / 1 / 465 |
| `threshold_below` | 6 / 2 / 149 | 2 / 0 / 76 | 6 / 2 / 149 |
| `threshold_at` | 4 / 1 / 117 | 2 / 0 / 76 | 3 / 0 / 84 |
| `threshold_above` | 6 / 2 / 150 | 2 / 0 / 76 | 5 / 1 / 116 |
| `large_carry0` | 5 / 1 / 1,398,172 | 3 / 0 / 1,398,140 | 5 / 1 / 1,398,172 |
| `large_carry1` | 7 / 2 / 2,446,758 | 3 / 0 / 1,398,140 | 6 / 1 / 1,398,181 |
| `large_carry2` | 5 / 1 / 2,446,726 | 3 / 0 / 1,398,140 | 4 / 0 / 1,398,148 |
| `stream_aligned` | 65 / 0 / 87,840 | 65 / 0 / 87,840 | 65 / 0 / 87,840 |
| `stream_carry` | 151 / 43 / 131,615 | 65 / 0 / 87,924 | 109 / 1 / 88,292 |

## Counter-free CPU screen

Cells show the median candidate / baseline process-CPU ratio and the
minimum–maximum of five observed pairs. ↓ means every pair was below one,
↑ means every pair was above one, and ~ means the range crossed one.
Smaller is faster. Values are rounded to four decimals; classifications use
the unrounded values. These ranges describe this run, not confidence intervals.

| Sequence | Chunked | Direct | Fast |
|---|---:|---:|---:|
| `short_carry1` | 0.7178 (0.7151–0.7213) ↓ | 1.0062 (1.0012–1.0309) ↑ | 1.0071 (1.0004–1.0112) ↑ |
| `short_carry2` | 0.7591 (0.7467–0.7603) ↓ | 1.0037 (0.9722–1.0095) ~ | 1.0054 (0.9895–1.0135) ~ |
| `small_carry0` | 0.9970 (0.9921–1.0168) ~ | 0.9969 (0.9877–1.0000) ~ | 1.0031 (0.9993–1.0068) ~ |
| `small_carry1` | 0.7771 (0.7741–0.7855) ↓ | 1.0084 (0.9920–1.0224) ~ | 1.0058 (0.9536–1.0199) ~ |
| `small_carry2` | 0.7015 (0.6928–0.7050) ↓ | 0.9977 (0.9926–1.0052) ~ | 0.9966 (0.9837–1.0853) ~ |
| `threshold_below` | 0.6462 (0.6448–0.6526) ↓ | 1.0027 (0.9989–1.0314) ~ | 1.0040 (1.0010–1.0336) ↑ |
| `threshold_at` | 0.7853 (0.7794–0.8034) ↓ | 0.7977 (0.7932–0.8049) ↓ | 0.7896 (0.7457–0.7988) ↓ |
| `threshold_above` | 0.7164 (0.6151–0.7235) ↓ | 0.8280 (0.8239–0.8463) ↓ | 0.8184 (0.8140–0.8415) ↓ |
| `large_carry0` | 0.9995 (0.8132–1.0149) ~ | 0.9978 (0.8232–1.0365) ~ | 0.9974 (0.8189–1.0244) ~ |
| `large_carry1` | 0.8309 (0.7869–0.8529) ↓ | 0.7880 (0.7038–0.8084) ↓ | 0.7839 (0.7448–0.8940) ↓ |
| `large_carry2` | 0.8319 (0.8272–0.8483) ↓ | 0.7802 (0.7693–0.7940) ↓ | 0.7768 (0.7687–0.7923) ↓ |
| `stream_aligned` | 1.0120 (0.9906–1.0184) ~ | 0.9967 (0.9612–1.0136) ~ | 1.0032 (0.9950–1.0192) ~ |
| `stream_carry` | 0.9796 (0.9714–0.9898) ↓ | 0.8918 (0.8914–0.9210) ↓ | 0.8975 (0.8932–0.9073) ↓ |

Direct and fast show five lower ratios for the carried 32/33-byte, large
and streaming cases. Both show five higher ratios for `short_carry1`, and
fast also does so for `threshold_below`. Direct's `threshold_below` range
crosses one. The `small_carry0`, `large_carry0` and aligned-stream
controls are inconclusive for every candidate. Chunked wins every observed
pair for the ten carried cases but its three no-carry controls are
inconclusive.

Timing binaries lacked the counting-allocator marker and the feature graph
did not enable `copy-counts`. Each sample ran in a separate process on one
allowed CPU; variant and N/2N orders alternated. A CPU delta below five
milliseconds would be marked inconclusive. Process CPU can include other test
threads; affinity does not control shared-host load or frequency. This screen
does not establish absence of a regression, RPC improvement, production
qualification, or a global ranking.
