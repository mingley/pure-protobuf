# What to work on next

The immediate goal is a trustworthy comparison of the current gRPC stack,
followed by measured improvements to client and server efficiency. The
[2026-09-29 audit](docs/audit-2026-09-29.md) explains the current gaps. The
[performance program](docs/plan/world-class/README.md) defines the required
comparisons and checks; [task cards](docs/plan/world-class/tasks.json) carry
acceptance criteria and dependencies.

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

The [execution follow-up](docs/evidence/grpc-architecture-20261022/README.md)
implements wake-indexed scheduling, runtime-local cancellation parking,
selected-method prost dispatch and complete compressed-input handling. Candidate
`9d6fb9c4` also fixes blocked duplex uploads, task-abort cancellation, waiter
ownership and upload-pump panic cleanup. Its leak-detecting sanitizer run passes
**731 tests**, with one existing ignore, and all ten pre-workspace checks pass.
The clean `deb0d1d6` all-feature workspace passes **3,755 tests**, zero failures
and nine existing ignores across 139 targets. Only the serving test fixture
differs from the runtime; each proof keeps its actual source pin.

The original C8 workspace failure (3,753 passed, two failed, nine ignored), C3
failure (3,739 passed, five failed, nine ignored, interrupted outer recorder)
and earlier failures remain archived.
The actual `9d6fb9c4` preview passes 30.54 seconds and nine drains, each returning
client/server charges, tokens and active calls to zero. The selected comparison
passes 400 captures and 120,000 RPCs; the expanded current matrix passes 2,400
captures and 720,000 RPCs. The 1 MiB pairs pass 120 captures and 4,500 RPCs.
The actual 24-hour run started at 02:52 UTC on 2026-10-23; its result is pending.
Passing functional checks is not a speed or production proof.

The focused server-instruction diagnostic passes 72 captures and 1,080 RPCs.
All campaigns including pilots pass 3,000 captures and 845,790 RPCs. Selected
prost gzip server instructions improve against the old source; the allocation-
instrumented profiler run does not measure shipping CPU, latency or throughput.

The `6cd91911` [day attempt failed after 32 min 2.73 s](docs/evidence/grpc-streaming-soak-20261022/README.md).
Cycle 586, seed 20261588, TCP bidi reset at `BodyStarted` returned INTERNAL.
The exact failure was not uniquely reproduced in 2,008 scenario replays. New
independent-peer controls check reset-reason preservation on failed writes and
gRPC mappings for 15 reasons; they do not establish its unique cause or close
the day. Local clean-half-close misuse remains INTERNAL.

| Work | Next result | Completion criterion |
|---|---|---|
| Duplex progress and pump ownership | Carry the passing independent-peer and detecting controls into sustained resource and cross-peer checks. | Reply/status ordering, task-abort cleanup, normal producer lifetimes and finite resources remain correct under load. |
| SV-09, CL-08, CL-09, RX-10 | Attribute the worst large-gzip byte costs, then identity bytes and instruction losses. | Measure output growth, copies and ownership at equal settings; preserve retained-memory bounds and report every loss. |
| Handler execution | Compare inline execution with one owned Tokio task per admitted RPC under mixed expensive/cheap load. | Preserve finite admission, drop-abort, deadlines, reset, backpressure and drain; measure small-call handoff costs. |
| PK-27, PK-29 | Experiment with checked eager merge for singular nested fields, keeping current representation and API; compare read-all and sparse corpora. | Preserve validation, unknown/merge semantics and retention; prove span validity before removing repeated checks. |
| Current candidate qualification | Complete remaining required platform, feature and release checks after the passing workspace. | Exact-source records preserve deadlines, status, drop assertions and finite limits; original failures stay visible. |
| QG-06: resources and recovery | Pass a new actual day on the final validated source. | 24 hours pass the original finite limits, deadlines, recovery and fairness checks. |
| CI and platforms | Extend the passing local strict lint/rustdoc checks to all required feature, platform and packaged-consumer gates. | Run the required profiles on recorded sources; keep unsupported combinations and failures explicit. |
| SB-27: client/server matrix | Extend the measured 1/64 KiB, one/16-in-flight scope. | Cover all payloads, profiles, read-all corpora and lifecycle cases; retain failures and assign losses. |
| CH-12 and remaining integration cards | Complete discovery/balancer runtimes and churn tests. | External consumers use supported APIs; existing connections stay stable across updates. |
| Remaining credential and codec cards | Complete providers and typed-extension support. | Consumer, wire-compatibility and security checks pass. |
| SB-28, SB-22 and SB-16 | Qualify the complete matrix on dedicated x86_64 and arm64 hosts. | Paired statistics, headroom and latency checks support the comparisons. |

The [current selected comparison](docs/benchmarks.md) reduces 64 KiB prost gzip
upload server requested bytes by 14.5%, but still requests 27.4% more than tonic.
At 1 MiB gzip bidi, requested bytes remain 75.5% above tonic on the server and
71.0% above on the client, despite about 4 MiB saved per endpoint against the old
source. Those are the largest measured byte gaps to attribute first.
Expanded small identity bidi requests 31.9% more bytes on the server and 36.7%
more on the client. Small unary client bytes and gzip server-stream server allocations also
regress against the old source. Attribute those costs before proposing reuse or
representation changes; expanded TLS/load cases retain these losses. Test output
growth boundaries and retained memory under concurrent large streams;
raising retention caps without attribution is not an accepted remedy.

PK-26's old eager mutex/TypeId default lookup is already removed from current
generated getters. Do not redo that source fix; its measurement/regeneration
acceptance remains separate. Lazy singular materialization still repeats work;
repeated children, including the adoption Query's recursive lists, already parse
eagerly. Do not attribute all recursive-query costs to lazy singular fields.
The [execution decision](docs/decisions/grpc-execution.md) separates those codec,
transport, scheduling and generated-code costs. Required platforms, remaining features
and release gates remain open. Production readiness and broad superiority are
unproved; use the [benchmark contract](docs/benchmark-contract.md) for comparisons.

<a id="dominance-program"></a>

## Workload comparisons

Goal: use less CPU, allocate less and complete calls faster than tonic/prost
across the defined workloads, for both client and server with prost and pbrs
messages. Measure each side and metric separately. The
[workload comparison plan](docs/plan/dominance/README.md) defines acceptance
criteria and assigns each recorded loss to a task card.
Start with SB-27 (after SB-26) and RX-10, then address the recorded losses: SV-09,
CL-08, CL-07, CL-09, PK-26, PK-27 and PK-29.

## Adoption program

Goal: teams on prost and tonic adopt pbrs one layer at a time, without
rewriting handlers, middleware or build pipelines, and get clearly lower CPU
and memory cost. The [adoption program](docs/plan/adoption/README.md) defines
targets, phases and cards. Start with SB-26 (measure what adopters run), then
the P0 cards: GN-10, PK-26 then PK-27, TC-25, TC-29, TC-30 and AD-04.

### Findings that opened the program (2026-09-29)

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
