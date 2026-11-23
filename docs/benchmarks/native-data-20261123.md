# Native gRPC throughput and latency on November 23, 2026

The large unary workload sends a 271,828-byte request field and returns a
314,159-byte response field. Native HTTP/2 can now send each prefix and
shared field together, so an ordinary plaintext peer can read the complete
message from one DATA frame. [Copy counts and limits](../zero-copy.md#native-transport-pbrs-grpc)

## Native transport change

Two native endpoint processes ran on the same hosted Linux machine. We alternated
Rust 1.100.0 release binaries across eight pairs of 1.5-second samples,
with copy and allocation counters disabled. Each entry is the median change
within pairs. The load server used the harness's 400 KiB send buffer.
Lower latency and higher RPC/s are better.

| Workload | p50 | p99 | RPC/s |
| --- | ---: | ---: | ---: |
| Large unary | −28.1% | −34.8% | +9.1% |
| Large unary, 16 KiB flow-control windows | +0.02% | +11.5% | −1.4% |
| Empty unary | +0.3% | −4.6% | +0.3% |
| Small unary | +0.9% | −0.9% | −0.1% |
| Small unary, multiplexed | −1.1% | −0.6% | +0.9% |
| Server stream | +0.3% | −0.2% | −0.5% |

Large unary p99 fell in seven of eight pairs with the default flow-control windows. With
16 KiB stream and connection windows, both builds reassembled the payload.
The change in p99 ranged from 64% lower to 112% higher across the eight
pairs. Multiplexed unary used 32 concurrent calls on one connection. Server
streaming returned 32 messages of 1 KiB each. The timed sources were
`05e96639` and `e766145f`.
[Per-pair samples, commands and host](https://github.com/mingley/pure-protobuf/actions/runs/45267810626/artifacts/14864343140)

A longer run compared the integrated build (`9f10505c`) with the same
starting build in eight pairs of four-second intervals. With default windows,
large-unary p50 fell 30.4%, p99 fell 27.3%, and throughput rose 12.2%.
With 16 KiB windows, p99 fell 2.1% and throughput rose 0.5%. An additional
check that sent the segments separately for small advertised windows changed
throughput by less than 0.2% in that configuration.
[Longer run and window experiment](https://github.com/mingley/pure-protobuf/actions/runs/45270189078/artifacts/14864798672)

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

The CSV preserves all 328 records with throughput units and latency in
nanoseconds. Streaming rows count messages or round trips. The archives
include gate messages and the original reports. To repeat locally, run the
standard smoke command at both revisions.
[Download the results as CSV](native-data-20261123-smoke.csv) ·
[All four reports](https://github.com/mingley/pure-protobuf/actions/runs/45268590411/artifacts/14865960189) ·
[Run the benchmark](running.md#grpc-client-and-server)

On another hosted runner, the integrated production code (`0c999ebe`)
missed the standard server-download gate in both invocations and the
large-unary latency gate in one. The gates can change outcome across hosts.
[Independent standard reports](https://github.com/mingley/pure-protobuf/actions/runs/45275243740/artifacts/14867262990)

A separate two-process run used 2,000 messages of 1 KiB per RPC and eight
paired five-second samples after warm-up. Download throughput changed by
+0.3% and upload by −1.0% against the starting build. Both streaming p99
changes were within 0.4%. All 101,579 measured streams completed.
[Per-pair streaming samples](https://github.com/mingley/pure-protobuf/actions/runs/45271655133/artifacts/14866730570)

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

### Tail latency at a fixed arrival rate

A separate open-loop run scheduled large TLS RPCs at two fixed rates, with
one connection and at most 16 calls in flight. The latency below starts at
the scheduled arrival and ends when the call completes. It includes timer
and task scheduling delay. All 205,200 offered calls started and finished
successfully across the 36 measured intervals.

| Offered RPC/s | Starting native p99 | Integrated native p99 | Tonic with Prost p99 |
| --- | ---: | ---: | ---: |
| 650 | 2.943 ms | 2.801 ms | 2.979 ms |
| 1,250 | 2.892 ms | 2.738 ms | 3.020 ms |

Each value is the median of six intervals. At 1,250 RPC/s the integrated
native p99 was lower than Tonic with Prost in all six.
[Fixed-rate samples, arrival counts and scheduling delay](https://github.com/mingley/pure-protobuf/actions/runs/45272403969/artifacts/14865529049)

## Streaming encode buffer

A streaming encoder can keep its batch buffer in place across successful
messages and take ownership only during rollback. A pinned AMD EPYC 9V74
core compared `0c999ebe` and `f8449549` with Rust 1.100.0. Each trial
encoded 3,145,728 generated messages and exposed the completed frames.
The bytes matched an independent gRPC and protobuf reference.

| Workload | Starting ns/message | Candidate ns/message | Median paired change |
| --- | ---: | ---: | ---: |
| Batches of 32 one-KiB messages | 72.3 | 48.5 | −32.9% |
| One 32-byte message per flush | 67.3 | 46.3 | −31.0% |
| One shared 64 KiB field per flush | 117.4 | 99.3 | −15.4% |

All three workloads took less time in all 12 pairs. These times cover
encoding and taking the output frame, before any HTTP/2 writes.
[Fixture, byte checks and per-pair measurements](https://github.com/mingley/pure-protobuf/actions/runs/45279264965/artifacts/14869220434)

The same release binaries then ran separate client and server processes
with 2,000 messages of 1 KiB per RPC. Across eight paired five-second
intervals, download throughput rose 1.6% and p99 fell 1.1% in all eight.
Upload throughput rose 12.6% at the median and was higher in six pairs.
Upload client CPU per RPC fell 12.5%, while upload p99 rose 1.5%.

In the standard smoke harness on this runner, native download and upload
throughput rose 3.7% and 3.5%. Both revisions missed the download and
large-unary latency gates in both runs. The median paired change in native
large-unary p99 was +4.9%. All reported QPS error counts were zero.
[Streaming samples and all four stock reports](https://github.com/mingley/pure-protobuf/actions/runs/45277681826/artifacts/14867934867)

## Two-MiB field experiment

Two native peers advertised 4 MiB DATA frames and used 4 MiB send buffers.
Each RPC carried a 2 MiB field in both directions. An experimental encoder
eliminated 2,097,167 bytes of request reassembly and 2,097,162 bytes of
response reassembly per call. The sender still copied the 20-byte request
prefix and the 15-byte response prefix.
[Copy counts and smaller-peer controls](https://github.com/mingley/pure-protobuf/actions/runs/45274932837/artifacts/14867332141)

The first implementation cut p99 by 14.1% and 11.4% on two runners, while
throughput fell 2.6% and 2.9%. Moving the rare size check out of the
ordinary send function recovered the small-call throughput on a third
runner. Throughput for the 2 MiB case fell 22.9% there, with a loss in
all eight pairs. The native implementation on `main` keeps the 1 MiB cap.
[First run](https://github.com/mingley/pure-protobuf/actions/runs/45274392122/artifacts/14868300215) ·
[Longer repeat](https://github.com/mingley/pure-protobuf/actions/runs/45276544494/artifacts/14866589301) ·
[Out-of-line size check](https://github.com/mingley/pure-protobuf/actions/runs/45278155533/artifacts/14868733843)
