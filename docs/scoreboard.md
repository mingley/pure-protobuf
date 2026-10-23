# Scoreboard

No performance category has passed qualification yet. The project has useful
local results and working benchmark tools; it has not completed the controlled
campaigns needed for repeatable comparisons across the required workloads.

The [execution follow-up](evidence/grpc-architecture-20261022/README.md) adds
wake-indexed scheduling, runtime-local cancellation parking, selected-method
prost dispatch, complete compressed-message
handling, tonic gzip normalization and a separate 1,024-member/frame default
limit. Candidate `783bcd53` also fixes blocked duplex uploads, cancellation and
ownership defects, including resetting streams when an upload task panics.
Its leak-detecting AddressSanitizer run passes 720 tests with one existing ignore;
Rust 1.85 default-library and Rust 1.87 optional-Zstd checks also pass.
A 10-cycle resource preview passes with post-drain byte charges, tokens and active
calls at zero; this does not close the day-long requirement.
Its full workspace run fails with 3,739 passes, five failures and nine existing
ignores across 139 targets. Serving/Tower deadline fixtures are being made
deterministic without changing the runtime. A new clean-source workspace run,
strict CI and performance captures remain pending.
Predecessor `90201458` records 3,717 workspace passes, three failures and nine
existing ignores; its failures remain archived. No performance standing changes.

Pending benchmark captures use matched 400 KiB server send buffers and request
queues of 16 at `2eba72d8`/`804b5c59`. Historical full-matrix counts are not the
source-paired baseline for this harness. Source comparisons cover selected
workloads; the full current matrix compares native with tonic.

The `6cd91911` [day attempt](evidence/grpc-streaming-soak-20261022/README.md)
failed after 32 min 2.73 s with an unexpected INTERNAL on TCP bidi reset.
The exact failure was not uniquely reproduced in 2,008 scenario replays.
Focused reset-ordering controls are separate evidence; no full day is proved.

The [earlier streaming comparison](evidence/grpc-streaming-20261022/README.md)
retains its 2,400-capture native campaigns and 144-capture instruction pilot.
It reports selected 1 KiB allocation savings alongside 64 KiB gzip byte losses
and instruction regressions. Its values belong to those source pins. Native CPU,
latency, throughput and scheduling qualification remain open, as do strict lint,
feature/platform/release gates. No qualification standing changes.

The table began with the 2026-09-29 audit; later source-scoped records above
update specific findings. It remains a manual index until SB-16 supplies the
generated scoreboard and does not certify the latest GitHub Actions run.

The [2026-10-08 gRPC record](evidence/grpc-readiness-20261008/README.md) adds
separate-process functional coverage and per-side N/2N counter measurements.
It retains streaming instruction/byte losses, a failed counter capture,
intermittent fairness failures, and a 24-hour attempt that stopped after
504 seconds. These diagnostics do not change the qualification standings below.

The machine-readable source is
[bench/scoreboard/categories.json](../bench/scoreboard/categories.json). It
defines 52 categories, one owner lane per category, metric direction,
comparator pins, and corpora. The
[strategy](plan/world-class/README.md#scoreboard) defines the groups, and the
[benchmark contract](benchmark-contract.md) defines evidence tiers. This page
records the current evidence available for each category. Start with
[benchmarks.md](benchmarks.md) for the findings and
[the evidence index](evidence/README.md) for individual investigations.

`unmeasured` below means **no qualified comparative result**, even when a local
diagnostic exists. An `invalid` historical measurement cannot support a ranking.
For performance categories, `win`, `tie`, or `loss` requires contract §6–§10
evidence. Correctness gates are listed separately: implemented checks and old
passing reports do not establish that an arbitrary current revision is green.

## Current standing

| Category | Standing | Evidence / note |
|---|---|---|
| A1 fresh encode | unmeasured | Direct first-encode-after-parse and construction diagnostics now exist; historical tonic fresh cells were derived estimates |
| A2 cached encode | unmeasured | Local tables and process gates exist; no controlled-host paired intervals |
| A3 owned decode | unmeasured | Local diagnostics exist; `rpc_sparse`/`tags_32` gates are smoke checks |
| A4 borrowed decode | unmeasured | buffa-view rows; person/TAT sit in an explicit ~3% non-gated band |
| A5 parse+touch | unmeasured | tonic survey checks all populated fields/payload bytes; semantic coverage and independent qualification remain BM-03/SB-15 work |
| A6 merge | unmeasured | No measurements |
| A7 size | unmeasured | No measurements |
| A8 clone | unmeasured | No measurements |
| A9 JSON encode | unmeasured | Not gated, no table |
| A10 JSON decode | unmeasured | Not gated, not measured |
| A11 text | unmeasured | Not gated, not measured |
| A12 dynamic decode | unmeasured | Dynamic C++/upb and hyperpb peer harnesses exist; complete matched comparisons remain open |
| A13 allocations/retained | unmeasured | Counting-allocator snapshots exist; upb C arenas and borrowed input buffers are excluded, so total-memory comparisons remain open |
| A14 1–64 MiB | unmeasured | Historical 1–5 MiB tables plus SB-13 diagnostics through 8 MiB; no full qualified 64 MiB matrix |
| A15 mutate-then-encode | unmeasured | Diagnostic declares no winners |
| B1 generation time | unmeasured | SB-09 has five-repeat seeded and realistic-corpus diagnostics; pbrs wins and loses depending on corpus/peer |
| B2 generator RSS | unmeasured | SB-09 reports process RSS, with sampling/shared-page limits |
| B3 generated bytes | unmeasured | SB-09 shows substantial output-size losses; profile/feature equivalence matters |
| B4 clean check | unmeasured | SB-09 reports broad pbrs losses; GN follow-up records measure reductions |
| B5 incremental check | unmeasured | Included in SB-09, not qualified across controlled hosts |
| B6 release build | unmeasured | SB-09 reports broad pbrs losses; later changes need a full matched rerun |
| B7 build RSS | unmeasured | Included in SB-09; same qualification gap |
| B8 binary size | unmeasured | Included in SB-09; historical six-message pair is not the whole matrix |
| B9 no protoc/no C | unmeasured | Rust frontend selected (CG-16/GN-05); integration and packaged-consumer proof remain CG-17/18 |
| C1 unary latency | unmeasured | SB-01 fair rerun favors pbrs p50, tonic empty-unary p99; old mismatched-settings tables are invalid |
| C2 QPS/core | unmeasured | Fair loopback QPS exists; controlled CPU budgets/scaling campaign remains open |
| C3 streaming msgs/s | unmeasured | Fair rerun favors pbrs server-stream/upload, tonic ping-pong; still local diagnostic |
| C4 large messages | unmeasured | SB-01/SB-13 diagnostics exist; no qualified network campaign |
| C5 instructions/RPC | unmeasured | Selected 144-capture server pilot retains gzip and uncompressed source regressions; 25/50-call diagnostics are not a qualified speed ranking |
| C6 memory | unmeasured | Matched 2,400-capture campaigns measure per-side allocations/requested bytes; large gzip and identity byte losses remain; retained-memory qualification is separate |
| C7 time to first RPC | unmeasured | SB-12 same-process diagnostic favors tonic; no qualified external-peer campaign |
| C8 CPU/RPC matched load | unmeasured | SB-11 open-loop harness exists; smoke is not a controlled campaign |
| D1 latency at load | unmeasured | grpc-go tables are separate-process but one host, best-of, no intervals |
| D2 QPS (queries per second)/core in SLO | unmeasured | No measurements |
| D3 streaming/core | unmeasured | grpc-go ping_pong/upload tables unqualified (contended, best-of) |
| D4 CPU/allocs per RPC | unmeasured | Native counter and selected instruction campaigns retain losses; native CPU and full cross-peer qualification remain open |
| D5 memory per conn | unmeasured | SB-12 diagnostics and scale scenarios exist; 100k topology/OS limits need qualification |
| D6 accept/TLS rate | unmeasured | Native TLS diagnostic and scale runner exist; required-peer comparisons remain open |
| D7 overload fairness | unmeasured | RT-07 diagnostic explicitly unqualified |
| D8 multi-core scaling | unmeasured | No stage-3 runs |
| E1 grpc_bench-style | unmeasured | SB-17 diagnostic ran; emulated client and shared Docker host limit interpretation |
| E2 WorkerService | unmeasured | SB-10 async scenarios ran; SB-21 claim definitions need full accounting and repeat-order integration |
| E3 TLS | unmeasured | Plaintext/TLS counters cover 1/64 KiB and one/16 in-flight calls; qualified latency/CPU/network comparison remains open |
| E4 compression | unmeasured | Complete native member/frame handling, tonic normalization and a 1,024-member/frame default are implemented at 90201458; fresh qualification and comparative measurements remain open |
| E5 real-network RTT | unmeasured | No stage-3 runs |

## Correctness and delivery gates

These are eligibility checks for a performance claim, not speed rankings.
Use the [current status](status.md), task records, and CI for the exact revision
being measured.

| Category | What is delivered | What still needs proof |
|---|---|---|
| F1 conformance | Conformance and upstream-Rust compatibility harnesses | All required checks pass at the campaign source revision |
| F2 interop | Go, C++, and tonic interop harnesses; complete-input handling at 90201458 | Required directions/features and resource limits pass at the campaign source pin |
| F3 HTTP/2 negatives | Negative tests and upstream HTTP/2 probes | Required negative/flow-control checks pass at the campaign revision |
| F4 gRFC coverage | Implementations and tests tracked in GF/CH/XD task lanes | Claim only the implemented subset; check each required card |
| F5 xDS interop | xDS contracts and implementation work | Independent control-plane coverage for the claimed behavior |
| F6 fuzz/Miri | QG-01 policy, targeted Miri workflow, QG-02 fuzz campaign tooling | Passing artifacts and sustained campaign coverage for the measured revision |
| F7 dependency audit | QG-04 feature-resolved audit and CI job | Audit passes for each advertised shipping profile at the measured revision |

## Benchmark claims

The current [benchmark page](benchmarks.md) reports scoped allocation and
instruction diagnostics. It retains both source regressions and tonic losses;
these measurements do not qualify native CPU, latency, throughput or overall speed.

| Statement | Categories | Evidence limit |
|---|---|---|
| Selected 1 KiB gzip allocation reductions | C6, D4 | Matched 2,400 captures per revision; selected 1 KiB, one-in-flight cases |
| Large gzip allocation-byte regressions | C6, D4, E4 | 64 KiB upload server bytes rise 9.0% with prost and 10.6% with pbrs against the old source; shared-host cumulative requests |
| Selected server instructions retain regressions | C5, D4 | 144 captures, 25/50-call samples and three-repeat ranges; full instruction scope not run |
| Historical codec and RPC tables | A–E | Archived source and original host/ownership limits apply |
| Historical 7,200-capture counter matrix | C5, C6, D4 | Invalid for declared concurrency; the generator used four workers |

The [previous claim mapping](https://github.com/mingley/pure-protobuf/blob/99eccf209fd4cda28c992b7b8855531a31c2d249/docs/scoreboard.md#benchmarksmd-claim-mapping)
retains the detailed verdicts for the archived tables.

## Next evidence to produce

1. Finish claim-runner accounting, scenario loading, repeat ordering, and
   peer-setting validation (SB-21/SB-24). SB-21's scenario files alone do not
   satisfy these; optional competitor harnesses are tracked in SB-25.
2. Verify under SB-23 that the repaired perf workflow produces usable base/head
   measurements, then collect at least 30 qualifying runs before SB-20 chooses
   blocking thresholds. Placeholder/error artifacts do not count as measurements.
3. Close QG-05 recovery and QG-06 current-backend resource/soak gates, then run
   SB-15 and SB-22 on controlled Linux x86_64 and arm64 hosts. Preserve
   every loss, unsupported cell, offered-call count, and raw artifact.
4. Publish SB-16's generated scoreboard with reproducible intervals and
   revision-specific correctness links.

See the [2026-09-29 audit](audit-2026-09-29.md) for the repository-wide
findings behind this order.

## Win rules

- **Dev-loop guidance** (SB-03/04): instructions or allocations fall ≥2% on
  targeted cells; no primary cell regresses >1% (2% RPC cells); F
  gates green. These are local comparison defaults; CI remains advisory until
  SB-20 calibrates the thresholds from observed noise. Never leadership.
- **Qualified** (SB-15/22/16): contract §6–§10 — dedicated x86_64
  and arm64 hosts, 5+ randomized paired runs, 95% intervals,
  §7 margins (≥20% CPU/RPC or throughput, ≤5% p99 regression).
- **Wording** (§10.2): every claim names cells, peer versions,
  hosts, budgets, intervals. No universal "fastest" claims.
