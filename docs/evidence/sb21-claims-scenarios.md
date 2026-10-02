# SB-21: scenario execution and independent QPS accounting

**Status: runner integration and native accounting implemented; campaign qualification remains open.**
The 2026-09-29 changes export scheduled-send latency, reconcile observed mark
windows, and execute independently randomized repeat orders. These are
functionality checks, not an E1/E2 performance claim.

## Frozen inputs and execution

- `rpc-bench/scenarios/claims.json` retains eight open-loop Poisson variants:
  empty, 1 KiB, 64 KiB, streaming-empty, streaming-1 KiB, open25k, poisson5k,
  and overload2x; each specifies 15 seconds warmup and 60 seconds measurement.
  It remains a strict official `Scenarios` message without custom top-level keys.
- The QPS runner accepts `--repeats=5 --order-seed=210021`, writes
  `execution-plan.json`, and independently shuffles scenario order and peer
  direction order for every repeat. Repeat artifacts use separate
  `repeat-N/` directories. The executable archived order replaces the old
  manually documented rep1–rep5 order; input file order is not an execution promise.
- The stack-matrix runner now loads
  `bench/stack-matrix/scenarios/grpc-bench-echo.json` through `--scenario`,
  executes its frozen cells/repeats and per-repeat peer ordering, and supports
  explicit smoke/cell filters. See [SB-24](sb24-matrix.md) for execution evidence.
- Closed-loop `official.json` and earlier ghz comparisons remain diagnostics.

Rates are initial shared-host pilot values, not campaign-host capacity
measurements. The overload2x name does not prove saturation or recovery.

## Native worker measurement contract

The official WorkerService protobuf is unchanged. `ClientStats.latencies`
retains dispatch-relative service time. Each native client mark also writes
one `QPS_ACCOUNTING {json}` record to its process log containing both service
and scheduled-send latency histograms with observed counters. Histogram
values and counters are captured under one lock.

These are **completion-mark windows**, not arrival cohorts or drain reports.
Admission is recorded synchronously before the generator spawns the RPC
future. `dispatched` means admitted to a channel/task; a task cancelled before
its first poll is still unfinished. Service time starts when the admitted
future executes; scheduled-send time starts at its original Poisson arrival.
A timeout records actual elapsed time, including scheduler delay, rather than
substituting its configured deadline.

For each mark:

```text
offered = dispatched + rejected
incoming_in_flight + dispatched = completed + unfinished
completed = successful + failed
timed_out <= failed
```

Here `failed` counts completed non-OK calls; unfinished calls are a separate
outcome. Marks have `drain_seconds = 0`. A reset carries unfinished calls into
the next window as `incoming_in_flight`; `carried_in_completed` identifies their
completions. Rejections have status counts but no invented latency sample.
This explicit carry-in matters: completed may legitimately exceed offered
within a short window. Neither offered nor unfinished is inferred from
configured QPS multiplied by duration.

`scripts/qps-proof.py` checks conservation, reset boundaries, actual elapsed
time, both histogram totals, official error counts, and the driver's QPS and
service histogram against this independent record. It rejects gross Poisson
offered-rate violations even in diagnostic mode. `--claim-check` additionally
requires open-loop input, a reset boundary, and independent accounting with
scheduled-send latency. Reference clients without equivalent evidence fail
closed. The output always keeps `claim_eligible: false`: this check alone
cannot establish controlled resources, matched peer behavior, a spare-capacity
generator, adequate tail samples, or confidence intervals.

## Reproducible accounting smoke (2026-09-29)

[Raw evidence](sb21-accounting-smoke.json) retains both native repeats'
complete integrated-driver results, all worker mark records, the execution
plan, scenario, binary fingerprints, and the rejected Go result. Source:
`deea5e3f` plus the SB-21 working changes. Native binary is a **debug build**;
the summary correctly reports dirty/unverified source because an explicitly
selected prebuilt binary was used. This shared Darwin host smoke is not a
speed comparison.

```sh
GRPC_QPS_NATIVE_WORKER="$PWD/rpc-bench/target/debug/rpc-bench" \
  bash scripts/grpc-qps-interop.sh --skip-build \
  --driver=/path/to/pinned/qps-driver \
  --scenarios=rpc-bench/scenarios/claims.json \
  --scenario=claims_unary_poisson_5000qps --mode=native_pair \
  --warmup=1 --duration=2 --repeats=2 --claim-check \
  --log-dir=target/qps-sb21-native-accounting
```

Both accounting checks passed:

| Repeat | Actual seconds | Offered / dispatched | Incoming | Completed | Unfinished |
|---|---:|---:|---:|---:|---:|
| 1 | 2.014601292 | 10,210 / 10,210 | 11 | 10,213 | 8 |
| 2 | 2.015884792 | 10,221 / 10,221 | 10 | 10,217 | 14 |

Both had zero rejected, failed or timed-out calls. Scheduled-send mean latency
was 1.737 / 1.603 ms, versus service mean 0.738 / 0.634 ms, proving the exported
histograms have distinct time origins. Nonzero incoming and unfinished
counts reconcile directly in the retained mark records.

The earlier 2026-09-28 note inferred 300,000 offers from 5,000 QPS × 60 seconds
and called the histogram difference in-flight. Those figures lacked
independent offer and boundary counters; that note did not prove conservation
and is superseded by the observed accounting above.

## Investigating the archived Go 11,058.2 QPS at nominal 5,000 QPS

The archived SB-10 artifact contains only `{"qps": 11058.2}`. Its scenario
has one channel and 64 outstanding slots. It cannot establish the actual
offered count, carry-in, failures, or measured window that produced that value.

Inspection of the pinned grpc-go
[`benchmark_client.go`](https://github.com/grpc/grpc-go/blob/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef/benchmark/worker/benchmark_client.go#L259)
establishes a concrete workload mismatch: `unaryLoop` creates a Poisson chain
for each channel/outstanding slot, passing the full configured `OfferedLoad`
to every chain; `poissonUnary` reschedules each chain with that same rate.
Thus this source does not interpret 5,000 as an aggregate rate for 64 slots.
Its theoretical scheduled aggregate is 64 × 5,000, before timer/runtime
limitations. This is not a measurement of achieved offers, and does not
uniquely reconstruct the historical 11,058.2 result.

A fresh 1-second warmup / 2-second measurement of that pinned Go worker against
the debug native server reported 11,240.4 QPS. The runner rejected it with
`reported QPS exceeds configured Poisson offered load`; the result and source
pin are retained in the smoke artifact. We do not normalize or relabel the Go
number as a 5,000-QPS comparison. A matched campaign needs a verified aggregate
schedule and independent accounting for that peer.

## Tests and remaining qualification gates

Focused validation: 12 Python QPS tests, 93 worker integration tests, the binary
unit suite, and a separate admitted-before-first-poll regression. Tests cover
concurrent mark/reset races, carry-in conservation, scheduling delay, real
timeout elapsed time, malformed histograms/counters, randomized peer order,
saved native evidence, and rejection of the archived Go outlier. The runner
also works with macOS's older Bash array indexing.

Before SB-22: qualify all required peer/direction implementations, establish
matched offered-load semantics, retain independent accounting for reference
clients, measure generator spare capacity, choose measured saturation/overload
points, and run on declared controlled hosts. The full
[benchmark contract](../benchmark-contract.md), including samples and paired
95% intervals, remains mandatory. Five repeats or a successful accounting
preflight does not qualify a claim.

## Aggregate-arrival harness follow-up

The new benchmark-only Go overlay replaces the pinned client's Poisson
arrival/accounting leaf. It uses one aggregate SplitMix64 chain at the canonical
scenario rate with seed `0x5eed20260918`, positive rounded exponential intervals,
the same configured slot cap as native and a five-second per-call deadline.
Delayed arrivals retain their scheduled times. Independent admission and outcome
counters feed atomic completion-mark windows, including rejected calls, failures,
timeouts, carry-in, unfinished calls and both service/scheduled latency histograms.
`STREAMING` explicitly matches native's current new-stream/one-message/one-reply
unit rather than upstream Go's persistent-stream message loop. Transport, codec,
server and all original closed-loop paths stay pinned upstream.

The original benchmark client SHA-256 is
`73ae33ebad3f1bf2f5e0196bb28b06d96618594dfe650b2de0ef6e410be5f464`;
the complete pinned module source tree SHA-256 is
`743826376a0fbdca8e0550dd587c83281b597a7b637a187b7c5b7e3c5e556266`.
Preparation rejects drift before creating an overlay. Go does not permit build
overlays under its module cache, so the harness copies that exact module into
its generated target directory and uses a generated modfile replacement. The
repository's Go module/dependency lockfiles remain unchanged. Run manifests also
retain the helper, patched client, build overlay, generated modfile/checksum and
worker binary hashes; skip-build execution verifies these hashes again.

Native and Go tests share an independently derived twelve-interval vector at
5,000 aggregate QPS. The proof records the legacy 64-slot interpretation explicitly:
5,000 configured QPS means 320,000 nominal aggregate QPS for the original worker;
5,000 aggregate would require 78.125 QPS per legacy slot. The overlaid client uses
5,000 directly as an aggregate rate and does not repair or infer reported counts.
The driver now consumes the prepared scenario, including actual smoke overrides,
and verifies emitted load/slot/shape/payload/duration metadata. Missing or inconsistent
stack load counters and service/scheduled histogram counts fail before SLO rating;
the existing scheduling-lag and server-headroom thresholds remain unchanged.

The archived native and failed Go runs above remain unchanged and tested. The
overlay is explicitly an **overlaid pinned grpc-go benchmark client**, not an
unmodified official worker. Real two-harness smoke/headroom validation and all
claim qualification requirements remain pending until recorded below.

The [2026-10-02 aggregate-arrival diagnostic](sb21-arrival-accounting-20261002.md)
records eight successful independent-accounting preflights, every rejected call,
all ten invalid frozen stack rows, the current unmodified-Go diagnostic, exact
source/binary/SDK pins and complete pre-existing strict-Clippy failures. Headroom
and performance qualification remain blocked; the observed-thread-count CPU
normalization defect is explicitly retained for SB-24.
