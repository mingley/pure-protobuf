# Benchmarks

[Run benchmarks](benchmarks/running.md) · [Earlier results](benchmarks/grpc-20261024.md)
· [Qualification](scoreboard.md)

The latest measured implementation reduces large compressed-stream allocations
and fixes an early-response race. CPU results are mixed, and several workloads
still cost more than tonic/prost. There is no qualified overall performance ranking.

These results describe **`1d773f97` versus `e7595eb7`**, Linux x86_64, Rust 1.99.1,
on a shared host. Both endpoints use Prost unless stated otherwise. Each result
has a [dated source and raw captures](evidence/grpc-encode-reuse-20261026/README.md).

## Allocation bytes

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

## CPU

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

The [codec survey](../tonic-bench/README.md) separates parsing from reading
every field, and owned messages from borrowed views. The
[codegen matrix](evidence/sb09-codegen-matrix.md) records generator time,
generated output size and downstream build costs, including pbrs losses.
Use the [benchmark commands](benchmarks/running.md) to run either harness.

The [earlier gRPC tables](benchmarks/grpc-20261024.md) retain instruction
counts and previous CPU, allocation and bulk results at their original pins.
The [benchmark contract](benchmark-contract.md) defines the stronger evidence
needed for a performance claim.
