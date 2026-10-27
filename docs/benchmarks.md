# Benchmarks

[Run benchmarks](benchmarks/running.md) · [Earlier results](benchmarks/grpc-20261024.md)
· [Qualification](scoreboard.md)

[Maps](#typed-map-iteration-october-27) · [RPC and nested messages](#server-scheduling-and-nested-messages-october-27)
· [Codec and codegen](#codec-and-codegen-comparisons)

The measurements below show lower allocation costs in some workloads and losses
in others. CPU results are mixed. There is no qualified overall performance ranking.

## Typed map iteration, October 27

Typed map views now project borrowed entries as they are read instead of
building a temporary vector. Iterator size hints use conservative bounds
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

The new transport controls also have a separate comparison with their defaults
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
