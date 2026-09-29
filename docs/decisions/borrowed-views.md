# PK-13: Borrowed-view model

Date: 2026-09-29
Status: draft for maintainer approval (gates PK-20)
Base SHA: `2598aa12`
Scope: public borrowed-view API for plugin-generated messages (pbrs owned path)

This is a DECISION record only. It authorizes no implementation; PK-20
implements it behind a codegen option. No new benchmarks were run for this
card (doc-only); all numbers below cite existing PK-09, SB-13, and PK-01
evidence.

## Context

Today `FooView<'msg>` is `&Owned` after copy
(`impl_typed_message!` in `src/gen_support.rs`; see `docs/upb.md` §Views):
it borrows an already-parsed owned message and offers zero-copy access only
in the trivial sense that reborrowing is free. Field-level laziness exists
inside owned messages (`LazyStr`/`LazyBytes` windowing a shared `Wire`,
`LazyMsg` validating eagerly and materializing `T` on first access), but
there is no first-class wire-backed view type: something that decodes
directly off caller-held bytes without building the owned struct.

Dependency evidence:

- PK-01 (`docs/evidence/codec-profiles.md`): broad TAT owned decode costs
  7 allocs/op and 1072 bytes/op; owned-decode allocations/copies dominate
  the broad decode cell. Small `name_80` decode ties prost at 1 alloc,
  80 bytes/op. Anything view-shaped must beat ~7 allocs / ~1 KiB on TAT to
  matter.
- PK-09 (`docs/evidence/pk09-zero-copy.md`): `Wire` is a private-Arc /
  shared-`Bytes` enum. `parse_bytes` on an 8 MiB blob costs 59–93 ns/op
  with 0 allocs and 0 wire bytes (tag walk only; field bytes untouched
  until read), versus the copying `parse` at ~104 µs, 1 alloc, 8.39 MB
  wire bytes/op. The 4 KiB `SHARE_THRESHOLD` keeps small bytes fields from
  pinning large frames (threshold sweep: 512 and 4096 identical; 65536
  copies 4 KiB fields, 6x slower). `Wire` grew 24 B → 40 B, adding 48 B
  (+4–5%) to `owned_decode`/`parse_touch` alloc bytes — the measured cost
  of shared-capable storage. Nested messages still copy their span on
  merge; only top-level bytes/map-bytes fields share the grandparent frame.
- SB-13 (`docs/evidence/large-payload-baseline.md`): copying `parse` does
  exactly 1 copy + 1 alloc at every size (64 KiB–8 MiB), ~2x faster than
  prost's 3-alloc eager decode; touch converges (memory-bound scan).
- buffa 0.9.1 (pinned comparator): `MessageView::decode_view(&'a [u8])`
  validates the whole message tree eagerly and borrows field slices from
  the input; `LazyMessageView::decode_lazy` (opt-in `lazy_views` codegen
  option) does one non-recursive scan and records nested messages as
  undecoded byte ranges, surfacing errors on access. `HasMessageView`
  additionally offers `decode_view_handle(Bytes)` — an owning handle for
  leased buffers. Deferred validation is visible in the type: generic code
  over `MessageView` never silently inherits it.

## Decision

Offer **both** lifetime-bound views over `&[u8]` **and** Bytes-leased
owning views, with **eager whole-tree validation by default** and an
opt-in lazy family. In short: mirror the buffa shape (eager view + lazy
view + view handle), adapted to pbrs codegen and the PK-09 `Bytes`
backing.

### D1. Two view roots: borrowed slice views and leased handle views

- `FooWireView<'a>` (working name; see open question O1): borrows the
  caller's contiguous bytes, `decode_view(buf: &'a [u8])`. Zero allocation
  on the decode path itself; field accessors return borrowed slices.
  Lifetime `'a` ties the view to the input buffer.
- `FooViewHandle` (working name): owns a `bytes::Bytes` (refcount clone,
  never a copy) and derefs/offers `.as_view()` to the borrowed view. This
  is the gRPC-frame type: `decode_frame` already holds a `Bytes`, and
  PK-09 showed the shared path is only reachable when the backing buffer
  is owned somewhere. Without a handle type, every RPC handler would have
  to keep the frame alive manually next to the view — error-prone and
  unidiomatic.

Rationale for both rather than one: the `&[u8]` form serves zero-dependency
callers and stack/embedded buffers; the handle form serves the transport,
where the buffer is already a `Bytes` and the natural unit of ownership.
PK-09's variant-B `Wire` proves the runtime can already represent both
backings behind one window type; views expose that choice at the API level
instead of hiding it inside owned messages.

### D2. Eager validation by default; lazy nested views opt-in

- The default view family validates the **whole message tree eagerly** at
  decode: malformed nested bytes fail `decode_view`, matching buffa's
  `MessageView` contract ("decode succeeded ⇒ the whole tree was
  validated") and the existing `LazyMsg` precedent (validate eagerly,
  materialize lazily). This is what scoreboard A4 measures (see §Comparison
  rules).
- Nested-message accessors on the eager view return nested views
  (`bar_view()` → `BarWireView<'a>`), not owned messages and not raw
  bytes: one eager validation pass, then free re-viewing of subspans.
- A second, opt-in lazy family (`FooLazyView<'a>`, behind a codegen option
  mirroring buffa's `lazy_views`) performs a single non-recursive scan and
  defers nested-message validation to access time, with fallible accessors.
  Deferred validation must be visible in the type/trait bound, never silent.

Rationale: eager-by-default keeps the "parsed ⇒ valid" invariant the
owned API and conformance suite assume, and keeps A4 comparisons honest.
Lazy exists for large sub-message trees where the caller touches a few
fields — the same case `LazyMsg` already serves inside owned messages —
but partial validation must never be compared against full parse (the A4
category definition already forbids this).

### D3. Accessor parity with the owned API

- Read accessors mirror owned getter names and shapes exactly where a
  borrowed return type exists: `id()` → scalar by value, `name()` →
  `&ProtoStr`, `data()` → `&[u8]`, `child()` → nested view,
  `items()` → borrowed repeated/map view, `foo_opt()` → `Option<...>`.
  Same field order, same presence semantics (proto3 implicit, optional,
  oneof, proto2 required/presence), same default values for absent fields.
- No setters, no `*_mut`, no builders on views. Mutation flows through
  `to_owned() -> Foo` (full copy, documented cost) followed by owned
  mutation. This keeps views `Copy`-friendly location-transparent reads
  and avoids a second mutation semantic.
- `to_owned()` cost is the owned-conversion cost OP-06 requires quantified:
  bounded by one owned `parse` of the same bytes (7 allocs / ~1 KiB on TAT
  per PK-01; 1 copy + 1 alloc on blob shapes per SB-13). PK-20 must measure
  and document the actual per-corpus numbers; the decision only fixes the
  ceiling expectation (no worse than parsing the same input owned).
- Unknown fields: views preserve and expose unknown-field bytes for
  round-trip (`to_owned()` must reproduce them byte-identically), but do
  not offer a mutable unknown-field API. Conformance-relevant: unknown
  round-trip is already a pbrs guarantee (`docs/upb.md`).
- The existing `Proxied::View` (`&Owned` wrapper) types are untouched.
  Wire-backed views are a separate family with their own trait
  (sketch below), so no existing API changes meaning and no generic code
  silently changes behavior.

### D4. Retained-memory policy

Views retain the **entire backing buffer** until dropped. No per-field
threshold copy-out at the view layer:

- Rationale: the whole point of a view is zero-copy access. PK-09's
  4 KiB threshold exists for *owned* messages, where a small field would
  otherwise pin a large frame for the message's whole life. A view's life
  is explicitly bounded by its borrow (`'a`) or its handle, so the caller
  already sees the retention. Copying small fields out of views would
  reintroduce the copy the view exists to avoid, while still retaining the
  buffer for the remaining fields.
- The retention hazard is documented, not hidden, following the existing
  rule in `docs/resource-budgets.md` §2.2 and `docs/zero-copy.md`: if a
  caller caches a small piece of a large message long-term, it must copy
  (`to_vec` / `to_owned`) and drop the view. View-type docs carry a
  one-paragraph retention warning with a pointer to the resource budget.
- Measured retention expectations (cited, not re-measured):
  - Slice view: retains exactly the caller's `&[u8]` span — 0 bytes
    allocated by decode, `len` bytes retained by borrow. Reference point:
    PK-09 `parse_shared` at 0 allocs / 0 wire bytes shows a tag-walk-only
    decode is achievable; a view adds per-field offset storage on top.
  - Handle view: retains the whole `Bytes` allocation (e.g. a 16–64 KiB
    transport chunk or a coalesced 1–8 MiB frame), plus one 48-byte `Bytes`
    handle. Reference point: SB-13 unary 8 MiB frames retain ~8.39 MB by
    construction; PK-09 mixed-shape `parse_shared` holds 2 allocs / 0 wire
    bytes while viewing 8 fields.
  - Eager nested views add no further retention: subspans re-window the
    same backing (the `Wire::window` refcount-clone shape, ~40 B per live
    window struct on the stack, no heap alloc for the window itself).
  - PK-20 must add A13 cells measuring per-view-type retained bytes
    (backing bytes + aux) on the SB-05 corpora; the policy decision here
    only fixes *what* is retained (whole buffer, documented), not the
    numbers.
- `to_owned()` detaches: the owned message copies field data out (via the
  existing owned parse/merge machinery) and never aliases the view's
  backing. After `to_owned()` + dropping the view, no transport buffer is
  retained.

### D5. What is explicitly not decided here

- Codegen option spelling and default (on/off, per-message opt-out):
  PK-20 proposes, maintainer approves.
- Whether the table-driven parse engine (PK-06/PK-07) backs view decode:
  PK-20 may reuse inline `merge_inner`-style tag walks; table reuse is a
  later optimization, not a model constraint.
- UK-kernel (`google-protobuf` API) views: out of scope; UK-18 owns
  plugin/kernel convergence.

## API sketches

Illustrative only; PK-20 finalizes signatures. Uses `Person` as the
example message.

```rust
// Borrowed slice view: zero allocation, borrows caller bytes.
let view = PersonWireView::decode_view(&frame)?;
assert_eq!(view.name(), "ada");          // &ProtoStr, borrowed
assert_eq!(view.data(), &blob[..]);      // &[u8], borrowed
let child: ChildWireView<'_> = view.child(); // nested re-view, free
let owned: Person = view.to_owned();    // full copy, detaches

// Leased handle view: owns the Bytes, views into it.
let handle = PersonViewHandle::decode_view_handle(frame_bytes)?; // Bytes in, no copy
let view: PersonWireView<'_> = handle.as_view();
process(view)?;                          // handler never names the frame lifetime
drop(handle);                            // backing released here

// Opt-in lazy family: nested messages deferred, fallible access.
let lazy = PersonLazyView::decode_lazy(&frame)?; // top-level fields only validated
let child: Result<ChildLazyView<'_>, ParseError> = lazy.child(); // validates on access
```

Runtime trait shape (new trait; does not disturb `Proxied`):

```rust
pub trait WireView<'a>: Sized {
    type Owned: Parse + Serialize;
    type Handle: ViewHandle<View<'a> = Self>;

    fn decode_view(buf: &'a [u8]) -> Result<Self, ParseError>;
    fn to_owned(&self) -> Self::Owned;
}

pub trait ViewHandle {
    type View<'a>;
    fn decode_handle(buf: Bytes) -> Result<Self, ParseError>
    where Self: Sized;
    fn as_view(&self) -> Self::View<'_>;
}
```

Error contract: `decode_view` returns the same `ParseError` taxonomy as
owned `parse` (truncation, overlong varint, depth limit 100, UTF-8 per
edition/field rules, required-field absence). Lazy `decode_lazy` reports
own-field errors only; deferred field bytes report the same taxonomy on
access.

## Retained-memory trade-offs (cited evidence)

| Shape | Owned `parse` today | View (expected) | Source |
|---|---|---|---|
| TAT populated decode | 7 allocs, 1072 B/op | 0 allocs; retains input span; per-field offsets on stack | PK-01 codec-profiles |
| 8 MiB blob parse | 1 alloc, 8.39 MB copied | 0 allocs, 0 copied; retains whole frame | PK-09 + SB-13 |
| 8×4 KiB mixed parse | 3 allocs, 32.8 KiB copied | 0 allocs; retains frame (~32 KiB + framing) | PK-09 parse_shared mixed 2 allocs/0 wire B |
| Small `name_80` decode | 1 alloc, 80 B copied | 0 allocs; borrows input | PK-01 small cells |
| `Wire` window overhead | +48 B/message alloc bytes (40 B `Wire`) | same window shape reused; no new backing type | PK-09 |
| Sharing threshold | 4 KiB (owned bytes fields) | none at view layer (whole-buffer retention, documented) | PK-09 sweep |

Trade-off summary: views trade *retention scope* (whole buffer must stay
alive) for *zero copy and zero allocs*. For RPC handlers that read a few
fields and drop the message, this strictly dominates owned parse. For
handlers that cache one small field from a large message, views are a
footgun without the copy-out rule — hence D4's documentation requirement
and `to_owned()` detach. The 4 KiB PK-09 threshold stays exactly where it
is (owned `bytes` fields); views do not get their own threshold because
partial copy-out would keep the retention while losing the zero-copy win.

## buffa eager/lazy comparison rules (scoreboard group A)

These rules bind PK-20's benchmark cells and any A4/A5/A13 claim involving
views. Comparators are pinned by SB-02; buffa arms are owned, eager view,
and lazy view, always kept separate (README scoreboard table, group A).

1. **A4 (borrowed decode, full validation)** compares only full-validation
   decoders against each other: pbrs `decode_view` vs buffa eager
   `decode_view` vs `google-protobuf` (upb) view decode vs any other
   borrowed full-parse arm. PK-20 accept: match or beat buffa's eager view
   within 5% on borrowed string/bytes cells (contract §7.2).
2. **Lazy/partial decode is reported separately, never compared with full
   parse** (A4 category definition). pbrs `decode_lazy` (if PK-20 ships
   it) is compared only against buffa `decode_lazy` and other explicitly
   lazy arms, in separately labeled rows. A lazy row that wins against an
   eager row is not a win and must not be presented as one.
3. **A5 (parse+touch)** applies to every arm including views: decode plus
   the recursive touch walk from `bench/src/main.rs` (touch formula equal
   across decoders; checksums must match). Touch exposes deferred
   materialization that parse-and-drop hides — this is where lazy views
   pay for deferral and where the comparison stays honest.
4. **A13 (allocations/retained memory)** is measured per arm: allocator
   counts (allocs/op, bytes/op) plus retained backing bytes after decode
   (view backings included). Owned arms report owned heap; view arms
   report retained input/handle bytes. No arm may omit its backing.
5. **Legacy owned-vs-view gates stay smoke-only.** The `view_gated` cells
   in `bench/src/main.rs` (owned pbrs decode gated against buffa view
   except `tat_populated`/`person`/packed-fixed) predate comparable view
   evidence and are not contract-grade comparisons (`docs/benchmarks.md`:
   "smoke checks only"). PK-20 replaces the relevant gates with rule-1/2
   cells; it must not extend or rebless owned-vs-view gating as evidence.
6. **No `&Owned`-as-view sleight of hand.** The existing `FooView(&Owned)`
   reborrow types must never appear in a borrowed-decode column (OP-06
   accept: "do not rename &Owned as zero-copy"). A4 cells time from wire
   bytes to usable view, including validation.

## Open questions for maintainer

- **O1. Naming.** Proposed: `FooWireView<'a>` (borrowed), `FooViewHandle`
  (Bytes-leased), `FooLazyView<'a>` (opt-in lazy). Alternatives: (a) claim
  `FooView` for the wire-backed view and rename today's `&Owned` wrapper
  (breaking; needs a migration story for the `Proxied::View` associated
  type); (b) `FooRef<'a>`/`FooViewRef` shorter forms. Preference?
- **O2. Codegen default.** Should views be generated by default for all
  messages (larger gencode, PK-07 code-size budget impact), opt-in per
  message/file via config, or default-on with opt-out? Default-on risks
  B3/B4/B6 regressions; default-off risks nobody using them.
- **O3. Lazy family in PK-20 scope?** PK-20 is size L already. Options:
  (a) PK-20 ships eager + handle only, lazy follows as PK-20b; (b) PK-20
  ships all three families. This record recommends (a): eager + handle
  covers the A4 accept criterion, and lazy rows are separately reported
  anyway.
- **O4. `no_std`/alloc-only story.** `FooViewHandle` needs `bytes::Bytes`
  (already a pbrs dependency via PK-09). Is a handle type acceptable in
  every shipping profile, or must views compile under a minimal feature
  without `bytes`? (QG-04 pure-Rust audit applies regardless.)
- **O5. Unknown-field access shape on views.** This record says views
  preserve unknown fields for `to_owned()` round-trip but offer no mutable
  API. Should views expose read access to unknown fields (e.g. raw bytes
  iterator), or keep them entirely opaque until `to_owned()`?
