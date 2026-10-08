# Evidence guide

These records explain what was measured, what changed, and where an experiment
stopped. They are dated investigations, usually local diagnostics. A result
belongs to its recorded revision, host, workload, and peer versions; it does
not automatically describe today's code.

Start with the [repository audit](../audit-2026-09-29.md) for current priorities,
then the [scoreboard](../scoreboard.md) for qualification status and
[benchmarks](../benchmarks.md) for the main performance findings. Use the
[benchmark contract](../benchmark-contract.md) before turning any record into a
comparative claim. No category currently has a qualified performance win.

## Benchmark coverage and campaign readiness

| Record | What it contributes | How to read it |
|---|---|---|
| [gRPC checks and endpoint measurements, 2026-10-08](grpc-readiness-20261008/README.md) | Resource fixes, 2,560 functional cells, N/2N per-side counters, failed soak and fairness controls | Streaming instruction/byte losses remain; counter coverage and production qualification are incomplete |
| [Codegen matrix (SB-09)](sb09-codegen-matrix.md) | Five-repeat seeded, realistic-schema, and service-stub comparisons | Broad local evidence with many compile-cost losses; later GN changes need a matched rerun |
| [Official worker scenarios (SB-10)](qps-sb10.md) | Native/Go/C++ async worker runs | Compatibility/QPS diagnostics; SB-21 identifies a per-slot versus aggregate arrival-rate mismatch, not a reconstruction of the old measurement window |
| [Cross-stack matrix (SB-11)](stack-matrix-sb11.md) | Separate-process open-loop client/server harness | Smoke proves wiring, not the server ceiling or claim readiness |
| [Connection scale (SB-12)](sb-12.md) | Local memory, cold-start, and native TLS measurements | Earlier same-process diagnostic; the [newer scenario runner](../../bench/stack-matrix/scenarios/README.md) has separate limits |
| [Large payload baseline (SB-13)](large-payload-baseline.md) | Ownership/copy costs and payload sizes | Diagnostic baseline for subsequent zero-copy work |
| [Public grpc_bench entry (SB-17)](grpc-bench.md) | External-method reproduction and peer comparisons | Shared Docker host and emulated client limit interpretation |
| [Claim scenario execution (SB-21)](sb21-claims-scenarios.md) | Scheduled-send latency, conserved window accounting, frozen cells and randomized repeats | Native accounting smoke passes; reference rate normalization/independent accounting and a headroom-valid two-harness smoke remain open |
| [Transport matrix (SB-24)](sb24-matrix.md) | 144 TLS/compression wiring cells passed, with actual native handshake observations | Not throughput evidence: native/native uses AES-256, native/tonic uses AES-128, and tonic/tonic lacks actual session telemetry; no core cipher-policy API changed |
| [Performance CI (SB-23)](sb23-perf-ci.md) | Two corrected-parser CI pairs have 88 cells per revision; failed-build evidence is retained and rejected | SB-23 done; instructions excluded; Intel/AMD are separate cohorts, each still short of 30 compatible noise-study pairs |
| [RPC instruction study (RX-09)](rx-09.md) | 48 retained before/after captures; native instructions and allocated bytes fell in both shapes | Within-stack diagnostic improvement; cross-stack workloads differ and separate-process qualification remains open |
| [Compressed-frame decoding (RX-11)](rx-11-slice-decoding.md) | Slice decoders remove one allocation and 32 KiB per decode in 16 paired cells | Isolated decoder measurements; client/server RPC comparisons remain open |
| [Load accounting and endpoint diagnostics (SB-27a)](grpc-load-terminal-accounting.md) | Failed load runs exit nonzero; 256 RPC cells and 64 finite-limit recovery cycles passed | Short completion/recovery diagnostics; per-side performance floors and the production soak remain open |

The [plan](../plan/world-class/README.md) tracks completion. SB-21 remains in
progress pending reference-peer arrival/accounting equivalence. SB-23's real
CI artifact review is complete; SB-20's calibration is open. SB-24 still needs cipher
equality, full observed peer settings, Go/C++ parity and dedicated-host
headroom/resource/topology evidence. SB-25 owns optional competitor
harnesses. SB-15/SB-22 then produce controlled campaigns, and SB-16 publishes
a reproducible scoreboard. RX-09's existing cells need equivalent response
bytes/counts and codec/handler work before any beat-tonic interpretation.

## Finding the cost before changing code

| Area | Starting profiles | Follow-up experiments |
|---|---|---|
| Codec | [Codec profiles](codec-profiles.md) | [String thresholds](pk15-thresholds.md), [small-string encode](pk22-string-encode.md) |
| Generated code | [Compile-cost attribution](compile-cost.md) | [Generated-code shrink](gn-02-generated-shrink.md), [reflection/format options](gn-03-reflection-options.md), [shared descriptors](gn-03-descriptor-sharing.md), [generator speed](gn04-codegen-speed.md) |
| Client | [Client profiles](client-profiles.md) | [Unary allocations](cl-02.md), [header/timeout experiment](cl-03.md), [pool picking](cl-04.md), [stream pump](cl-05.md), [cold start/memory](cl-06.md) |
| Server | [Server profiles](server-profiles.md) | [RPC allocations](sv-02.md), [static routing](sv-04-static-routing.md), [trailers](sv-05.md), [bounded batching](sv-06.md), [accept/memory premise](sv-07.md) |
| HTTP/2 and scheduling | [HTTP/2 costs](h2-costs.md) | [Transport seam](h202-transport.md), [runtime seam](rx01-runtime.md), [per-core experiment](rx02-per-core.md), [Tokio scheduling](rx-08.md) |

An experiment that reports no change, a failed premise, or a rejected approach
is useful evidence. Read its verdict before proposing the same optimization.
Use the [profiling guide](../profiling.md) to collect a new base/after pair.

## Ownership, adapters, and feature costs

- Zero-copy work: [shared-buffer parsing](pk09-zero-copy.md),
  [shared-field sends](pk11-send.md), and the [Phase 3 stop decision](pk10-gate.md).
- Codec/transport separation: [prost on the native transport](tc-03-prost.md),
  [tonic adapter premise](tc10-adapter.md), and
  [tonic + prost comparison arm](tonic-prost-arm.md).
- Resource and feature costs: [load-balancer picking](picker-cost.md),
  [adaptive flow-control windows](tc-15-adaptive-window.md),
  [TLS providers](tls-providers.md), and [mixed-load fairness](rt-07.md).

## Correctness and implementation records

- [QG-05 shared-map recovery](shared-map-recovery.md) records exact-source
  `c89608bd` ordinary/Miri runs of all 19 original consumer crates (233 tests
  plus three regressions), with the restored Linux compatibility CI gate.
- [Closed-enum follow-up](closed-enum-recovery.md) records the separate
  `3a7aa128` wire-decoding repair and clean-source ordinary/Miri runs (233
  original tests plus six regressions). Unknown values stay out of typed
  storage while surviving serialization; full upb qualification remains open.
- QG-07 (`80013bfc`): the [operations](../guides/operations.md) and
  [production-service](../guides/production-service.md) recipes are compiled
  and exercised by [onboarding tests](../../tests/onboarding.rs), including
  negative API mutation and unchanged behavior under prose edits. The
  [status page](../status.md#public-guide-recipes-qg-07) records validation;
  this is bounded recipe coverage, not production qualification.
- [Fuzz campaign, 2026-09-28](fuzz-2026-09-28.md) records findings from that
  campaign; it is not proof of unlimited or continuing fuzz coverage.
- [Upstream HTTP/2 probe triage](io09-upstream-triage.md) records outstanding
  compatibility work and the distinction between parser and engine coverage.
- [ALTS record-layer review](alts-record-review.md) is scoped to that layer,
  not an independent qualification of an entire authenticated deployment.
- [Upstream Rust ABI inventory](rust-out-abi.md),
  [codegen module split](mx01-split.md), and
  [runtime module split](mx04-split.md) are implementation/verification records,
  not comparative benchmark results.

## Using and adding evidence

Keep the original numbers and unfavorable rows. When later work changes the
interpretation, add a dated correction or a new record that links back. Record
the source revision, dirty state, command, peer pins, host, workload, raw output,
correctness checks, and limits. An artifact under ignored `target/` is a path
on the original operator's machine unless it has also been published; it is
not guaranteed to exist in a fresh checkout.

For a completed measurement, state whether it is a local diagnostic or meets
the contract for a named comparative claim. For incomplete work, name the
missing capability or artifact and its plan task. Never infer success from a
missing row, a `not_run` metric, a zero exit containing invalid cells, or a task
marked done without its acceptance evidence.
