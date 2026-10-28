# Benchmarks

[Run benchmarks](benchmarks/running.md) · [Earlier results](benchmarks/grpc-20261024.md)
· [Qualification](scoreboard.md)

[Latest results](#latest-runtime-and-generator-results-october-27)
· [Earlier maps](#typed-map-iteration-october-27)
· [RPC and nested messages](#server-scheduling-and-nested-messages-october-27)
· [Codec and codegen](#codec-and-codegen-comparisons)

The measurements below show lower allocation costs in some workloads and losses
in others. CPU results are mixed. There is no qualified overall performance ranking.

## Latest runtime and generator results, October 27

The [complete record](evidence/decode-and-map-speed-20261027/README.md) compares
`71422f00` with `0287879d`. Regenerated singular lazy children reuse validation,
short map entries avoid backing-buffer work, and an index that proves every
map key unique lets iteration walk the borrowed entries directly.

| Parse, read and drop | Before | After | Prost | Median paired after/Prost |
|---|---:|---:|---:|---:|
| Lazy depth 64 | 28,300 ns | 3,968 ns | 1,657 ns | 2.339 |
| Owned depth 64 | 1,507 ns | 1,502 ns | 1,583 ns | 0.946 |
| Lazy mixed message | 864 ns | 802 ns | 895 ns | 0.909 |
| Owned mixed message | 855 ns | 837 ns | 845 ns | 0.991 |

Large unique-map iteration falls from 1,943 to 94 ns at 256 entries, versus
253 ns for the standard HashMap control: a paired after/before ratio of 0.049
and after/reference ratio of 0.368. That is warm iteration; parsing and index
construction are outside this microbenchmark. Only three of 42 map rows beat
the HashMap median. Small and duplicate-heavy maps remain losses.

The owned profile has lower CPU than Prost in 15 of 16 parse/read rows;
depth-64 parsing is near parity at 1.004. All 16 encoding holdout rows favor
pbrs. Lazy depth-64 full reads and the depth-eight partial read still lose.
Five randomized pairs retain every sample, with separate CPU and allocation
builds. These results do not meet the goal of winning every benchmark.

Generator timings use a corrected completion timer. The previous polling
wait introduced millisecond plateaus, so those older wall times cannot support
a speed claim. In the corrected comparison, all six canonical debug generation
rows favor pbrs, while four of six matched release rows do. Small release
fresh/unchanged generation still loses slightly. The 1,000-message release
fresh-generation paired ratio is 0.798; its output is 36.31 MB versus Prost's
0.413 MB. CLI time does not establish downstream build time or output-size
parity. The [record](evidence/decode-and-map-speed-20261027/README.md#correcting-generator-timing)
keeps both profiles, output volume and the timer regression controls.

The compressed-only transport follow-up in `6d1336e0` measures allocator
traffic with the same Prost codec and fresh tonic controls. These are two
paired N/2N repeats; requested MB per RPC means 1,000,000 bytes, not RSS:

| Work / native endpoint | Before | After | Tonic | Paired after/before | Paired after/tonic |
|---|---:|---:|---:|---:|---:|
| Four × 1 MiB gzip noise, bidi client | 30.864 | 22.288 | 22.774 | 0.722 | 0.979 |
| Four × 1 MiB gzip noise, bidi server | 29.805 | 21.229 | 21.720 | 0.712 | 0.977 |
| 1 MiB gzip noise, unary client | 12.763 | 10.666 | 10.971 | 0.836 | 0.972 |
| Four × 1 MiB gzip noise, upload client | 16.260 | 16.072 | 12.151 | 0.988 | 1.323 |
| Four × 1 MiB gzip noise, download server | 23.360 | 21.075 | 18.557 | 0.902 | 1.136 |
| 8 MiB identity, unary client | 66.133 | 66.133 | 44.129 | 1.000 | 1.499 |

Compressed bidi and unary requested bytes fall below tonic locally. Upload
client, download server and large identity still use more. Zero-filled bidi
requested bytes are essentially unchanged. Allocation counts and CPU are
separate metrics; these savings do not establish a CPU win.

A broader reservation experiment cut large-identity bytes by 25.4% but its
longer check showed about 50% higher client CPU and 56% higher server CPU.
That identity reservation was rejected. The final source keeps the earlier
identity growth policy and was built and measured separately; the rejected
captures remain in the complete record.

The original `71422f00` framer adjustment alone did not reduce RPC allocation
traffic. The [resource guide](resource-budgets.md#2-buffer-ownership) explains
the shared retention ledger, unchanged capacity limits and compressed-only
scope of the later input hint.

The caller-worker and small-map-cache prototypes were rejected after mixed
results and regressions. Their full campaigns remain alongside the selected
source. Read older tables below at the revisions they name.

## Typed map iteration, October 27

This earlier comparison describes the `0287879d` delivery. Typed map views
project borrowed entries as they are read instead of building a temporary
vector. Iterator size hints use conservative bounds
instead of rescanning duplicate keys. Small-map lookups compare borrowed keys
without cloning them.

The map-only comparison against `77f361eb` uses five randomized process pairs
per row. These rows time already-built maps; parsing, map construction and
index construction are outside the measured loop. The control is a standard
`HashMap`, the map representation Prost uses. Median CPU falls in all 42
direct iteration rows; collecting a map into a vector uses 11.1–44.2% less CPU. Iterating every distinct entry gives:

| Map entries | Before | After | Standard HashMap | Median paired CPU change |
|---|---:|---:|---:|---:|
| 4 | 27.4 ns | 8.9 ns | 6.2 ns | −67.7% |
| 64 | 525.5 ns | 345.1 ns | 64.5 ns | −34.5% |
| 256 | 2,441.6 ns | 1,919.6 ns | 245.1 ns | −21.7% |

These gains leave substantial losses to the `HashMap` control, especially on
larger maps. Full-message reads also show why a direct iteration win does not establish
an overall codec win:

| Mixed-message parse, read and drop | Before | After | Median paired CPU change |
|---|---:|---:|---:|
| Default lazy fields | 914.5 ns | 870.2 ns | −3.62% |
| Owned message fields | 917.3 ns | 955.6 ns | +5.85% |

Both profiles remove one allocation and 128 requested bytes per mixed-message
read. Lazy full reads are near Prost locally, with a paired CPU ratio of
1.006. Owned full reads cost 1.128 times Prost's CPU. Owned mixed parsing alone
regresses 16.3%, and sparse-message full reads regress about 20% in both
profiles. CPU and allocation measurements use separate binaries.

The [complete record](evidence/parity-and-maps-20261027/README.md) retains all
32 codec rows, all 42 map rows, every process pair and the
intermediate map implementation's collection regressions. These shared-host
results guide the next optimization; they do not establish a ranking.

## Default transport costs, October 27

This earlier transport comparison describes `12cfd09f`. The new controls also have a separate comparison with their defaults
unchanged, using Prost on both endpoints. All 120 captures complete:
1,890,000 successful RPCs across 1 KiB unary and 64 × 1 KiB pipeline calls.
Five before/after pairs include fresh Tonic controls. Every native endpoint's
before/after CPU ratio spans one across the repeats, so this run establishes
neither a CPU improvement nor zero overhead from the added controls. The
[complete record](evidence/parity-and-maps-20261027/README.md) retains all
repeats, including substantial control variation.

Against the fresh tonic control, the candidate unary client uses 19.5 versus
65.5 µs/RPC and the server uses 25.5 versus 31.0 µs/RPC. Median paired ratios
are 0.294 and 0.823; all five unary pairs favor native on both endpoints.
These are scoped endpoint-CPU results under the stated closed-loop settings,
not an overall implementation ranking.

## Server scheduling and nested messages, October 27

The [earlier server and nested-message fixes](evidence/performance-fixes-20261027/README.md)
follow the [direct parity audit](parity.md). Coalescing connection wakes lowers
native-server CPU in five local before/after pairs: unary 1 KiB medians move
from 45 to 23.5 µs/RPC, a median paired reduction of 54.8%. A 64 × 1 KiB
pipeline moves from 270 to 200 µs/RPC, a paired reduction of 28.0%. All
1,890,000 RPCs complete without terminal errors. Fresh tonic controls and
client results, including inconclusive comparisons, remain in the record.

An explicit [owned-message codegen profile](guides/codegen.md#owned-nested-message-fields)
constructs nested messages during parsing and avoids repeated validation when
reading their fields. The default lazy profile preserves its advantages for
partial reads and forwarding unchanged messages. The complete record compares
full reads, partial reads and encoding before recommending either profile.

| Parse, read and drop | Default lazy | Owned | Prost | Paired owned/prost CPU |
|---|---:|---:|---:|---:|
| 8-level message | 660 ns | 130 ns | 144 ns | 0.902 |
| 64-level message | 29,925 ns | 1,523 ns | 1,594 ns | 0.963 |
| Mixed RPC-shaped message | 908 ns | 906 ns | 860 ns | 1.053 |

The deep-64 owned read is about 19.6 times faster than the lazy read locally,
while the mixed-message read loses to Prost at that source pin. Owned deep-64
parsing alone costs more than lazy parsing, because it constructs the tree
immediately.

Encoding favors lazy storage in these fixtures. At depth 64, owned first
encode costs 567 ns versus lazy's 26 ns and Prost's 6,831 ns. Cached encode
costs 329 ns versus 30 ns and 6,750 ns. The
[complete record](evidence/performance-fixes-20261027/README.md#partial-reads-and-encoding)
includes partial reads, first-child mutation, lifecycle costs and allocations.

These are shared-host diagnostics, not a qualified ranking. The
[original audit](evidence/parity-proof-20261027/README.md) retains the earlier
16 codec rows, including the lazy depth-64 read-all loss of 16.92× prost's CPU.
That result still describes the default layout at its source pin. The codegen
audit checks compatibility; it does not establish a generator-speed ranking.

## Integrated changes, October 27

On a shared Linux x86_64 host with Rust 1.99.1, compare `a24c0e35` with the
previous `1d773f97` implementation. Both endpoints use Prost. Allocation rows
are median paired changes across two N/2N runs; they measure requested bytes,
not resident memory. Each stream has 64 messages of 1 KiB, one connection and
one call in flight.

| Identity bidi pipeline | Before, MiB/RPC | After, MiB/RPC | Paired change |
|---|---:|---:|---:|
| Native client | 0.344 | 0.326 | −5.20% |
| Native server | 0.317 | 0.328 | +3.79% |

The change is the median of each paired change, not the ratio of the two
displayed medians. Repeats range from −11.58% to +1.18% on the client and
−2.27% to +9.86% on the server. A separate counters-off run of 1 KiB unary
calls gives native-server CPU medians of 46 and 38 µs/RPC, versus 30 µs for
the fresh tonic/prost control.
The shared host and coarse CPU sampling limit timing conclusions. The
[dated source and raw captures](evidence/grpc-parity-and-costs-20261027/README.md)
include all repeats, controls and smaller isolated measurements.

## Compressed streams, October 26

These results describe `1d773f97` versus `e7595eb7`, on a shared Linux x86_64
host with Rust 1.99.1. Both endpoints use Prost unless stated otherwise. The
[full record](evidence/grpc-encode-reuse-20261026/README.md) contains its own
source and raw captures.

### Allocation bytes

Requested MiB per RPC, lower is better. These are allocator requests, not RSS.
Four-message streams use 1 MiB bodies, gzip level 6, one connection and one RPC
in flight. Values are medians from two repeated N/2N comparisons.

| Native endpoint | Payload | Before | After | Paired change |
|---|---|---:|---:|---:|
| Bidi client | Zero | 19.405 | 14.421 | −25.69% |
| Bidi server | Zero | 18.395 | 13.411 | −27.10% |
| Bidi client | Noise | 38.197 | 29.435 | −22.94% |
| Bidi server | Noise | 37.187 | 28.425 | −23.56% |
| Upload client | Zero | 11.766 | 7.277 | −38.15% |
| Server-streaming server | Zero | 14.773 | 10.285 | −30.38% |

Against fresh tonic/prost controls, zero-filled bidi uses 13.50% fewer client
bytes and 14.39% fewer server bytes. Noise bidi still uses **35.53% more client
bytes and 37.23% more server bytes**. Identity pipelined client bytes for
64 × 1 KiB rise 2.98% versus baseline. All controls and repeated values remain
in the complete record: 448 captures and 83,160 successful RPCs.

### CPU

A separate build with counters and profilers disabled completes 180 captures
and 272,160 successful RPCs. Three repeats show mixed CPU changes. In 1 MiB
noise unary calls, the paired median CPU change is +6.90% on the native client
and +16.00% on the native server; the server's individual repeats span −7.14%
to +22.22%. Allocation reductions do not establish a speed improvement.

For 1 KiB identity unary, native-server CPU slopes range from 30–56 µs/RPC,
versus 24–30 µs for fresh tonic controls. The host and coarse CPU sampling do
not support a universal CPU or latency claim. See the
[complete CPU results](evidence/grpc-encode-reuse-20261026/README.md).

## Codec and codegen comparisons

The [latest component follow-up](evidence/efficiency-20261027/README.md) removes
MiniTable length-calculation allocations and lowers generator allocation
requests by 7.60–13.17% in the measured multi-file cases. Generator timing is
mixed, and some parallel cases use more peak live memory. These component
results do not update the endpoint tables above.

The [codec survey](../tonic-bench/README.md) separates parsing from reading
every field, and owned messages from borrowed views. The
[codegen matrix](evidence/sb09-codegen-matrix.md) records generator time,
generated output size and downstream build costs, including pbrs losses.
Use the [benchmark commands](benchmarks/running.md) to run either harness.

A later [MiniTable compatibility repair](evidence/minitable-groups-20261027/README.md)
preserves groups and unknown fields. Its 100-fragment non-group fixture costs
about 5% more time in a local diagnostic; group-body timing was not measured.

The [earlier gRPC tables](benchmarks/grpc-20261024.md) retain instruction
counts and previous CPU, allocation and bulk results at their original pins.
The [benchmark contract](benchmark-contract.md) defines the stronger evidence
needed for a performance claim.

[Compatibility checks](evidence/grpc-compatibility-fallback-20261026/README.md)
cover trait objects, native MSRV profiles and consumer gates. Their earlier
encode experiment retains its own source and measurement scope.
