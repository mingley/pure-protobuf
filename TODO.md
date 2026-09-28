# Execution queue

The [protobuf and gRPC roadmap](docs/ROADMAP.md) defines program-level scope.
The [granular execution plan](docs/plan/README.md) records the 2026-09-18 gap
assessment, official-suite pins, measurable goals and small-executor contract.
Its [127 task cards](docs/plan/tasks.json) are authoritative for leaf-task
dependencies/status; this checklist tracks milestones, not a second backlog.
The [world-class gRPC program](docs/plan/world-class/README.md) adds 169
dependency-checked cards; its first assignments start with MX-00, SB-01,
SB-10, MX-01/03/04/05, QG-04 and UK-01.
Nothing here claims current production certification or universal performance
leadership.

**Coordinator:** Michael Ingley. Later packages stay with the coordinator
until claimed. Existing code and historical test results are starting
points, not completion of the larger qualification packages.

## Delivered foundation slices

- [x] [GR-02 recovery slice](docs/ROADMAP.md#resume-from-the-frozen-work-not-from-assumed-completion). Reconcile interrupted experiments and status notes with committed source, assign unfinished work, and repair the reproduced macOS source-bind test without weakening its assertion. **Implementer:** this recovery change. **Reviewer:** Michael Ingley. Evidence: `pbrs-grpc` TCP/serving tests listen on `127.0.0.1`, dial with the shipped bound connect, and assert the accepted peer IP equals the bound source IP.
- [x] [GR-01: Executable onboarding and support matrix](docs/ROADMAP.md#gr-01-make-the-supported-contract-and-onboarding-executable). Compile messages/native/tonic examples as external consumers, fix tonic stub selection and document actual build dependencies. Record supported versus untested versions. **Implementer:** Michael Ingley. **Reviewer:** Michael Ingley. Evidence: `tests/pbrs_build.rs` and `tests/onboarding.rs` run advertised messages-only, native gRPC, and tonic quickstarts as fresh-directory consumers (SayHello / parsed field `ada`); core-without-`protoc` vs adapter `compile_protos` are independent PATH-filtered checks; README support matrix labels declared MSRV vs tested rustc 1.98 vs untested/unsupported.
- [x] [GR-02 remaining CI and release gates](docs/ROADMAP.md#gr-02-make-ci-and-releases-enforce-the-contract). Test declared MSRVs and Linux/macOS, isolate package consumers, select one CI-gated publisher and reconcile the release guide. **Implementer:** Michael Ingley. **Reviewer:** Michael Ingley. Evidence: required jobs `msrv-core` (1.85 `--lib` for `pbrs` / `pbrs-grpc`), `msrv-tonic` (1.88 `protobuf-tonic`), `macos` (`tcp::tests` + onboarding), `package-consumers`, `generated-output`; only [`.github/workflows/release.yml`](.github/workflows/release.yml) publishes (`v*` / confirmed dispatch after that lane). [docs/RELEASE.md](docs/RELEASE.md) matches: `CRATES_IO_TOKEN`, no Trusted Publishing, no publish on `main` pushes.

## Next assignments

These starting assignments have disjoint write scopes. Detailed acceptance,
read/write paths and check references live in the task cards. Architecture
decisions go to the coordinator, not an unattended small executor.

| Order within each lane | Task | Purpose |
|---|---|---|
| GT-01, then GT-02/03/04/05 | Official-gate infrastructure | Inventory every applicable suite/case, pin peers, fail on missing execution and retain logs/results. |
| CG-01/02, with CG-03 design before CG-04 | Codegen foundations | Useful diagnostics, explicit configuration, canonical multi-file identity and compatible output layout. |
| RT-01, then RT-02/03 | Retry safety | Prove execution/commitment boundaries and preserve one deadline before optimizing the client. |
| BM-01, then BM-02/04/05/06 | Measurement foundations | Separate processes, offered load and independent client/server CPU/resource accounting. |
| DX-01, then DX-02/03 | Documentation | Replace duplicated inventories with task-oriented guides while preserving focused documentation and behavior contracts. |
| PB-01, then the PB hardening lane | Rust and parser correctness | Remove numeric-parsing libc dependence, add differential/fuzz/original-upstream evidence. |

The current cards default to pending: committing this plan does not complete
them. Follow dependencies rather than table order, serialize overlapping files,
and reconcile FieldMask PR #68 before OP-04. Only reviewed dependencies may be
installed; pushing, publication, cloud spend and deployments require separate
approval.

## Qualify a bounded production profile

| Done | Work package | Depends on | Completion evidence |
|---|---|---|---|
| [ ] | [GR-03: Required cross-peer evidence](docs/ROADMAP.md#gr-03-extend-compatibility-evidence-instead-of-counting-self-tests) | GR-01 | Pinned grpc-go, no missing required passes, retained failure logs and the next independent peer. |
| [ ] | [GR-04: Parser/codegen hardening](docs/ROADMAP.md#gr-04-harden-protobuf-parsing-and-generated-code-evolution) | GR-01, reference pins from GR-03 | Differential fixtures, sustained fuzz results, unsafe-invariant review and generated/runtime compatibility matrix. |
| [ ] | [GR-05: Failure and overload](docs/ROADMAP.md#gr-05-prove-transport-behavior-under-failure-and-overload) | GR-01, GR-03 | Seeded call histories, transport failure matrix, bounded resource model and cancellation/drain cleanup. |
| [ ] | [GR-08: Operations and maintenance](docs/ROADMAP.md#gr-08-make-the-stack-operable-and-maintainable) | GR-01 | Telemetry/redaction tests, TLS/shutdown recipes, dependency policy and incident exercise. |
| [ ] | [GR-09: Comparable benchmarks](docs/ROADMAP.md#gr-09-build-a-credible-comparative-benchmark-system) | GR-01 | Reproducible codec/transport/end-to-end baseline with equivalent semantics, offered load, tail latency, CPU and memory. |
| [ ] | [GR-11: Adoption and API stability](docs/ROADMAP.md#gr-11-promote-narrowly-then-stabilize-the-public-api) | Applicable profile gates | Two adopter records, 24-hour and 7-day exercises, rollback proof, supported-version policy and per-crate signoff. |

Core protobuf and the tonic adapter do not depend on native discovery/retry
features. A benchmark win is not required for a narrow production profile;
correctness, bounded behavior and usable evidence are.

## Expand and compete

- [ ] [GR-06: Dynamic endpoint management](docs/ROADMAP.md#gr-06-add-dynamic-endpoint-management-as-a-distinct-capability), after GR-05: resolver lifecycle, `pick_first` then `round_robin`, TLS identity and churn/recovery proof.
- [ ] [GR-07: Explicit retry policy](docs/ROADMAP.md#gr-07-make-resilience-policy-safe-and-explicit), after GR-05 (and GR-06 for cross-endpoint retry): commitment/replay rules, bounded attempts, pushback, cancellation and no arbitrary stream replay.
- [ ] [GR-10: Profile-driven optimization](docs/ROADMAP.md#gr-10-optimize-the-measured-limiting-path), after GR-04/05/09: one explained change per PR, multi-host reruns, no hidden correctness or p99 regression.
- [ ] [GR-12: Scope decisions](docs/ROADMAP.md#gr-12-expand-scope-only-with-a-demonstrated-need): CG/EX track the requested Rust-only generation and full applicable official profile, including edition, ORCA/xDS and credential boundaries. Additional WKT/Serde, views/no_std, proxies, gRPC-Web and hedging remain demand-led.

## Program exit gates

- [ ] QL-03: independently reproduced codegen/codec/client/server/end-to-end leadership on the named matrix, with uncertainty and losses visible.
- [ ] QL-04: complete applicable official-gate inventory with real peer evidence; unsupported or externally blocked cases remain open.
- [ ] QL-05: per-crate supported-profile/API/promotion decision after adoption and rollback evidence. A bounded release may precede the first two gates but cannot claim them.

## How to close an item

Update the card's status and evidence after review, then link the implementing
commit/PR and evidence alongside the milestone checkbox. Record implementer,
reviewer, exact source/toolchain/peer versions, commands, artifact location,
observed metrics and remaining exclusions. Split partial work into dependency-
linked cards; leave the parent open until all its acceptance criteria hold.

At each milestone, review the plan's proposed targets before starting the next
exercise. Do not silently move thresholds, infer production readiness from a
crates.io upload, or schedule a version/date before the applicable gates close.
