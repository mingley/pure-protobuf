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

The [message merging and gRPC costs record](docs/evidence/grpc-parity-and-costs-20261027/README.md)
adds final-tree required checks for dynamic and MiniTable messages, direct Tower
stream input, less frame copying and shared generator metadata. It keeps the
map-validation, codegen timing and endpoint losses alongside the improvements.

The [bounded encoding and streaming record](docs/evidence/grpc-encode-reuse-20261026/README.md)
adds 3,855 passing workspace tests, 448 allocation captures / 83,160 successful
RPCs and 180 counters-off CPU captures / 272,160 successful RPCs. Large compressed
streams reuse bounded plaintext storage and avoid compressed-output overshoot;
an early-response race is fixed. Allocation costs fall in the measured large
gzip cases. CPU remains mixed, noise traffic still allocates more than tonic,
and a 64-message identity client case regresses. Keep those losses in scope. The separate
[compatibility repair](docs/evidence/grpc-compatibility-fallback-20261026/README.md)
restores `dyn WireOut`, the default native MSRV profile and consumer gate checks.

Source `af00f348` fixes required-field checks after singular proto2 message merges.
Split child fields, same-case oneofs, maps, groups, MessageSet extensions, cold
fields and parallel generation have detecting controls. Affected consumer bindings
must be regenerated. The two affected bundled proto2 outputs are regenerated;
eleven other outputs are byte-identical. Required-reachable validation can now
allocate a temporary tree.

The [October 26 record](docs/evidence/grpc-required-merge-20261026/README.md) has
33 generated-consumer checks, 113 core checks, 44 runtime/kernel checks and three
official conformance runs, each passing 5,631 binary/JSON and 909 text cases.
Benchmark `eac85b04` adds checked zero/noise payloads for Prost TestService: five
unit checks, 24 positive captures / 48 RPCs and eight expected rejection controls
pass. These are correctness checks, not performance results.

The [compatibility follow-up](docs/evidence/grpc-compatibility-fallback-20261026/README.md)
records the hosted public-API, MSRV and shared-suite inventory failures and their
repairs. Full hosted CI remains separate from the recorded local checks.

The earlier runtime `933bf877` passes 3,840 all-feature workspace tests and 961
selected ASAN/leak tests. Its [dated record](docs/evidence/grpc-ownership-20261024/README.md)
also has 1,116 native/instruction captures / 29,700 successful RPCs; bulk,
shipping-build and tracing controls remain separate. Those results do not
qualify later source changes.

The [benchmarks](docs/benchmarks.md) identify the next costs. Native-server
1 KiB identity unary CPU slopes are 30–56 µs/RPC versus fresh tonic's 24–30.
Four-message 1 MiB noise bidi still requests 35.53% more client bytes and
37.23% more server bytes than tonic/prost; zero-filled traffic now requests
13.50% / 14.39% fewer bytes. The 64 × 1 KiB identity pipeline client requests
2.98% more bytes than baseline. Earlier instruction counts and source regressions
retain their own pins in the [historical tables](docs/benchmarks/grpc-20261024.md).

| Work | Next result | Completion criterion |
|---|---|---|
| Unary-server CPU and scheduling | Find a repeatable CPU improvement with fresh tonic controls. | The new counters-off campaign completes 272,160 successful RPCs; repeats remain mixed and native unary-server CPU still loses to tonic in two of three paired slopes. Earlier wake experiments remain unadopted. |
| Identity/gzip bytes and bulk | Reduce remaining noise and small-pipeline costs. | Bounded reuse now has endpoint measurements across zero/noise and one/four/64-message streams. Custom contiguous codecs keep their original path. Preserve the remaining noise and 64-message identity-client losses. |
| QG-06: sustained resources | Run full exact-source general/deadline days on a host retained for at least 24 hours. | Both `933bf877` attempts are interrupted, not running. Deadline progress ends at 12.35 measured hours; general progress ends at 13,673 cancellation cycles without a final duration. Preserve finite limits, ownership and recovery. |
| Remaining backend destruction | Audit generic current-frame panic, queue-clear, reset and EOF owners beyond the repaired retired DATA frames. | Independent bounded controls before broader safety claims; no hidden work detached from resource ownership. |
| Remaining protobuf compatibility | Complete MiniTable group decoding and extend the cross-runtime corpus. | Keep split-field, oneof/map, recursion and malformed-wire controls; generated-code conformance does not prove every kernel path. |
| PK-27, PK-29: owned read-all decode | Compare one checked singular merge with lazy access on equal sparse/read-all corpora. | Preserve required/merge/oneof/extensions/unknown/recursion semantics and retention; repeated Query children already parse eagerly. |
| Handler isolation and SB-27 | Complete per-side shipping CPU, latency, throughput, entropy, field-access, overload and physical-network comparisons. | Keep inline/task handoff costs, sibling fairness, actual concurrency and all losing scopes. |
| CH-12, credentials and telemetry | Finish discovery/xDS, async OAuth2/JWT, typed extensions and remaining instruments. | Supported external consumers, bounded refresh/cancellation/labels and wire-compatible behavior. |
| Platforms and release | Complete required feature/platform/consumer profiles and dedicated-host headroom/statistics. | Exact-source release checks and claimed workload scopes pass. |

Both `933bf877` days and the earlier `7c02eb38` attempts are **interrupted**, with no terminal resource
outcome or application failure established. Preserved progress and the newer boot
indicate an environment discontinuity; the exit cause is unavailable. They are
not running and cannot qualify the current source. Fresh `933bf877` retry grants
are prepared but have not launched. Older `beb4ecd9` general/cache days passed
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
| TC-27 | P1 | The original adapter covered unary only; all four shapes now have typed Tower adapters. Continue checking stream ownership and middleware lifetime semantics. |
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
