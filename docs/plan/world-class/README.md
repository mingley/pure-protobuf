# gRPC performance plan

Improve pbrs codec, client, and server performance. Measure CPU, allocations,
and latency on comparable workloads, and preserve protocol behavior and
resource limits. Publish the workload, competitors, and results, including losses.

**Current audit:** [2026-09-29](../../audit-2026-09-29.md), source
[`37683917`](https://github.com/mingley/pure-protobuf/tree/37683917c65fd06082a392f07e4d71bd587b18b9).
**Coordinator:** Michael Ingley.
**Start work:** [current priorities](../../../TODO.md).
**Every-cell bar:** the [dominance program](../dominance/README.md) tracks
each workload cell that still loses to tonic and prost.
**Task records:** [performance and expansion](tasks.json), plus
[foundation work](../tasks.json).

## Where we stand

The stack already has four RPC shapes, TLS, retries, resolvers, load
balancing, observability hooks, optional prost/Tower integration, and
substantial codec/codegen optimizations. The missing piece is a complete,
current proof of correctness and performance across comparable peers.

| Area | Current source and evidence | What to do next |
|---|---|---|
| Correctness | QG-05 is done: [original recovery](../../evidence/shared-map-recovery.md) at `c89608bd` passes 233 original tests plus three regressions. The [closed-enum follow-up](../../evidence/closed-enum-recovery.md) at `3a7aa128` passes the same originals plus six regressions in clean-source ordinary/Miri runs and Linux compatibility CI. | Preserve these gates and finish current-backend qualification; passing compatibility is not production certification. |
| Public guides | QG-07 is done: extracted operations recipes compile and run; the production TLS/mTLS resource-limit demo runs from a fresh consumer. API mutation fails while prose changes remain green. [Status](../../status.md#public-guide-recipes-qg-07) records the tests. | Keep these compiled checks with the documented features; they do not replace the QG-06 soak. |
| RPC efficiency | [RX-09's 48 captures](../../evidence/rx-09.md) show native unary/streaming instructions down 9.33%/8.58% and allocated bytes down 19.07%/15.93% against the pre-change source. Existing cross-stack cells differ in response work. | Match response bytes/counts and codec/handler work before any beat-tonic claim, then measure separate client/server Linux profiles. |
| Benchmark machinery | SB-21 exports scheduled latency/window accounting and executes frozen randomized repeats. SB-24 passed 144 TLS/compression wiring cells, not throughput qualification. | Normalize reference arrival rates and independent accounting; resolve TLS cipher/session-evidence gaps, complete Go/C++ parity and prove dedicated-host headroom. |
| Performance CI | SB-23 is done: two verified corrected-parser pairs have 88 cells per revision; error reports are retained and rejected. Instructions remain unavailable. | Keep Intel/AMD cohorts separate. SB-20 calibrates thresholds after 30 compatible runs, using only available metrics; the lane remains advisory. |
| Codegen | Feature separation and generated-code reductions exist. The broader recorded build-cost matrix still has many losses. | Finish GN-02/GN-03 and rerun downstream build/RSS cells. |
| Codec | Shared-buffer parsing, scalar fast paths, dynamic tables and hybrid maps exist. Generated table emission and borrowed views remain separate work. | Finish BM-03/PK-18 measurements, then prioritize the losing cells that matter to applications. |
| HTTP/2 | The current backend remains upstream `h2`; transport/runtime seams and adaptive windows exist. | H2-04 is blocked. H2-16 revisits the recorded no-go only with new evidence. |
| Production behavior | Hostile-peer, cancellation and fairness tests exist; a total-process resource guarantee and sustained qualification do not. | QG-06 qualifies the shipping backend under finite limits, slow readers, churn and a 24-hour soak. |
| Broader ecosystem | upb replacement, a Rust `.proto` frontend and xDS have distinct designs and incomplete implementations. | Continue their cards when ownership permits; keep them off the critical path to measuring today's gRPC stack. |

A merged optimization is not a measured win. A completed design card may
conclude **do not build**. Neither result closes a performance campaign.

## Scoreboard

The category list lives in [`tasks.json`](tasks.json)
(`scoreboard_categories`). SB-02 established these categories.
The word **leadership** is reserved for claim-grade results. Every
optimization card names the categories it targets.

| Group | Categories | Comparators (pinned by SB-02) |
|---|---|---|
| A. Codec | A1 fresh encode, A2 cached encode, A15 mutate-then-encode, A3 owned decode, A4 borrowed decode, A5 parse+touch, A6 merge, A7 size, A8 clone, A9/A10 JSON, A11 text, A12 dynamic decode, A13 allocations/retained memory, A14 1-64 MiB payloads | prost, buffa (owned, eager view, lazy view kept separate), `google-protobuf` (upb), C++ protobuf (arena), upb C, Go protobuf, vtprotobuf, hyperpb (A12 only) |
| B. Codegen | B1 generation time, B2 generator RSS, B3 generated bytes, B4 clean check, B5 incremental check, B6 release build, B7 build RSS, B8 binary size, B9 no `protoc`/no C compiler | prost-build + tonic-build, `protoc --rust_out` (upb), buffa-build |
| C. Client | C1 unary latency, C2 QPS/core, C3 streaming msgs/s/core, C4 large messages, C5 instructions/allocations per RPC, C6 memory, C7 time to first RPC, C8 CPU/RPC at matched load | tonic, grpc-go, grpc-java (Netty), grpc-c++, grpc-dotnet, volo-grpc, connect-rust, Google `grpc` |
| D. Server | D1 latency at fixed load, D2 QPS/core within the p99 SLO, D3 streaming/core, D4 CPU/allocations per RPC, D5 memory per connection/stream, D6 accept and TLS handshake rate, D7 overload goodput/fairness, D8 multi-core scaling | same as C, plus the Vert.x and Quarkus rows from grpc_bench |
| E. End-to-end | E1 grpc_bench-style, E2 official WorkerService scenarios, E3 TLS, E4 compression, E5 real-network RTT | same as C/D |
| F. Gates | F1 conformance and unmodified upstream Rust tests, F2 interop, F3 HTTP/2 negative tests and h2spec, F4 gRFC coverage, F5 xDS interop, F6 fuzz/Miri, F7 pure-Rust graph | These must never regress. A win that breaks a gate does not count. |

Evidence comes in two tiers. They are never mixed.

| Tier | Purpose | Rule |
|---|---|---|
| **Dev-loop** (SB-03/04) | Fast iteration on any Linux host and in CI | Instructions retired, exact allocation counts, syscalls and copies per operation, then wall time. A change is a **dev-loop win** when instructions or allocations fall by at least 2% on the targeted cells and no primary cell regresses by more than 1% (2% for RPC cells), with all F gates green. Results are labeled "dev-loop" and are never called leadership. |
| **Claim-grade** (SB-15, SB-22, SB-16) | Publishable category results | [Benchmark contract](../../benchmark-contract.md) §6-§10: dedicated x86_64 and arm64 hosts, five or more randomized paired runs, 95% intervals, percentile sample floors, and the §7 margins (for example ≥20% lower CPU per RPC, or ≥20% higher sustainable throughput, with no p99 regression above 5%). |


## Strategy

1. **Preserve trustworthy gates.** Retain the QG-05 compatibility recovery,
   QG-07 compiled recipes, conformance, interop, hostile-input and pure-Rust
   checks. No speedup earns a pass by dropping validation or changing semantics.
2. **Complete comparable measurements.** Finish SB-21 and SB-24, then calibrate SB-20.
   Freeze workloads and effective settings; verify offered/completed counts
   and generator headroom. Retain raw paired observations.
3. **Improve the measured limiting path.** Finish RX-09 on the current
   backend. Attribute client and server costs separately, including TLS,
   compression, scheduling, allocation and copies. Pick one hypothesis per
   change, keep a before/after comparison, and reject regressions.
4. **Work the independent codec/codegen lane.** Close the measurements for
   BM-03, PK-18 and GN-02/GN-03. Optimize fresh, reused and mutated messages
   separately. Reduce build costs without hiding runtime or memory losses.
5. **Qualify and reproduce.** QG-06 tests the current transport under
   sustained failure/load. SB-15 and SB-22 then run the frozen matrix on
   dedicated x86_64 and arm64 hosts. SB-16 publishes reproducible results.
6. **Broaden the claim only as evidence broadens.** SB-25 runs Java/.NET and
   other relevant competitors. A result against tonic alone is a result
   against tonic alone. Revisit a custom engine through H2-16 only when
   current profiles justify its correctness and maintenance cost.

Useful experiments may end in rejection. Keep the negative result and the
reason in [evidence](../../evidence/README.md) or
[closed experiments](../../inventory/README.md) so the next contributor
does not repeat it.

## Target architecture

The shipping path is:

```text
application + generated stubs
  -> pbrs messages or optional prost codec
  -> client channel / server routing and handlers
  -> HTTP/2 transport seam (upstream h2)
  -> runtime seam (Tokio)
  -> TCP or Unix sockets, with optional TLS
```

The [architecture guide](../../architecture.md) describes implementation.
The [decision records](../../decisions/h2-engine.md) explain alternatives.
The official `rust_out` compatibility runtime is a separate incomplete
path; sharing application API names does not make it a complete upb kernel.
A future `pbrs-h2` backend is conditional and must not appear in shipping
support claims.

## Milestones and exit criteria

Milestones group outcomes. They are not dates, and tracks run in parallel:
M4 (upb) and M5 (tonic) can start during M0.

| Milestone | Exit criteria |
|---|---|
| **M0** Truthful baseline and fast feedback | Tonic comparator fixed and its old claims retracted (SB-01). The seven core official scenarios run end-to-end as diagnostics (SB-10). Dev-loop harness and CI lane live (SB-03/04). Docs reconciled with the landed A6 retry work (MX-00) and monoliths split (MX-01..05). Pure-Rust audit and Miri policy in CI (QG-04/01). Corpora frozen (SB-05). Scoreboard published with honest standing (SB-02). |
| **M1** Codec and codegen wins | Dev-loop wins on at least 80% of primary A cells against equivalent comparators, including generated C++ protobuf and upb C for typed claims; no unexplained loss above 5%. Claim-grade leadership uses contract §7.2 and SB-15. B4/B6 beat prost-build and upb on 100/1,000-message corpora. The optional Rust frontend has its own GN-07b/GN-09 exit and does not block measuring native gRPC. |
| **M2** Transport wins on the `h2` backend | On the same host, pbrs-grpc wins dev-loop C2 and D2 cells at 1 and 4 CPUs against the required peers (tonic, grpc-go, grpc-c++; SB-11) and every optional peer that runs (SB-18). Its grpc_bench entry beats the re-measured leaders in a diagnostic reproduction (SB-17). F2/F3 green. The transport seam has landed (H2-02). |
| **M3** Conditional engine and runtime experiments | Start engine implementation only after H2-16 records GO. If built, one candidate must improve primary-cell CPU/RPC by at least 15% and pass H2-15 (h2spec, negative interop, 24 h soak, 24 CPU-hours of fuzzing per target) before H2-14 considers the default. A NO-GO is a valid outcome. Runtime modes are retained only where measured scaling/fairness improves. M3 does not gate claims on today's backend. |
| **M4** upb replacement kernel | Rerunning the UK-01 inventory shows zero stubs (UK-10). Every upstream shared suite passes unmodified (UK-11). The rust_out conformance testee has zero unexpected results (UK-12). Dev-loop wins against upb C on Google's messages (UK-13). Unmodified `grpc` helloworld and downstream workspaces run on the `[patch]` facades for every supported release pin (UK-14), with a scheduled drift gate (UK-19). Upstream packet ready (UK-16). |
| **M5** Better tonic | Prost messages run over the native transport (TC-03). Axum co-hosting example (TC-04). tonic's examples ported with published diffs (TC-07). Parity matrix published (TC-08). |
| **M6** Fleet | Channel architecture with every LB policy qualified against the picker budget (CH-11). Core xDS (A27/A28/A30/A29/A36) passing local psm-interop cases (XD-10). |
| **M7** Observability, security and full gRFC profile | GF-09: every merged A- and G-series gRFC in the [gRFC matrix](../../grfc.md) is **shipped with tests** or an **approved boundary with a reason**; nothing is left planned or partial. This feeds legacy QL-04. |
| **M8** Claims | Claim-grade codec/codegen (SB-15) and transport/end-to-end (SB-22) campaigns and the generated scoreboard (SB-16) feed QL-03. Wording follows contract §10.2. |


## First assignments

Use the ordered [current queue](../../../TODO.md#current-priorities). The
initial 2026-09-26 assignments have largely landed and are no longer the
starting wave. QG-05 and QG-07 are delivered. Current ownership should
prioritize SB-21/SB-24; SB-20's noise study after SB-23; equivalent-work RX-09
measurements; and the unfinished codec/codegen measurements.

Hardware-backed campaigns are operator tasks. Prepare commands, pins and
artifact schemas before booking hosts. This plan does not authorize spending
or replace the per-task evidence requirements.

## Worker protocol

Use the [foundation execution contract](../README.md#small-executor-contract)
with the rules below. Authorization already given by the maintainer applies;
do not ask again for the same work. External publication, new spending and
changes outside the assigned scope remain separate decisions.

1. **Pick and claim.** Run the [ready query](#ready-query) and claim one card
   through the coordinator, who sets `status: in_progress` and `implementer`.
   Work from the exact base SHA in a separate worktree, on a branch named
   `<user>/<card-id>-<slug>`.
2. **Check the premise first.** Read the card, its dependencies' artifacts and
   the current source. If the gap is already closed, or a measurement
   disproves the hypothesis, report the evidence and stop. The coordinator
   re-scopes the card. A disproved hypothesis is a useful result.
3. **Measure before editing (every PK, GN, CL, SV, H2 and RX optimization).**
   Capture dev-loop baseline JSON and a flamegraph on the base SHA. State one
   hypothesis and name the target categories. Make one coherent change.
   Capture the after-JSON and compare it. Fill in the perf-PR template
   ([pull_request_template.md](../../../.github/pull_request_template.md),
   SB-14; kit: [profiling.md](../../profiling.md),
   `scripts/profile.sh`). A change that fails the dev-loop win rule does
   not merge as a performance change.
4. **Gates never regress.** Run the card's `checks` plus every gate it can
   affect: conformance (F1) for codec/codegen changes; interop and hostile
   tests (F2/F3) for transport changes; Miri for new `unsafe`. Add a failing
   regression test before fixing a defect.
5. **Scope.** Stay inside the card's `write` list. Change generated files
   only through the regeneration scripts. An `L` card may span several PRs,
   but each PR must leave the tree green. The coordinator may split any `L`
   card into lettered children (for example `XD-04b`) before assigning it.
   If scope grows, split before continuing. Never run two cards listed
   together under one `write_hotspots` path at the same time; that list
   includes open legacy cards. Legacy cards that name a pre-split file (for
   example FL cards on `pbrs-grpc/src/client.rs`) write the matching
   post-split submodule, and must not run while the MX split is in flight.
6. **Dependencies and unsafe.** Get review before adding any dependency;
   shipping profiles must pass QG-04. Every new `unsafe` needs a `SAFETY`
   comment, an entry in [unsafe invariants](../../unsafe-invariants.md) and
   Miri coverage.
7. **Claims.** Label every number `dev-loop` or `claim-grade`. Never write
   "fastest" without a named matrix. Never add a code path that fires only
   under benchmark traffic ([contract §10.3](../../benchmark-contract.md#103-no-benchmark-specific-escape-hatches)).
8. **Return.** Report the diff or commit, the exact commands and results,
   dev-loop JSON paths, pins and remaining limits. A card is `done` only after
   review confirms every `accept` item.
9. **Stop and escalate** on: a public API or crate-name decision; any change
   to protocol semantics or security defaults; a threshold change; a new
   dependency; any external action (upstream PRs or comments, publishing,
   cloud spend, sending data to third parties). Each of these needs explicit
   maintainer approval.

Card fields: `executor` is `small` (one focused worker), `coordinator`
(design or decision; produces a record in `docs/decisions/`) or `operator`
(needs hardware, people or external approval). `size` is S (about one PR),
M (a few PRs) or L (several PRs; split whenever possible). `relates` points
to legacy cards whose intent the card carries. `planned_checks` are commands
that do not exist yet; the card named in `introduced_by` must create and
document them, and every card that cites one depends on that card.
`write_hotspots` lists paths shared by cards with no dependency order
between them; the coordinator serializes those.

Assignment prompt for a worker:

```text
Implement CARD_ID at BASE_SHA, and nothing else.
Read docs/plan/world-class/README.md (worker protocol), the CARD_ID object in
docs/plan/world-class/tasks.json, docs/plan/README.md (small-executor contract)
and the artifacts of its dependencies. Stay within the write scope.
For optimization cards, capture dev-loop baseline JSON before editing.
Meet every accept item; run the listed checks and affected gates.
Do not add dependencies, change public APIs or thresholds, or take external
actions without approval. Stop with a bounded blocker if scope expands.
Return changed files, commands and results, dev-loop JSON and limitations.
```


## File ownership

The major modules have already been split. Assign current files, not obsolete
monolithic write paths.

| Shared area | Lanes | Coordination |
|---|---|---|
| `src/codegen/` | GN, PK, TC | One writer per emitter module; regenerate affected output with the change. |
| `src/runtime/`, `src/map.rs` | QG-05, UK, PK | Restore enum-map correctness before overlapping map/kernel optimizations. |
| `pbrs-grpc/src/client/`, `server/`, `wire/` | RX, CL, SV, QG | Serialize per module; preserve cancellation, limits and stream semantics. |
| `rpc-bench/`, `bench/stack-matrix/` | SB-21, SB-24, SB-25 | Split write sets before parallel assignments. |
| `bench/devloop/`, performance workflow | SB-23, SB-20, optimization authors | One workflow owner; review new measurement cells. |
| Task records and evidence summaries | Coordinator | Update status only after reviewing the actual acceptance evidence. |

## Guardrails

| Gate | Existing entry point | Required for |
|---|---|---|
| Protobuf behavior | `./scripts/conformance.sh`, core/generated tests | Codec and codegen changes |
| Official compatibility | `./scripts/test-rust-out-shared.sh` | Runtime/kernel changes; QG-05 first |
| Cross-peer behavior | `./scripts/grpc-interop.sh`, C++ and tonic runners | Transport changes |
| Malformed HTTP/2 and resource behavior | HTTP/2 interop scripts, hostile/lifecycle tests | Transport and new engines |
| Unsafe correctness | Miri and fuzz workflows; [invariants](../../unsafe-invariants.md) | Unsafe/parser changes |
| Pure Rust | `./scripts/pure-rust-audit.sh` | Shipping dependency/profile changes |
| Documentation and queue | `cargo test --test documentation`, `python3 scripts/plan-lint.py` | Guides, contracts and cards |

Local smoke tests do not replace missing upstream suites, long fuzz
campaigns or a soak. Explicitly record `not_run`, failures and exclusions.

## Decisions required

Read a decision's outcome before scheduling its dependents. In particular:

- H2-01 is **NO-GO** for a new engine. H2-03 records a possible design;
  H2-16 must record a reviewed **GO** before H2-04 can be unblocked.
- SB-20 still needs noise measurements before performance CI can block merges.
- GN-05, UK-02 and XD-01 remain in progress: their design drafts exist, but
  their recorded decision review is unfinished. Dependent implementation
  stays out of the ready queue until that acceptance is met.
- New crates/names, support commitments and public API changes need a
  concrete reviewed proposal. Existing decisions are in
  [the documentation map](../../documentation-map.md).
- UK, frontend and xDS acceptance remain independent of a bounded native
  gRPC release. Release promotion follows [RELEASE.md](../../RELEASE.md).

## Risks

| Risk | How the plan detects it |
|---|---|
| Winning a harness artifact | Scheduled latency, equal peer settings, holdout workloads and mixed-peer directions |
| Optimizing the wrong subsystem | Separate endpoint profiles and end-to-end regression cells |
| Trading throughput for tail latency or memory | Fixed-load p99, sustainable throughput within the SLO, retained-memory and soak checks |
| Declaring completion too early | Acceptance evidence, explicit blocked decisions and a generated queue |
| Broad claims from narrow competition | Executed peer lists, missing rows and separate broad-peer work |
| Documentation drift | Task-specific guides, repository link/snippet checks and one authoritative status source |

## Relationship to the existing plan

Both JSON files are authoritative for their own cards. The `reconciliation`
array in [this plan's cards](tasks.json) maps older work to newer executors:
`executed_by` hides duplicated legacy assignments; `retained` keeps the
older task active. `gate` adds closure requirements. The queue resolves those
relationships transitively.

The [foundation plan](../README.md) retains its original audit and suite
contracts as reference. The [roadmap](../../ROADMAP.md) owns product promotion
packages. [TODO.md](../../../TODO.md) gives the short ordered queue.
No prose table is a second source of task status.

## Ready query

```sh
python3 scripts/plan-status.py --ready
python3 scripts/plan-status.py --card RX-09
python3 scripts/plan-status.py --lane SB
python3 scripts/plan-status.py --summary
```

The commands apply defaults and reconciliation gates across both plans.
Blocked decisions and work already in progress never appear as unclaimed
ready work. Run `python3 scripts/plan-lint.py` after changing cards.

## Card index

Use the lane commands below for current titles, states and dependencies.
This replaces the duplicated static index whose readiness markers had become
stale.

### MX: Maintainability splits that unlock parallel workers

Maintainability splits that unlock parallel workers. Read current cards with
`python3 scripts/plan-status.py --lane MX`.

### SB: Scoreboard and evidence engine

Scoreboard and evidence engine. Read current cards with
`python3 scripts/plan-status.py --lane SB`.

### PK: Protobuf kernel performance (plugin/owned path)

Protobuf kernel performance (plugin/owned path). Read current cards with
`python3 scripts/plan-status.py --lane PK`.

### GN: Code generator and pure-Rust frontend

Code generator and pure-Rust frontend. Read current cards with
`python3 scripts/plan-status.py --lane GN`.

### UK: upb replacement kernel for official rust_out gencode

upb replacement kernel for official rust_out gencode. Read current cards with
`python3 scripts/plan-status.py --lane UK`.

### H2: gRPC-specialized sans-IO HTTP/2 engine

gRPC-specialized sans-IO HTTP/2 engine. Read current cards with
`python3 scripts/plan-status.py --lane H2`.

### RX: Runtime, IO, TLS and compression

Runtime, IO, TLS and compression. Read current cards with
`python3 scripts/plan-status.py --lane RX`.

### CL: Client fast paths

Client fast paths. Read current cards with
`python3 scripts/plan-status.py --lane CL`.

### SV: Server fast paths

Server fast paths. Read current cards with
`python3 scripts/plan-status.py --lane SV`.

### TC: Better tonic: ecosystem and migration

Better tonic: ecosystem and migration. Read current cards with
`python3 scripts/plan-status.py --lane TC`.

### CH: Channel architecture, resolvers and load balancing

Channel architecture, resolvers and load balancing. Read current cards with
`python3 scripts/plan-status.py --lane CH`.

### XD: xDS

xDS. Read current cards with
`python3 scripts/plan-status.py --lane XD`.

### GF: Observability, security and remaining gRFCs

Observability, security and remaining gRFCs. Read current cards with
`python3 scripts/plan-status.py --lane GF`.

### QG: Quality gates for new kernels and engines

Quality gates for new kernels and engines. Read current cards with
`python3 scripts/plan-status.py --lane QG`.

## Sources

This revision uses the [repository audit](../../audit-2026-09-29.md), pinned
source, CI logs and the [evidence index](../../evidence/README.md).
The original 2026-09-26 program also consulted the
[official gRPC benchmarking guide](https://grpc.io/docs/guides/benchmarking/)
and [Google's Rust design decisions](https://protobuf.dev/reference/rust/rust-design-decisions/).
Those references explain context; local paired measurements determine wins.
