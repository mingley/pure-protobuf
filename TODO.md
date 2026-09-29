# What to work on next

The immediate goal is a trustworthy comparison of the current gRPC stack,
followed by measured improvements to client and server efficiency. The
[2026-09-29 audit](docs/audit-2026-09-29.md) explains the current gaps. The
[performance program](docs/plan/world-class/README.md) defines the route to
leadership; [task cards](docs/plan/world-class/tasks.json) carry acceptance
criteria and dependencies.

Michael Ingley coordinates assignments and reviews. A card marked `done`
means its recorded deliverable was accepted; it does not certify the whole
product. Older [foundation cards](docs/plan/tasks.json) remain authoritative
where they have not been handed to newer cards.

## Start here

Run these from the repository root:

```sh
python3 scripts/plan-lint.py
python3 scripts/plan-status.py --summary
python3 scripts/plan-status.py --ready
python3 scripts/plan-status.py --card QG-05
```

The ready list is generated from both card files, including their dependency
and reconciliation gates. It excludes claimed and blocked work. It is a list
of eligible work, not an instruction to start every lane at once.

## Current priorities

| Order | Work | First concrete result | Finished when |
|---|---|---|---|
| 1 | **QG-05: restore upstream enum-map compatibility** | Reproduce `test_map_int32_enum`, then design a sound enum conversion hook. | All 19 original shared consumer crates run successfully; the fix has regression and applicable Miri evidence. |
| 2, in parallel | **SB-21: finish claim scenario execution** | Load frozen scenarios, report latency from scheduled send, randomize each paired repeat, reconcile counts. | Both harnesses pass the scenario smoke with consistent offered/completed/rejected/timed-out accounting. |
| 2, in parallel | **SB-24: complete required transport comparisons** | Run every required shape/TLS/compression cell with verified effective peer settings. | No required row is silently unsupported; generator headroom, topology and endpoint CPU are recorded. |
| 2, separate workflow owner | **SB-23, then SB-20: make performance CI useful** | Retain a real CI base/head result with nonempty measured cells. | At least 30 comparable artifact runs support reviewed regression thresholds. |
| 3 | **RX-09: measure the landed RPC changes** | Compare current code with its pre-change base using deterministic counters and separate-process Linux profiles. | Instructions, bytes and allocations improve on target cells without correctness or tail-latency regressions. |
| 3, codec/codegen lane | **BM-03, PK-18, GN-02/GN-03** | Finish paired codec measurements, dynamic-parse competitor runs and downstream build-cost reruns. | Their existing acceptance criteria pass with raw evidence; merged code alone is insufficient. |
| 4 | **QG-06: qualify current-backend resources and recovery** | Freeze finite limits and the slow-reader/cancellation/overload matrix. | A 24-hour exact-candidate soak demonstrates bounded resources and recovery. |
| 5 | **SB-15, SB-22, then SB-16: run and publish comparisons** | Freeze peer versions, workloads, hosts and thresholds before timing. | Independent x86_64 and arm64 runs meet the benchmark contract; losses and missing rows remain visible. |
| Broader claim scope | **SB-25: execute more competitors** | Turn Java/.NET and other optional peer manifests into runnable, equivalent comparisons. | Published claims name only peers and directions actually executed. |

SB-21, SB-23 and RX-09 already contain delivered slices; finish their
measurements and harness gaps rather than restarting them. The
[benchmark contract](docs/benchmark-contract.md) owns numeric thresholds.

## Work that should wait

- **Custom HTTP/2 engine:** H2-04 is blocked by the recorded no-go decision.
  H2-16 can revisit it after current-backend profiles demonstrate at least
  15% modeled gain on primary client/server cells or a concrete correctness
  blocker. Completing a design document does not satisfy that gate.
- **Runtime and kernel experiments:** `io_uring`, kernel TLS and new arenas
  need a measured bottleneck, a bounded experiment and their existing gates.
- **Broader compatibility:** the Rust frontend, complete upb replacement and
  xDS keep their own lanes. They can proceed with available ownership, but
  they do not substitute for proving the shipping transport is efficient.

## Working in parallel

Assign one owner to each changing module. In particular, SB-21 and SB-24
share benchmark code; RX-09 and resource work can share transport files;
GN-02/GN-03 share generated-code emission. Sequence those edits or split their
write sets explicitly. The [worker protocol](docs/plan/world-class/README.md#worker-protocol)
defines the handoff: source revision, hypothesis, checks, artifacts, reviewer
and remaining limits.

Use `mingley` for GitHub work and `michael.ingley@gmail.com` for commits.
Preserve unrelated local edits and use an isolated worktree when necessary.

## Promotion is a separate decision

[GR work packages](docs/ROADMAP.md) describe supported product profiles.
QL-03 is performance qualification, QL-04 is applicable official-suite
coverage, and QL-05 is API/support promotion. None is closed by this audit.
A release follows the [release policy](docs/RELEASE.md); a push to `main`
does not publish crates.
