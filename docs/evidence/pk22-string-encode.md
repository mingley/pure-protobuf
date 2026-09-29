# PK-22 small-string encode gap (2026-09-29)

Card: close the `name_80` / `small_id` encode gap to prost. Base
`98739d11`. Branch `mingley/pk22-string-encode`. Write scope:
`src/wire.rs` + this file.

Method: `scripts/devloop-linux.sh` (pinned Linux container, callgrind
differential `2N-N`, 500 iters x3) on the small-encode cells, plus
targeted callgrind attribution with pinned `--prepare-iters` and
`--warmup 0`, plus disassembly of the encode loop.

## 1. Fair gap (copy-counts off)

PK-15 attributed the 16.6 Ir `name_80` gap as ~10 Ir copy-counts
atomics in `wire.rs::put_slice` (bench-only instrumentation prost
doesn't pay), ~4-5 Ir `encode_len_header` dispatch, ~2 Ir harness.
The fair measurement rebuilds devloop without the `copy-counts`
feature (temporary `bench/devloop/Cargo.toml` edit, reverted; the
`copy_counts()` API still compiles, returning zeros).

| cell | counts-on (std) | fair (counts off) |
|---|---|---|
| pbrs `small_name80_encode` | 219.75 | 202.20 |
| prost `small_name80_encode` | 204.13 | 204.02 |
| gap | +15.62 (+7.7%) | **-1.81 (-0.9%, pbrs wins)** |
| pbrs `small_id_encode` | 123.36 | 121.94 |
| prost `small_id_encode` | 112.09 | 112.20 |
| gap | +11.27 (+10.1%) | **+9.75 (+8.7%)** |

(JSON: `/tmp/pk22-baseline.json`, `/tmp/pk22-fair-baseline.json`.)

Per the card premise: the `name_80` fair gap is already ~0 (a win),
so it gets only tuning that still wins. The real `wire.rs` work is
the `small_id` fair gap of +9.75 Ir. (`small_id` encodes an int64, so
it never touches string code; on this aarch64 host the counts-on
delta is ~17 Ir for `name_80` (one `put_slice` emission) and ~1.4 Ir
for `small_id` (no `put_slice` on path), vs PK-15's x86 ~10 Ir.)

## 2. Attribution of the fair `small_id` gap

Differential callgrind (20k vs 10k iters, pinned prepare) of the
pre-change fair build, per op:

- pbrs 122.0 = `codec_work` 85 (harness + cached-size-hit
  `compute_size` + `check_size` + call setup) + `write_to` 37
  (out-of-line; the `pbrs_cases::Id::write_to<Vec<u8>>` symbol is
  ICF-folded into `UInt64Value::write_to<Vec<u8>>`).
- prost 112.0 = `codec_work` 94 + 2x `encode_varint` 18.

Disassembly showed the root cause in `src/wire.rs`: `encode_varint`'s
multi-byte loop (buffer into `[u8; 10]`, then one `put_slice`) fully
unrolls at each call site into ~350 bytes of branch chains (two
copies in `Id::write_to`: tag + value). That bloat keeps small
`write_to` bodies out-of-line, so every small encode pays an
out-of-line frame, call, and epilogue (~11 Ir).

## 3. Tuning (`src/wire.rs` only)

Final diff (24 insertions, 2 deletions):

1. `encode_varint`: one- and two-byte fast paths stay
   `#[inline(always)]` (`push` for 1 byte; a single `put_slice` of a
   2-byte array for values `< 0x4000`, fixed size, nothing to
   unroll). Lengths 3+ move to `#[cold] #[inline(never)]
   encode_varint_slow`. Byte-identical output, including the single
   `put_slice` emission per value (copy-counts `emit_*` semantics
   unchanged: `small_name80_encode` still reports `emit_calls=1`,
   `emit_bytes=80`).
2. `UnknownFields::encode` gains `#[inline(always)]`: the empty check
   folds into the caller and the cold loop body stays out-of-line,
   saving a tail call on every encode.

No public API change, no new dependencies.

Design note: the first iteration outlined all multi-byte varints
(1-byte inline only). That closed more of `small_id` (fair +3.4%)
but regressed TAT `cached_encode` +19.8%: a TAT encode emits ~16
two-byte varints (tags for fields >= 16), and each outlined call
costs ~10 Ir. The two-byte inline path recovers TAT fully
(`cached_encode` now -1.6% vs before) at a cost of one more cold pad
in small `write_to` bodies (~+2.8 Ir frame vs the 1-byte-only
variant). An `#[inline]` hint on `FieldList::iter` was tried and
reverted (no win).

Fair result after tuning (same method; `/tmp/pk22-fair-v3.json`):

| cell | pbrs before | pbrs after | prost | gap after |
|---|---|---|---|---|
| `small_name80_encode` | 202.20 | 193.20 (-4.4%) | 202.93 | **-9.73 (-4.8%, pbrs wins)** |
| `small_id_encode` | 121.94 | 115.94 (-4.9%) | 110.67 | **+5.26 (+4.8%)** |

Both targeted cells beat the 2% dev-loop improvement bar. `name_80`
now beats prost decisively on fair evidence. `small_id` is nearly
halved but still +5.3 Ir short of a tie.

## 4. Remainder: bounded blocker for the codegen owner

Post-tuning attribution: pbrs ~115 = `codec_work` ~84 + `write_to`
~31 (still out-of-line) vs prost ~111 = ~93 + ~18. The remaining ~5
Ir is generated-code dispatch outside `src/wire.rs` (`wire.rs` has no
further lever: the hot path is now two `put_u8` reserve-checks, two
stores, branches, and the out-of-line frame):

- `src/codegen/parse.rs:229-231` emits `pub fn write_to(&self, out:
  &mut impl pbrs::rt::WireOut)` with **no inline attribute**, so
  single-field `write_to` bodies (e.g. `cases.Id`) stay out-of-line:
  ~9 Ir frame + call + epilogue per encode. Emitting
  `#[inline(always)]` (or `#[inline]`) on small `write_to` bodies
  would remove it; estimated effect is a ~4-5 Ir net win (tie, then
  win, vs prost at ~111 Ir/op).
- `src/codegen/encode.rs:380-388` (singular implicit-presence scalar)
  emits `if {pred} { encode_tag(...); encode_varint(...); }` with
  separate field loads and separate reserve checks per byte; a fused
  tag+value small-varint helper (single load, single reserve check)
  would shave a further ~3-4 Ir.
- `src/gen_support.rs:848-856`: `Serialize::encode` runs
  `check_size(self.compute_size())` before `write_to` (~5 Ir floor on
  the cached-size-hit path: load + compare + branch + `check_size`
  compare + branch). Not removable from `wire.rs`.

Suggested codegen-owner follow-up: inline attribute on `write_to`
(first, smallest, measured ~9 Ir gross / ~4-5 Ir net), then tag+value
store fusion.

## 5. Standard (counts-on) dev-loop evidence and regression check

Full 88-cell matrix before/after (counts-on; `/tmp/pk22-full-before.json`,
`/tmp/pk22-full-v3.json`), formal `devloop compare` with the picker budget:

| cell | before | after | delta | allocs |
|---|---|---|---|---|
| pbrs `small_name80_encode` | 219.95 | 214.88 | -2.30% | 0 -> 0 |
| pbrs `small_id_encode` | 123.17 | 117.96 | -4.23% | 0 -> 0 |
| pbrs `small_empty_encode` | 92.0 | 84.6 | -7.99% | 0 -> 0 |
| pbrs `fresh_encode` | 5954.6 | 5916.4 | -0.64% | same |
| pbrs `cached_encode` | 1664.3 | 1638.2 | -1.57% | same |
| pbrs `blob_encode_shared_mixed` | 6643.1 | 6408.1 | -3.54% | same |
| pbrs `small_empty_decode` | 93.14 | 95.07 | **+2.08% (flagged)** | 0 -> 0 |

Compare prints "improved" for the five encode cells above and flags
only `small_empty_decode` (+2.08%, limit 1%; exit 1). All other 83
cells pass; allocs/bytes identical everywhere.

The `small_empty_decode` flag is a harness systematic, not a product
regression:

- The diff provably does not touch the decode path: `decode_tag`,
  `decode_varint`, `varint_len`, `tag_len`, `encoded_len`,
  `FieldList::iter`, `merge_inner`, and gencode `compute_size` are
  character-identical before/after (the only decode-adjacent
  experiment, an `#[inline]` hint on `iter`, was reverted).
- Dispatch-free proof: a scratch crate (`target/mini`, since
  removed) looping `Empty::parse(&[])` + `serialized_len()` with no
  harness dispatch measures 16.0000 Ir/op before vs 15.9999 Ir/op
  after (100k/200k differential) — delta -0.0001 Ir/op, i.e. zero.
- Mechanism: every devloop op pays the shared `codec_work` string
  dispatch, whose compiled shape changes whenever any arm's callee
  changes. Prost control cells, with zero prost code change, shifted
  -1.00 Ir (-0.89%) and -1.21 Ir (-0.59%) between the same two runs,
  while same-binary repeat spread is only +/-0.25 Ir. So cross-binary
  tiny-cell deltas carry a ~+/-2 Ir deterministic dispatch
  systematic, and the 1% gate on sub-200-Ir cells sits below it. The
  +1.93 Ir on `small_empty_decode` is that systematic.

No true primary-cell regression: every encode cell improves or holds,
decode/compute codegen is bit-identical (16.0000 vs 15.9999 Ir/op),
and allocs/bytes are unchanged across the matrix.

## 6. Checks

- `cargo test -p pbrs --lib`: ok (74 passed).
- `cargo test -p pbrs --test typed --test depth --test fuzz_parse`: ok.
- `./scripts/conformance.sh`: PASS (909 successes, 0 failures).
- devloop-compare: see section 5.

## 7. Limitations

- Fair (counts-off) builds required a temporary, reverted
  `bench/devloop/Cargo.toml` edit; the committed harness still builds
  with `copy-counts`, so standard evidence keeps the bench-only
  atomic cost on `put_slice` paths (`name_80`).
- Counts are callgrind Ir on aarch64 Linux (Docker on Apple Silicon);
  absolute values are host-specific but pbrs-vs-prost deltas are
  apples-to-apples within each run.
- The `small_id` tie needs the codegen-owner change in section 4;
  this card's `src/wire.rs` scope is exhausted (both tunings kept are
  measured wins; see section 5).
- Cross-run systematics: recompiling the shared `codec_work`
  dispatch perturbs every tiny cell by ~+/-2 Ir between binaries
  (prost control cells, with zero prost code change, shifted -1.0%
  and -0.6% between the before/after runs; same-binary repeat spread
  is only +/-0.25 Ir). The 1% gate on sub-200-Ir cells sits below
  this systematic floor; see section 5.
