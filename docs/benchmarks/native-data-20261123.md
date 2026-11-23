# Native gRPC throughput and latency on November 23, 2026

The large unary workload sends a 271,828-byte request field and returns a
314,159-byte response field. Native HTTP/2 can now send each prefix and
shared field together, so an ordinary plaintext peer can read the complete
message from one DATA frame. [Copy counts and limits](../zero-copy.md#native-transport-pbrs-grpc)

## Native transport change

Two native endpoint processes ran on the same hosted Linux machine. We alternated
Rust 1.100.0 release binaries across eight pairs of 1.5-second samples,
with copy and allocation counters disabled. Each entry is the median change
within pairs. Lower latency and higher RPC/s are better.

| Workload | p50 | p99 | RPC/s |
| --- | ---: | ---: | ---: |
| Large unary | −28.1% | −34.8% | +9.1% |
| Large unary, 16 KiB flow-control windows | +0.02% | +11.5% | −1.4% |
| Empty unary | +0.3% | −4.6% | +0.3% |
| Small unary | +0.9% | −0.9% | −0.1% |
| Small unary, multiplexed | −1.1% | −0.6% | +0.9% |
| Server stream | +0.3% | −0.2% | −0.5% |

Large unary p99 fell in seven of eight pairs with the native defaults. With
16 KiB stream and connection windows, both builds reassembled the payload.
The change in p99 ranged from 64% lower to 112% higher across the eight
pairs. Multiplexed unary used 32 concurrent calls on one connection. Server
streaming returned 32 messages of 1 KiB each. The timed sources were
`05e96639` and `e766145f`.
[Per-pair samples, commands and host](https://github.com/mingley/pure-protobuf/actions/runs/45267810626/artifacts/14864343140)

## Standard TCP comparison with Tonic

The unchanged `rpc-bench smoke` compared native gRPC with Tonic 0.14.6 using
generated pbrs messages for both. It ran the starting build (`05e96639`),
the integrated build (`9f10505c`) twice and then the starting build again.
Each invocation recorded 82 results on the same Linux host. Both starting
runs failed the large-unary latency gate. Both integrated runs passed every
smoke gate.

| Large unary | Native p50 | Tonic p50 | Native p99 | Tonic p99 |
| --- | ---: | ---: | ---: | ---: |
| First integrated run | 216.0 µs | 335.2 µs | 487.1 µs | 488.4 µs |
| Second integrated run | 207.9 µs | 335.5 µs | 440.6 µs | 479.1 µs |

Native large-unary p50 fell 39.0% and p99 fell 24.0% against the starting
build. The first integrated run's p99 margin over Tonic was 1.3 µs.
Throughput below uses the median of matching repetitions. A ratio of 2×
means native completed twice as much work per second as Tonic. The QPS
rows recorded zero RPC errors in all four runs.

| Workload | Native change from starting build | Integrated native / Tonic |
| --- | ---: | ---: |
| Empty unary, concurrency 1, 1 connection | −0.1% | 3.97× |
| Empty unary, concurrency 16, 4 connections | +0.2% | 1.81× |
| Large unary, concurrency 1, 1 connection | +26.4% | 3.36× |
| Large unary, concurrency 16, 4 connections | +23.2% | 1.49× |
| Server stream, 1 KiB messages | −3.7% | 1.24× |
| Bidi ping-pong | +2.0% | 1.22× |
| Client upload, 1 KiB messages | −3.8% | 1.99× |

The full report includes all four process exits, gate messages and 328
benchmark records. To repeat the comparison locally, use the standard
smoke command at both revisions.
[All four reports](https://github.com/mingley/pure-protobuf/actions/runs/45268590411/artifacts/14865960189) ·
[Run the benchmark](running.md#grpc-client-and-server)

## Unary RPC over TLS

A separate Linux runner alternated the starting native build, integrated
native build and stock Tonic with Prost in six blocks. The client and
server ran in separate processes on pinned CPUs with one RPC in flight
over an established connection. All three used the same certificate,
Graviola provider, payload sizes and identity compression. The native
observer recorded TLS 1.3, AES-128-GCM and HTTP/2.

| Large unary, median across blocks | p50 | p99 | RPC/s |
| --- | ---: | ---: | ---: |
| Starting native | 590.3 µs | 708.6 µs | 1,637 |
| Integrated native | 474.2 µs | 683.6 µs | 1,937 |
| Tonic with Prost | 605.3 µs | 842.2 µs | 1,568 |

Across matching blocks, large native throughput rose 18.0% from the
starting build and was 20.5% higher than Tonic with Prost. Small native
unary p99 increased 1.1% and throughput fell 1.0% from the starting build.
All 288,000 measured calls succeeded. The readback verified all 36 captures
after correcting how the checker spelled the TLS version.
[Raw endpoint captures](https://github.com/mingley/pure-protobuf/actions/runs/45269398125/artifacts/14865696225) ·
[Verified counts, handshakes and samples](https://github.com/mingley/pure-protobuf/actions/runs/45271270299/job/138031381877)
