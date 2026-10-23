# Benchmarks

Prost gzip streams request **31–36% fewer bytes** than the previous revision in selected cases.
Important bidi byte and instruction losses remain. Production readiness and a
broad speed advantage over tonic/prost remain unproved.

2026-10-23 · Linux x86_64 · Rust 1.99.1 · Shared host.

## Remaining byte costs

**Requested allocation bytes/RPC.** Prost, plaintext, one connection/one in flight.
The 1 MiB rows use two paired repeats of 25/50-call batches; 1 KiB rows use
three repeats of 200/400-call batches. Lower is better.

| Side | Workload | Current | tonic/prost | More |
|---|---|---:|---:|---:|
| Server | Bidi, 1 MiB gzip | 18.5 MiB | 15.7 MiB | +18.2% |
| Client | Bidi, 1 MiB gzip | 19.5 MiB | 16.7 MiB | +17.1% |
| Client | Bidi, 1 KiB identity | 53.9 KiB | 39.4 KiB | +36.7% |
| Server | Bidi, 1 KiB identity | 43.5 KiB | 32.9 KiB | +31.9% |

The full pbrs interop server requests **43.5 MiB versus 15.7 MiB (+178%)** for
1 MiB gzip bidi. That route includes extra handler work; it is not an isolated
codec or transport comparison. Unique prost buffer recovery does not force
retained pbrs/Bytes input to give up ownership.

## Source changes

**Gzip requested bytes/RPC.** Before: `9d6fb9c4`; current: `beb4ecd9`.
Matched handlers/settings, two paired repeats of 25/50-call batches.

| Side | Workload | Before | Current | Change |
|---|---|---:|---:|---:|
| Server | Bidi, 1 MiB | 27.5 MiB | 18.5 MiB | -32.6% |
| Client | Bidi, 1 MiB | 28.5 MiB | 19.5 MiB | -31.5% |
| Server | Upload, 64 KiB | 1,516.3 KiB | 964.4 KiB | -36.4% |
| Client | Server stream, 64 KiB | 1,671.6 KiB | 1,119.8 KiB | -33.0% |

Observed increases remain: 1 MiB server-stream server bytes +2.9% with mixed
repeat directions; tiny identity bidi client allocations +1.6% in both repeats.
Tiny unary client bytes rise 0.86%; the fresh tonic source control rises 0.87%.
All byte/count increases and repeat ranges remain in the raw ledger, without a
2% cutoff or unsupported allocation-site attribution.

Values/percentages use unrounded medians, displayed to one decimal. Requested
bytes are cumulative allocation requests, not retained memory or RSS.

## Instruction diagnostics

**Million instructions/RPC.** Prost gzip, plaintext, one in flight;
10/20-call batches, two paired repeats. These four cases differ from tonic by over 2%.

| Side | Workload | Current | tonic/prost | Difference |
|---|---|---:|---:|---:|
| Client | Upload, 1 KiB | 2.385 | 2.237 | +6.6% |
| Server | Bidi, 1 KiB | 2.834 | 2.774 | +2.2% |
| Server | Upload, 1 KiB | 1.063 | 1.163 | -8.6% |
| Server | Upload, 64 KiB | 1.939 | 2.037 | -4.8% |

The other four cases are within 2% of Tonic. All eight changes from the previous
revision are under 0.4%: **this change establishes no CPU gain**. Both endpoints run under Callgrind using release
binaries with allocation counting enabled; this is not shipping CPU, latency or
throughput qualification. Profiles attribute the client upload gap chiefly to
repeated gzip output-scratch initialization; no replacement is implemented.

## Scope and records

| Completed campaign | Captures | Successful RPCs |
|---|---:|---:|
| Selected source pairs | 568 | 21,300 |
| Current expanded matrix | 2,400 | 720,000 |
| Selected instructions | 96 | 1,440 |

Every offered/dispatched/completed call reconciles; zero failures/timeouts.
Expanded: all five RPC shapes, plaintext/TLS, identity/gzip, both native codecs,
1/64 KiB and one/16 in-flight calls. Only selected scopes have a matched
baseline. Gzip level 6, zero-filled application payloads, four messages on each
streaming leg; server send buffers 400 KiB, queues 16. Shipping defaults and native
accounting/channelz are unchanged. Payload size is not encoded message size.

[Full results, dated source and raw captures](evidence/grpc-decode-reuse-20261023/README.md)
· [Benchmark contract](benchmark-contract.md)

## Reproduce

Use a clean checkout with Rust, Cargo and protoc on `PATH`. Keep outputs outside
the checkout and use identical harness settings on both revisions:

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

Exact selected comparison/profiler commands are in the raw record. Context switches and
coarse CPU ticks do not qualify scheduling; syscalls/task wakes, native latency,
throughput, retained memory, full feature/platform coverage and a current-source
24-hour result remain open. On 2026-10-23, 24-hour tests started at **06:07 UTC
(general resources)** and **06:23 UTC (Prost cache)**. Both are running; results
are pending, with no 24-hour pass established.

[Earlier results](evidence/grpc-architecture-20261022/README.md) retain their
original source and scope. [Current priorities](../TODO.md#current-priorities)
include the source-reviewed pre-shape service-deadline gap; its independent
reproduction and repair are still pending.
