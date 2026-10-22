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

The [streaming follow-up](docs/evidence/grpc-streaming-20261022/README.md) passes
3,677 workspace tests on `6cd91911`, with nine existing ignores. Strict Clippy
still has 115 library diagnostics. Matched native runs complete 2,400 captures
and 720,000 successful RPCs per revision. Selected 1 KiB streaming allocations
improve, but production readiness and broad superiority remain unproved.

**Compression correctness comes first.** Native gzip/Zstd can omit fields after
the first member/frame of valid concatenated input. Gzip, deflate and Zstd do
not fully validate trailing input; Zstd reserved descriptor bit 3 is not rejected.
The opaque tonic adapter has an independent first-member gzip path, so a
native-only repair leaves partial protobuf data. The external proposal is
uncompiled, untested and unapplied. Validate complete input, aggregate limits,
fragmentation, checksums, cancellation and adapter behavior before accepting it.

| Work | Next result | Completion criterion |
|---|---|---|
| Compression input correctness | Fix concatenation, trailing input, reserved-bit rejection and the separate tonic-adapter path. | Complete protobuf data or an explicit error; aggregate output/window limits and bounded resource checks pass. |
| SV-09, CL-08, CL-09, RX-10 | Attribute the largest 64 KiB gzip allocation-byte costs, then identity bytes and instruction regressions. | Matched before/after gains retain message/budget safety, with retained-memory checks and all losses reported. |
| QG-06: resources and recovery | Finish the running 24-hour diagnostic on `6cd91911`. | The actual day passes the original finite limits, deadlines, recovery and fairness checks. |
| SB-27: client/server matrix | Extend the verified 1/64 KiB, one/16-in-flight comparison. | Cover every payload, TLS/compression profile, read-all corpus and lifecycle; retain failures and assign losses. |
| CH-12 and remaining integration cards | Complete external discovery/balancer runtimes and churn tests. | An external consumer uses supported APIs and existing connections stay stable across updates. |
| Remaining credential and codec cards | Complete providers and broader typed-extension support. | Consumer, wire-compatibility and security checks pass. |
| SB-28, SB-22 and SB-16 | Run the complete matrix on dedicated x86_64 and arm64 hosts. | Paired statistics, headroom and latency checks support the comparisons. |

The 64 KiB prost gzip upload server requests 1,773.8 KiB/RPC versus 1,190.6 KiB
for tonic/prost (+49%), and **9.0% more than the old source**; pbrs grows 10.6%
against its old source. The prost gzip bidi server uses 111.2 allocations versus
99.0 (+12%). Uncompressed 1 KiB prost bidi requests 45.8 KiB versus 32.9 KiB
(+39%). These are cumulative allocation requests, not retained memory.
Selected gzip server instructions also remain 2.2–2.4% above tonic/prost;
uncompressed source comparisons retain regressions too.

A message containing a 64 KiB payload exceeds the 64 KiB scratch-retention
threshold. Attribute scratch discard, decoder-output growth/copying and ownership,
and fixed batch reservations before changing caps. These are design hypotheses,
not proven causes. Measure retained memory and bounded behavior under concurrent
large streams while preserving allocation-budget enforcement.

The earlier [day attempt failed after 4 hours 56 minutes](docs/evidence/grpc-resource-soak-20261022/README.md).
Its failed record remains archived. The new running day, the finite preview and
passing workspace do not certify production readiness. Older counter captures
with incorrect concurrency remain invalid; the final matched comparison uses
the repaired generator. Numeric thresholds remain in the
[benchmark contract](docs/benchmark-contract.md).

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
