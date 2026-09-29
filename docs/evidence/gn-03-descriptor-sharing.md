# GN-03 remainder: shared, lazy descriptor loading

This is dev-loop, contended-host evidence for the GN-03 remainder
("Make descriptor embedding optional and shared"). Base: `3a0cc56d`
(GN-03 option slice landed; see
[gn-03-reflection-options.md](gn-03-reflection-options.md)).

## Hypothesis

Default multi-file output must keep embedding descriptor bytes per
standalone generated file (existing `include!(OUT_DIR/service.rs)` users
stay source-compatible), while `mod.rs`-style consumers that include many
files should pay the descriptor parse once per process, and only when
reflection is actually used.

## Design

Sharing is implemented in the runtime, not the emitter, for two reasons:

- Any single generated file must compile standalone, so the bytes cannot
  move to a shared sibling module without breaking `include!` users.
- `tests/dynamic.rs` requires `DescriptorPool::from_file_descriptor_set`
  to report malformed input eagerly (visibility and Edition 2024
  reference errors), which rules out deferring the parse past that call.

Changes (`src/dynamic.rs` only, plus docs):

- `DescriptorPool` now holds `inner: Arc<PoolInner>`; all resolved maps
  moved into the private `PoolInner`. Cloning a pool shares everything.
- `from_file_descriptor_set` consults a process-wide, lock-free cache
  (64 `OnceLock` slots sharded by a hash of the input bytes) before
  parsing. A hit returns the shared pool with no parse; a miss parses
  eagerly exactly as before (all error behavior preserved) and stores
  the result. Shard collisions fall back to the fresh parse, so sharing
  is best-effort and never incorrect.
- `register_message` / `register_enum` copy on write via `Arc::make_mut`,
  so a mutated pool never disturbs the cached entry or its siblings.
- No public API change; `Debug` output keeps its exact pre-change shape
  via a manual impl.
- `src/codegen/reflection.rs` documents this contract on `fds_hex_block`
  and gains a rendering roundtrip test. Emitted bytes are untouched.

Laziness is the combination of two existing-plus-new behaviors: each
generated file's `generated_pool()` already defers to first reflection
use via `OnceLock`, and the cache makes the second and later files'
loads free. Net effect: descriptors load once per process per unique
descriptor set, on first reflection use.

## Method and results

New unit tests (in-file, since the task allowlist excludes `tests/`):

- `dynamic::tests::identical_bytes_share_one_parsed_pool`: two pools
  from identical bytes share message/enum/service/file `Arc`s (`ptr_eq`
  proves no second parse).
- `dynamic::tests::distinct_bytes_do_not_share_pools`.
- `dynamic::tests::shared_pool_mutation_is_copy_on_write`: mutation is
  invisible to siblings and to the cached entry.
- `dynamic::tests::malformed_bytes_still_fail_eagerly`.
- `dynamic::tests::debug_shape_is_stable`.
- `codegen::reflection::tests::fds_hex_block_roundtrips_bytes`.

Byte-identity proofs (dev-loop host, macOS arm64, protoc 36.2):

| Check | Result |
|---|---|
| SB-09 small/pbrs generation-only tree, branch vs base `3a0cc56d` | `diff -r` identical (3 files; both parts still embed `FILE_DESCRIPTOR_SET`) |
| `scripts/regen-generated.sh` output, branch vs base | `diff -r` identical |
| Regen output vs checked-in `src/generated` | still drifts (pre-existing, GN-02-owned; regenerated files not committed) |

Descriptor-load microbench (`/tmp` scratch crates, release, 200 loads of
a 17,194-byte / 154-message SB-09 `100`-case descriptor set; small
1,223-byte set in parentheses):

| Load | Branch | Base |
|---|---|---|
| Repeat (identical bytes) | 30.5 µs (2.1 µs) | 960.5 µs (56.9 µs) |
| Cold (200 distinct valid blobs) | 966.1 µs | 929.6 µs |

Repeat loads are ~31x faster with byte-identical resolution results
(30,800 names on both sides); cold parses match within dev-loop noise.

Accept verdicts:

1. Reflection-free consumers: PASS. `protoc
   --pbrs_opt=stubs=none,emit_reflection=false,emit_json=false,emit_text=false`
   output contains no `pbrs::json` / `pbrs::text` / `DescriptorPool` /
   `FILE_DESCRIPTOR_SET` references, and a scratch consumer roundtrips
   binary messages against `pbrs = { default-features = false }`
   (`lean-consumer-ok`). `cargo check --no-default-features --lib` and
   `cargo test --no-default-features --lib` (66 passed) are green; the
   `conformance` bin failure under `--no-default-features` reproduces on
   base (pre-existing, feature-gated bin). Reflection suites are green:
   `dynamic` (27), `table_parse`, `depth`, `runtime`, `typed`, lib (86),
   `pbrs_build` (30), `codegen_compat` (10), `plugin` (41).
2. SB-09 binary size / check time: NO CHANGE BY CONSTRUCTION. Generated
   source is byte-identical to base (proven above), so SB-09 check-time
   and binary-size cells cannot move beyond build noise; the runtime
   delta is ~120 lines of cache code with no new dependencies. Full
   SB-09 cells were not re-run for that reason. The measured win is
   descriptor-load cost and memory (parse once, share `Arc`s).

Also green: `cargo clippy --all-targets --all-features -- -D warnings`
(a `std::sync::Mutex` design was rejected for the repo's
`disallowed_types` lint in favor of the lock-free slot cache) and
`cargo fmt --check`.

## Limits

- Embedded source bytes are unchanged: per-file `.rs` size, `cargo
  check` time, and binary size do not fall. A future opt-in
  `mod.rs`-shared emission could shrink those at the cost of
  standalone-`include!` compatibility; that needs an emitter change
  outside this slice's allowlist.
- The cache holds at most 64 pools (first stored per shard) for the
  life of the process. Shard collisions only cost sharing, never
  correctness.
- Eager validation is intentionally preserved, so the first load of a
  unique descriptor set still parses up front.
