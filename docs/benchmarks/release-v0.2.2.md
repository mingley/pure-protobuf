# v0.2.2 performance

These measurements use `7ff91e36`, the source of the signed `v0.2.2`
tag. Each benchmark ran on a shared Linux x86-64 host with Rust 1.100.0.
[Source](https://github.com/mingley/pure-protobuf/commit/7ff91e36ace2097ddd63f7f4480c1b3846a007f2) ·
[Reproduction guide](running.md)

## Owned message codecs

Each entry counts pbrs wins, ties and peer wins, in that order, across
28 fixtures. All three encoders returned owned output. The survey ran on
one logical CPU in fixed order with allocation counters off.

| Operation | pbrs / tie / upb | pbrs / tie / Prost |
| --- | ---: | ---: |
| First encode | 28 / 0 / 0 | 15 / 0 / 13 |
| Repeated encode | 28 / 0 / 0 | 16 / 0 / 12 |
| Owned decode | 27 / 0 / 1 | 16 / 1 / 11 |
| Full read | 28 / 0 / 0 | 14 / 1 / 13 |
| Construction | 28 / 0 / 0 | 5 / 1 / 22 |

Upb's faster owned decode was `rpc_sparse_path`, at 52.951 ns against
54.535 ns for pbrs. For the depth-eight full read, Prost took 127.317 ns
and pbrs took 464.344 ns. The artifact includes the raw samples and
`summary.md` lists every slower fixture.
[Survey](https://github.com/mingley/pure-protobuf/actions/runs/44678224644/artifacts/14603621401) ·
[Independent readback](https://github.com/mingley/pure-protobuf/actions/runs/44678964870/job/136097489642)

Five fresh processes on an AMD EPYC 9V45 repeated the full survey. The
first host used an Intel Xeon 6975P-C. Across the AMD runs, pbrs led upb on
133–135 of 140 comparisons and on 23–24 of 28 owned decodes. The 11-byte
`rpc_sparse_path` took 5.5–17.3% longer to decode with pbrs in the five
runs. The 30-byte `envelope` also favored upb in all five. Every benchmark
process exited successfully. The first summary omitted three measured Person
fixtures. The independent readback recovered all 28 from the saved samples.
[Raw repeat](https://github.com/mingley/pure-protobuf/actions/runs/44679443902/artifacts/14603633381) ·
[Verified summary](https://github.com/mingley/pure-protobuf/actions/runs/44680507802/artifacts/14603054506)

## gRPC over TCP

The standard `rpc-bench smoke` run compared the native and Tonic 0.14.6
transports over loopback in one process. Both used generated pbrs
messages. The large unary call sent 271,828 bytes and received 314,159.

| Unary latency (µs) | Native p50 | Tonic p50 | Native p99 | Tonic p99 |
| --- | ---: | ---: | ---: | ---: |
| Empty | 44.9 | 70.2 | 67.8 | 109.2 |
| Large | 353.5 | 333.6 | 629.5 | 460.9 |

QPS uses the best of three sustained rounds. The streaming rates use the
best of eight rounds after one warm-up round. All four QPS rows recorded
zero RPC errors.

| Throughput | Native | Tonic |
| --- | ---: | ---: |
| Empty unary, 1 concurrent RPC on 1 connection (RPC/s) | 93,131 | 24,207 |
| Large unary, 1 concurrent RPC on 1 connection (RPC/s) | 9,985 | 3,896 |
| Empty unary, 16 concurrent RPCs on 4 connections (RPC/s) | 164,624 | 90,981 |
| Large unary, 16 concurrent RPCs on 4 connections (RPC/s) | 11,605 | 9,626 |
| Server stream, 1 KiB (messages/s) | 1,068,813 | 968,276 |
| Bidi ping-pong (round trips/s) | 62,257 | 25,183 |
| Client upload, 1 KiB (messages/s) | 2,239,539 | 1,119,732 |

The large-unary latency gate failed. The retained report includes all
82 records, the process exit status and endpoint settings.
[TCP report](https://github.com/mingley/pure-protobuf/actions/runs/44678784422/artifacts/14604055887) ·
[Run the benchmark](running.md#grpc-client-and-server)
