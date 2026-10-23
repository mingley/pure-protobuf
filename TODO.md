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

The [execution follow-up](docs/evidence/grpc-architecture-20261022/README.md)
implements wake-indexed scheduling, runtime-local cancellation parking,
selected-method prost dispatch and complete compressed-input handling. Candidate
`783bcd53` also fixes blocked duplex uploads, cancellation/ownership defects and
upload-pump panic cleanup. A message admits at most 1,024 gzip members or Zstd
frames by default, independently of byte and history limits. Exact-candidate
AddressSanitizer with leak detection passes 720 tests with one existing ignore.
Rust 1.85 default-library and Rust 1.87 optional-Zstd checks also pass.
A 10-cycle resource preview passes with post-drain charges, tokens and active
calls at zero; the actual day remains open.
The full workspace run fails with 3,739 passes, five failures and nine existing
ignores across 139 targets. Deterministic serving/Tower fixture repairs and a new
clean-source workspace run are pending, alongside strict CI and fresh captures.
Predecessor `90201458` records 3,717 workspace passes, three failures and
nine existing ignores; those original results remain archived. Source changes
are not a speed or production proof.

The `6cd91911` [day attempt failed after 32 min 2.73 s](docs/evidence/grpc-streaming-soak-20261022/README.md).
Cycle 586, seed 20261588, TCP bidi reset at `BodyStarted` returned INTERNAL.
The exact failure was not uniquely reproduced in 2,008 scenario replays. New
independent-peer controls check reset-reason preservation on failed writes and
gRPC mappings for 15 reasons; they do not establish its unique cause or close
the day. Local clean-half-close misuse remains INTERNAL.

| Work | Next result | Completion criterion |
|---|---|---|
| Duplex progress and pump ownership | Finish qualification of early-response, panic and cancellation cleanup. | Independent-peer cases return replies/status and recover finite resources and the same connection without an RPC timeout. |
| SV-09, CL-08, CL-09, RX-10 | Attribute the worst large-gzip byte costs, then identity bytes and instruction losses. | Measure output growth, copies and ownership at equal settings; preserve retained-memory bounds and report every loss. |
| Handler execution | Compare inline execution with one owned Tokio task per admitted RPC under mixed expensive/cheap load. | Preserve finite admission, drop-abort, deadlines, reset, backpressure and drain; measure small-call handoff costs. |
| PK-27, PK-29 | Experiment with checked eager merge for singular nested fields, keeping current representation and API; compare read-all and sparse corpora. | Preserve validation, unknown/merge semantics and retention; prove span validity before removing repeated checks. |
| Current candidate qualification | Repair serving/Tower scheduling races with admission witnesses and controlled clocks, then rerun the clean-source workspace and required checks. | Preserve original failures, deadlines, status and drop assertions; keep real I/O externally guarded and finite limits unchanged. |
| QG-06: resources and recovery | Pass a new actual day on the final validated source. | 24 hours pass the original finite limits, deadlines, recovery and fairness checks. |
| Strict CI and platforms | Close strict lint, feature, platform and packaged-consumer gates. | Run the required profiles on the same source; keep unsupported combinations and failures explicit. |
| SB-27: client/server matrix | Extend the measured 1/64 KiB, one/16-in-flight scope. | Cover all payloads, profiles, read-all corpora and lifecycle cases; retain failures and assign losses. |
| CH-12 and remaining integration cards | Complete discovery/balancer runtimes and churn tests. | External consumers use supported APIs; existing connections stay stable across updates. |
| Remaining credential and codec cards | Complete providers and typed-extension support. | Consumer, wire-compatibility and security checks pass. |
| SB-28, SB-22 and SB-16 | Qualify the complete matrix on dedicated x86_64 and arm64 hosts. | Paired statistics, headroom and latency checks support the comparisons. |

The [previous comparison](docs/evidence/grpc-streaming-20261022/README.md) retains
important losses: 64 KiB prost gzip upload requests 49% more allocation bytes
than tonic/prost and grows about 9% against its old source; pbrs grows 10.6%.
Identity bytes and selected gzip/uncompressed instruction regressions remain
recorded. Those values must not be presented as measurements of `90201458`.
The new bounded output lookahead needs matched measurement at `2eba72d8` versus
`804b5c59`, including tail sizes around growth boundaries. Both harnesses use
400 KiB server send buffers and request queues of 16. Historical full matrices
cannot serve as that source baseline; only selected source-paired scopes are
planned. Test retained memory under concurrent large streams;
raising retention caps without attribution is not an accepted remedy.

PK-26's old eager mutex/TypeId default lookup is already removed from current
generated getters. Do not redo that source fix; its measurement/regeneration
acceptance remains separate. Lazy singular materialization still repeats work;
repeated children, including the adoption Query's recursive lists, already parse
eagerly. Do not attribute all recursive-query costs to lazy singular fields.
The [execution decision](docs/decisions/grpc-execution.md) separates those codec,
transport, scheduling and generated-code costs. Strict lint, remaining features
and release gates remain open. Production readiness and broad superiority are
unproved; use the [benchmark contract](docs/benchmark-contract.md) for comparisons.

## Dominance program

Goal: pbrs-grpc beats tonic with prost in every cell, with prost or pbrs
messages and on client and server alike, in CPU, allocations and wall time.
The [dominance program](docs/plan/dominance/README.md) defines the matrix and
the bar, and seeds a loss ledger in which every loss has an owning card.
Start with SB-27 (after SB-26) and RX-10, then flip the known losses: SV-09,
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
