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

Source `933bf877` fixes completed DATA-owner retirement, pre-shape reset ownership
and malformed unknown-group acceptance, then adjusts singleton/bulk reservations.
[The current record](docs/evidence/grpc-ownership-20261024/README.md) keeps detecting
failures, source snapshots and every measured loss.

The full all-feature workspace passes **3,840 tests**, zero failures and nine
existing ignores across 145 target summaries. ASAN/leak checks pass **961 tests**
(one existing ignore) across 13 selected ELFs. Three official conformance passes
through Edition 2023 each pass 5,631 binary/JSON and 909 text cases. Strict lint,
formatting, warning-strict docs and the recorded Rust 1.85 / 1.87 profiles pass.
Native/instruction counters complete 1,116 captures / 29,700 successful RPCs;
bulk, shipping-build and tracing controls remain separate.

The [benchmarks](docs/benchmarks.md) identify the next costs. Unary-server shipping
CPU still loses to tonic; the quiet four-repeat replay uses 53–60 µs/call
versus tonic's 40–45 µs at 10,000 calls. Identity bidi client bytes remain 5.70% higher; 1 MiB gzip bidi bytes are
16.40% higher on the client / 17.43% on the server. The 64-message pipelined server
byte changes are +1.15% and +26.75% versus the safety source. All 30 instrumented
instruction groups use fewer instructions than tonic in both repeats; 22 of 30
instruction medians rise versus `7c02eb38`. These metrics answer different questions.

| Work | Next result | Completion criterion |
|---|---|---|
| Unary-server CPU and scheduling | Test notification forwarding and executor work with a separate wake/cancellation correctness model. | The [ready-set experiment](docs/evidence/grpc-ready-set-20261024/README.md) passes 87 focused checks and 360,000 RPCs but has mixed CPU results; adoption is deferred. Preserve status, flow control, cancellation, deadlines, owner destruction and every losing control. |
| Identity/gzip bytes and bulk | Attribute remaining buffers and copies, including the 64-message pipeline loss. | Keep cold/bulk tradeoffs, finite retention budgets and every losing cell; capacity arithmetic is separate from measured allocation sites. |
| QG-06: sustained resources | Complete the running general/deadline days. | Previews pass 34.37 / 30.80 seconds; both `933bf877` days are advancing in the 22:48:16 UTC snapshot. Full-duration results remain pending; keep finite limits, balanced ownership and recovery. |
| Remaining backend destruction | Audit generic current-frame panic, queue-clear, reset and EOF owners beyond the repaired retired DATA frames. | Independent bounded controls before broader safety claims; no hidden work detached from resource ownership. |
| PK-27, PK-29: owned read-all decode | Compare one checked singular merge with lazy access on equal sparse/read-all corpora. | Preserve required/merge/oneof/extensions/unknown/recursion semantics and retention; repeated Query children already parse eagerly. |
| Handler isolation and SB-27 | Complete per-side shipping CPU, latency, throughput, entropy, field-access, overload and physical-network comparisons. | Keep inline/task handoff costs, sibling fairness, actual concurrency and all losing scopes. |
| CH-12, credentials and telemetry | Finish discovery/xDS, async OAuth2/JWT, typed extensions and remaining instruments. | Supported external consumers, bounded refresh/cancellation/labels and wire-compatible behavior. |
| Platforms and release | Complete required feature/platform/consumer profiles and dedicated-host headroom/statistics. | Exact-source release checks and claimed workload scopes pass. |

The earlier `7c02eb38` day attempts are **interrupted**, with no terminal resource
outcome or application failure established. Preserved progress and the newer boot
indicate an environment discontinuity; the exit cause is unavailable. They are
not running and cannot qualify `933bf877`. Older `beb4ecd9` general/cache days passed
only their recorded source and scope. The [execution decision](docs/decisions/grpc-execution.md)
explains ownership and comparison boundaries; [the benchmark contract](docs/benchmark-contract.md)
sets performance requirements. Production readiness, full specification coverage
and broad superiority remain open.

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
