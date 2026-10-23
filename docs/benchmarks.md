# Benchmarks

Gzip bidi at 1 MiB still requests **71–76% more allocation bytes** than
tonic/prost. Selected stream costs improve against the old source. Production
readiness and a broad speed advantage remain unproved.

2026-10-23 · Linux x86_64 · Rust 1.99.1 · Shared host.

## Remaining byte costs

**Requested allocation bytes/RPC; lower is better.** Prost, plaintext HTTP/2,
one connection and one in-flight call. The 1 MiB rows use two-repeat medians;
the smaller rows use three repeats from the expanded matrix.

| Side | Workload | Current | tonic/prost | More |
|---|---|---:|---:|---:|
| Server | Bidi, 1 MiB gzip | 27.5 MiB | 15.7 MiB | +75.5% |
| Client | Bidi, 1 MiB gzip | 28.5 MiB | 16.7 MiB | +71.0% |
| Server | Upload, 64 KiB gzip | 1,516.3 KiB | 1,190.6 KiB | +27.4% |
| Server | Bidi, 1 KiB identity | 43.5 KiB | 32.9 KiB | +31.9% |
| Client | Bidi, 1 KiB identity | 53.9 KiB | 39.4 KiB | +36.7% |

## Source changes

**KiB requested per RPC.** Before: `2eba72d8` (runtime `6cd91911`);
current: `9d6fb9c4`. Separate selected campaign, four-repeat medians; same
settings and handlers as the representative cases above.

| Side | Workload | Before | Current | Change |
|---|---|---:|---:|---:|
| Server | Upload, 64 KiB gzip | 1,773.8 | 1,516.3 | -14.5% |
| Client | Server stream, 64 KiB gzip | 1,927.6 | 1,671.7 | -13.3% |
| Server | Bidi, 64 KiB gzip | 3,455.3 | 3,196.9 | -7.5% |
| Client | Unary, 1 KiB identity | 9.4 | 9.6 | +2.5% |
| Client | Bidi, 1 KiB identity | 53.4 | 53.9 | +0.9% |

Gzip upload server allocations fall **65.0 → 61.0/RPC** (tonic: 63.0).
Gzip server-stream server allocations rise **79.5 → 80.2/RPC (+0.9%)** in all
four repeats, while requested bytes fall 1.5%. Improvements do not cover every cost.

Bytes are rounded to one decimal; percentages use ratios of unrounded medians.
Requested bytes are cumulative allocation requests, not retained memory or RSS.

## Server instructions

**Million instructions/RPC; lower is better.** Prost, 64 KiB gzip, plaintext, one
in-flight call. Three seeded 10/20-call repeats with fresh tonic controls.

| Workload | Before | Current | tonic/prost | vs before | vs tonic |
|---|---:|---:|---:|---:|---:|
| Upload | 2.41 | 1.94 | 2.04 | -19.5% | -4.8% |
| Bidi | 12.34 | 11.86 | 12.06 | -3.8% | -1.6% |

Callgrind uses allocation-counts-enabled release binaries with both endpoints
instrumented. Native-server diagnostics only; no native-client CPU measurement.
Bidi versus tonic is within the 2% tie band. These are not uninstrumented CPU,
latency or throughput results; fewer allocations alone also do not prove speed.

## Scope

| Completed campaign | Captures | Successful RPCs |
|---|---:|---:|
| Expanded current matrix | 2,400 | 720,000 |
| Selected source pairs | 400 | 120,000 |
| 1 MiB source pairs | 120 | 4,500 |
| Focused server instructions | 72 | 1,080 |

Including pilots: **3,000 captures, 845,790 successful RPCs**, zero failures or
timeouts. Expanded coverage: all five RPC
shapes, plaintext/TLS, identity/gzip, both native codecs, 1/64 KiB and one/16
in-flight calls; three repeats. Selected coverage: 1 KiB identity unary/bidi and
64 KiB gzip upload/bidi/server-stream; four ABBA repeats, 200/400-call differences.
Only selected scopes have a matched source baseline. Tables above select prost.

The 1 MiB gzip bidi cases save about 4 MiB per endpoint against the old source;
identity source changes are essentially ties. These API cases do not isolate the blocked
closed-upload fallback. The raw record retains its independent correctness tests.

Gzip is level 6 on both stacks, with four messages on each streaming leg.
Payloads are zero-filled application bytes, not encoded messages. **Server** send
buffers match at 400 KiB and request queues at 16; shipping defaults are unchanged.
Native accounting/channelz stay enabled. Equal prost handlers measure API costs;
the pbrs route includes additional interop-handler work. Native speed qualification
remains open.

[Full results, dated source and raw captures](evidence/grpc-architecture-20261022/README.md)
· [Benchmark contract](benchmark-contract.md)

## Run the comparison

Use a clean checkout, outside build outputs and identical harness code:

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

The command runs the expanded native matrix; exact selected ABBA/profiler commands
are in the raw record. Native captures record allocations, bytes and raw context
switches; CPU ticks are coarse, syscalls/task wakeups unmeasured. Qualified latency,
throughput, scheduling, retention and the full 24-hour result remain open.

[Historical results](evidence/grpc-streaming-20261022/README.md) used different
send buffers/queues and are not this source-paired baseline. The
[scoreboard](scoreboard.md) tracks broader payloads, real field access, overload,
cold connections, real-network traffic and release qualification.

The actual 24-hour resource run started at 02:52 UTC on 2026-10-23. Its result
is pending; an active attempt does not establish production readiness.
