# PK-09: Bytes-owning parse + shared large-field backing — evidence

**Status: diagnostic results, not claim-grade.** Same shared macOS host as
SB-13 (Apple M4 Pro, 14 CPUs). Base SHA `aab25667` (SB-13 done), head SHA
`80502a27`. This card implements zero-copy plan phases 1a + 1b, then
**reverts 1a**: the reserve-once wins 5x in isolation but reproducibly costs
-18% on loopback 8 MiB RPCs. What ships is 1b. Raw captures:
[pk09-zero-copy/](pk09-zero-copy/) (`devloop-base.json`,
`devloop-head.json`, `compare.txt`, `rpc-sweep.json`).

## Hypothesis

Large `bytes` fields cost one full-message copy on every inbound parse
(`Wire::ensure` → `Arc::from`, SB-13 baseline). Parsing straight off the
gRPC frame's `Bytes` with per-field windowing removes that copy; a 4 KiB
sharing threshold keeps small fields from pinning large frames. Target
categories: A4 (large-payload decode), A14, C4 (fewer copies), D3.

## What shipped (1b)

- `Wire` is a private-Arc / shared-`Bytes` enum (variant B; variant A,
  Bytes-only, cost +1 promo-box alloc on every windowed copying parse and
  was replaced after measurement).
- `Parse::parse_bytes(Bytes)` + `ClearAndParse::merge_from_bytes_shared`
  (default: copy; generated code pre-fills the wire slot, zero copies).
- `LazyBytes::from_parse_span/from_wire_span` with
  `SHARE_THRESHOLD = 4096`, applied **only to shared parents** (private
  parses window exactly as before — verified byte-identical alloc counts).
- `ProtoBytes` re-backed by `Bytes` (`From<Vec>`/`From<Bytes>` share;
  `&[u8]` accessors and `Debug` unchanged), `IntoProxied` for `Bytes` on
  singular setters and repeated push, `as_shared` getters.
- `decode_frame` (native) and the tonic decoder parse shared
  (tonic: 2 copies → 1; native whole frames: 1 → 0).
- Codegen templates + 13 checked-in generated files use the new span
  helpers (mechanical substitution; full `regen.sh` diverges for unrelated
  pre-existing reasons — emitter output spot-checked byte-consistent on
  the changed lines).

## What was reverted (1a) and why

Reserve-once in `FrameReader::push` (reserve the whole spanning frame when
its header lands): isolated BytesMut microbench 5x faster (1.5–4 ms vs
9–15 ms per 20× 8 MiB), -6.5% cycles/RPC, 100x fewer page reclaims,
-40 MB RSS — but alternating A/B at 8 MiB unary (base/1a/1b × 3):

| build | 8 MiB qps (3 runs) |
|---|---|
| base | 116.5, 113.3, 113.1 |
| 1a-only | 91.0, 93.4, 94.0 |
| 1b-alone | 112.7, 112.9, 108.9 |

`sample` profiles both sides: 1a does ~5x less `memmove` yet waits ~15%
longer in `recvfrom` — an emergent loopback pipeline/batch interaction,
not CPU. Per ship-only-if-faster-or-better, 1a is out (kept: the
reassembly + hostile-header tests). Phase 3 (segmented receive) re-asks
framing with fresh data. Bisect detail: 4 MiB base 224 → 1a 201 → 1b 225;
1 MiB base 985 → 1a 999 → 1b 1091.

## Dev-loop evidence (before / after)

Accept #1 — one fewer inbound copy for large bytes fields (SB-13
counters; new `parse_shared` cells, zero-copy path):

| cell (8 MiB unless noted) | ns/op | allocs/op | wire B/op |
|---|---|---|---|
| parse (base, 2000×3) | 102974 | 1.0 | 8388635 |
| parse (head, 2000×3) | 104932 | 1.0 | 8388635 (identical counts) |
| **parse_shared (head)** | **59–93** | **0.0** | **0** |
| parse mixed (base) | 515 | 3.0 | 32796 |
| **parse_shared mixed (head)** | **130–296** | **2.0** | **0** |

Small-cell wall guard (direct base-vs-head A/B, same host state,
2000 iters; the full-suite run showed transient host slowness during
its pbrs block, contradicted by same-binary repeats):

| cell | base ns/op | head ns/op |
|---|---|---|
| fresh_encode | 306 | 303 (−1%) |
| owned_decode | 364 | 366 (+0.5%) |
| parse_touch | 514 | 480 (−7%) |
| cached_encode | 88 | 78 (−11%) |
| blob_parse_mixed | 655 | 671 (+2.4%) |
| blob_encode_64kib | 1372 | 1399 (+2%) |

RPC loopback (native, closed loop, 10 s runs, head `80502a27`):

| cell | base QPS | head QPS | Δ |
|---|---|---|---|
| unary 1 MiB | 950 | 1095 | +15% |
| unary 4 MiB | 208 | 230 | +11% |
| unary 8 MiB | 114 | 120 | +5% |
| client_stream 8×1 MiB | 218 RPC/s | 233 RPC/s | +7% |

Full `devloop compare` (2000 iters × 3): **zero wall regressions**;
alloc-count medians identical except the understood micro-deltas below;
5 new `parse_shared` cells have no baseline by construction.

## Remaining compare deltas (all understood)

- `owned_decode` / `parse_touch` alloc_bytes +48 B (+4–5%): three Boxed
  wire-holder allocs grew 64→80, 88→104 ×2 with the 40 B `Wire`
  (histogram-verified; fundamental to shared-capable storage — a
  `Bytes`-capable window cannot fit the old 24 B).
- `rpc.pbrs.unary_compressed` +2 allocs, `server_stream_compressed` +57
  allocs, both +0.01% bytes, wall −1.2%/−0.7%: threshold small-copies on
  the gzip shared path (each avoids pinning a ~90 KiB decompressed
  buffer).

## Threshold sweep (T_share)

`parse_shared_mixed` (8× 4 KiB fields) + `parse_shared_1mib` at
512 / 4096 / 65536: 512 and 4096 identical (fields windowed; 296–445 ns
mixed noise); 65536 copies the 4 KiB fields (10 allocs, +32 KiB,
1715 ns, 6x slower). 4 KiB stands (page-granular sharing).

## Profile evidence

`sample` server/client pairs at 8 MiB unary (base vs 1a) in raw dir.
Codec profiles from SB-13 still describe the copying path; the shared
path is too fast to profile meaningfully at 65 ns/op (tag walk only —
field bytes are never touched until read).

## Correctness checks run

- `core-lib`: `cargo test -p pbrs --lib` — 40 pass.
- `parser`: `--test typed --test depth --test fuzz_parse` — pass.
- `native-rpc`: `pbrs-grpc --test rpc --test gaps` — 6 + 11 pass;
  `--lib` — 388 pass.
- `conformance`: `./scripts/conformance.sh` — 909/909 pass.
- `protobuf-tonic` suite — pass. Clippy clean (`pbrs --all-targets`,
  `pbrs-grpc --lib`, devloop release).
- Accept #2 test `small_bytes_field_does_not_pin_shared_frame`
  (`tests/runtime.rs`): 100 B field storage lies outside the 1 MiB
  frame, 1 MiB field inside (pointer-exact).
- `parse_bytes_matches_parse` (0/7/1K/1M equivalence),
  `bytes_setter_shares_without_copy` (pointer-identical sharing).

## Rollback criterion

Any wall regression in `devloop compare` or LP RPC sweep on the
claim-grade Linux hosts reverts the 1b commits as a group (they stack
cleanly: `git log --oneline aab25667..80502a27`).

## Claim label

Diagnostic (shared macOS host). No public performance claim.

## Caveats / future work

- `LazyStr` policy unchanged per plan: medium strings can still window
  shared frames (strings pin; bytes don't). String threshold is PK-10+.
- Nested messages copy their span on merge (same as base); only
  top-level bytes/map-bytes fields share the grandparent frame.
- Gzip path pays one 24 B `Bytes` promotion box per message (first
  window of a `Vec`-backed buffer); inherent to `bytes` 1.x.
- Full `regen-generated.sh` output diverges from `src/generated` for
  pre-existing reasons (header/attribute drift) — a separate cleanup.
