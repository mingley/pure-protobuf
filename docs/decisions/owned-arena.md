# PK-12: Per-request arena allocation for owned messages

Date: 2026-09-29
Status: decided — **REJECTED** for owned messages (evidence below)
Base SHA: `3bcf0440`
Scope: opt-in bump arena prototype for owned messages parsed per RPC

This record **rejects** adopting a per-request bump arena for owned
messages. Rejection is an explicitly valid outcome of PK-12. The
prototype (`bench/devloop/cells/arena.rs`) is kept as a measured,
Miri-covered starting point in case a lifetime-parameterized message API
ever makes arenas cheap to adopt.

## Context

PK-01 (`docs/evidence/codec-profiles.md`) shows broad TAT owned decode
costs 7 allocs/op and 1072 bytes/op, with allocator/drop/memmove costs
dominating the profile more than varint validation; its follow-up #1
recommends reducing `Wire::ensure`/cold-box/map allocations before
further micro-optimizations. PK-12 tests the hypothesis that parsing
owned messages into a per-request bump arena (no individual frees; drop
resets the arena) beats the global allocator on allocation counts,
parse time, and teardown cost.

Dependency evidence bounds the prize: small `name_80` decode already
ties prost at 1 alloc / 80 bytes per op, and PK-09's `Wire` sharing
covers the large-payload axis (0 allocs on tag-walk-only decode). An
arena has to win on the mid-size alloc-count-heavy shapes to matter.

## What was built

`bench/devloop/cells/arena.rs` (new, self-contained, `std`-only) holds:

- `BumpArena`: chunked bump allocator. `alloc_copy` / `alloc_bytes` /
  `alloc_str` / `alloc_slice` hand out `Copy`-only payloads with no
  per-value frees; `reset(&mut self)` and drop release everything at
  once. The unsafe core is bump-pointer arithmetic plus one base-pointer
  capture per chunk, each with a `SAFETY` comment.
- `ArenaVec<'a, T: Copy>`: growable in-arena buffer (doubling, abandoned
  prefixes reclaimed at reset), so single-pass parsing needs no
  global-allocator scratch.
- `probe`: a deterministic TAT-like workload parsed two ways — global
  allocator (`OwnedMsg`: `String`/`Vec`/pairs) and arena (`ArenaMsg<'a>`:
  `&str`/`&[u8]`/`&[i32]`/`&[(i32,i32)]`/`&[&str]`) — with an identical
  touch walk on both sides, so checksums must agree.
- 10 unit tests: alignment, chunk growth, reset reuse, arena-vec order,
  owned-vs-arena equivalence on three specimens, error parity, many-small
  allocs, reset-then-reuse soundness.

Safety design (accept criterion 2, demonstrated though not adopted):
every handed-out reference borrows the arena (`&'a BumpArena ->
&'a mut T`), so the borrow checker forbids `reset(&mut self)` and
`drop(arena)` while any view is live — no dangling views by
construction. `Copy`-only payloads mean `reset`/drop runs no
destructors, so there is no drop glue to forget. The arena is `Send`
but `!Sync` (single-threaded use, movable between requests).

## Evidence

Method: standalone `/tmp` harness mirroring the devloop methodology
(counting `GlobalAlloc` armed only around the timed loop, warmup,
9 repeats, per-op medians, wall CVs), because wiring cells into
`bench/devloop/src/main.rs` is outside PK-12 write scope (see
Follow-ups). Strategies: `owned` (global allocator, parse + touch +
drop per op), `arena_fresh` (new arena per op), `arena_reuse`
(pre-grown arena, parse + touch + reset per op). Global allocators:
system, mimalloc 0.1.50, tikv-jemallocator 0.6.1 (all from the offline
crate cache; no repo dependency added).

Label everything below **dev-loop, contended host** (load ~21 on 14
CPUs during the final runs; absolute ns move between runs, ratios are
stable across 5 runs). Not claim-grade evidence.

Wire sizes: `tat_like` = 168 B, `packed_256` = 1123 B. Arena footprint
after one parse: `tat_like` 194 B used of 8192 reserved (1 chunk);
`packed_256` 2112 B used of 8192 reserved.

### Parse + touch + drop (wall ns/op medians; allocs/op and bytes/op exact)

| specimen / strategy | system | mimalloc | jemalloc |
|---|---|---|---|
| `tat_like` owned (9 a, 201 B) | 170.4 | 125.6 | 191.8 |
| `tat_like` arena_fresh (2 a, 8352 B) | 120.2 | 108.2 | 149.7 |
| `tat_like` arena_reuse (0 a, 0 B) | 92.1 | 92.5 | 126.0 |
| `packed_256` owned (2 a, 1104 B) | 324.1 | 275.8 | 416.1 |
| `packed_256` arena_fresh (2 a, 8352 B) | 325.0 | 303.9 | 428.2 |
| `packed_256` arena_reuse (0 a, 0 B) | 290.2 | 284.7 | 395.1 |

### Teardown per message and touch parity (wall ns/op medians)

| cell | system | mimalloc | jemalloc |
|---|---|---|---|
| `owned.teardown` | 63.4 | 14.3 | 118.9 |
| `arena_drop.teardown` | 25.2 | 15.6 | 61.8 |
| `owned.touch_only` | 22.2 | 21.9 | 29.5 |
| `arena.touch_only` | 21.5 | 22.0 | 32.4 |

Touch parity (equal checksums, equal touch time) confirms both paths do
the same work; deltas are allocation/teardown effects, not skipped work.

### Miri

`cargo +nightly miri test` with
`MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance'`
(miri 0.1.0, nightly 2026-08-24): 10/10 prototype tests green across 4
separate runs. Miri caught two real soundness bugs during development,
both fixed in the delivered file:

1. Chunk backing typed `[u8; 16]` carries alignment **1**, not 16 —
   real allocators over-align by luck, Miri did not. Backing is now
   `MaybeUninit<u128>` (size and alignment 16).
2. Repeated `as_mut_ptr()` reborrows of the same chunk backing, and
   capturing a base pointer before the typed `Box` move into the chunk
   list, pop outstanding pointers under Stacked Borrows. The base
   pointer is now captured exactly once per chunk, from the in-place
   box; all later access is raw arithmetic on the stored base.

## Decision: reject

The arena win is real but too narrow, and it requires the opposite of
the card's literal shape:

1. **Only steady-state reuse wins.** Fresh-arena-per-RPC (the card's
   "drop resets" shape without pooling) allocates 8352 B/op — 40x the
   owned path's 201 B on `tat_like` — and is time-parity or worse on
   copy-dominated `packed_256` (mimalloc: 303.9 vs 275.8 ns, a 10%
   loss). The win needs pre-grown pooled arenas plus reset, i.e. a
   per-connection/per-worker pool, not a per-RPC arena.
2. **Even reuse wins only where malloc dominates.** On alloc-heavy
   `tat_like`, reuse beats owned by 1.4–2.3x depending on allocator
   (system 170 → 92 ns, mimalloc 126 → 93 ns, jemalloc 192 → 126 ns).
   On copy-dominated `packed_256` (2 allocs either way), all strategies
   land within ~10%: memcpy, not malloc, is the bottleneck.
3. **mimalloc captures most of it for free.** Switching the global
   allocator cuts owned `tat_like` by ~30–45% with zero API churn
   (system 170 → mimalloc 126 ns), leaving the arena a further ~26%
   on its friendliest shape — and teardown reaches full parity under
   mimalloc (14.3 vs 15.6 ns).
4. **Adoption cost dwarfs the prize.** The owned API cannot use arena
   storage without lifetime-parameterizing every generated message
   type (`ArenaMsg<'a>` instead of owned `String`/`Vec`), redesigning
   `Clone`/mutation/`Drop` semantics, accepting the `Copy`-only
   restriction (no owned strings/vecs/`Wire` arcs inside), threading
   `&Arena` through parse and the gRPC stack, and setting a retention
   policy for pooled chunks. That redesign is not justified by ~26%
   on small alloc-heavy messages and ~0% on copy-dominated ones.

## What would change this decision

- A lifetime-parameterized message API adopted for other reasons
  (borrowed views, PK-13/PK-20): an arena is then the natural backing
  store, and this prototype plus its Miri coverage is the starting
  point. Re-run the cells against real generated types before adopting.
- A workload where per-RPC alloc counts are 10x today's TAT shape with
  the same small byte sizes (malloc-dominated, not memcpy-dominated).

## Follow-ups for the coordinator

1. **Harness wiring (after SB-08 lands).** `bench/devloop/cells/` does
   not exist on main; SB-08 creates it with its own registration.
   `arena.rs` is deliberately unregistered (editing `src/main.rs`,
   `build.rs`, or manifests is outside PK-12 scope). The intended thin
   wiring: `mod` declaration plus `codec.arena.*` registry entries for
   `owned_decode`, `arena_fresh_decode`, `arena_reuse_decode`, and the
   two teardown cells, calling the existing `probe` functions.
2. **`docs/unsafe-invariants.md` entry.** The prototype adds `unsafe`
   (bump-pointer arithmetic, base capture) with local `SAFETY`
   comments, but QG-01 pre-registration in that document is outside
   PK-12 write scope. If the prototype (or any descendant) is ever
   wired into a shipping path, register its invariants there first.
3. **Evaluate opt-in mimalloc.** The single cheapest measured win in
   this card is the global allocator itself. Suggest a follow-up card
   to evaluate (not necessarily adopt) an opt-in mimalloc feature on
   real codec cells, including RSS and tail-latency effects this
   prototype does not measure.
4. **Raw JSON artifacts.** Standalone run outputs live only in
   `/tmp/pk12meas/out/{system,mimalloc,jemalloc}.json` (throwaway);
   re-run after wiring to produce in-tree dev-loop artifacts.

## Commands run (all offline, no pushes)

```sh
git checkout -b mingley/pk12-arena 3bcf044029290d111b6562eb2e32203b99ef410c
rustc --edition 2021 --test bench/devloop/cells/arena.rs -o /tmp/pk12-arena-test && /tmp/pk12-arena-test        # 10/10
rustc --edition 2024 --test bench/devloop/cells/arena.rs -o /tmp/pk12-arena-test24 && /tmp/pk12-arena-test24    # 10/10
rustfmt --edition 2021 --check bench/devloop/cells/arena.rs
MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance' cargo +nightly miri test --offline --lib          # 10/10 x4
cargo clippy --offline --all-targets                                                                           # clean
CARGO_NET_OFFLINE=true cargo build --release --offline [--features mimalloc|jemalloc]
./target/release/pk12meas --repeats 9 --out out/<allocator>.json   # x3 allocators, medians tabulated above
```
