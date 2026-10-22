# Benchmarks

**Production readiness and an overall speed advantage over tonic/prost remain
unproved.** The latest recorded comparison measures allocations. It does not
establish latency or throughput wins.

## Latest measured results

Source: `8fe0f8ae` · Linux x86_64 · shared host · release build, Rust 1.99.1.

**Allocations per RPC; lower is better.** Values are rounded medians of three
repeats, using the difference between 200-call and 400-call samples to remove
fixed startup costs. Each endpoint runs in its own process. The other endpoint
uses tonic/prost, so client and server costs are measured separately.

| Side | Workload | Codec | pbrs-grpc | tonic/prost | Difference |
|---|---|---|---:|---:|---:|
| Client | Unary, uncompressed | pbrs | 11.0 | 37.0 | 70% fewer |
| Server | Unary, uncompressed | pbrs | 21.1 | 37.0 | 43% fewer |
| Client | Bidirectional, gzip | pbrs | 116.1 | 99.0 | 17% more |
| Server | Bidirectional, gzip | pbrs | 122.0 | 94.0 | 30% more |
| Server | Pipelined bidirectional, gzip | pbrs | 122.8 | 94.0 | 31% more |
| Client | Bidirectional, gzip | prost | 102.0 | 99.0 | 3% more |
| Server | Bidirectional, gzip | prost | 107.1 | 94.0 | 14% more |
| Server | Pipelined bidirectional, gzip | prost | 107.4 | 94.0 | 14% more |

These selected rows show unary savings and the largest streaming costs.
Switching the native codec to prost leaves a server streaming allocation gap.
Fewer allocations alone do not imply faster RPCs or lower retained memory.

The full run contains **600 successful captures**: five call shapes, plaintext
and TLS, uncompressed and gzip, three repeats, and matched 200/400-call samples.
It uses **1 KiB messages, one connection and one in-flight RPC**. Streaming calls
exchange four messages. The table shows plaintext results; the archive also
contains TLS and requested-byte results.

[Full results and limitations](evidence/grpc-validation-20261022/README.md#performance-evidence)
· [Raw archive](evidence/grpc-validation-20261022/raw.tar.gz)
· [Archive checksums](evidence/grpc-validation-20261022/manifest.json)

Inside the archive, `counter-route-final/campaign.json` describes the run and
`counter-route-final/group-*/ledger.json` contains every comparison.

## Run the same comparison

Start from a clean checkout with Rust, Cargo and protoc on `PATH`. Choose new
output directories outside the checkout so generated files do not dirty it.

```sh
source_commit=$(git rev-parse HEAD)
bench_output=$(mktemp -d)
python3 scripts/build-rpc-bench.py \
  --source "$source_commit" --output "$bench_output/build"
python3 scripts/grpc-counter-campaign.py \
  --source-checkout "$PWD" \
  --binary "$bench_output/build/rpc-bench" \
  --build-record "$bench_output/build/build.json" \
  --output "$bench_output/counters" --phase native \
  --payloads 1024 --load-levels 1:1 --repeats 3
```

The build record pins the source, toolchain and binary. The campaign checks
completed call counts and records failures. Use a quiet host; these commands
produce local diagnostics, not a qualified speed ranking.

To verify the checked-in archive:

```sh
python3 docs/evidence/grpc-validation-20261022/check.py
```

## What remains to prove

The complete comparison needs more payload sizes and concurrency levels,
realistic field access, cold connections, overload and real-network traffic.
Client and server measurements must include latency, throughput, CPU,
allocations, bytes, syscalls and scheduling. Correctness, feature coverage and
the full 24-hour resource test must pass on the measured candidate.

The [benchmark contract](benchmark-contract.md) defines the required host
controls, repeated measurements and acceptance margins. The
[scoreboard](scoreboard.md) tracks qualification; the
[evidence index](evidence/README.md) retains attempts and failures.

## Older measurements

The [previous benchmark tables](https://github.com/mingley/pure-protobuf/blob/99eccf209fd4cda28c992b7b8855531a31c2d249/docs/benchmarks.md)
contain codec timings and older tonic/grpc-go runs. Those tables lack complete
dated source and raw captures. Their host, ownership and measurement limits
still apply; they are not current speed claims. The older
7,200-capture counter matrix used incorrect concurrency and cannot establish a
comparison at its declared load.
