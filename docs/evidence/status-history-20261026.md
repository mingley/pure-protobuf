# Status history, October 26

Source `af00f348` fixes required-field validation after merging proto2 messages.
Affected consumer bindings need regeneration. The [October 26 record](grpc-required-merge-20261026/README.md)
keeps detecting failures, regenerated bindings, reference checks and deferred
experiments. Required-reachable validation can allocate a temporary tree.
Benchmark `eac85b04` adds checked noise payloads for Prost TestService.

| New-source check | Result |
|---|---:|
| Generated consumers | 30 single-target + 3 parallel/imported checks pass |
| Core / runtime / MiniTable-upb | 113 / 26 / 18 checks pass |
| Official conformance | Required twice and recommended once; each 5,631 binary/JSON + 909 text pass |
| Bundled binding regeneration | 11 outputs unchanged; 2 proto2 outputs regenerated |
| All-feature workspace compilation | `cargo check --locked --workspace --all-features` passes; tests were not run |
| Benchmark correctness | 5 unit checks, 24 positive captures / 48 RPCs, 8 expected rejections |

These selected checks do not replace full workspace, sanitizer, endpoint and
sustained-resource qualification for the combined source. A new LeakSanitizer
attempt passes 113 core tests before exiting fatally and printing its ptrace
limitation; the precise cause is unavailable. Separate AddressSanitizer-only
checks pass 190 cases with leak detection disabled. Neither is a leak-check pass. The following broader
checks belong to the earlier [ownership source](grpc-ownership-20261024/README.md).

| Check | Actual source/scope | Result |
|---|---|---|
| Full all-feature workspace | Clean `933bf877` | **3,840 passed, zero failed, nine existing ignores**, 145 summaries; 1,350.798 seconds |
| ASAN with leak detection | `933bf877`, 13 selected ELFs | **961 passed, zero failed, one existing ignore** |
| Official protobuf conformance | Pinned upstream v35.1; maximum Edition 2023 | Required twice and recommended once: each **5,631 binary/JSON + 909 text**, zero skipped/expected/unexpected failures |
| Lint, formatting and docs | Source-bound precommit snapshots; clean final rustdoc | Strict workspace Clippy/fmt and warning-strict rustdoc pass; equivalence proof binds tested batch/kernel files to final Git blobs |
| Minimum Rust profiles | Clean `933bf877` | Rust **1.85** core and **1.87** optional Zstd pass |
| Native / Callgrind counters | `7c02eb38` → safety `ca4ef720` → measured `933bf877` | **576 / 540 captures**, 21,600 / 8,100 successful RPCs; all accounting errors zero |
| Separate bulk / shipping / quiet replay | Explicit source/feature scopes | **240 / 48 / 16 captures**, 9,000 / 360,000 / 120,000 successful RPCs; CPU/byte losses remain |
| Separate syscall / fd traces | Own-child tracing, timing unqualified | **8 / 8 captures**, 12,000 / 600 successful RPCs; raw errors and unresolved teardown calls retained |

Before/after workspace proofs match all **3,797 physical Git blobs, modes and
sizes: 910,511,102 bytes**. The original native counter controller exited 1 after
all captures when sparse checkouts lacked plan files. Exact Git materialization
and a separate postprocessing recovery preserve that failure and reuse all raw
captures; no failed process is relabeled as passed.

The quiet same-source replay retains a native unary-server CPU loss in all four
raw comparisons (53–60 versus 40–45 µs/call); timing is mixed. The earlier run
records a throughput loss whose magnitude is not generalized to the replay. Identity bidi client bytes exceed tonic by 5.70%; 1 MiB gzip bidi
bytes exceed it by 16.40% client / 17.43% server. All 30 native instruction groups
are lower in both repeats, while 22 medians rise against the previous runtime.
The benchmark page separates those results and keeps every source/control increase.

Earlier runtime release previews pass **34.370 / 30.796 seconds**. Both
`933bf877` 24-hour attempts are now **interrupted and incomplete** after an
environment restart. Deadline progress ends at **12.35 measured hours** and
43,781 repetitions of the same 30 cases. General progress ends at 13,673
cancellation cycles without a final measured duration. No terminal process result
survives, the exit cause is unknown, and no application failure is established.
Fresh same-source retry grants are prepared but have not launched. Earlier
`7c02eb38` attempts are also interrupted; `beb4ecd9` general/cache passes apply
only to their recorded source.

The latest wake proxy passes 100 focused checks and 360,000 RPCs but has mixed
CPU results, so it remains unadopted. Bounded encode scratch passes 738 library
checks and reduces a measured scratch allocation site by 75%; it adds copies for
custom contiguous codecs and its endpoint matrix is unrun. A later CPU diagnostic
fails before RPCs because this session denies local listeners. Neither experiment
is included in the current runtime.

Stream decode reuse keeps the 4 MiB per-buffer / 16 MiB process idle-capacity limit.
Retained Bytes/lazy pbrs input keeps ownership. Active decoding, application
messages, codec scratch and RSS remain separate. Complete compressed members,
streams and frames retain output/history/work limits.

Sustained resources, remaining features/specification coverage, required platforms,
releases and the complete per-side performance matrix remain open. These checks
are not a production certification; broad superiority remains unproved.

Use this page to distinguish source features, recorded tests and missing
qualification. The [2026-09-29 audit](../audit-2026-09-29.md) reviewed
`37683917`; [the queue](../../TODO.md) names the next work.

At that revision, ordinary workspace tests and conformance passed in CI, but
warning-strict rustdoc and the original shared-consumer suite failed. The
documentation repair landed in `deea5e3f`. The enum-map and repeated-enum
repair landed in `c89608bd`; its [Linux compatibility run](https://github.com/mingley/pure-protobuf/actions/runs/36615248310)
passes all 19 original consumer crates. See the
[recovery evidence](shared-map-recovery.md) for the conversion contract
and exact-source ordinary/Miri runs: 233 original tests plus three regressions.
QG-05 and the compiled public-guide work (QG-07, `80013bfc`) are delivered.
The [closed-enum follow-up](closed-enum-recovery.md) at `3a7aa128`
also validates wire numbers before populating typed fields, lists and maps.
Its clean-source ordinary/Miri runs pass 233 original tests plus six
regressions, with 96 core library tests. Unknown values survive serialization
without creating list holes or replacing valid map entries.
These repairs are not a production certification.

