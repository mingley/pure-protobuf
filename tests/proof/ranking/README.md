# Check RPC CPU evidence

[`verify-performance-claim.py`](../../../scripts/verify-performance-claim.py)
checks a **named set of client or server CPU comparisons**. It catches missing
peers, source drift, broken call accounting, insufficient observations and
numerical losses. It does not establish an overall performance ranking.

Run it with the exact candidate revision:

```sh
python3 scripts/verify-performance-claim.py campaign.json \
  --expected-source FULL_40_CHARACTER_COMMIT \
  --output preflight.json
```

The command exits successfully only when real evidence passes every supported
check and every named comparison meets these bounds at the upper end of its
recomputed 95% interval:

| Metric | Candidate / reference limit |
|---|---:|
| CPU per successful RPC | 0.80 |
| p99 scheduled-send latency | 1.05 |
| Steady RSS | 1.10 |
| Peak RSS | 1.10 |

A missing requirement exits with status 1. A numerical failure or an interval
that crosses a limit also exits with status 1, retaining the comparison.
A synthetic fixture can pass the numerical checks but always fails the
measurement-evidence check. The report always sets `qualified: false`:
independent review, correctness gates, soak tests, other workloads and broader
comparators remain separate requirements in the
[benchmark contract](../../../docs/benchmark-contract.md).

## Input

Use schema `pbrs.rpc-cpu-evidence.v1`, scope `named-rpc-cpu-cells`, and category
`C8` for client CPU or `D4` for server CPU. Required comparators come from the
[category register](../../../bench/scoreboard/categories.json); an input cannot
remove a required peer. This checks named cells, not the complete primary matrix.

The [synthetic fixture](../../test_performance_claim.py) is the executable
format example. Its manifest contains:

- A candidate source commit and explicit `synthetic` boolean.
- Peer versions, source commits, allocators, and separate compiler, binary hash
  and optimized build settings for each architecture. Rust, Go and C++ have
  different build settings.
- SHA-256 references to Linux host snapshots. Client and server use separate
  hosts, with dedicated resources, pinned isolated cores, performance governor,
  disabled turbo, and recorded NIC and operating-system settings.
- Named cells with frozen transport/load settings, payload hash and a hash of
  the shared **handler behavior contract**. The latter identifies agreed work;
  it is not a claim that different languages have identical source code.
- A random seed and SHA-256 references to at least five paired raw run files
  for every cell, required peer and x86_64/arm64 architecture.

Each raw pair records its cell, architecture, reference, repeat and seeded
execution order. Candidate and reference records carry the effective pins and
settings, host pair, warmup and measurement durations, conserved call counts,
separate client/server process CPU (user + system) and RSS, and exact nanosecond vectors for
scheduled-send latency and scheduling lag.

Warmup requires both 15 seconds and 10,000 RPCs; measurement requires 60
seconds and at least 10,000 completions per endpoint/run. Primary CPU records
with errors, rejections or unfinished work fail. Open-loop counts must match
the frozen offered rate, and generator lag must stay within 10% of p50 latency.
The checker computes paired mean-ratio percentile bootstrap intervals using
10,000 resamples. Supplied winner labels or intervals cannot replace raw data.

Hash checks bind the manifest to the supplied files. They do **not** establish
that host snapshots are truthful, binaries were built from the asserted source,
recorded CPU came from the named endpoint processes, or results came from those binaries. Those require independent
provenance and campaign review. The report lists those outstanding requirements.

## Test the checker

```sh
python3 -m unittest discover -s tests -p test_performance_claim.py -v
```

The tests include a synthetic numerical pass, CPU/tail/memory losses, uncertain
intervals, omitted peers and architectures, short runs, dropped calls, changed
sources/settings, loopback, unsafe paths, duplicate records, booleans, NaNs and
duplicate JSON keys. Synthetic measurements are never performance evidence.
