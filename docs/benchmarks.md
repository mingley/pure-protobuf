# Benchmarks

All **22 measured native Prost endpoint instruction comparisons beat tonic/prost**.
Identity and large bidi allocation-byte losses remain.

Current: `7c02eb38`; before: `beb4ecd9`. 2026-10-24 UTC · Linux x86_64 · Rust 1.99.1 · shared host.

## Remaining byte costs

**Requested allocation bytes/RPC. Lower is better.** Equal Prost handlers, plaintext, one in flight.
Bytes are cumulative allocation requests, not retained memory or RSS.

| Side | Workload | Current | tonic/prost | Difference |
|---|---|---:|---:|---:|
| Client | Bidi, 1 KiB identity | 54.23 KiB | 40.19 KiB | +34.92% |
| Server | Bidi, 1 KiB identity | 43.58 KiB | 32.95 KiB | +32.26% |
| Client | Bidi, 1 MiB gzip | 19.41 MiB | 16.67 MiB | +16.40% |
| Server | Bidi, 1 MiB gzip | 18.40 MiB | 15.67 MiB | +17.43% |

The pbrs interop server requests **43.37 MiB versus 15.67 MiB (+176.88%)** for 1 MiB gzip bidi.
That route includes additional handler work; it does not isolate codec or transport cost.

## Source changes

**Before versus current.** Both paired repeats increase for the rows below; all increases remain in the full ledger.

| Side | Workload | Metric/RPC | Before | Current | Change |
|---|---|---|---:|---:|---:|
| Server | Unary, 1 KiB identity | Requested bytes | 14.51 KiB | 14.84 KiB | +2.29% |
| Server | Unary, 1 KiB identity, TLS, 16 in flight | Requested bytes | 18.24 KiB | 19.17 KiB | +5.12% |
| Client | Bidi, 1 KiB identity | Allocations | 37.96 | 39.08 | +2.95% |
| Server | Unary, 1 KiB identity | Million instructions | 0.03443 | 0.03998 | +16.14% |

Selected gzip bytes also fall: 64 KiB server-stream server **2.30 → 2.17 MiB/RPC (−5.69%)**.
A separate 48-capture Router control cuts server bytes **19.76% unary / 7.72% bidi** versus `40f219fa`;
it retains small allocation-count increases and does not replace the full source comparison.

## Instructions

**Million instructions/RPC: current / tonic (change).** All 22 scopes win in both paired repeats.
The 20 gzip scopes use **3.19–16.52% fewer instructions**. These are allocation-instrumented Callgrind results.
Both endpoints use Prost, plaintext, one connection and one in-flight call. The table covers every measured instruction scope.

| Workload | Client: current / tonic | Server: current / tonic |
|---|---:|---:|
| Unary, 1 KiB gzip | 0.624 / 0.738 (-15.5%) | 0.637 / 0.727 (-12.4%) |
| Upload, 1 KiB gzip | 2.022 / 2.237 (-9.6%) | 0.976 / 1.163 (-16.0%) |
| Server stream, 1 KiB gzip | 1.031 / 1.235 (-16.5%) | 2.047 / 2.238 (-8.5%) |
| Bidi, 1 KiB gzip | 2.468 / 2.785 (-11.4%) | 2.474 / 2.774 (-10.8%) |
| Pipelined bidi, 1 KiB gzip | 2.446 / 2.753 (-11.1%) | 2.446 / 2.740 (-10.7%) |
| Unary, 64 KiB gzip | 2.897 / 3.033 (-4.5%) | 2.911 / 3.007 (-3.2%) |
| Upload, 64 KiB gzip | 10.226 / 10.708 (-4.5%) | 1.853 / 2.037 (-9.0%) |
| Server stream, 64 KiB gzip | 3.994 / 4.229 (-5.5%) | 10.405 / 10.815 (-3.8%) |
| Bidi, 64 KiB gzip | 11.562 / 12.169 (-5.0%) | 11.500 / 12.062 (-4.7%) |
| Pipelined bidi, 64 KiB gzip | 11.604 / 12.112 (-4.2%) | 11.478 / 12.029 (-4.6%) |
| Unary, 1 KiB identity | 0.0287 / 0.0655 (-56.2%) | 0.0400 / 0.0553 (-27.7%) |

Gzip upload client, 1 KiB: **2.384 → 2.022 M instructions/RPC (−15.21%)** versus before.
Ratios use unrounded medians; a ±2% reporting band is not a significance test.
Every cost increase, including values below 2%, and every repeat range stays in the raw record.

## Scope and records

| Completed campaign | Captures | Successful RPCs, both sources |
|---|---:|---:|
| Native counters | 1,088 | 40,800 |
| Instructions | 264 | 3,960 |
| Fresh process | 96 | 144 |

**Total: 1,448 captures / 44,904 RPCs.** The separate Router control adds 48 / 1,800; counts are not combined.
Offered/dispatched/completed/successful calls reconcile; zero errors, timeouts, overflow or unfinished calls.
Native: 136 selected workloads, 25/50 calls; instructions: 33 workloads, 10/20 calls; both use two paired repeats.
Fresh process: six workloads, 1/2 calls, four repeats, **including setup**. Identity server process bytes rise
**0.49% / 0.90%** for one/two RPCs; no startup subtraction or isolated cold-RPC cost is claimed.
Gzip level 6, zero-filled payloads, four messages per streaming leg, 400 KiB server send buffers, queues 16.
Native accounting/channelz stay enabled. Entropy, field access and overload need wider measurements.

[Results, dated source and raw captures](evidence/grpc-deadlines-20261024/README.md) · [Benchmark contract](benchmark-contract.md)

## Reproduce

Use a clean checkout with Rust, Cargo and protoc on `PATH`; keep outputs outside the checkout.

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

Exact source-paired commands and profiles are in the raw record. **Workspace: 3,812 passed, zero failed, nine
existing ignores**, 144 targets at repository `8144576f`, production-equivalent to measured runtime `7c02eb38`.
Both current 24-hour days are **running; results pending** ([09:27 UTC snapshot](evidence/grpc-deadlines-20261024/README.md#resource-days)).
Shipping CPU, latency, throughput, real networks, remaining features and platform/release checks remain open.
Older `beb4ecd9` resource/cache days passed only their recorded scope.
Production readiness and broad superiority remain unproved. [Next work](../TODO.md#current-priorities)
