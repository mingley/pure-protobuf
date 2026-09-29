# Pure-Rust `.proto` frontend route (GN-05)

Decision: contribute the missing full-profile surface to `protox`
upstream as bounded slices, and do not build an in-repo `.proto`
compiler. Keep `compile_descriptor_set` as the only Rust-only path
until every slice below is differentially proven; keep `compile_protos`
an explicit `protoc` route with no automatic fallback between compilers.

**Status:** Draft for maintainer review; no code ships with this
document. **Baseline:** `3a0cc56d` on 2026-09-29. **Supersedes:**
nothing (first route decision). **Companion:**
[the Rust frontend review](../rust-frontend.md) (CG-16 audit, the
evidence base). **Depends on:** [the Edition 2024
contract](../edition-2024.md) for feature semantics, and the GN-08
corpus as the acceptance oracle.

**Review gate:** the maintainer must approve the upstream-first route,
the contribution slices, and the reduced-profile experiment boundary
before GN-06 starts. GN-08 (pinned corpus plus baseline-only harness)
is approved to proceed immediately because it is route-independent.

## What was evaluated

The CG-16 audit (2026-09-23, tree `d34334e1`) reviewed pinned
releases by direct source reading; no candidate was installed or
compiled here. A 2026-09-29 freshness check found no newer editions
support in any candidate, so the audit stands as baseline:

| Candidate | Pinned version | License (direct manifest) | MSRV (declared) | Full-profile gap |
|---|---|---|---|---|
| `protox` (+ `protox-parse`) | 0.9.1 (2025-12-02) | MIT OR Apache-2.0 | 1.74 | grammar is proto2/proto3-only; imports lack `option`; no Editions 2023/2024, `export`/`local`, or feature resolution |
| `protobuf-parse` | 3.7.2 (2025-03-10) | MIT | none declared | proto2/proto3-only grammar; convenient FDS API omits imports; lower-level result unstable; pure conversion drops source locations |
| `protofish` | 0.5.3 (2025-12-06) | MIT OR Apache-2.0 | none declared | a decoding-context library, not an FDS compiler: proto3-only grammar, ignores imports and file options |

Neither `protobuf-parse` nor `protofish` can reach the full profile
(deterministic, import-complete `FileDescriptorSet` with source
locations and extension/custom options intact) without becoming a new
compiler. They are rejected as upstream targets.

## Option 1: contribute upstream to `protox` (selected)

Bounded contribution slices, each reviewed upstream and proven here
against the GN-08 differential corpus before the next starts:

1. `import option` plus import-closure and same-stem fixture parity.
2. Editions 2023/2024 grammar and `FileDescriptorSet` encoding.
3. `export`/`local` visibility, naming and feature-default resolution.
4. ProtoJSON/text-relevant option fidelity where the corpus demands it.

- **License:** `protox` declares MIT OR Apache-2.0, compatible with
  this repository. No dependency is added until the complete locked
  transitive graph is audited for license, MSRV, and C/FFI.
- **MSRV:** declared 1.74 is below `pbrs`'s 1.85 floor, so no bump is
  implied. Transitive MSRV is part of the pre-dependency audit.
- **Transitive graph:** unknown today (the audit never installed or
  compiled it). Gate: record the locked graph, confirm the
  Rust-only/no-FFI boundary across it, and only then vendor the
  dependency behind the reduced-profile experiment.
- **Maintenance ownership:** the compiler stays upstream
  (`andrewhickman/protox`); `pbrs` owns its contribution patches and
  the GN-08 differential fixtures that prove them. No fork unless the
  fallback triggers with dated evidence.
- **Upstream acceptance risk:** real. `protox` is a small-maintainer
  project and the editions slices are a large feature ask.
  Mitigation: small reviewable PRs, each carried by corpus fixtures
  the maintainer can run without `pbrs`. Fallback: if an ask is
  explicitly rejected, or gets no maintainer response within four
  weeks of submission, the coordinator re-opens GN-05 with the dated
  thread and re-scopes GN-06 to the smallest in-repo slice the corpus
  still proves — still bounded, still no unbounded compiler.
- **Schedule:** sequenced, not dated. GN-08 baseline first, then one
  slice at a time; the reduced-profile opt-in experiment
  (`Config::compile_protos_rust`, proto2/proto3-only, Editions and
  unresolved options fail loudly) ships only after the transitive
  audit plus green corpus differentials. Full-profile CG-17 stays
  blocked until the editions slices land upstream.

## Option 2: in-repo frontend (rejected)

Rejected as unbounded. A correct `.proto` compiler (Editions
2023/2024 grammar, linking, visibility, feature inheritance, protoc
diagnostic parity) is a multi-card project that duplicates `protox`
and `protoc` behavior the corpus would still have to prove line by
line. The CG-16 audit explicitly warns against building a new
compiler as an unbounded workaround. Revisit only through the
Option 1 fallback, one bounded slice at a time.

## Follow-ups

- GN-08: build the pinned corpus and `scripts/frontend-diff.sh`
  (parse, link, full modes; baseline-only runs with no frontend).
- Pre-dependency audit card: locked transitive graph review for the
  chosen `protox` pin (license, MSRV, FFI) before any manifest change.
- Reduced-profile experiment: opt-in `compile_protos_rust` behind the
  audit plus green differentials; no silent semantic acquisition.
- CG-17 stays blocked on the upstream editions slices.
