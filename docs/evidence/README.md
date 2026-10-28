# Evidence guide

These records explain what was measured, what changed, and where an experiment
stopped. They are dated investigations, usually local diagnostics. A result
belongs to its recorded revision, host, workload, and peer versions; it does
not automatically describe today's code.

The [October 28 correctness and optimization review](optimizations-20261028/README.md)
repairs transport counter and frame-length overflow and rejects eight measured
optimization candidates. Its selected ledger retains 80 timing nonwins, 271
resource nonwins and nine missing axes. Source-bound tests and conformance pass;
the all-benchmark goal and full tonic/prost parity remain unmet.

The [decoder, generator and transport follow-up](decode-and-map-speed-20261027/README.md)
reuses validated child wire, avoids short-map backing buffers and directly
iterates indexed unique maps. It retains the full codec, map, partial-read,
encoding, generator and RPC comparisons, including remaining losses and rejected
experiments. Final transport checks pass 3,980 workspace tests; the raw records
keep each source and metric separate.

The [transport, codegen and map follow-up](parity-and-maps-20261027/README.md)
adds connection executor hooks, idle keepalive
policy and native TCP_NODELAY controls, plus named-message and enum-prefix
codegen options. Independent Prost comparisons and complete map measurements
record both the added coverage and the remaining gaps. Median CPU falls in
all 42 direct map iteration rows, while full-message CPU results include
regressions.

The [server and nested-message fixes](performance-fixes-20261027/README.md)
record lower server CPU from coalesced connection wakes and an explicit owned
message profile. Full, partial and encode comparisons retain their tradeoffs,
along with rejected decoder and generator experiments.

The [direct parity audit](parity-proof-20261027/README.md) retains protobuf
behavior and embedded-runtime counterexamples, two transport repairs, all
3,921 passing workspace tests, and the earlier codec/RPC comparisons. Its
lazy read-all loss motivated the profile above. Neither record establishes
full parity or an overall ranking.

The [allocation and generator follow-up](efficiency-20261027/README.md)
records allocation-free MiniTable length calculation, canonical unsigned map
keys, imported-path caching and lazy server timing. It preserves cold-codegen,
peak-memory and rejected parser-lookup results.

Start with the [repository audit](../audit-2026-09-29.md) for current priorities,
then the [scoreboard](../scoreboard.md) for qualification status and
[benchmarks](../benchmarks.md) for the main performance findings. Use the
[benchmark contract](../benchmark-contract.md) before turning any record into a
comparative claim. No category currently has a qualified performance win.

[Bounded encoding and early streaming responses](grpc-encode-reuse-20261026/README.md)
records 3,855 passing workspace tests, 448 allocation captures and 180 separate
counters-off CPU captures. It includes remaining noise, CPU and pipeline losses.

[Message merging and gRPC costs](grpc-parity-and-costs-20261027/README.md)
adds dynamic and MiniTable merge controls, direct Tower input, frame-copy and
generator measurements, and fresh endpoint comparisons with their losses.

[MiniTable groups and unknown fields](minitable-groups-20261027/README.md)
records group round trips, unknown-field preservation and the remaining
non-group parsing cost.

The [2026-10-22 day campaign](grpc-resource-soak-20261022/README.md) now has a
terminal resource outcome. Its recorded revision and qualification limits apply.

The [streaming resource day, source `6cd91911`](grpc-streaming-soak-20261022/README.md) has a terminal resource outcome; its source and qualification limits apply.

The [architecture resource day, source `9d6fb9c4`](grpc-architecture-soak-20261023/README.md) has a terminal resource outcome; its source and qualification limits apply.

<!-- architecture-c11-day-terminal -->
The [decode reuse resource day, source `beb4ecd9`](grpc-decode-reuse-soak-20261023/README.md) has a terminal resource outcome; its source and qualification limits apply.

<!-- architecture-c11-prost-cache-terminal -->
The [Prost cache diagnostic, source `beb4ecd9`](grpc-prost-cache-diagnostic-20261023/README.md) has a terminal diagnostic outcome; its scope and qualification limits apply.

## Benchmark coverage and campaign readiness

| Record | What it contributes | How to read it |
|---|---|---|
| [Compatibility checks and encode fallback, 2026-10-26](grpc-compatibility-fallback-20261026/README.md) | Detecting public-API, native MSRV and shared-suite inventory failures; source-bound repairs and a paired custom-codec allocation experiment | The scratch follow-up removes the temporary bridge while retaining Prost site counts. It is not adopted; layout grows by 8 B and endpoint performance is unmeasured. Hosted and local checks remain distinct. |
| [Required-field merges and deferred experiments, 2026-10-26](grpc-required-merge-20261026/README.md) | Correct final-tree initialization checks, regenerated proto2 bindings, official conformance, checked benchmark noise and source-bound experiment records | Kernel/benchmark changes are adopted; wake and encode-scratch candidates remain deferred. New endpoint comparisons cannot open listeners in this session. Earlier 24-hour attempts are interrupted. |
| [Ownership, batching and measured costs, 2026-10-24 UTC](grpc-ownership-20261024/README.md) | Source `933bf877`; 3,840 workspace tests, 961 selected sanitizer tests and three official conformance passes; complete source-bound native/Callgrind, separate bulk/shipping/tracing records | Unary-server shipping CPU, identity/gzip bytes, source instruction costs and bulk tradeoffs remain. All 30 instrumented instruction groups are lower than tonic; release previews pass. The dated snapshot had both days running; the October 26 record above establishes interruption without terminal results. |
| [Server deadlines and gzip, 2026-10-24 UTC](grpc-deadlines-20261024/README.md) | Runtime `7c02eb38`; workspace repository `8144576f` passes 3,812 tests; 787 sanitizer tests, all 11 prechecks, 1,448 counter captures / 44,904 RPCs and separate 48-capture Router control pass | All 22 measured native Prost instruction groups beat tonic; byte/source-cost losses remain. Both days were running in the 09:27 UTC snapshot; later observation records interruption without a terminal result or known exit cause. |
| [Bounded stream decode reuse, 2026-10-23](grpc-decode-reuse-20261023/README.md) | Bounded recovery of uniquely owned decoded storage; source `beb4ecd9`; 3,771 workspace tests, 746 sanitizer tests and 3,064 diagnostic captures pass | Large Prost gzip streams request fewer bytes; client CPU, identity streams and pbrs interop still have losses. Full production and performance qualification remain open. |
| [Execution, compression and ownership, 2026-10-22](grpc-architecture-20261022/README.md) | Ready-stream scheduling, bounded compression, duplex progress and teardown ownership; runtime `9d6fb9c4`, fixture checkpoint `deb0d1d6` | 3,755 workspace tests and 731 sanitizer tests pass in their recorded scopes; 3,000 diagnostic captures with matched endpoint settings complete; large streaming byte losses remain; the resource day was running at publication; its terminal result is linked above |
| [Streaming fixes and measurements, 2026-10-22](grpc-streaming-20261022/README.md) | Cancellation, batch rollback and compression repairs; 3,677 workspace tests pass; matched 2,400-capture native runs and 144 instruction captures | Selected allocations improve; larger-message byte and instruction regressions remain. Compression-format gaps, strict lint and production/performance qualification remain open; the new 24-hour diagnostic is running at publication |
| [2026-10-22 production and performance validation](grpc-validation-20261022/README.md) | Confirmed safety/correctness defects, repairs, full-workspace failures, resource preview and benchmark count/concurrency controls | Production readiness and broad superiority are unproved; invalid declared-load captures and failed checks remain visible |
| [Current-source prerequisites](grpc-prerequisites-abeb-20261009/README.md) | Strict lint, formatting, campaign contracts, documentation retry and eight caller-rustls tests passed | Native admission stress, features, resource duration and performance remain open |
| [Adapter send-buffer checks](grpc-adapter-buffer-20261009/README.md) | Corrected adapter expectation, real small-buffer forwarding and retained disk failure | Admission fairness, resource duration and performance gates remain open |
| [Complete current native inventory](grpc-native-abeb-20261009/README.md) | All requested native targets attempted on the reset-fix source, plus actual Rust 1.85 checks | Failures remain in the capsule; feature, resource and performance qualification stay separate |
| [Queued-reset admission](grpc-reset-admission-20261009/README.md) | Confirmed queued-reset slot bug, deterministic positive/negative control and broad candidate checks | Timing and mixed-progress failures retained; complete clean-source checks are running |
| [Resource attempt 009](grpc-resource-soak-20261009-attempt-009/README.md) | Release preview passed; day attempt failed after 7,583.5 seconds | Failed resource checks and incomplete duration leave the 24-hour gate open |
| [2026-10-09 endpoint counters](grpc-counter-matrix-20261009-abeb/README.md) | Native phase archived 7,200 captures, including 18 runner failures | The 2026-10-22 audit invalidated the declared concurrency; repeat the matrix with the repaired generator |
| [Mixed resource fairness failure](grpc-resource-admission-20261009/README.md) | 19 more integration targets pass, then a resource fairness test fails; 28 of 30 isolated repetitions pass | Original rejection and two bulk-progress failures retained; resource and counter queues stop before captures |
| [Endpoint counter queue](grpc-counter-matrix-20261009-a9fe/README.md) | Scheduled wider native, syscall and Callgrind coverage with automatic raw publication | Blocked by the native resource regression; no sweep captures ran |
| [Server regression checks](grpc-server-regressions-20261009/README.md) | All 1,100 server tests pass serially; health 180 and reflection 188 pass; strict documentation checks pass | Original parallel failures are retained; 40 isolated repetitions pass without changing assertions or deadlines |
| [Resource attempt 008](grpc-resource-soak-20261009-attempt-008/README.md) | Source `a9fe4c2d`, release; failed at its regression prerequisite | Preview and actual day never started; production qualification remains open |
| [Compiler, docs and counter runner](grpc-compatibility-20261009/README.md) | Actual Rust 1.85 checks, concise docs, 197 codegen and 25 documentation tests, 600 runner captures | Full native continuation, wider measurements, features and production qualification remain open |
| [Stalled upload response](grpc-stalled-upload-20261009/README.md) | Reads early rejection while upload waits for credit and preserves ordinary headers | 658 unit/integration tests and strict Clippy pass; earlier-source day does not qualify this runtime change |
| [Queued upload rejection](grpc-upload-rejection-20261008/README.md) | Reproduced inactive-stream overload failure and bounded response-header fix | 569 unit and 84 integration tests pass; the separate stalled-upload race is fixed in the record above; broader qualification remains open |
| [Corrected overload campaign](grpc-resource-soak-20261008-attempt-006/README.md) | Optimized preview and attempted day on source 68484746 | Preview passed; actual day failed after 1,869.51 seconds with a streaming deadline error |
| [Stalled-upload source campaign](grpc-resource-soak-20261009-attempt-007/README.md) | Optimized preview on source 930b3e17 | Failed probe warmup; no day started |
| [Boxed service dispatch](grpc-boxed-dispatch-20261008/README.md) | Smaller generated Router futures; 2,560 functional cells and per-side N/2N captures | Generated server instruction counts fell roughly 1–5% in five measured shapes, but still lose to tonic; one instruction capture failed |
| [Optimized resource campaign, fifth attempt](grpc-resource-soak-20261008-attempt-005/README.md) | Release-profile build, finite limits, and preview gating | Preview failed with Internal instead of ResourceExhausted on overload; the day did not start |
| [Endpoint syscall matrix](grpc-syscall-matrix-20261008/README.md) | 150 source-pinned N/2N captures, five pairings and shapes, three repeats | Server loses 16 of 30 syscall comparisons; TLS, gzip, other sizes and loads remain unmeasured |
| [Resource campaign, fourth attempt](grpc-resource-soak-20261008-attempt-004/README.md) | Passing preview and an actual-day attempt with observed finite limits | Day failed after 705.18 seconds: 1 MiB TLS bidi exceeded the 3-second server deadline at cycle 159 |
| [Endpoint syscall checks](grpc-syscall-capture-20261008/README.md) | All-thread tracer capture, source/build separation, 25 historical-binary plumbing cells, and counter guards | Wiring checks; the source-pinned syscall matrix above supplies fresh measurements |
| [Fresh endpoint matrix, corrected launcher](grpc-endpoint-matrix-20261008-attempt-003/README.md) | Release build with protoc 35.1 supplied, followed by requested functional and native/Callgrind N/2N captures | 2,559/2,560 functional cells passed; native captures passed 150/150 and instruction captures 149/150; failed cells and server losses remain |
| [Previous endpoint build](grpc-endpoint-matrix-20261008-attempt-002/README.md) | Actual failed build and guarded automatic outcome publication | Build failed because protoc was absent from PATH; no captures ran |
| [Response status and endpoint counter checks](grpc-response-status-20261008/README.md) | Partial-frame status and binary-log regressions, API doc cleanup, all-thread context-switch capture | Correctness and instrumentation checks; fresh measurements and qualification remain separate |
| [Third resource campaign](grpc-resource-soak-20261008-attempt-003/README.md) | Corrected memory sampling and independent recovery probes | Preview failed on a TLS bidi response after 18.40 seconds; the day did not start |
| [Second resource campaign](grpc-resource-soak-20261008-attempt-002/README.md) | Source-pinned preview with automatic outcome publication | Preview failed memory validation after 32.88 seconds; the day did not start |
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
