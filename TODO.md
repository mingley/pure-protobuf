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

Source `7c02eb38` implements one accepted absolute deadline, cooperative service
ownership, committed unary status accounting, bounded gzip output reads and
compact routed dispatch. The [current evidence](docs/evidence/grpc-deadlines-20261024/README.md)
retains the detecting failures, repairs and source-specific measurements.

All 11 prechecks pass, including 3,250 gRPC tests (four existing ignores), strict
lint/docs, benchmark checks and both minimum-version profiles. Selected
ASAN/leak checks pass 787 tests (one existing ignore) across nine binaries.
**The full all-feature workspace passes 3,812 tests**, zero failures and nine
existing ignores across 144 targets, at repository `8144576f`. Its production,
codegen, harness, manifest and lock blobs match measured runtime `7c02eb38`.
Canonical counters pass 1,448 captures / 44,904 RPCs; the Router control separately
passes 48 / 1,800. Release general/deadline previews pass 33.418 / 30.792 seconds.
The deadline preview repeats the same 27 cases for 31 cycles; it is not 837
independent tests or a completed day. Both current-source 24-hour days are
**running, with results pending**, in the 2026-10-24 09:27 UTC snapshot.

The [benchmarks](docs/benchmarks.md) identify the remaining costs: identity bidi
bytes exceed tonic by 34.92% on the client / 32.26% on the server; the pbrs interop
1 MiB gzip bidi server gap is 176.88% with additional handler work. Source server
unary costs rise 5.12% in requested bytes for TLS/16 in flight and 16.14% in
instrumented instructions for plaintext. All 22 measured native instruction
comparisons beat tonic, but these counter results do not establish shipping speed.

| Work | Next result | Completion criterion |
|---|---|---|
| QG-06: sustained resources | Complete the running current-source general/deadline days. | Actual terminal records for runtime `7c02eb38`, repository validation `8144576f`, finite limits, balanced ownership and recovery; older-source days never substitute. |
| Ordinary DATA-owner destruction | Reproduce the source concern about a successful DATA payload's destructor running under a backend lock. | A bounded independent control first; this remains an unreproduced source finding, not an established deadlock or a completed repair. |
| Identity/bidi costs and first-frame reservations | Attribute allocation/copy owners before tuning reservations or scratch vectors. | Retain burst behavior, frame rollback, byte permits, cancellation and all losing cells; source arithmetic is not measured savings. |
| Owned encode scratch and compressed reservation | Attribute each allocation owner before retaining more storage. | Finite capacity budgets, short/cold costs and terminal cleanup; active/idle definitions remain explicit. |
| PK-27, PK-29: owned read-all decode | Compare checked eager singular merge with lazy access on equal sparse/read-all corpora. | Preserve validation, unknown/merge/oneof/recursion semantics and retention; repeated Query children already parse eagerly. |
| Handler isolation | Compare inline execution with owned per-RPC tasks under finite admission and spare workers. | Preserve deadline origin, reset/response ownership, permits through cleanup and abort/drain; measure handoff and mixed siblings. |
| SB-27: complete client/server matrix | Add shipping CPU, latency, throughput, field access, entropy, overload and real networks. | Both endpoint metrics, scheduling/syscall/wake attribution and all losing scopes remain visible. |
| CH-12 and discovery | Complete balancers/discovery, xDS targets and control plane. | Supported external consumers; refresh, failure and churn preserve live connections and bounded work. |
| Credentials and codec coverage | Finish async OAuth2/JWT providers and typed protobuf extensions. | Refresh, expiry, cancellation, consumer and wire-compatibility checks pass. |
| Telemetry | Complete retry, message-size, connection, WRR and dynamic-label instruments. | gRFC coverage, finite label costs and measured observability overhead. |
| Platforms, release and SB-28/SB-22/SB-16 | Complete required features/platforms/consumers and dedicated-host comparisons. | Exact-source profiles, paired statistics, headroom and latency checks support each claimed scope. |

Earlier [stream decode reuse](docs/evidence/grpc-decode-reuse-20261023/README.md)
keeps a 4 MiB per-buffer / 16 MiB process idle-capacity limit; retained Bytes/pbrs
input preserves ownership. The older `beb4ecd9` [general resource day](docs/evidence/grpc-decode-reuse-soak-20261023/README.md)
and [Prost-cache day](docs/evidence/grpc-prost-cache-diagnostic-20261023/README.md)
both passed their own full durations. They cannot qualify changed `7c02eb38` code.

Timers cannot preempt synchronous application polls. A per-connection driver
cannot isolate sibling handlers; spawning on a current-thread runtime adds no
thread. Keep permits and buffers until actual cleanup and do not detach RPC work.
PK-26's eager mutex/TypeId lookup is already removed. Shared-input keeper and
encode-cache proposals remain experiments. Use the [execution decision](docs/decisions/grpc-execution.md)
and [benchmark contract](docs/benchmark-contract.md) for design and comparison rules.
Production readiness, complete specification coverage and broad superiority
remain open alongside the concrete work above.

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
