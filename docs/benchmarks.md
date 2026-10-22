# Benchmarks

**The selected 1 KiB gzip streams use fewer allocations.** Production readiness
and an overall speed advantage over tonic/prost remain unproved.

## Streaming allocations

Before: `8fe0f8ae` · Now: `6cd91911` · Linux x86_64 · Rust 1.99.1.

**Allocations per RPC; lower is better.** These selected cases use gzip,
plaintext HTTP/2, 1 KiB zero-filled payloads, one connection and one in-flight
RPC. Bidirectional calls send four messages each way. Both stacks use gzip
level 6. Each endpoint runs in its own process against tonic/prost. Only the
native endpoint uses the listed codec.
The pbrs server route includes more interop handler work than the prost routes.
Use prost on both sides to compare transport; each before/after pair keeps its
handler unchanged.

| Side | Workload | Codec | Before | Now | tonic/prost |
|---|---|---|---:|---:|---:|
| Client | Bidirectional | pbrs | 116.3 | 101.2 | 99.0 |
| Client | Bidirectional | prost | 102.2 | 83.2 | 99.0 |
| Server | Bidirectional | pbrs | 122.1 | 107.1 | 94.0 |
| Server | Bidirectional | prost | 107.2 | 88.1 | 94.0 |
| Client | Pipelined bidi | pbrs | 113.5 | 98.4 | 99.0 |
| Client | Pipelined bidi | prost | 99.2 | 80.3 | 99.0 |
| Server | Pipelined bidi | pbrs | 121.3 | 107.8 | 94.0 |
| Server | Pipelined bidi | prost | 107.4 | 88.8 | 94.0 |

Values are medians of three repeats, rounded to one decimal. Counts use the
difference between 200-call and 400-call samples to remove fixed startup costs.
The tonic column is the control from the current run; both runs retain theirs.

Each revision completed **2,400 captures and 720,000 successful RPCs** across
all five RPC shapes, plaintext/TLS, identity/gzip, both native codecs, 1/64 KiB
payloads and one/16 in-flight calls. The table selects 1 KiB, one-in-flight cases.
Fewer allocations alone do not establish faster RPCs or lower retained memory.

## Remaining costs

Server with prost, plaintext, one in-flight RPC; lower is better.

| Payload | Workload | Metric | pbrs-grpc | tonic/prost | More |
|---|---|---|---:|---:|---:|
| 64 KiB | Bidi, gzip | Allocations/RPC | 111.2 | 99.0 | +12% |
| 64 KiB | Client stream, gzip | KiB/RPC | 1,773.8 | 1,190.6 | +49% |
| 1 KiB | Bidi, uncompressed | KiB/RPC | 45.8 | 32.9 | +39% |

These are cumulative allocation requests, not retained memory. Against the old
source, 64 KiB gzip upload server bytes rose **9.0% with prost and 10.6% with
pbrs**. A 144-capture
instruction pilot retains gzip and uncompressed regressions. Gzip bidi servers
use 2.2–2.4% more instructions than tonic and about 0.7% more than the old code.
Its 25/50-call samples do not establish a native speed ranking.

[Full results and raw evidence](evidence/grpc-streaming-20261022/README.md)
· [Earlier validation](evidence/grpc-validation-20261022/README.md)

## Run the comparison

Start from a clean checkout with Rust, Cargo and protoc on `PATH`. Use output
directories outside the checkout so generated files do not dirty the source.

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
  --payloads 1024,65536 --load-levels 1:1,1:16 --repeats 3
```

The build record pins the source, toolchain and binary. The campaign verifies
completed call counts and records failures. Repeat on the before and after
revisions with a quiet host. These shared-host runs also collect raw process
context switches. This phase omits instructions, syscalls and task wakeups;
qualified latency, throughput and scheduling comparisons remain open.

## What remains to prove

The full comparison needs broader payloads/load, realistic field access, cold
connections, overload and real-network traffic. Measure both endpoints
for latency, throughput, CPU, allocations, bytes,
syscalls and scheduling. The full 24-hour resource test remains open.
Concatenated compressed input can be partly decoded in native and tonic-adapter
paths; trailing-input and Zstd reserved-descriptor-bit checks also remain open.

The [benchmark contract](benchmark-contract.md) defines the host controls,
repeats and acceptance margins. The [scoreboard](scoreboard.md) tracks
qualification; the [evidence index](evidence/README.md) retains attempts and
failures.

## Older measurements

The [older tables](https://github.com/mingley/pure-protobuf/blob/99eccf209fd4cda28c992b7b8855531a31c2d249/docs/benchmarks.md) lack complete dated source and raw captures.
Their timings are not current speed claims. The older 7,200-capture counter
matrix used incorrect concurrency and cannot establish its declared load.
