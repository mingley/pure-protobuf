# PK-15 string-threshold decision (2026-09-29)

## Decision

In `LazyStr::from_parse_span` (`src/lazy.rs`), a field copies exact
storage without touching the parent when near-whole (unchanged) **or**
when sparse (under a quarter of the frame) in a frame over 1024 bytes
(new). At `len <= 256` the copy lands in exact `ProtoString` heap
storage; above 256 it lands in a payload-only `Wire`. Dense fields and
frames ≤1024 bytes keep historical parent windowing, representation
identical to before.

Three-way comparison (matched parse+touch+encode, byte-identical
roundtrips, exact `copy_counts` wire signals): owned dominates Arc at
≤256 (same 1 alloc, 16 fewer bytes, no refcount); both dominate
windowing for sparse fields (a 24-byte field in a 2050-byte frame pinned
~85x its size before, exact storage now).

## Accept status

- **Accept (2) met:** four in-module tests prove sparse 24/80/200-byte
  fields in large frames land `Owned` with no parent `Arc` (`slot` stays
  `None`, so pinning is impossible), sparse 300-byte fields land in a
  payload-only `Wire`, and dense medium/large cases keep windowing.
- **Accept (1) partially met, remainder out of scope:** decode holds
  (`name_80` ties prost at +0.8% Ir with equal allocs/bytes; broad A3
  beats prost and v4). `name_80` encode loses 7.7% and `small_id` encode
  9.8%. Exact callgrind attribution of the 16.6 Ir encode gap: ~10 Ir is
  `copy-counts` atomics in `src/wire.rs::put_slice` (bench-only
  instrumentation prost doesn't pay), ~4-5 Ir is `encode_len_header`
  dispatch in `wire.rs` plus generated code, ~2 Ir harness. The in-scope
  `LazyStr::as_bytes` dispatch is fully inlined; `small_id` encode
  touches no string code at all.

Dev-loop differential (Linux callgrind, 500 iters ×3): all 18 cells
within ±0.35% noise, allocs/bytes exactly identical; formal `devloop
compare` exits 0. No primary-cell regression from this change; the
encode losses above are pre-existing gaps versus prost, not regressions.

## Remainder

The encode-side gap lives in `src/wire.rs` (hotspot-shared with PK-06)
and generated-code dispatch. It splits to card PK-22, which runs after
PK-06's wire rewrite.
