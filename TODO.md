# Execution queue

The [protobuf and gRPC leadership plan](docs/ROADMAP.md) defines scope,
dependencies and acceptance evidence. This checklist schedules that work; it
does not repeat a feature-parity inventory or claim the current crates are
production-certified.

**Coordinator:** Michael Ingley. Later packages stay with the coordinator
until claimed. Existing code and historical test results are starting
points, not completion of the larger qualification packages.

## First three PRs

- [x] [GR-02 recovery slice](docs/ROADMAP.md#resume-from-the-frozen-work-not-from-assumed-completion). Reconcile interrupted experiments and status notes with committed source, assign unfinished work, and repair the reproduced macOS source-bind test without weakening its assertion. **Implementer:** this recovery change. **Reviewer:** Michael Ingley. Evidence: `pbrs-grpc` TCP/serving tests listen on `127.0.0.1`, dial with the shipped bound connect, and assert the accepted peer IP equals the bound source IP.
- [x] [GR-01: Executable onboarding and support matrix](docs/ROADMAP.md#gr-01-make-the-supported-contract-and-onboarding-executable). Compile messages/native/tonic examples as external consumers, fix tonic stub selection and document actual build dependencies. Record supported versus untested versions. **Implementer:** Michael Ingley. **Reviewer:** Michael Ingley. Evidence: `tests/pbrs_build.rs` and `tests/onboarding.rs` run advertised messages-only, native gRPC, and tonic quickstarts as fresh-directory consumers (SayHello / parsed field `ada`); core-without-`protoc` vs adapter `compile_protos` are independent PATH-filtered checks; README support matrix labels declared MSRV vs tested rustc 1.98 vs untested/unsupported.
- [x] [GR-02 remaining CI and release gates](docs/ROADMAP.md#gr-02-make-ci-and-releases-enforce-the-contract). Test declared MSRVs and Linux/macOS, isolate package consumers, select one CI-gated publisher and reconcile the release guide. **Implementer:** Michael Ingley. **Reviewer:** Michael Ingley. Evidence: required jobs `msrv-core` (1.85 `--lib` for `pbrs` / `pbrs-grpc`), `msrv-tonic` (1.88 `protobuf-tonic`), `macos` (`tcp::tests` + onboarding), `package-consumers`, `generated-output`; only [`.github/workflows/release.yml`](.github/workflows/release.yml) publishes (`v*` / confirmed dispatch after that lane). [docs/RELEASE.md](docs/RELEASE.md) matches: `CRATES_IO_TOKEN`, no Trusted Publishing, no publish on `main` pushes.

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
- [ ] [GR-12: Demand-led scope decisions](docs/ROADMAP.md#gr-12-expand-scope-only-with-a-demonstrated-need): Edition 2024, WKT/Serde, views/no_std, xDS, proxies, gRPC-Web and hedging each need an adopter, boundary and proof plan.

## How to close an item

Add the implementing PR and evidence link alongside the checkbox. Record
implementer, reviewer, exact source/toolchain/peer versions, commands, artifact
location, observed metrics and remaining exclusions. Split partial work into
sub-PRs but leave the parent open until its acceptance criteria hold.

At each milestone, review the plan's proposed targets before starting the next
exercise. Do not silently move thresholds, infer production readiness from a
crates.io upload, or schedule a version/date before the applicable gates close.
