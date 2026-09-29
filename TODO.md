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
python3 scripts/plan-status.py --card SB-21
```

The ready list is generated from both card files, including their dependency
and reconciliation gates. It excludes claimed and blocked work. It is a list
of eligible work, not an instruction to start every lane at once.

## Current priorities

QG-05 is delivered: `c89608bd` passes all 19 original consumer crates
(233 tests plus three regressions), including exact-source Miri and Linux
compatibility CI. QG-07 now compiles and exercises the operations/production
guide recipes and rejects API drift without pinning prose. See
[status](docs/status.md) for their validation and limits.
The [closed-enum follow-up](docs/evidence/closed-enum-recovery.md) at `3a7aa128`
keeps unknown wire values out of typed storage and passes a new clean-source
ordinary/Miri qualification (233 original tests plus six regressions).
SB-23 has also completed a real corrected-parser CI comparison: 88 cells per
revision with retained artifacts and unavailable instructions excluded.

| Order | Work | First concrete result | Finished when |
|---|---|---|---|
| 1, in parallel | **SB-21: finish claim scenario execution** | Normalize reference-peer aggregate arrival rates and add independent accounting; the pinned Go worker applies its rate per outstanding slot. | Both harnesses pass the frozen scenario smoke with reconciled window counts, scheduled-send latency and generator headroom. |
| 1, in parallel | **SB-24: complete required transport comparisons** | Close the observed TLS cipher mismatch and missing session/settings exports; run equivalent Go/C++ cells. | Required directions, generator/reference headroom, aligned endpoint CPU windows and feasible topology are verified on the campaign hosts. |
| 1, separate workflow owner | **SB-20: calibrate performance CI** | Build on the two verified corrected-parser pairs in separate Intel/AMD cohorts; collect comparable runs by host and available metrics. | At least 30 comparable artifact runs support reviewed thresholds; thresholds stay advisory meanwhile. |
| 2 | **RX-09: measure the landed RPC changes** | First match response bytes/counts and codec/handler work in the differing dev-loop cells, then compare the candidate with its pre-change base and tonic. | Audited equivalent-work counters and separate-process Linux profiles show improvements without correctness or tail-latency regressions. |
| 2, codec/codegen lane | **BM-03, PK-18, GN-02/GN-03** | Finish paired codec measurements, dynamic-parse competitor runs and downstream build-cost reruns. | Their existing acceptance criteria pass with raw evidence; merged code alone is insufficient. |
| 3 | **QG-06: qualify current-backend resources and recovery** | Freeze finite limits and the slow-reader/cancellation/overload matrix. | A 24-hour exact-candidate soak demonstrates bounded resources and recovery. |
| 4 | **SB-15, SB-22, then SB-16: run and publish comparisons** | Freeze peer versions, workloads, hosts and thresholds before timing. | Independent x86_64 and arm64 runs meet the benchmark contract; losses and missing rows remain visible. |
| Broader claim scope | **SB-25: execute more competitors** | Turn Java/.NET and other optional peer manifests into runnable, equivalent comparisons. | Published claims name only peers and directions actually executed. |

SB-21 already exports scheduled latency and conserved window counts and runs
frozen, randomized repeats; its first matrix smoke correctly failed the
generator-lag gate. SB-24's 144-cell TLS/compression smoke proves wiring, not
throughput: native/native negotiated AES-256, while native/tonic negotiated
AES-128 and tonic/tonic lacks actual session telemetry. No core cipher-policy
API was changed. Finish these gaps, RX-09's equivalent-work profiles and
SB-20's calibration. The
[benchmark contract](docs/benchmark-contract.md) owns numeric thresholds.

## Adoption findings (2026-09-29)

An adoption evaluation against a production-shaped schema set and
tonic-based service stack filed these cards. Dev-loop evidence is in each
card's `notes`.

| Card | Priority | Finding |
|---|---|---|
| GN-10 | P0 | Fields named `default`, `clone` or `serialize` generate code that does not compile. |
| PK-26 | P0 | Message getters take a global mutex and hash a `TypeId` on every read of a present field. |
| PK-27 | P0 | Reading every field after decode costs 2-5x prost instructions on deep or `Any`-heavy messages. |
| TC-25 | P0 | TLS hard-wires the graviola provider; callers cannot pass rustls configs, providers or cert resolvers. |
| GN-11 | P1 | Each generated file carries private well-known-type copies; `::pbrs::wkt` in the docs does not exist. |
| TC-26 | P1 | Services cannot mount in tonic-typed frameworks (`NamedService`, tonic connect-info types). |
| TC-27 | P1 | The Tower client adapter covers unary calls only. |
| TC-28 | P1 | The config-only `codec_path` route from tonic-prost-build works but is untested and undocumented. |
| PK-28 | P2 | Serialized field order differs from prost, C++ and Go. |

In the same evaluation, prost messages on pbrs-grpc cost 5-7% fewer
instructions and up to 29% fewer allocated bytes per unary RPC than on
tonic. pbrs messages cost more CPU on either transport once handlers read
every field (PK-27).

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
