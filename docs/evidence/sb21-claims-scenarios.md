# SB-21: contract-compliant scenario variants for E1/E2 claims

**Status: scenario definitions delivered; claim-runner integration incomplete.**
The smokes below verify selected workload paths. They do not qualify an E1/E2
claim or complete the campaign. This distinction was rechecked on 2026-09-29.

## What

Two frozen variant sets so E1/E2 claims never rely on closed-loop
diagnostic runs:

- `rpc-bench/scenarios/claims.json`: 8 open-loop poisson QPS
  variants of the 7 SB-10 `protobuf_*` scenarios (15 s warmup /
  60 s measurement), plus one 2x overload variant. Offered loads
  sit at ~70% of the SB-10 native-pair closed-loop QPS (empty 12k,
  1kb 10k, 64kb 5k, streaming 9k/9k, open 25k, poisson 5k,
  overload 24k); in-flight caps stay within the worker resource
  policy (1x64; 2x100 for the open variant). File order is the
  frozen rep1 execution order (the runner executes in file order).
  No extra top-level keys: the integrated Go driver proto-parses
  the whole file strictly (verified: an extra block fails with
  `unknown field "claims"`).
- `bench/stack-matrix/scenarios/grpc-bench-echo.json`: 19 claims
  cells translating the SB-17 workload (unary, 1kib plaintext —
  nearest above the ~200 B complex_proto message — 1/2/4 server
  CPUs, matched-rate client cells), with frozen params (15 s /
  60 s, seed 210021, SLO search bounds) and per-repeat orders. All
  19 cell ids verified present in the `primary` expansion.
  `run.py` has no scenario loader yet (follow-up). Running `--stage primary`
  and filtering `report.json` can locate the matching diagnostic cells, but
  does not execute the frozen per-repeat orders or constitute this claim run.

Originals stay labeled diagnostic: `official.json` (closed-loop,
5 s / 30 s) and `docs/evidence/grpc-bench.md` Run A/B (closed-loop
ghz, 5 s / 20 s).

## Pre-registration (seed 210021, 5 repeats)

QPS pairings per repeat: native_pair, cpp_client_to_native_server,
native_client_to_cpp_server, go_client_to_native_server,
native_client_to_go_server; claim driver is the pinned C++ driver
(SB-10 pins). QPS rep orders (S=short names: empty, 1kb, 64kb,
s-empty, s-1kb, open25k, poisson5k, overload2x):

- rep1 (== claims.json file order): empty, 64kb, poisson5k,
  open25k, overload2x, s-1kb, s-empty, 1kb
- rep2: overload2x, 64kb, poisson5k, 1kb, s-empty, open25k,
  s-1kb, empty
- rep3: overload2x, s-1kb, s-empty, 1kb, poisson5k, empty, 64kb,
  open25k
- rep4: empty, s-1kb, 64kb, 1kb, s-empty, overload2x, open25k,
  poisson5k
- rep5: s-1kb, 1kb, open25k, poisson5k, empty, 64kb, overload2x,
  s-empty

Stack-matrix rep orders are frozen per cell id in
`grpc-bench-echo.json# frozen_order_per_repeat` (same seed).

## Smoke: QPS harness

Integrated Go driver (`dd51b1c9`), native_pair, no warmup/duration
overrides (2026-09-28 runs under `target/qps-logs/`):

- `claims_unary_poisson_5000qps`: PASS, 4997.6 QPS; 60.0 s
  measurement window; offered 300,000 -> 299,859 histogram +
  zero failures/rejections (141 in flight at drain).
- `claims_unary_ping_pong_empty` + `claims_unary_empty_overload_2x`
  (one invocation): executed in file order (empty first); PASS
  11,986 / 23,445 QPS; 60.0 s windows; counts reconcile
  (719,203 / 1,406,789).
- Asserts: configured durations honored (60.0 s windows),
  offered/completed/rejected reconcile, file (frozen) order
  executed. The overload variant did not saturate this host (no
  rejections observed); rejection accounting itself is covered by
  existing worker tests.

## Smoke: stack-matrix workload path

`rpc-bench load` with the frozen params against a native server
(15 s warmup at 1k qps + one 60 s probe at 20k qps, seed 210021):

- Wall 15.012 s / 60.080 s; offered = dispatched = completed =
  1,200,228 with 0 failures/timeouts/overflows.
- e2e (T_complete - T_sched) p50/p90/p99 1.86/2.68/3.35 ms vs
  service p50 0.97 ms: schedule-relative accounting verified,
  scheduling lag separately recorded.

## Known gaps (coordinator follow-ups, outside SB-21's write set)

- Worker ClientStats latencies are dispatch-relative: the QPS
  worker records `track_worker_rpc` service time, while the
  schedule-relative e2e lives only in the unexported `LoadRecord`.
  Claim latencies from the QPS path need a worker export change.
- `run.py` needs a scenario loader for `scenarios/*.json`
  (repeats + frozen order + cell filter); until then the mapping
  in the scenario file's `run` key is manual.
- Per-repeat re-shuffling needs runner support in
  `grpc-qps-interop.sh`; reps 2-5 orders above are the frozen
  procedure for the claim-run operator.

## Before SB-22 can use these definitions

The campaign preflight must verify the gaps above are closed, reject missing
required peers/settings, and retain evidence that the generator has spare
capacity. Reconcile offered, admitted, rejected, completed, failed, and
unfinished calls over the same measurement/drain windows. Retain both service
and schedule-relative latency; one cannot substitute for the other.

The listed rates are initial pilot values derived from a shared-host
diagnostic. Re-freeze them on the campaign hosts before measuring. In
particular, the `overload_2x` name does not prove overload: the recorded smoke
did not reach saturation. A campaign must establish its saturation knee and
show actual overload/recovery under the declared budget.

Five repeats, 60-second windows, or a scenario filename alone do not qualify
the results. Apply the full [benchmark contract](../benchmark-contract.md),
including paired 95% intervals and correctness checks at the measured revision.

## Gates

`rpc-bench-tests` and `stack-matrix` (smoke stage) re-run at
completion; `plan-lint.py` OK.
