# Plan for a best-in-class protobuf and gRPC stack

The goal is a Rust stack that users choose for correct behavior, predictable
resource use, low tail latency, and a straightforward development experience.
"Best" is a workload-specific result to demonstrate, not a description of the
current release. A faster codec does not by itself make a better RPC system.

This is an implementation plan, not a production certification or a promise of
release dates. [TODO.md](../TODO.md) is the execution queue. All work packages
below remain open until their acceptance evidence is linked there.

## Starting point and evidence

Source review baseline: [9d6f211a](https://github.com/mingley/pure-protobuf/tree/9d6f211ac258e2791e252c27acb3a848b2826f5c),
2026-09-04. File links below follow the current checkout; use that revision to
reproduce the baseline. Recorded measurements are not new results from this
planning pass, and source coverage is not evidence of a successful deployment.

| Area | What exists | What still needs proof or work |
|---|---|---|
| Crate boundaries | [Core manifest](../Cargo.toml), [native transport](../pbrs-grpc/Cargo.toml), [tonic adapter](../protobuf-tonic/Cargo.toml): `pbrs` 0.1.0 and two 0.1.0-alpha.1 adapters; separate dependency graphs. | Publication and a non-alpha version do not certify production readiness. Assess each crate separately. |
| Protobuf compatibility | [Recorded conformance](status.md): v35.1, maximum Edition 2023, 5,631 binary/JSON and 909 text cases with no unexpected results; [CI](../.github/workflows/ci.yml) runs conformance. | This is not every protobuf feature or upstream Rust test. [Known boundaries](upb.md) include Edition 2024, official generated internals, and non-owning views. |
| Native gRPC | [Client](../pbrs-grpc/src/client.rs), [server](../pbrs-grpc/src/server.rs), [wire](../pbrs-grpc/src/wire.rs): four call shapes, TLS/mTLS, UDS, health/reflection, deadlines, cancellation, bounds, pooling and limited transparent retry. | Establish a cross-peer, failure, platform and sustained-load matrix; do not reimplement these existing features merely to check a roadmap box. |
| Discovery | [TCP dialing](../pbrs-grpc/src/tcp.rs) resolves hostnames. `Target` takes `host:port`; connection pools serve one authority. | Resolver URI support, endpoint refresh and multi-endpoint balancing are different capabilities and are not implemented. |
| Cross-language tests | [Interop script](../scripts/grpc-interop.sh) runs self and grpc-go passes; compression cases run against self. | Go is fetched without a version pin, and unavailable Go/fetch/build paths exit successfully after a skip. Required CI must distinguish missing evidence from a pass. |
| Parser safety tests | [fuzz_parse.rs](../tests/fuzz_parse.rs) feeds four fixed inputs to two parsers. | This is a corpus smoke test, not a coverage-guided fuzz campaign or memory-safety proof. |
| Build and onboarding | [Codegen](../src/codegen.rs) defaults to native stubs and invokes `protoc`; core [build.rs](../build.rs) has a bundled descriptor fallback. | Test explicit messages/native/tonic modes, real minimum `protoc` versions, MSRV and fresh package consumers. Both adapter build scripts currently run codegen and need `protoc`. |
| Performance | [Benchmarks](benchmarks.md) include codec and transport harnesses, scoped wins, losses, and host-specific results. | Loopback/shared-runtime tests and best-of-short-window rates do not establish network, multicore or production tail-latency leadership. |
| Releases | [Release-plz](../.github/workflows/release-plz.yml) and [manual/tag release](../.github/workflows/release.yml) can publish independently. | Release-plz is not ordered after the separate CI workflow; manual publication hardcodes the core index probe. [Release guide](RELEASE.md) is stale. |

## Product scope and promotion

| Profile | Intended adoption boundary | Promotion requirements |
|---|---|---|
| Core protobuf | Plugin-generated messages, explicitly tested proto2/proto3/Edition 2023 behavior and target/toolchain matrix. | GR-01 through GR-04, core portions of GR-08/09, and a core-specific GR-11 adoption record. No transport requirement. |
| Native gRPC, bounded deployment | Explicit endpoints or deployment-managed service discovery/load balancing; documented TLS, limits, deadlines and shutdown policy. | GR-01 through GR-05, GR-08/09, and GR-11. No need to ship xDS first. |
| Native gRPC, dynamic fleet | Client-managed endpoint churn, balancing and optional retry policy. | Bounded-deployment gates plus GR-06; GR-07 before advertising policy retries. |
| Tonic adapter | Regenerated stubs and compatible tonic middleware, on tested tonic 0.14 versions. | Shared core/package gates, adapter interop and GR-11. Native resolver/retry work does not gate it. |

Semver 1.0 means a supported public API and upgrade contract, not universal
feature parity. A production profile may be narrower than the full crate.
Keep alpha warnings until evidence supports promoting that adapter. Do not
rename the crates or force their versions to advance together.

## Scorecard

These are **proposed planning targets**, not achieved SLOs. GR-01 records a
maintainer-approved workload, host budget and target before execution; any
target change needs a rationale, not a retrospective adjustment to get green.

| Dimension | Proposed bar | Evidence required |
|---|---|---|
| Correctness | Zero unexplained conformance/interop failures, corrupt messages, lost acknowledged stream elements, or forbidden replays. | Peer/version matrix, numbered request/message histories, seeds, minimized failures and rerun logs. RPCs with an unknown outcome remain unknown, not silently successful. |
| Recovery | In a controlled local failure fixture, new eligible calls recover within 5 seconds after service restoration; pending calls honor their own earlier deadline. | At least 1,000 seeded disconnect/drain/cancel cycles per advertised transport; separately state that an in-flight stream is not automatically resumable. |
| Resource bounds | No unbounded growth under overload; explicit caps for connections, calls, queues, bytes and decompression. | A resource-budget equation plus a 24-hour soak. After warmup, final-hour median RSS no more than 10% above first steady-state hour; active tasks/permits return to quiescent counts. RSS alone is not a leak detector. |
| Performance leadership | At least 20% lower CPU per successful RPC **or** 20% more successful RPC/s at equal latency/error budgets on named target workloads. | GR-09 repeated cross-host comparisons; no hidden correctness tradeoff, no more than 5% p99 regression at matched offered load, and explicit losing cells. |
| Ergonomics | A newcomer reaches a working generated unary service in 15 minutes using only the guide; all four shapes have runnable examples. | Fresh-directory package-consumer runs and feedback from at least two users not authoring those examples. Record tool installation time separately. |
| Operations and support | A failed call and a saturation event can be diagnosed without payload logging; every supported release has a rollback path. | Telemetry example, runbook exercise, support matrix, release artifact inventory and adoption signoff. |

## Ordered work packages

Michael Ingley is the coordinating maintainer and scope/signoff DRI. Package
implementers and independent reviewers are **unassigned** until claimed in
TODO.md. Split each package into reviewable PRs; do not wait for a large rewrite.
The package IDs are stable even if scheduling changes.

### GR-01: Make the supported contract and onboarding executable

**Priority:** P0. **Depends on:** none.

1. Inventory supported APIs, proto features, OS/architectures, tonic versions,
   and codegen modes in a compact matrix. Separate a tested version from a
   declared MSRV, and do not invent a universal minimum `protoc` from syntax history.
2. Turn the README examples into fresh-directory consumers: messages only,
   native gRPC and tonic. Align proto filenames, emitted files, generated
   service signatures and dependencies. Explicitly select tonic stubs with
   `Config::emit_tonic_stubs(true)`; do not promise a `prost::Message` drop-in.
3. Consolidate short tutorials around compiled examples. Replace sprawling
   prose-equality inventories with focused documentation contracts plus behavior
   tests, without removing behavioral coverage.

**Work surfaces:** [README](../README.md), [tonic guide](../protobuf-tonic/README.md),
[greeter](../examples/greeter), [codegen](../src/codegen.rs),
[serving tests](../pbrs-grpc/tests/serving.rs).
**Done when:** every advertised quickstart runs outside the workspace against
packaged crates, generated-output changes fail CI, and the matrix explicitly
labels unsupported and untested cases. Core-without-`protoc` and each adapter's
current `protoc` requirement are independently exercised.

### GR-02: Make CI and releases enforce the contract

**Priority:** P0. **Depends on:** GR-01's initial matrix.

1. Test declared Rust 1.85 core/native and Rust 1.88 tonic minimums separately,
   stable Rust, Linux and macOS; add other targets only with a named support
   commitment. Fix the [loopback-alias test](../pbrs-grpc/src/tcp.rs) without
   losing the assertion that source binding actually changes the connection.
2. Select one publishing path. Gate the exact release SHA and package graph on
   required CI, serialize publishers, make partial-release retries idempotent,
   and derive crate/version/index checks from manifests rather than literals.
3. Build unpacked archives in isolated consumers, inspect license/notice
   inclusion and generated inputs, and prevent drift among bundled proto copies.
   Test reproducible codegen at pinned tools. Reconcile the release guide,
   credential permissions, tag conventions, changelogs and recovery procedure.

**Work surfaces:** [CI](../.github/workflows/ci.yml),
[publishing workflows](../.github/workflows), [release-plz config](../release-plz.toml),
[native build](../pbrs-grpc/build.rs), [adapter build](../protobuf-tonic/build.rs).
**Done when:** failed or missing required evidence prevents publication; a
staged no-publish rehearsal covers partial success/retry and names exact
artifacts. No claim of Trusted Publishing unless actually configured and used.

### GR-03: Extend compatibility evidence instead of counting self-tests

**Priority:** P0. **Depends on:** GR-01.

1. Pin the Go peer and make required cross-language CI fail if it cannot run.
   Keep `--self-only` as an explicit local mode, not release evidence; retain
   failed attempt logs instead of hiding flakes behind the current retry.
2. Add a second independent reference peer (C++ or Java) and a tonic peer.
   Test both directions for all four RPC shapes, TLS/mTLS, gzip negotiation,
   metadata, status details, deadlines, cancellation, health and reflection.
3. Maintain a case-by-peer matrix. Use a peer that implements compression cases;
   record genuine unsupported cases explicitly. Run pinned conformance on every
   relevant change and a scheduled upstream-version compatibility lane.

**Work surfaces:** [interop script](../scripts/grpc-interop.sh),
[interop cases](../pbrs-grpc/src/interop_cases.rs),
[tonic tests](../protobuf-tonic/tests), [conformance](../scripts/conformance.sh).
**Done when:** every supported matrix cell has a versioned result; removing a
required peer makes the job fail. Custom OK-path trailers remain a documented
tonic 0.14 API limitation, not a promised adapter helper that bypasses transport.

### GR-04: Harden protobuf parsing and generated-code evolution

**Priority:** P0. **Depends on:** GR-01; use GR-03's reference pins.

1. Differentially exercise binary, JSON and text across generated schemas:
   presence/oneofs, unknown fields/enums, maps, extensions, packed/unpacked
   values, merge semantics and WKT mappings. Compare semantics, not byte order
   where protobuf serialization is not canonical.
2. Extend the four-input smoke test into sustained coverage-guided campaigns
   for parser/descriptor/codegen paths. Bound malformed lengths, recursion,
   allocation and decompression; persist seeds and minimized regressions.
3. Inventory unsafe invariants in runtime and lazy/packed storage. Apply Miri
   or sanitizers where supported, with explicit exclusions. Add old-generated-
   code/new-runtime and regenerated-code compatibility fixtures.

**Work surfaces:** [parser corpus](../tests/fuzz_parse.rs), [runtime](../src/runtime.rs),
[generator](../src/codegen.rs), [tests](../tests), [upb boundaries](upb.md).
**Done when:** pinned differential suites pass, supported sanitizer lanes have
no unresolved findings, and each release candidate has a recorded campaign
budget (initial target: 24 CPU-hours per major target). Campaign time alone
never proves absence of bugs; corpus coverage and discovered regressions matter.

### GR-05: Prove transport behavior under failure and overload

**Priority:** P0. **Depends on:** GR-01; expands GR-03.

1. Specify the call state machine: queued, headers sent, body started, response
   committed, trailers received, cancelled. At each boundary inject disconnect,
   RST_STREAM, GOAWAY, expired deadlines and dropped client/server futures.
2. Cross these with unary/upload/download/bidi, TLS/mTLS/UDS/TCP/custom I/O,
   slow readers/writers, half-close, handshake stalls and graceful drain.
   Check message order, final status, cancellation notification and task cleanup.
3. Derive a memory budget from connections, stream windows, queue depth and
   message caps. Stress fragmented/coalesced frames, compressed size expansion,
   reset storms and many idle peers. Measure fairness between bulk streams and
   small RPCs, not just aggregate throughput.

**Work surfaces:** [client](../pbrs-grpc/src/client.rs),
[server](../pbrs-grpc/src/server.rs), [wire](../pbrs-grpc/src/wire.rs),
[stream](../pbrs-grpc/src/stream.rs), [config](../pbrs-grpc/src/config.rs),
[limits](../pbrs-grpc/src/limits.rs), [tests](../pbrs-grpc/tests).
**Done when:** seeded histories satisfy the scorecard; permits/tasks return
after cancellation and shutdown; a saturated peer cannot consume resources
beyond its configured budget. Existing per-message caps are not assumed to
bound total process memory when global concurrency caps are unset.

### GR-06: Add dynamic endpoint management as a distinct capability

**Priority:** P1; required for the dynamic-fleet profile. **Depends on:** GR-05.

1. Design a small resolver/update interface with endpoint identity, TTL,
   refresh/backoff, shutdown and stale-result policy. Preserve current
   `host:port` behavior; use an explicit new API for resolver URI semantics.
2. Implement endpoint state, connection lifecycle and `pick_first`, then
   `round_robin`. Keep TLS identity/authority separate from resolved socket
   addresses and test IPv4/IPv6, endpoint removal and certificate changes.
3. Exercise DNS failure, empty results, churn and partial backend outage.
   Specify new-call routing separately from draining long-lived streams.

**Work surfaces:** [Target/Channel](../pbrs-grpc/src/client.rs),
[TCP](../pbrs-grpc/src/tcp.rs), [TLS](../pbrs-grpc/src/tls.rs),
[config](../pbrs-grpc/src/config.rs), [generated clients](../src/codegen.rs).
**Done when:** fake-clock resolver tests and real multi-backend fixtures show
bounded refresh/reconnect work, correct certificate verification and recovery
within the declared TTL/backoff budget. Keep external-LB deployments supported.

### GR-07: Make resilience policy safe and explicit

**Priority:** P1, opt-in. **Depends on:** GR-05; GR-06 for cross-endpoint retries.

1. Document existing transparent retries versus application retries. Model
   replay commitment explicitly; never infer that a failed transport means the
   server did not execute a side effect.
2. Add opt-in service-config retry support only with attempt, deadline, buffer
   and retry-throttling budgets, jitter, pushback and cancellation propagation.
   Bound replayable unary payloads; do not auto-replay arbitrary streaming bodies.
3. Test ambiguous outcomes and side-effecting handlers. Define async auth and
   middleware extension points without forcing tonic/tower into the native
   crate. Defer hedging until retry correctness and load amplification are proven.

**Work surfaces:** [client](../pbrs-grpc/src/client.rs),
[request lifecycle](../pbrs-grpc/src/request.rs),
[interceptors](../pbrs-grpc/src/interceptor.rs), [config](../pbrs-grpc/src/config.rs).
**Done when:** fault histories show no replay outside the documented contract,
total attempts and bytes stay within budget, cancelled/deadline-expired calls
stop retrying, and users can observe each attempt. Never promise exactly-once RPC.

### GR-08: Make the stack operable and maintainable

**Priority:** P0 for basic production diagnostics/security. **Depends on:** GR-01.

1. Expose low-cardinality lifecycle metrics: attempts, status, latency, inflight
   calls, queue wait, bytes, reconnects, rejected work and stream cancellation.
   Provide optional tracing/OpenTelemetry integration without a mandatory exporter.
2. Add deployable examples for TLS/mTLS, credential refresh, bounded streaming,
   health/readiness and graceful termination. Document reflection exposure,
   trust roots and authentication versus authorization responsibilities.
3. Establish security reporting and dependency/advisory review, unsafe-code
   review ownership, supported versions and an incident/runbook checklist.
   Never log credentials or request/response payloads by default.

**Work surfaces:** [interceptors](../pbrs-grpc/src/interceptor.rs),
[TLS](../pbrs-grpc/src/tls.rs), [health](../pbrs-grpc/src/health.rs),
[greeter](../examples/greeter), [Cargo manifests](../Cargo.toml), [guide](grpc.md).
**Done when:** a fixture explains an overload/reconnect/timeout from telemetry,
redaction and metric-cardinality tests pass, and disabled instrumentation has a
measured cost within a proposed 2% CPU budget on the reference workload.

### GR-09: Build a credible comparative benchmark system

**Priority:** P0 for a baseline; continuous for leadership. **Depends on:** GR-01.

1. Freeze named workloads before optimizing: codec-only (owned versus borrowed
   separately), same-codec transport, and end-to-end application RPCs. Include
   empty, 1 KiB, 64 KiB and 1 MiB messages, mixed sizes and all four call shapes.
2. Compare version-pinned tonic/prost, tonic/pbrs and grpc-go; add a C++ reference
   for relevant deployments. Match TCP_NODELAY, TLS, compression, message limits,
   runtime cores, concurrency, connections, deadlines and handler work. Explain
   any remaining semantic differences instead of attributing them to architecture.
3. Extend existing harnesses with separate client/server processes, real-network
   RTT, x86_64 and arm64, open-loop load through saturation, and recovery load.
   Record scheduling delay to avoid coordinated omission; count timeout/rejection
   rates beside completed-call latency, never discard slow failed calls.
4. Report p50/p95/p99/p99.9 with sample counts, successful RPC/s, CPU/RPC,
   allocations, RSS and fairness. Randomize A/B order; retain at least five
   sufficiently long repetitions per cell, raw samples, uncertainty intervals,
   hardware/toolchain/configuration metadata and exact commit IDs.

**Work surfaces:** [bench](../bench), [tonic-bench](../tonic-bench),
[rpc-bench](../rpc-bench), [Go server harness](../scripts/grpc-server-bench.sh),
[benchmark report](benchmarks.md).
**Done when:** another machine/operator can reproduce the matrix from recorded
inputs. Preserve current gates while adding statistically justified noise
budgets. Do not promote best-of-two-second results, shared-runtime tail outliers,
or a one-host codec win to a universal "fastest" claim.

### GR-10: Optimize the measured limiting path

**Priority:** P1, after trustworthy baseline. **Depends on:** GR-04/05/09.

1. Rank profiles by user impact: CPU/RPC and tail latency at matched load,
   then memory/build cost. Separate serialization, allocation, h2 driver,
   TLS/compression, syscalls and executor contention before selecting a change.
2. Re-measure the documented `name_80` loss and large packed-fixed encode,
   generated-schema versus handwritten specialization, JSON/text dynamic
   fallback, and bulk-stream/small-call interference. None is a presumed
   production bottleneck until profiled.
3. Try one reversible change per PR: buffer lifetime/copy reduction, measured
   batching/window choices, WKT specialization or task/connection scheduling.
   Keep defaults safe; borrowing APIs require explicit lifetime and retained-
   memory tradeoffs. Reject wins dependent on one benchmark schema.

**Work surfaces:** [codec](../pbrs-grpc/src/codec.rs),
[wire](../pbrs-grpc/src/wire.rs), [generator](../src/codegen.rs),
[runtime](../src/runtime.rs), [recorded losses](benchmarks.md).
**Done when:** before/after profiles explain the gain, correctness and resource
gates still pass, GR-09 reproduces it on both architectures, and tradeoffs or
regressions are recorded. Leadership requires an independent rerun, not just a PR.

### GR-11: Promote narrowly, then stabilize the public API

**Priority:** production gate, not a feature sprint. **Depends on:** applicable
profile gates above; benchmark *baseline* required, benchmark victory not required.

1. Recruit two independent adopters with different workloads. Record topology,
   feature use, crate/tool versions, traffic bounds and unresolved gaps. Never
   silently treat a local benchmark as an adoption record.
2. Run 24-hour qualification, then a 7-day controlled soak. With operator
   approval, use read-only/shadow traffic where safe, then 1%/10%/50% rollout.
   Do not duplicate side-effecting RPCs merely to compare implementations.
3. Pause immediately on corruption, a replay violation, credential exposure or
   unbounded resources. Proposed rollback triggers: p99 > baseline by 10% or
   unexpected-error rate > baseline by 0.1 percentage points for two 5-minute
   windows, with sample floors and any stricter service SLO set before rollout.
4. Publish per-crate promotion decisions, supported platform/protoc/tonic
   versions, upgrade fixtures, deprecation/MSRV policy and a known-gaps list.
   Rehearse rollback with the old binary/config and compatible wire schema.

**Done when:** operators and maintainer sign off on linked evidence with no
unresolved blockers in that profile. A 1.0 proposal additionally requires an API
compatibility review and a stated maintenance commitment. No automatic version
bump or production rollout is authorized by this plan.

### GR-12: Expand scope only with a demonstrated need

**Priority:** P2, separate design decisions. **Depends on:** a named adopter,
maintainer capacity and a proof plan; not universal production blockers.

| Candidate | Decision and acceptance requirement |
|---|---|
| Edition 2024 and broader descriptor options | Pin the upstream feature contract and add differential/codegen fixtures before advertising it; not a blind generator maximum-edition bump. |
| Specialized WKT/Serde APIs | Preserve official protobuf JSON semantics, unknown/presence behavior and roundtrips. Measure against the current dynamic fallback; ordinary Serde derives are not automatically equivalent. |
| First-class borrowed views or `no_std`/WASM | Measure retained-buffer memory and lifecycle ergonomics; identify supported targets and dependencies. Keep the owned API and native transport boundaries intact. |
| xDS, ORCA, channelz and cloud auth | First justify native ownership versus external load balancing or tonic; require interoperable control-plane and operational tests for the chosen subset. |
| HTTP CONNECT, gRPC-Web, newer transports | Separate deployment requirements from core gRPC compliance; design auth/proxy and framing behavior with an independent peer before implementation. |
| Hedging | Only after GR-07, with explicit idempotency, concurrency/amplification limits and an outage-load experiment proving it does not worsen overload. |

## Execution and proof discipline

Start with three small PRs: **GR-01** executable quickstarts/support matrix,
**GR-02** release/package/platform gates, then **GR-03** pinned, fail-closed
interop. GR-04/05/08 and GR-09 can follow in parallel once contracts are stable.
Only then expand discovery/retries or tune performance against the baseline.

Existing entry points below are useful building blocks, **not commands that
already prove this entire plan**. Run from the repo root; conformance and interop
currently assume the Linux CI environment and fetch/build upstream tools.

| Purpose | Existing command |
|---|---|
| Workspace behavior | `cargo test --workspace` |
| Parser smoke fixture | `cargo test -p pbrs --test fuzz_parse` |
| Native source-binding behavior | `cargo test -p pbrs-grpc --lib tcp::tests` |
| Tonic adapter | `cargo test -p protobuf-tonic` |
| Official conformance | `./scripts/conformance.sh` |
| Cross-language RPC cases | `./scripts/grpc-interop.sh` (inspect skipped passes until GR-03 lands) |
| Codec / transport baselines | `(cd bench && cargo build --release && ./target/release/bench)`; equivalent commands in `tonic-bench` and `rpc-bench` |
| Release package candidates | `cargo package -p pbrs --registry crates-io`; repeat separately for each adapter |

Each completed item must link a PR, exact revision/tool versions, configuration,
commands, raw artifacts, observed outcome, limitations and reviewer signoff.
New harnesses are deliverables, not pretend commands in this document. Review
the scorecard at each milestone, retire obsolete work, and keep a failed or
missing measurement visible. That evidence, rather than a feature count, is
the route to becoming a leading implementation.
