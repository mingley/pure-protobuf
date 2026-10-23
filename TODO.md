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

[Bounded stream decode reuse](docs/evidence/grpc-decode-reuse-20261023/README.md)
is implemented at `beb4ecd9`: uniquely recoverable prost input can reuse decoded
storage under a **4 MiB per-buffer / 16 MiB process idle-capacity limit**. Retained
Bytes/pbrs input keeps its ownership. Logical initialization is bounded separately
from physical cached capacity; observed terminals release idle storage.

The clean all-feature workspace passes **3,771 tests**, zero failures and nine
existing ignores across 141 targets. All ten prechecks pass; leak-detecting
AddressSanitizer passes **746 tests**, zero failures and one existing ignore.
The completed campaigns pass **3,064 captures and 742,740 successful RPCs**.
These are scoped functional/counter results, not production or broad speed proof.
Original C10 failures, fixture repairs and the failed control classifier remain
in the capsule with their actual outcomes.

The 24-hour general-resource test started at **06:07 UTC on 2026-10-23**; the
separate Prost-cache test started at **06:23 UTC**. Both are running; results
are pending. The general workload uses pbrs, while the cache harness exercises
production code equivalent to `beb4ecd9`. Neither has established a 24-hour pass.
C8's day remains pinned to C8 and cannot qualify changed C11 code.

| Work | Next result | Completion criterion |
|---|---|---|
| Admission deadline and full service ownership | Reproduce the source-reviewed pre-shape custom-Service deadline gap, then repair it in a separate source. | One fixed deadline from local header acceptance; expire cooperative pre-shape waits with one valid terminal response, exact drops and recovery. The proposed raw-peer control has not run. |
| Gzip output initialization | Replace only the profile-attributed repeated output-scratch initialization, then compare. | Preserve independent gzip members, CRC/trailers, writer errors and limits; lower the measured 1 KiB upload client instruction cost without hiding other costs. No replacement is implemented. |
| Identity first-frame reservations | Test the prepared known-single reservation option; tune 64-slot scratch vectors separately. | Attribute actual allocations/copies; retain burst behavior, frame rollback, byte permits and cancellation. Source arithmetic is not measured savings. |
| Owned encode scratch and compressed reservation | Attribute each allocation owner independently before retaining more storage. | Finite explicit capacity budgets, short/cold-stream costs and terminal cleanup; active/idle definitions and any extra ledger allowance remain visible. |
| PK-27, PK-29: owned read-all decode | Compare checked eager singular merge with the lazy path on equal sparse/read-all corpora. | Preserve parse-time rejection, unknown/merge/oneof/recursion semantics and retention; repeated Query children already parse eagerly. |
| Handler isolation | Compare inline execution with owned per-RPC tasks under explicit finite process admission and spare workers. | Preserve fixed deadline origin, reset, response ownership, permits until actual cleanup and abort/drain; measure small-call handoff and expensive/cheap siblings. |
| QG-06: current sustained resources | Complete the generic day and run a separate bounded Prost-cache campaign. | Actual full-duration records at the correct source; cache saturation, retention and cleanup are exercised, not inferred from C8 or generic traffic. |
| SB-27: complete client/server matrix | Extend measured cells to realistic field access, broader payloads, overload and real networks. | Record latency, throughput, CPU, allocations, bytes and scheduling on both endpoints, retaining all losing cells. |
| CH-12 and discovery | Complete discovery/balancer integrations, xDS targets and the xDS control plane. | External consumers use supported APIs; refresh, failure and churn preserve live connections and bounded work. |
| Credentials and codec coverage | Finish async OAuth2/JWT credential providers and typed protobuf extensions. | Refresh, expiry, cancellation, consumer and wire-compatibility checks pass. |
| Telemetry | Complete retry, message-size, connection, WRR and dynamic-label instruments. | Record gRFC coverage, finite label costs and observability overhead. |
| Platforms, release and SB-28/SB-22/SB-16 | Complete required feature/platform/consumer gates and dedicated-host comparisons. | Exact-source required profiles, paired statistics, headroom and latency checks support each claimed scope. |

Current prost gzip stream requested bytes fall **31–36%** against C8 in selected
cases, but 1 MiB bidi still costs **17–18% more than tonic**. Expanded 1 KiB identity
bidi bytes remain **36.7% higher on the client / 31.9% on the server**. The full
pbrs interop server has a **+178%** 1 MiB gzip bidi byte gap; its additional handler
work prevents a codec-only attribution. Selected source byte/count increases stay
visible too; no 2% cutoff removes them from the ledger.

C8→C11 instruction changes are all under 0.4%; no CPU gain is established. The
current 1 KiB gzip upload client uses **6.629% more diagnostic instructions** than
tonic. Retained profiles attribute the extra work chiefly to repeated gzip writer
output initialization, rather than different compressor initialization or levels.
This is allocation-instrumented Callgrind evidence, not shipping CPU or latency.

The current deadline finding exists before any new task queue: a cooperative
custom Service can hold Rpc before choosing a shape, while its timer is not yet
owned; shape preparation restarts the duration and repeated deadline getters
advance the reported expiry. The independent reproduction is **not run**. Fixing
only a future handoff would not close this current origin/outer-owner gap.

Task isolation is a separate concern. One driver per connection can isolate
protocol progress but not a cheap RPC from a heavy sibling on that driver.
Spawning on a current-thread/per-core runtime adds no execution thread. Timers
and Tokio abort cannot preempt a poll already executing; keep permits and buffers
owned until actual cleanup, and do not detach work by dropping a JoinHandle.

PK-26's old eager mutex/TypeId getter lookup is already removed; do not redo it.
Shared-input keeper and encode-cache proposals remain unimplemented experiments.
Use the [execution decision](docs/decisions/grpc-execution.md) and
[benchmark contract](docs/benchmark-contract.md) for design and comparison rules.
Production readiness, complete spec coverage and broad superiority remain unproved.

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
