# Leadership execution plan

**Snapshot:** 2026-09-18. **Source baseline:**
[`139af0c2559ebc36ecae86c647482a758dff4d64`](https://github.com/mingley/pure-protobuf/tree/139af0c2559ebc36ecae86c647482a758dff4d64).
**Coordinator and scope approver:** Michael Ingley.

The objective is leading protobuf code generation, documentation, codec
performance, and **both client and server** gRPC performance in Rust, without
trading away Google's protocol semantics. This plan supplements the stable
[GR work packages](../ROADMAP.md), not a second competing roadmap.
[`tasks.json`](tasks.json) is the authoritative, dependency-ordered work queue.
It contains bounded assignments rather than instructions to "make gRPC fast."
There are 127 initial cards across 14 lanes. This is a staged program, not 127
simultaneous jobs or a claim that speculative architecture is already settled.

The [large-payload zero-copy proposal](pbrs-zero-copy-large-payloads-plan.md)
sets out a separate, measurement-gated experiment. Its proposed ZC cards are
not yet part of `tasks.json`; Phase 0 must establish the baseline before any
runtime optimization is eligible to ship.

This is a source audit and execution plan, not a new benchmark result or a
certification. The baseline's [completed CI run](https://github.com/mingley/pure-protobuf/actions/runs/33948308400)
passed eight jobs, including conformance, MSRV, macOS, package consumers and
generated-output checks. That result dates to 2026-09-05. This planning pass did
not rerun performance, fuzz, soak, or official upstream campaigns.

## What is actually missing

| Area | Evidence at the baseline | Next work |
|---|---|---|
| Already delivered | Fresh-directory messages/native/tonic onboarding, source-bind repair, separate MSRVs, package consumers, and one CI-gated publisher exist. See [TODO](../../TODO.md) and [CI](../../.github/workflows/ci.yml). | Preserve these gates. Do not spend another implementation cycle recreating GR-01/02's delivered slices. |
| Trustworthy gRPC evidence | [grpc-interop.sh](../../scripts/grpc-interop.sh) runs 18 self cases, 14 Go cases in each direction, fetches Go without a version pin, exits successfully when Go/fetch/build is missing, and discards individual attempt logs. Compression is self-only. | GT-01 through GT-06; IO-01 through IO-10. A green job currently cannot prove every required cross-peer pass ran. |
| Exact official assertions | [interop_cases.rs](../../pbrs-grpc/src/interop_cases.rs) accepts clean EOF or another message after `cancel_after_first_response`; the official case requires terminal `CANCELLED`. The CLI binaries ignore unknown flags and expose plaintext serving/dialing, despite library TLS support. | IO-01 through IO-04. A case name alone is not a conformance proof. |
| Retry correctness | [client.rs](../../pbrs-grpc/src/client.rs) retries unary/server-streaming once on `status.is_transport()`, including failures after request transmission. This is broader than a proof that the server application never saw the request. | RT-01 through RT-03, before performance changes. Reproduce execution ambiguity and review against gRFC A6; do not infer safety from a successful reconnect test. |
| Codegen diagnostics and configuration | [Config](../../src/codegen.rs) has output/stub/dependency switches, uses ambient environment and thread-local state, ignores the request parameter field, and returns the runtime's unit [ParseError](../../src/error.rs), which discards diagnostic messages. | CG-01/02. Preserve the runtime error shape while giving build-time users useful errors. |
| Multi-file codegen | `compile_protos` reduces inputs to basenames; `file_matches` also matches stems; outputs use `{stem}.rs`. Imported files are included in descriptors, but only explicit inputs get Cargo rebuild directives. | CG-03 through CG-06: reproduce same-stem/import hazards, approve compatible naming, then fix dependency tracking and path mapping. |
| Generated API quality | Source comments are not requested/preserved/emitted; there is no explicit runtime-crate remapping or generated/runtime upgrade matrix. Broad generated lint allowances hide quality problems. | CG-07 through CG-11. Keep existing message/enum/name/presence tests. File, enum and method custom options already have coverage in [dynamic.rs](../../tests/dynamic.rs); old notes saying otherwise are stale. |
| Rust-only toolchain | Core builds without `protoc`, but the recommended generation path and both adapter build scripts require it. [json.rs](../../src/json.rs) calls libc `strtod`. TLS/compression deliberately select Rust implementations in the [native manifest](../../pbrs-grpc/Cargo.toml). | PB-01 and CG-15 through CG-18. Separate an entirely Rust implementation from "no C compiler needed for this particular build." |
| Protobuf completeness and hardening | v35.1 conformance is pinned through Edition 2023. [fuzz_parse.rs](../../tests/fuzz_parse.rs) has four fixed inputs; excluded `rust_out_shared` tests are not part of workspace CI. Edition 2024 and first-class borrowed views remain outside the advertised contract. | PB-02 through PB-07, CG-12 through CG-14; OP-06 for a measured borrowed-view decision. These are different gaps, not a reason to replace the working parser. |
| Client/server performance evidence | [rpc-bench](../../rpc-bench/src/main.rs) shares one runtime between clients and servers. [Server comparisons](../../scripts/grpc-server-bench.sh) use one pbrs client; short best-of windows and host-specific reports do not isolate client efficiency or establish network/multicore leadership. | BM-01 through BM-13. Add mixed-peer directions, separate processes, official worker integration, offered-load accounting, endpoint CPU and uncertainty. |
| Codec/codegen performance evidence | Existing wins are real recorded cells, but `Person` is handwritten, packed encoding benefits from reuse, and owned/view work differs. `name_80`, large packed-fixed encode, JSON/text fallback and retained backing buffers are unresolved axes. | CG-19/20, BM-03, OP-01 through OP-06. Keep fresh, mutated, cached, owned and borrowed workloads distinct. |
| Documentation quality | The [gRPC guide](../grpc.md) is 2,520 lines and the [status page](../status.md) 1,214 at this baseline, with repeated comparisons. Some guides still say the crates are unpublished. [serving.rs](../../pbrs-grpc/tests/serving.rs) enforces large exact prose fragments. | DX-01 through DX-08: task-oriented guides, compiled recipes, versioned API/migration docs and focused documentation contracts. More prose is not the missing feature. |
| Production and fleet behavior | Four RPC shapes, gzip, TLS/mTLS, UDS, health, reflection, limits, connection pooling and hostile-peer tests already exist. Global connection/RPC limits default to `None`. Endpoint refresh/LB, policy retries, ORCA/xDS, cloud credentials and standard telemetry need additional work/evidence. | RT-04 through RT-09, FL, EX and OB lanes. Per-message limits do not establish a bounded process. Do not rebuild existing features from scratch. |

Open work must be reconciled before editing the same files:
[#68](https://github.com/mingley/pure-protobuf/pull/68), head
`b483bad9ffb1070d4be17a612fefe8510819e9a7`, proposes FieldMask JSON/text
specialization; it is **not merged** at this snapshot. OP-04 starts by checking
its current state. [Discarded experiments](../inventory/README.md), including
the flattening and heap-copy drafts, are evidence, not patches to revive blindly.
The open release PR is not evidence of qualification or permission to publish.

## Define the goals before claiming them

### Pure Rust

The shipping codec, generator, RPC transport, TLS and compression algorithms
should be Rust implementations. OS interfaces through Rust's standard library
or platform bindings remain allowed; "pure Rust" does not mean no syscalls,
no assembler, or no reviewed `unsafe`. A runtime FFI call for number conversion
is different from an OS socket operation and is tracked for replacement.

The desired **Rust-only generation profile** must compile `.proto` inputs and
build fresh packaged consumers without `protoc` or a C/C++ compiler. Keep the
existing protoc plugin and descriptor-set entry points for interoperability.
CG-16 requires review of an existing Rust frontend before considering a new
parser: do not ask a small executor to invent a protobuf compiler.

C++/Go/Java reference peers, Google's C++ conformance runner, and upstream
performance drivers are isolated **test tools**, not shipping dependencies.
Audit feature-unified dependency graphs for each shipping profile; a
`default-features = false` line is not a whole-graph proof. No new dependency,
plugin or downloaded script is approved merely by being mentioned here.

### "All official Google gRPC tests/gates"

There is no single universal gRPC certification command. Use a **versioned case
registry** spanning the upstream specifications and runners, with a result per
case, direction, peer, transport and profile. Protobuf conformance, gRPC interop,
HTTP/2 negative tests, backoff, xDS and performance are different suites.

The long-term full-profile goal stays open until every applicable active case
is covered. A bounded HTTP/2 release may qualify earlier, but must say exactly
what it excludes. Missing credentials, unsupported ORCA/xDS, a peer skip, or an
unimplemented test adapter is **not a pass** and cannot be relabeled
"not applicable" simply to close the full-profile goal.

| Official family | Baseline coverage and required addition |
|---|---|
| Protobuf binary/JSON/text | Keep v35.1 required twice and recommended, maximum Edition 2023; recorded counts are 5,631 binary/JSON plus 909 text. Retain logs, counts, skip lists and descriptor/compiler SHAs. Raise editions only after semantic fixtures. |
| Upstream Rust application semantics | Run pinned original `rust/test/shared` consumers separately from the in-tree behavior ports. Kernel-specific C/C++ arena internals are not portable black-box tests; record why an exclusion is truly inapplicable. |
| Standard gRPC interop | Register both the description and runner inventories, not just the 18 local names. Add an independent compression-capable peer; validate terminal status, payload contents/counts, metadata and compression observations. |
| HTTP/2 negative interoperability | Include `goaway`, `rst_after_header`, `rst_during_data`, `rst_after_data`, `ping`, `max_streams`, `data_frame_padding`, and `no_df_padding_sanity_test`. Also evaluate the runner's distinct server `tls`/`framing` probes. Local hostile tests are complementary, not substitutes. |
| Connection backoff | Adapt to the official ReconnectService/C++ server contract, including its full approximately 540-second exercise. Fast fake-clock tests do not replace this scheduled interoperability run. |
| Soak and newer cases | Include `rpc_soak`, `channel_soak`, connection scaling, `pick_first_unary` where present in peer runners, and a disposition for `cacheable_unary`. Spec and runner inventories differ; do not union names without documenting the procedures. |
| Extended fleet/cloud | ORCA, xDS/LB, six auth cases in the pinned runner, and ALTS transport need explicit implementation/scope decisions and proof. Fake local credentials prove mechanics only. Real credential/cloud runs require separate operator approval and remain blocked without it. |
| Official performance framework | Implement BenchmarkService/WorkerService integration and compare using pinned scenarios/driver. This supplies comparable measurements; it is neither a correctness pass nor proof of being fastest. |

GT-01 must represent `passed`, `failed`, `not_run`, `unsupported`,
`blocked_external`, and `not_applicable` distinctly. Every non-pass needs a
reason and owner. Required-profile aggregation fails on anything but `passed`;
true not-applicable exclusions require maintainer review before the run.
Flaky first attempts stay visible and block qualification until triaged.

### Measurable leadership

These are proposed acceptance budgets to approve in BM-01, not measured
achievements. Changing a budget requires a decision recorded **before** rerunning.

| Dimension | Acceptance contract |
|---|---|
| Codegen correctness | Reproducible bytes for identical descriptor/config inputs; same-stem packages, public/transitive imports, unusual names, custom options, oneofs, extensions and supported editions have external-consumer fixtures. No API break without migration/version handling. |
| Codegen cost | Measure generation time, generated bytes, clean/incremental `cargo check`, release build time/size and peak RSS on small, 100-message and 1,000-message multi-file corpora. A no-change invocation does not rewrite output. Baseline first; proposed regression budget is 5% for time/RSS, outside measured noise. |
| Docs | All advertised examples compile/run from packaged consumers; zero broken local links; four RPC shapes and production recipes are runnable. Two unfamiliar users reach a working service in 15 minutes, excluding separately recorded tool installation time. |
| Codec | Compare equivalent schemas and materialization/access work. Record first encode, cached encode, mutation then encode, parse-only, parse-and-touch, maps, WKT/JSON/text and retained memory. No aggregate hides a losing case or substitutes handwritten code for generated code. |
| Client performance | Against the same independent server, measure successful RPC/s per client CPU-second, allocations, RSS, queue/scheduling delay and latency at matched offered load. Do not let server saturation hide the client ceiling. |
| Server performance | Against the same independent load generator, measure successful RPC/s per server CPU-second and tail latency under identical request/handler work. Verify the generator has spare capacity. |
| End-to-end leadership | On preregistered primary workloads, target at least 20% lower CPU/successful RPC **or** 20% higher sustainable success throughput than the strongest measured baseline at the same latency/error/resource budget; no unexplained >5% p99 regression. A codec win cannot satisfy client or server gates. |
| Statistical evidence | At least five randomized paired runs per cell, at least 60 seconds measured after warmup, raw histograms/counts, and 95% uncertainty intervals across runs. Report p50/p95/p99/p99.9; qualify p99.9 only with at least one million observations or a separately approved precision analysis. Insufficient data is inconclusive. |
| Network and scaling | Separate processes and real-network runs on dedicated Linux x86_64 and arm64 hosts; compare 1/2/4/8 cores within host limits, concurrency and equal connection counts. Include plaintext/TLS, gzip off/on, empty/1 KiB/64 KiB/1 MiB/mixed payloads, all RPC shapes, cold connect, steady state and overload. Avoid an impractical full Cartesian product: freeze representative cells and pairwise stress cases first. |
| Reliability | Zero corruption, forbidden replay or lost acknowledged stream elements; 1,000 seeded fault cycles per advertised transport. Explicit byte/task/connection/RPC budgets; 24-hour then 7-day qualification; quiescent tasks/permits recover and steady-state RSS has no unexplained growth. |

Use open-loop offered load and retain scheduling lag, timeouts, rejected work
and incomplete calls. Never compute a flattering latency distribution by
dropping slow failures. Separate client CPU, server CPU and their sum; attach
hardware, allocator, toolchain, flags, peer SHAs, kernel/NIC, TLS cipher,
compression level and application semantics to each result.

"Fastest in the world" is an aspiration, not a claim supported by a finite
benchmark set. The publishable outcome is leadership on a named, reproducible
matrix, including losses and independent reruns. Reuse `h2`, Tokio and rustls
unless a profile and a reviewed experiment justify replacing a component.

## Execution order and ownership

Tasks are assigned in dependency order, then priority. Wave numbers are
milestones, not dates or a promise that all tasks in a wave can run together.
Only tasks with satisfied dependencies and no overlapping write scope may run
in parallel.

Use an approved low-cost model for one `small` card at a time. Reserve the
coordinator for design, protocol/unsafe/API decisions and review; operators
provide access, hardware or human evidence. Model size does not relax any
acceptance condition, and an executor must return a blocker rather than guess.

| Wave | Purpose | Lanes |
|---|---|---|
| 0 | Make evidence honest, settle contracts, remove unsafe replay ambiguity and expose codegen errors. | GT-01/02/03/04, RT-01/02/03, CG-01/02/03, BM-01, DX-01, PB-01 |
| 1 | Close required baseline protocol/codegen gaps and stand up independent measurements. | Remaining GT, IO baseline, CG import/docs/compatibility, PB, RT, BM harness |
| 2 | Make Rust-only builds, excellent docs, observability and official benchmark integration usable. | CG frontend/cost, DX, OB, BM worker/host/statistics |
| 3 | Optimize separate measured codec, client and server limits. | OP, CP, SP; never ahead of their listed correctness/measurement dependencies |
| 4 | Extend discovery, safe policy retries and the full official profile. | FL and EX; designs precede implementation, external access is approval-gated |
| 5 | Independently qualify, publish evidence and make per-crate promotion decisions. | QL; no automatic release or deployment |

Mapping back to the stable packages:

| Existing package | Detailed lane |
|---|---|
| GR-01 / GR-02 | Delivered onboarding/release slices remain done; DX, CG and GT add stronger contracts. |
| GR-03 | GT and IO, plus the official cells in FL/EX. |
| GR-04 | CG and PB. |
| GR-05 | RT, with failure/resource checks retained during every optimization. |
| GR-06 / GR-07 | FL, with RT retry correctness required first. |
| GR-08 | OB and operational documentation in DX. |
| GR-09 / GR-10 | BM and CG cost baselines, then OP/CP/SP optimizations. |
| GR-11 | QL, separately for core/native/tonic and each support profile. |
| GR-12 | Edition 2024, Rust-only frontend, borrowed-view decisions and EX. The broad target is tracked without making every extension a bounded-profile release prerequisite. |

The first safe parallel assignments are **GT-01** (case registry), **CG-01**
(diagnostics), **RT-01** (retry contract/reproducer), **BM-01** (benchmark
contract), **DX-01** (docs map), and **PB-01** (Rust numeric parsing).
Their write scopes are disjoint. RT-01 and BM-01 need coordinator review;
do not send them to an unattended small executor as architecture decisions.

## Small-executor contract

`tasks.json` has a shared default of `pending` and `small` executor; a task may
override either. Missing a `status` is not completion. `coordinator` tasks
produce approved designs or decisions, and `operator` tasks require real
resources/people or explicit external-action approval. Their dependents remain
blocked until those artifacts exist. File paths are repository-relative;
`read` gives starting context, not permission to skip applicable instructions.
`write` is the ownership boundary, including proposed files not yet created.

Every assignment must include one task object, this contract, the dependency
outputs and an exact base SHA. A small executor must not reconstruct the whole
roadmap or independently choose an architecture.

1. Check status, dependency evidence, current source and active PR ownership.
   If the claimed gap is already fixed, report that evidence and stop rather
   than rewriting it. The coordinator decides whether to close or re-scope.
2. Read the listed sources and relevant tests. Restate the one acceptance
   behavior. Add a regression fixture before repairing an observed defect.
3. Make one coherent change, normally at most three hand-edited implementation
   files plus routine module/build wiring and focused tests/docs, all within
   the listed scope. Generated output may fan out only through the existing
   regeneration path. Do not hand-edit generated files.
4. Run the listed existing checks and add/run the new behavioral assertions in
   `accept`. A command reference is a starting regression check, not proof that
   a future harness exists. New commands must be implemented and documented by
   the task that introduces them.
5. Return the diff/commit, exact commands/results, source/tool pins, evidence
   paths and any remaining limitation. Mark `done` only after review confirms
   every acceptance condition. `checks` are not allowed to silently skip.
6. Stop and escalate when the patch crosses ownership boundaries, needs a new
   dependency or public API decision, changes protocol semantics/unsafe
   invariants, changes a threshold, or exceeds one focused patch. Split first;
   do not let a cheap executor turn a task into a subsystem rewrite.

For measurement tasks, a result that disproves the optimization hypothesis is
a useful outcome, but not a performance gate pass. Keep measured no-change
decisions; do not create an artificial code diff to close a task.

Use separate worktrees for concurrent implementations. Two tasks touching
`src/codegen.rs`, `pbrs-grpc/src/client.rs`, shared fixtures, the CI workflow, or
the same benchmark harness must be serialized even if the dependency graph
would otherwise allow them. Claims/state updates go through the coordinator.
New dependency manifests must be reviewed before installation; cloud use,
deployment, posting, pushing and publication need separate approval. This plan
authorizes none of those actions.

Suggested assignment:

```text
Implement TASK_ID at BASE_SHA, and nothing else.
Read docs/plan/README.md, the TASK_ID object in docs/plan/tasks.json,
and its dependency artifacts. Stay within write scope.
Preserve existing behavior except the stated correction.
Meet every accept condition; run the listed checks plus the new targeted test.
Do not lower thresholds, add silent fallbacks, install unreviewed dependencies,
publish, or change architecture. Stop with a bounded blocker if scope expands.
Return changed files, proof commands/results and the evidence for review.
```

Read a single card without an additional tool installation:

```bash
python3 - GT-01 <<'PY'
import json, sys
with open("docs/plan/tasks.json") as f:
    plan = json.load(f)
card = next(t for t in plan["tasks"] if t["id"] == sys.argv[1])
print(json.dumps({**plan["task_defaults"], **card}, indent=2))
PY
```

The coordinator adds `status`, `implementer`, `reviewer`, `commit`, `evidence`
and `blocked_reason` as work proceeds. Evidence records include exact pins,
commands, profile/matrix cells, actual results and artifacts. A design task
cannot close an implementation task. A local pass cannot close a required
cross-peer, hardware, adopter or cloud measurement.

When a decision discovers additional required work, add its bounded cards to
`tasks.json` and attach them to the affected implementation/qualification
dependencies **before** closing the decision. This is mandatory for unresolved
Edition 2024 features, a missing Rust frontend/ALTS provider, extra xDS cases,
or an applicable cacheable-unary implementation. Do not hide these leaves in a
prose note while allowing QL-04 to become dependency-ready.

### Check prerequisites

The `checks` map lists existing regression entry points, not installation
instructions or approval to run downloaded code. Add the smallest new targeted
check as part of each implementation and report its actual command.

| Check family | Required environment |
|---|---|
| Cargo consumers/codegen | Supported Rust, current documented `protoc` until the Rust-only tasks land, and reviewed manifest dependencies. Do not install unrelated tools to make a test green. |
| `conformance` | The existing script assumes Linux (`nproc`), CMake and a C++ toolchain for the pinned external runner; use the approved CI lane rather than claiming a macOS skip as a pass. |
| `interop` | Current script needs Go and `protoc`; GT makes pins/preflight strict. Additional C++/tonic/HTTP2/backoff commands are deliverables, not commands that already exist. |
| `codec-bench`, `shape-bench`, `rpc-bench` | Existing excluded crates have their own manifests/lockfiles. Local runs are smoke/evidence only; leadership gates need BM-12's approved isolated hosts. |
| Empty `checks` list | The card introduces its own harness or requires documentary/operator proof. Its acceptance criteria still need evidence; this never means automatic completion. |

For compatible plugin, versioned-stub and generated JSON/text, WKT, scalar and
oneof consumer tests, use the shared `target/integration-consumers` Cargo
cache. Each scratch
consumer has a distinct package/binary name, so parallel test cases cannot
replace one another's executable. Plugin tests also serialize the entire
nested Cargo command through doctests: Cargo's build lock alone can release
while rustdoc still needs dependency artifacts. Nested builds use at most
two jobs. Keep fresh-build/no-`protoc`
proofs and incompatible toolchain or sanitizer flags in separate targets
only when isolation is part of the check.

## Pinned upstream source map

These are read-only source references inspected on 2026-09-18, **not** versions
of newly executed peers. Runtime peer/tool pins are deliverables of GT-01/03.
Keep existing protobuf v35.1 as the regression baseline; upstream drift runs
are separate until their changes are reviewed.

| Source | Immutable revision | Relevant contracts |
|---|---|---|
| `protocolbuffers/protobuf` v35.1 | `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03` | [Conformance](https://github.com/protocolbuffers/protobuf/blob/35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03/conformance/README.md), [Rust shared tests](https://github.com/protocolbuffers/protobuf/tree/35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03/rust/test/shared), local `vendor/google/PIN` and `SHA`. |
| `grpc/grpc` | `d1487957db6658bc532b72871775148229836627` | [Interop descriptions](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/interop-test-descriptions.md), [runner](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/run_tests/run_interop_tests.py), [HTTP/2 protocol](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/PROTOCOL-HTTP2.md). |
| `grpc/grpc`, same revision | Same as above | [Negative HTTP/2 cases](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/http2-interop-test-descriptions.md), [backoff interop](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/connection-backoff-interop-test-description.md), [xDS](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/doc/xds-test-descriptions.md). |
| `grpc/grpc`, same revision | Same as above | [Performance framework](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/tools/run_tests/performance/README.md), [WorkerService](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/src/proto/grpc/testing/worker_service.proto), [control messages](https://github.com/grpc/grpc/blob/d1487957db6658bc532b72871775148229836627/src/proto/grpc/testing/control.proto). |
| `grpc/proposal` | `6342be729b96478a2897ceb208a8cddcd832a17b` | [A6 retry commitment, buffering and transparent retry](https://github.com/grpc/proposal/blob/6342be729b96478a2897ceb208a8cddcd832a17b/A6-client-retries.md). |
| `grpc/grpc-go` | `dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef` | [Interop client's implemented cases](https://github.com/grpc/grpc-go/blob/dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef/interop/client/client.go). Compression case support must be checked per pinned peer, not assumed. |

QL-04 is the final full-profile coverage reconciliation. Until it closes,
"all official gates pass" remains an unachieved goal, even if a narrower
profile is usable and very fast.
