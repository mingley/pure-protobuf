# Benchmarks

The native unary server still uses more CPU than tonic in the measured shipping build.
Some allocation and instruction counts are lower; remaining losses are listed first.

**Current `933bf877` · safety `ca4ef720` · before `7c02eb38`** · 2026-10-24 UTC · Linux x86_64 · Rust 1.99.1 · shared host.

## Shipping-build loss

Quiet same-source replay: identity unary, 1 KiB, one connection / one in-flight call, two Tokio workers, four repeats. Counters/profilers are off.

| Server CPU | Native | tonic/prost | Native / tonic |
|---|---:|---:|---:|
| Raw CPU/call, 10,000 calls | 53–60 µs | 40–45 µs | 1.18–1.46× |
| N/2N CPU slope | 46–68 µs/RPC | 38–46 µs/RPC | 1.00–1.79× |

Raw CPU loses all four pairs; one slope pair ties. The earlier 48-capture run had 2.08–2.89× slopes and 13.6–18.3% lower throughput; that magnitude does not repeat. Replay timing is mixed and remains unqualified.

## Requested allocation bytes

**Bytes/RPC, lower is better.** Equal Prost work, plaintext, one in flight. These are cumulative allocation requests, not RSS.

| Side | Workload | Current | tonic/prost | Change |
|---|---|---:|---:|---:|
| Client | Bidi, 1 KiB identity | 42.40 KiB | 40.11 KiB | +5.70% |
| Server | Bidi, 1 KiB identity | 31.32 KiB | 32.95 KiB | -4.94% |
| Client | Bidi, 1 MiB gzip | 19.41 MiB | 16.67 MiB | +16.40% |
| Server | Bidi, 1 MiB gzip | 18.40 MiB | 15.67 MiB | +17.43% |

Singleton identity bidi improves versus Safety: client −21.28%, server −27.92% requested bytes.

## Costs versus earlier source

| Comparison | Side / workload | Metric/RPC | Before → current | Change; paired range |
|---|---|---|---:|---:|
| Before → current | server, unary 1 KiB identity | k instructions | 34.51 → 40.40 | +17.08%; +2.88..+35.07% |
| Before → current | server, upload 1 KiB identity | k instructions | 37.58 → 44.22 | +17.66%; +10.16..+25.40% |
| Safety → current | client, pipelined bidi 64 KiB identity | allocations | 45.18 → 49.06 | +8.59%; +2.24..+15.30% |
| Safety → current | server, pipelined bidi 64 × 1 KiB | KiB requested | 319.34 → 359.10 | +12.45%; +1.15..+26.75% |

The bulk server repeats lose +26.90% / +15.67% bytes versus fresh tonic. All increases, mixed repeats and control endpoints remain in the full ledgers.

## Instructions

**Instructions/RPC: current / tonic (change); k = 1,000, M = 1,000,000.** All 30 scopes are lower in both repeats; medians are 3.24–55.77% lower.
Callgrind and allocation counting are enabled. Both endpoints use Prost, plaintext and one in-flight call. These counts do not establish shipping CPU speed.

| Workload | Client | Server |
|---|---:|---:|
| Unary, 1 KiB identity | 29.4k / 65.2k (-55.0%) | 40.4k / 56.3k (-28.2%) |
| Upload, 1 KiB identity | 30.2k / 68.2k (-55.8%) | 44.2k / 56.5k (-21.8%) |
| Server stream, 1 KiB identity | 44.6k / 71.9k (-38.0%) | 55.1k / 68.3k (-19.4%) |
| Bidi, 1 KiB identity | 85.6k / 130k (-34.2%) | 94.8k / 124.8k (-24.0%) |
| Pipelined bidi, 1 KiB identity | 66k / 96.8k (-31.9%) | 66.8k / 89.7k (-25.5%) |
| Unary, 1 KiB gzip | 624.4k / 738.6k (-15.5%) | 637.2k / 727.2k (-12.4%) |
| Upload, 1 KiB gzip | 2.023M / 2.237M (-9.6%) | 974.8k / 1.163M (-16.2%) |
| Server stream, 1 KiB gzip | 1.032M / 1.235M (-16.4%) | 2.046M / 2.237M (-8.5%) |
| Bidi, 1 KiB gzip | 2.471M / 2.786M (-11.3%) | 2.479M / 2.776M (-10.7%) |
| Pipelined bidi, 1 KiB gzip | 2.449M / 2.75M (-10.9%) | 2.447M / 2.738M (-10.6%) |
| Unary, 64 KiB gzip | 2.897M / 3.038M (-4.6%) | 2.912M / 3.009M (-3.2%) |
| Upload, 64 KiB gzip | 10.25M / 10.71M (-4.3%) | 1.851M / 2.037M (-9.1%) |
| Server stream, 64 KiB gzip | 3.97M / 4.226M (-6.1%) | 10.4M / 10.82M (-3.8%) |
| Bidi, 64 KiB gzip | 11.56M / 12.13M (-4.7%) | 11.5M / 12.06M (-4.6%) |
| Pipelined bidi, 64 KiB gzip | 11.6M / 12.11M (-4.2%) | 11.48M / 12.03M (-4.6%) |

Against before, 22 of 30 instruction medians rise. The ±2% ledger band is not a significance test; every smaller increase and repeat range is retained.

## Scope and records

| Campaign | Captures | Successful RPCs |
|---|---:|---:|
| Native counters, three sources | 576 | 21,600 |
| Callgrind, three sources | 540 | 8,100 |
| Separate bulk, Safety/current | 240 | 9,000 |
| Separate shipping builds / quiet replay | 48 / 16 | 360,000 / 120,000 |
| Separate selected syscall / fd traces | 8 / 8 | 12,000 / 600 |

Native N=25/50; Callgrind N=10/20; two paired repeats. The native controller failed after capture on missing plan files; separate recovery rebuilt reports without recapture.
Gzip level 6, zero-filled payloads, four streaming messages, 400 KiB server send buffers, queues 16. Bulk partitions use 1/2/3/4/64 messages.
Current-source cold-process, entropy/read-all, overload, physical networks and broader platform/feature coverage remain open; older C12 cold results are not inherited.

[Results, dated source and raw captures](evidence/grpc-ownership-20261024/README.md) · [Benchmark contract](benchmark-contract.md)

## Reproduce

Use a clean checkout and keep outputs outside it. Exact paired commands, profiles and source pins are in the record.

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

**Validation:** 3,840 workspace tests pass (nine existing ignores); selected ASAN/leak checks pass 961 (one existing ignore). Resource previews pass 34.37 / 30.80 seconds; both 24-hour runs are advancing in the 22:48:16 UTC snapshot, with results pending.
Older `7c02eb38` days are interrupted, with no terminal outcome or known exit cause. Production readiness and broad superiority remain unproved. [Next work](../TODO.md#current-priorities)

[Separate ready-set experiment](evidence/grpc-ready-set-20261024/README.md): 87 focused checks and 360,000 RPCs pass, but CPU results are mixed; the change is not adopted.
