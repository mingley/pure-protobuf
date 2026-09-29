# GN-04: Make the generator itself the fastest

Date: 2026-09-29. Base SHA `785c5e35`, branch
`mingley/gn04-generator-speed`. Host: Apple Silicon macOS, 14 CPUs,
shared/contended — absolute times drift run to run, so every speedup
below is a same-window A/B (base vs GN-04 binary back to back).

Prior attempt (base `4f081be4`, see git history of this file): shared
FDS hex block, `ColdPlacement`, hex tables. It left pbrs trailing
prost-build on most SB-09 time cells. This implementation attacks the
remaining generator-side costs: quadratic path matching, repeated pool
lookups, per-field allocation, and single-threaded emission.

## Profile (release, 1000-message / 20-file corpus, FDS in)

`sample` over the pre-change generator, emission only (protoc excluded):

- `file_matches` + `normalize_proto_path_str` ~40% of generation time:
  every (target, type) pair re-normalized paths and rebuilt `format!`
  suffixes, including a `targets × types × targets` scan hidden in the
  same-stem/pub-import checks (fresh `BTreeSet` per test).
- Per-target `pool.collect_names()`, `collect_enum_names()`, message-name
  set rebuilds, and `pool.get_message()` per (target, type).
- `DescriptorPool::from_file_descriptor_set` ~8 ms of ~35 ms
  (`src/dynamic.rs`, outside this card's write scope).

## Changes (all output-preserving)

`src/codegen/descriptors.rs`:

- `FileMatcher`: pre-normalized `wanted` set; `matches()` is
  allocation-free (`Cow` borrowed fast path for already-normal paths,
  boundary-aware suffix tests instead of `format!`).
- `file_matches` keeps its signature and semantics (reimplemented on the
  same core, so `prost_stubs` and validation call sites benefit too);
  new `file_matches_single` for singleton tests.
- `TypeFacts` (per message/enum: file, map-entry, extern, WKT) derived
  once while the type→file map is built; `FileFacts` per unique file
  (stem, matches-any-target) memoized; the two post-pass `retain`s are
  folded into the single selection pass with identical predicates.
- Per-target emission becomes a pure `emit_one` closure over
  shared-immutable inputs; multi-target requests emit via
  `std::thread::scope` (workers = `min(parallelism, targets)`), each
  worker initializing its own thread-local codegen state from the
  resolved config. Results re-sequenced in target order, first error in
  target order wins, panics propagate via `resume_unwind` — sequential
  behavior preserved exactly. Single-target requests run inline.
- `normalize_proto_path_str` keeps behavior (slow path factored out,
  borrowed fast path added).

`src/codegen/naming.rs`:

- `rust_type_path` resolves inside the `TYPE_FILES` borrow (no more
  per-field `(file, package)` + current-target clones, no singleton
  `BTreeSet`).
- `emit_public_uses` uses one `FileMatcher` over all targets.
- `rust_enum_values` builds the three strip prefixes once per enum
  (was: per value); `bind_field_idents` treats reserved `unknown` as
  used without seeding an allocation. Both provably identical.

`src/codegen.rs` needed no changes (hex tables already shared).

## Byte-identical proof

- 6-cell base-vs-GN-04 output matrix (`diff -r` clean on every file):

| cell | targets | output | base | GN-04 | result |
|---|---|---:|---:|---:|:---:|
| synthetic 1000-msg | 20 | 34.8 MB | 80.3 ms | 29.3 ms | IDENTICAL |
| single target | 1 | 1.7 MB | 14.5 ms | 13.5 ms | IDENTICAL |
| services + enums | 2 | 302 KB | 0.5 ms | 0.6 ms | IDENTICAL |
| WKT cross-file | 1 | 130 KB | 0.3 ms | 0.3 ms | IDENTICAL |
| realistic OTLP | 5 | 1.6 MB | 2.0 ms | 1.4 ms | IDENTICAL |
| realistic xds/envoy | 111 | 164 MB | 152.2 ms | 43.0 ms | IDENTICAL |

  Same-window release medians; RSS flat or slightly down on every cell
  (xds: 500.9 → 482.5 MB). Multi-target speedups 1.4–3.5×;
  single-target still wins (matchers); tiny cells are noise-flat.

- `scripts/regen-generated.sh` drift hash identical before/after:
  `57084f7c` (tracked diff) plus byte-identical untracked outputs.
  `regen-check` itself is red pre-existing (committed generated files
  lag the generator); the GN-04 change adds zero drift.
- SB-09 harness `generation_unchanged` checks pass on all 9 pbrs cells
  (full file counts verified, run fails otherwise).
- New unit tests: matcher/`file_matches_single` equivalence against a
  kept copy of the pre-change implementation over an adversarial path
  battery (~1k pairs); `Cow` fast-path test; hand-encoded two-file FDS
  multi-target determinism test (25 iterations).

## SB-09 verification (harness, pbrs vs prost-build vs protoc --rust_out)

Run: `CARGO_BUILD_JOBS=3 ./scripts/codegen-bench.sh --case all
--generators pbrs,prost,v4 --stub-generators pbrs-native --repeats 5
--jobs 3 --generation-only --out target/codegen-bench/gn04-full`.
B1/B2 generation-only: byte validation + unchanged-output checks, no
downstream cargo phases. Debug drivers; protoc 36.2 for pbrs/prost
cells (`b1505c80…`), pinned v35.1 protoc for v4
(`e2b116ef…`, matches the reference pin; source
`35cd01f9…`, `--rust_out` with `experimental-codegen=enabled,kernel=upb`).

Medians of 5, generation time (protoc + emission) / peak RSS:

| case | pbrs | prost-build | protoc `--rust_out` (v4) |
|---|---:|---:|---:|
| small | 93 ms / 12.6 MB | 88 ms / 12.6 MB | 173 ms / 18.3 MB |
| 100 | **173 ms / 13.6 MB** | 342 ms / 14.0 MB | 894 ms / 26.8 MB |
| 1,000 | **269 ms / 95.6 MB** | 564 ms / 38.1 MB | 5801 ms / 97.5 MB |
| OTLP | **161 ms / 13.7 MB** | 268 ms / 13.9 MB | 431 ms / 21.9 MB |
| googleapis | 170 ms / **15.1 MB** | 160 ms / 15.7 MB | 720 ms / 27.0 MB |
| Envoy core | 266 ms / 41.5 MB | **211 ms / 16.9 MB** | 1549 ms / 33.3 MB |
| Envoy discovery | **265 ms / 65.0 MB** | 326 ms / 18.4 MB | excluded (pre-existing flat-namespace collision) |

Excluding protoc descriptor parsing (measured per cell with
`protoc --descriptor_set_out`, 46–103 ms, common to the pbrs/prost
paths since both shell the same protoc 36.2 over identical inputs, so
deltas are emission deltas): pbrs emission beats prost emission on
100 (~120 vs ~289 ms), 1,000 (~186 vs ~481), OTLP (~81 vs ~188) and
Envoy discovery (~162 vs ~223); small is tied (~47 vs ~42); pbrs
trails googleapis by ~10 ms (inside host noise: ranges overlap) and
Envoy core by ~55 ms.

## Verdicts

- Accept (1), byte-identical and deterministic: **met**. Matrix,
  regen drift hash, harness unchanged checks, and unit tests all agree.
- Accept (2), time and RSS beat prost-build and `protoc --rust_out`:
  **substantially advanced, not literally met on every cell**. Time:
  pbrs beats prost on 4/7 SB-09 cells (up to 2.1×), ties small, trails
  googleapis within noise and Envoy core by ~55 ms; beats `protoc
  --rust_out` on all 6 measured cells (2–21×). RSS: beats/ties prost
  on 4/7, beats v4 on 5/6. The remaining gaps are structural and
  outside this card's write scope: pbrs emits 30–80× more Rust per
  cell (e.g. 34.8 MB vs 0.4 MB on 1,000; emitters live in
  `messages.rs`/`parse.rs`/`encode.rs`/`json.rs`/`text.rs`) and its
  `DescriptorPool` model (`dynamic.rs`) dominates the RSS floor. The
  generator code this card owns is itself 2.7–3.5× faster with
  flat-or-lower RSS.

## GN-02/GN-03 lean-profile rerun

After adding the accessor-only generation profile, GN-04 B1/B2 was rerun with:

```sh
SB09_PBRS_EMIT_REFLECTION=0 SB09_PBRS_EMIT_JSON=0 SB09_PBRS_EMIT_TEXT=0 \
SB09_PBRS_RUNTIME_PROFILE=minimal \
  CARGO_BUILD_JOBS=3 ./scripts/codegen-bench.sh --case all \
  --generators pbrs,prost,v4 --stub-generators pbrs-native --repeats 5 \
  --jobs 3 --generation-only \
  --out target/codegen-bench/gn04-lean-genonly-20260929T020504Z
```

Medians of 5, generation time / peak RSS:

| case | pbrs accessor-only | prost-build | protoc `--rust_out` |
|---|---:|---:|---:|
| small | **90 ms / 12 MB** | 160 ms / 12 MB | 162 ms / 17 MB |
| 100 | 197 ms / 13 MB | **169 ms / 13 MB** | 549 ms / 25 MB |
| 1,000 | **328 ms / 45 MB** | 333 ms / 37 MB | 4.26 s / 87 MB |
| OTLP | 157 ms / 13 MB | **153 ms / 13 MB** | 324 ms / 21 MB |
| googleapis | 202 ms / 14 MB | **181 ms / 15 MB** | 380 ms / 26 MB |
| Envoy core | **252 ms / 28 MB** | 261 ms / 16 MB | 847 ms / 31 MB |
| Envoy discovery | 484 ms / 39 MB | **281 ms / 18 MB** | excluded |

Verdict: shrinking optional reflection/format output moves pbrs much closer to
prost and keeps it faster than v4 on all measured v4 cells. pbrs now narrowly
beats prost on the 1,000-message and Envoy-core generation-time cells, but still
loses to prost on 100, OTLP, googleapis, and Envoy discovery. GN-04 remains
partial rather than complete.

## Gates

- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --check`: passed.
- `cargo test --lib --all-features`: 83 passed.
- `cargo test --all-features --test plugin`: 41 passed, 2 ignored
  (pinned-libprotoc 36.1 only).
- `cargo test --all-features --test pbrs_build`: 30 passed.
- `cargo test --all-features --test codegen_compat`: 10 passed.
- `cargo test --all-features --test onboarding`: 14 passed, 1 ignored.
- `python3 -B -m unittest discover -s bench/codegen -p 'test_run.py'`:
  62 OK, 1 skipped.
- SB-09 harness `--case all --generators pbrs,prost,v4
  --generation-only`: passed (110 cells; v4 envoy-discovery excluded
  by the harness as before).

## Notes

- The first full harness run was invalidated by the run itself
  (`source changed during measurement`) after a mid-run stash cycle
  for the regen A/B; it was re-run clean with a source watermark held.
- During verification an unrelated in-progress change
  (`pbrs-grpc/src/tcp.rs`, CL-06 cold-start) and a foreign
  `mingley/tc-11-grpc-web` autostash appeared in this worktree; the
  file change was backed up to `/tmp/gn04/foreign-tcprs.diff` and
  reverted here, the foreign stash left untouched. Neither is part of
  this branch.
