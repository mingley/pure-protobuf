# GN-01: downstream compile-time cost attribution

This evidence attributes local compile-time costs for pbrs generated consumers.
Numbers are **dev-loop, contended host** diagnostics on macOS arm64 with
`CARGO_BUILD_JOBS=3`; they are not claim-grade performance results.

## Inputs

- Source: `mingley/gn-codegen-speed` worktree, based on `7aec79e2`, with the
  PK-03 feature split applied.
- Corpora: SB-09 seeded 100-message and 1,000-message pbrs consumers generated
  by `target/codegen-bench/gn04-genonly-20260929T003526Z`.
- Tools: `cargo --timings` was used. Nightly `rustc -Z self-profile` raw
  profiles were collected for the 100-message `json,text` profile and the
  1,000-message default profile. `cargo-llvm-lines` and the `summarize`
  self-profile reader were not installed, so no `llvm-lines` or summarized
  self-profile table is claimed.

## Ranked cost sources

| Rank | Cost source | Evidence | Impact | Follow-up |
|---:|---|---|---|---|
| 1 | Generated source volume: per-message field-wise parse/encode/JSON/text/descriptor helpers | GN-01 `cargo --timings`: in the 1,000-message default check, the generated consumer crate itself took 54.37 s of the 73.6 s cargo timeline. GN-04 generation-only output sizes: pbrs 33.3 MB vs prost 403 KB on 1,000 messages. | Dominates downstream check/build time once message count is high; runtime feature trimming cannot remove this cost. | Code-size/emission shrink belongs to the codegen-parity/codec workers because `messages.rs`, `parse.rs`, and `encode.rs` are outside this card's write scope. |
| 2 | Default `pbrs` feature graph compiles build-time code in runtime consumers | 100-message `cargo --timings`: default `pbrs` check was 7.93 s; the `json,text` runtime profile's `pbrs` check was 1.45 s. | PK-03 removes generator and bundled conformance gencode from runtime dependency graphs that opt out of defaults; 100-message clean check improved 17.42 s → 9.48 s. | Keep adapters on defaults for compatibility; document split runtime/build dependency profiles. |
| 3 | Descriptor/format support in generated files | Current generated files call `DescriptorPool`, `DynamicMessage`, `pbrs::json`, and `pbrs::text` helpers for reflection-backed format methods. | Generated-code runtime profiles still need `json,text` (which imply `reflect`) until emission can make JSON/text optional; the generated consumer check became the top unit (4.66 s) after trimming `pbrs`. | Future emitter card: generate optional JSON/text methods or feature-gate format methods in generated output. |
| 4 | Deep generated type recursion / monomorphization | Nightly raw self-profile on the 1,000-message default consumer emitted a future-incompat warning: auto-trait recursion over `Message0600`/previous-message chains exceeded the recursion-depth lint. | Confirms the generated message graph itself is a rustc stressor, independent of runtime modules. | Break long generated type dependency chains or reduce per-message generic/default-instance shape in a coordinated emitter change. |
| 5 | Release monomorphization of many field-wise impls | SB-09 release builds: pbrs 45.1 s (100) and 196.7 s (1,000), versus prost 8.2 s and 82.3 s. | Larger release cost than clean check because every message has specialized parse/write/size/format code. | Use compile-cost rows to prioritize generated-code shrink before further runtime micro-optimizations. |

## Feature-split measurements

The runtime/build split was checked independently of generated-output changes:

| Profile | Command | Result |
|---|---|---|
| Core runtime only | `CARGO_BUILD_JOBS=3 cargo check -p pbrs --lib --no-default-features` | Passed; compiles only the binary wire runtime plus `testdata`. |
| Generated-code runtime | `CARGO_BUILD_JOBS=3 cargo check -p pbrs --lib --no-default-features --features json,text` | Passed; compiles descriptor/JSON/text support, but not generator or bundled conformance gencode. |
| Build-time codegen | `CARGO_BUILD_JOBS=3 cargo check -p pbrs --lib --no-default-features --features codegen` | Passed; compiles generator and descriptor support without JSON/text modules or optional serde dependencies. |
| 100-message generated consumer, default `pbrs` | `/usr/bin/time -l env CARGO_BUILD_JOBS=3 CARGO_TARGET_DIR=target/pk03-feature-split/target-default cargo check --offline --locked --manifest-path target/pk03-feature-split/100-default/Cargo.toml --bin cg19-consumer-100 --timings` | 17.42 s real, 967 MiB max RSS; cargo timing total 17.3 s; top units: `pbrs` check 7.93 s, consumer check 3.79 s. |
| 100-message generated consumer, `features = ["json", "text"]` | `/usr/bin/time -l env CARGO_BUILD_JOBS=3 CARGO_TARGET_DIR=target/pk03-feature-split/target-json-text cargo check --offline --locked --manifest-path target/pk03-feature-split/100-json-text/Cargo.toml --bin cg19-consumer-100 --timings` | 9.48 s real, 560 MiB max RSS; cargo timing total 9.4 s; top units: consumer check 4.66 s, `pbrs` check 1.45 s. |
| 1,000-message generated consumer, default `pbrs` | `/usr/bin/time -l env CARGO_BUILD_JOBS=3 CARGO_TARGET_DIR=target/pk03-feature-split/target-1000-default cargo check --offline --locked --manifest-path target/pk03-feature-split/1000-default/Cargo.toml --bin cg19-consumer-1000 --timings` | 73.84 s real, 4.00 GiB max RSS; cargo timing total 73.6 s; top units: consumer check 54.37 s, `pbrs` check 7.68 s. |

## GN-02/GN-03 follow-up

The accessor-only generation profile added in GN-02/GN-03 confirms row 1:
removing optional reflection/JSON/text helpers cuts generated source volume
substantially, but the remaining binary parse/encode/accessor surface still
dominates large corpora.

| Cell | Default pbrs | Accessor-only pbrs |
|---|---:|---:|
| 100 generated Rust bytes | 2,150,801 | 1,112,692 |
| 100 clean check | 16.38 s | 8.67 s |
| 100 release build | 54.49 s | 20.03 s |
| 1,000 generated Rust bytes | 34,130,773 | 10,980,049 |
| 1,000 clean check (same-tree diagnostic) | 51.70 s | 40.55 s |
| 1,000 release build (same-tree diagnostic) | 270.85 s | 241.34 s |

Raw self-profile files:

- `target/pk03-feature-split/100-json-text/target/pk03-feature-split/self-profile-json-text/cg19_consumer_100-0039453.mm_profdata`
- `target/pk03-feature-split/1000-default/target/pk03-feature-split/self-profile-1000-default/cg19_consumer_1000-0045867.mm_profdata`

## Limits

- The host was shared with other fleet workers, so wall-time rows are local
  diagnostics. SB-09 and this file label them as dev-loop evidence only.
- `cargo-llvm-lines` was not installed and was not installed for this task.
- Self-profile raw data without a summarizer is retained only as supporting
  artifact metadata. The only interpreted self-profile-adjacent signal is the
  nightly compiler warning emitted during the 1,000-message raw-profile run.
