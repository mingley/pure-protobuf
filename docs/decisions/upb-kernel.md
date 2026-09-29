# UK-02: upb-in-Rust kernel architecture

Date: 2026-09-29
Base SHA: `2598aa12`
Status: draft for maintainer sign-off (accept item 3; UK-03..UK-10 wait on it)
Scope: official `protoc --rust_out kernel=upb` path only (`src/runtime/*`)

## Positioning

Google's stated direction ([protobuf#24880 comment, 2025-12-16][24880]) is
that a pure-Rust kernel would most likely be "rewriting upb itself into Rust",
able to back Python, Ruby and PHP too, with no timeline. Their main objection
is the correctness and maintenance cost of "yet another parser", not speed.

This design takes that comment literally. The kernel is **upb semantics in
Rust**, not a second parser design:

- It decodes the same mini-descriptor strings gencode already embeds and
  builds tables with the same field content upb builds (verified field by
  field against C-dumped tables in UK-04).
- It lays messages out the way upb lays them out (hasbits, oneof cases,
  aligned field offsets), so offsets and sizes compare equal with C.
- It implements upb's observable semantics: arena fusion, last-wins maps,
  unknown-field retention, closed-enum rejection, deterministic encode,
  extension registries.
- It sits behind the official `__internal::runtime` ABI that
  `protoc --rust_out` already generates against, so no gencode changes and no
  `protoc` changes are needed.

What is deliberately *not* upb-C-compatible: the in-memory C struct shapes
(`upb_MiniTable`, `upb_Arena`, `upb_Map` pointers). Nothing links C here, so
bit-compatibility with C structs buys nothing and would force C aliasing
habits into Rust. Compatibility is defined at two boundaries instead:
mini-descriptor decoding plus layout algorithm (checked against C dumps), and
wire behavior (checked by conformance, shared suites and differential fuzz).

The `grpc-rust` stake: Google's `grpc` crate (and the tonic 0.14 line housed
in `grpc/grpc-rust`) builds on the protobuf 4.x Rust API. Today that means
crates.io `protobuf` 4.x / `google-protobuf` 0.36.x over the C upb kernel
(`links=upb`, `cc`). The UK lane's end state (UK-14) is a `[patch]` facade so
an unmodified `grpc` helloworld and downstream workspaces build against this
kernel. That sets two hard constraints on this design: the kernel must track
every supported `google-protobuf` release pin (drift gate UK-19), and its
public surface is dictated by gencode, not by our taste — the UK-01 inventory
(`docs/evidence/rust-out-abi.md`: 33 implemented, 8 stubs, 0 missing at
protoc 35.1 and 36.1) is the contract, and UK-10 closes it to zero stubs.

[24880]: https://github.com/protocolbuffers/protobuf/issues/24880

## Starting point

`src/runtime/*` (MX-04 split of `src/runtime.rs`, ~2.7k lines) is a
MiniTable/Arena stand-in, not a kernel: linear field scan, boxed per-message
slot/presence vectors, `Rc<RefCell>` "arena", and 8 stubs (enum tables,
extension registry + `MiniTableExtension`, raw view interop, `message_eq`,
`debug_string`, unknown-field retention in the generic parser). The UK-03..UK-10
sequence replaces each module behind the same ABI, one module per card. This
record fixes the target shapes so those cards do not re-decide them.

## MiniTable layout (UK-04)

MiniTables are decoded from the base92 mini-descriptor strings at
`build_mini_table` time (once per static, at first use), then linked by
`link_mini_table` with sub-message/sub-enum tables. Target shape:

```rust
pub struct MiniTable {
    pub fields: Vec<MiniField>,   // sorted by number (as today)
    pub subs: Vec<MiniTablePtr>,  // linked sub-tables, index-addressed
    pub enums: Vec<MiniTableEnumPtr>,
    pub size: u32,                // message byte size from layout algorithm
    pub hasbit_bytes: u32,
    pub is_map: bool,
    pub enforce_utf8: bool,
    // dispatch side-index, built at link time (see Decoder)
    dispatch: DispatchIndex,
}

pub struct MiniField {
    pub number: u32,
    pub ty: FieldType,        // 18 descriptor types (as today)
    pub mode: FieldMode,      // Scalar | Array | Map  (replaces bool pair)
    pub rep: FieldRep,        // storage class: I32/I64/U32/U64/F32/F64/Bool/
                              // StrView/Ptr/EnumClosed/EnumOpen
    pub offset: u32,          // byte offset in message (layout algorithm)
    pub presence: Presence,   // None | Hasbit(u16) | Oneof(case_offset, index)
    pub sub: u32,             // index into MiniTable.subs (message/group/map/enum)
    pub packed: bool,         // encode default; decode accepts both forms
    pub required: bool,
}
```

Rules:

1. **Decode must match upb's mini-descriptor decoding exactly**, including
   modifier bits, field-number skips, oneof groups, map-entry markers and the
   closed-vs-open enum bit. UK-04's accept test (field-by-field comparison
   against upb C dumps from a harness under `bench/xlang` for every upstream
   test proto) is the proof.
2. **Enum tables become real.** `build_enum_mini_table` decodes the closed-enum
   value set; closed enums reject unknown values at parse/access per upb
   semantics; open enums keep them. `link_mini_table` wires `_subenums` (today
   ignored).
3. **Extension mini-descriptors decode into `MiniTableExtension`** (extendee
   table, number, field shape, sub-table link). See Extensions.
4. `field_by_number` stays sorted-order today; the decoder does not call it on
   the hot path once `DispatchIndex` exists (cold fallback only).

## Arena (UK-03)

Replace `Rc<RefCell<Vec<Box<..>>>>` with a chunked bump arena with upb
`upb_Arena_Fuse` lifetime semantics.

```rust
pub struct Arena { inner: Arc<ArenaShared> }   // Clone, Send, !Sync
struct ArenaShared {
    head: UnsafeCell<ChunkPtr>,   // bump pointer, !Sync-guarded
    groups: RefCell<Vec<Arc<ArenaShared>>>,  // fused lifetime group
    cleanups: RefCell<Vec<Cleanup>>,         // (ptr, drop fn) for non-POD
    allocated: Cell<usize>,                  // size accounting (A13)
}
```

Rules:

1. **Bump chunks.** Linked list of blocks, first block small (256 B like upb),
   geometric growth, big allocations get dedicated blocks. All message, array,
   map, string and unknown-field bytes come from chunks: parse does O(1)
   allocator calls per message in the common case, not one `Box` per node.
2. **Fuse is lifetime union.** `fuse(other)` adds each side's group to the
   other (idempotent, cycle-safe via pointer-set check), so fused arenas die
   together — the same guarantee the current `Vec::append` move gives, without
   moving anything. Self-fuse is a no-op. This preserves the existing
   no-use-after-free / no-leak / idempotency invariants in
   `docs/unsafe-invariants.md` §2.3; UK-03 re-proves them for chunks.
3. **`Send` but `!Sync`.** Bump allocation through `&self` is sound only
   because `!Sync` forbids sharing `&Arena` across threads. The public
   `OwnedMessage`/`MessageMut`/`MessageView` threading guarantees must match
   what `threading_test.rs` (shared suite) requires; UK-03 writes the
   `Send`/`Sync` test matrix first and picks the weakest bounds that pass.
   `Sync` arenas (for hypothetical cross-thread builds) are explicitly out.
4. **Cleanups run at group drop.** Non-POD arena contents (hash-map shells,
   anything holding a `Vec`) register a `(ptr, drop_fn)` pair. POD message
   bytes need no drops. LeakSanitizer on the scheduled lane proves nothing is
   lost and nothing double-drops.
5. **Size accounting.** `allocated` tracks bytes for the UK-13 retained-memory
   cell (A13) and for `UnknownFields`/map growth adversarial tests.
6. **Pointer provenance.** Keep the lesson from the current code (Miri caught
   pre-insertion retags): derive every handed-out pointer from its final owner
   *after* it is stored. Bump allocation makes this natural (pointer = chunk
   slot, never moves), which removes one current `unsafe` hazard class.

## Message memory layout (UK-05)

Replace `MsgData { slots, has, strs, unknown, mt }` vectors with one contiguous
arena allocation per message, laid out by the same algorithm upb uses:

```text
offset 0:            hasbit words (u32 LE, ceil(has_count / 32))
next, aligned:       oneof case words (u32 field-number, 0 = unset), one per oneof
next, per field:     inline scalar (1/4/8 B, naturally aligned), or
                     StringView { ptr, len } for string/bytes, or
                     pointer slot for sub-message / array / map / extension dict
trailing:            unknown-field list head (null when empty)
total:               MiniTable.size, 8-aligned
```

Rules:

1. **Offsets come from the layout algorithm, not from Rust field order.**
   UK-04 computes `offset`/`presence`/`size` per field; UK-05's accessors are
   pure offset arithmetic. The UK-04 C-dump comparison pins the algorithm
   (including whether oneof members share storage — resolved from the dump,
   not assumed here).
2. **All accessor ABI entry points** (`get_*_at_index`, setters, mutable-map/
   repeated getters, sub-message adoption) operate on the new layout with
   identical signatures. `rust_out_shared` passing unchanged is the gate.
3. **Strings are views.** Parsed strings/bytes are `StringView` into arena
   bytes (or aliased input, see Decoder); setters copy into the arena. No
   per-field `Box<Vec<u8>>` headers.
4. **Adoption still fuses.** Assigning a sub-message/array/map from another
   arena fuses that arena into the parent's (same rule as today), so the child
   outlives its original handle.
5. UK-05 reports populated-TestAllTypes size and allocations-per-parse
   before/after (accept item; feeds UK-13).

## Decoder (UK-06): table-driven with tag-dispatch fast paths

Target is a fasttable-class decoder: a per-message dispatch structure built at
link time plus a generic fallback loop.

1. **Fast path.** `DispatchIndex` maps 1-byte tags (field numbers 1..15, the
   overwhelmingly common case in Google's messages) directly to
   `(MiniField, handler)`. Each handler is a monomorphized function per
   `(FieldRep × FieldMode)` that inlines the wire read (varint/fixed32/
   fixed64/len) and the offset write. Two-byte tags get a compact second-level
   index only if UK-13 shows wide-message dispatch is branch-bound; otherwise
   they take the generic path.
2. **Generic path.** Decode tag → binary search `fields` by number → wire-type
   check → handler. Handles multi-byte tags, groups, wire mismatches and
   schema evolution. Also the path fuzzing hammers for state-space coverage.
3. **Semantics (all conformance-gated).** Unknown fields retained into the
   arena in encounter order (closes the `parse_into` stub); packed and
   unpacked forms accepted for every packable repeated field regardless of the
   `packed` default; proto3 UTF-8 enforcement and `enforce_utf8` tables;
   closed-enum rejection; proto2 required check as a decode option;
   recursion depth limit 100; last-wins singular, append repeated, last-wins
   map entry.
4. **Decode options.** `check_required: bool`, `alias_input: bool`
   (`StringView`s point into the caller's buffer, which the arena retains via
   an `Arc<[u8]>` handle — no copy for large `bytes`), `depth_limit: u8`.
5. **Differential accept.** UK-06 decodes all conformance and corpus payloads
   through the new decoder and the plugin parser with identical results and
   errors; full qualification comes later via UK-12 (conformance testee) and
   UK-15 (differential fuzz vs upb C).

## Encoder (UK-07): single-pass

1. **Size first, then one forward pass.** `serialized_size` walks the message
   without allocating (fields, packed payloads, map entries, unknowns,
   extensions); encode reserves exactly that and writes forward into the
   buffer. No temporary `Vec`s for nested messages, packed fields or map
   entries on the encode path (accept item). No reverse-encoding pass: forward
   writing with precomputed size is simpler to prove safe and equally
   allocation-free.
2. **Deterministic mode.** Field-number order, sorted map keys, no padding —
   byte-identical to upb C on all corpora (accept item).
3. **Caller buffers.** Encode directly into a caller-provided slice, including
   a reserved 5-byte gRPC frame prefix variant, so the transport path
   (PK-09..PK-11, UK-13) never copies.
4. Cached sizes: kernel messages do *not* carry a `CachedSize` cell (upb does
   not; the cell costs 8 bytes per message and a dirty protocol). If UK-13
   shows repeated `serialized_size`+`encode` pairs on huge messages losing to
   upb, revisit — upb recomputes too, so parity is expected.

## Extensions (UK-08)

1. `MiniTableExtension { extendee: MiniTablePtr, number: u32, field: MiniField-shape, sub: u32 }`,
   decoded from extension mini-descriptors (UK-04 provides decode; UK-08 owns
   the type and registry).
2. `ExtensionRegistry`: hash map `(extendee, number) → MiniTableExtension`,
   O(1) lookup (accept item). Parse consults the registry for unknown field
   numbers on extendable messages; matches decode into the message's
   extension dict (a map-shaped side table, not inline field slots); misses
   stay unknown fields and round-trip.
3. Generated typed extension accessors (`ExtensionSub` and friends from the
   UK-01 inventory) work through the same registry.
4. Coordinate with CG-14b for the plugin path; the two registries share
   lookup semantics but not storage (plugin messages are structs).

## Maps and repeated fields (UK-09)

1. **Arrays.** Arena-backed `RawArrayInner` with inline scalar elements or
   `StringView`/pointer elements; `push/get/set/remove/iterate/clear` with
   upb-compatible semantics (pointer stability for message elements across
   growth — chunk allocation gives this free).
2. **Maps.** Hash map with last-wins insert, O(1) get/remove, iteration and
   clear. First implementation uses `hashbrown` (pure Rust, no new C) inside
   an arena-owned shell with cleanup registration. A custom open-addressing
   table is justified only by a UK-13-measured loss. Map-of-message values
   adopt (fuse) on insert. Deterministic encode sorts by key bytes.
3. String keys borrow from arena bytes; no per-iteration key clones (keeps the
   current §4.1.1 no-leak invariant).
4. Accept: `accessors_map_test`, `accessors_repeated_test`,
   `proto_macro_test` pass unmodified; no quadratic counting remains.

## Unknown fields

Retained per message as an arena singly-linked list of `(tag, payload)` spans
in encounter order; empty messages pay one null pointer. Re-emitted verbatim
on encode (including in deterministic mode — upb re-emits unknowns as kept),
counted in `serialized_size`. Unknown extensions that miss the registry live
here too, so extendable-message round-trips hold with or without a registry.

## Plugin-path sharing

Short answer: **share wire primitives now, converge engines later (UK-18).**

- Both paths must share, as one implementation each: varint encode/decode,
  fixed32/64 reads, tag encode/decode, UTF-8 validation modes, zigzag, and
  unknown-field capture/emit. UK-06/UK-07 reuse `src/wire.rs` helpers where
  they exist and extend that file rather than forking decoders. This directly
  answers Google's "yet another parser" objection: one wire layer, two
  front ends.
- The plugin path keeps its struct API and its PK-05 hybrid inline/table
  parser; it does **not** retarget onto MiniTables in the UK lane. Whether
  plugin gencode should target UK tables (smaller code, one engine) is UK-18's
  decision, made with PK-07 and UK-13 measurements in hand.
- The UK lane reads `src/wire.rs`, `src/lazy.rs`, `src/string.rs`,
  `src/packed.rs`, `src/map.rs` but does not restructure them; any change
  there is coordinated with the owning lane per the file-ownership table.

## Crate placement — RECOMMENDATION (maintainer decision)

**Recommended: keep the kernel inside `pbrs` at `src/runtime/*` (status quo
placement, MX-04 split).** The UK-14 `[patch]` drop-in is then a thin facade
crate named `protobuf` (version-matched per supported pin) that re-exports the
kernel API — the facade is a new crate, the kernel is not.

Reasons:

1. **Maintainer rule is "few new crates"** with a three-crate joint release
   gate. A kernel crate adds a fourth release artifact and a version-skew
   surface (kernel vs facade vs gencode pins) for no behavioral gain.
2. **Shared wire primitives stay zero-cost.** Cross-crate sharing would force
   `pub` stabilization of `wire.rs` internals mid-optimization; in-crate
   `pub(crate)` keeps PK and UK iterating without API promises.
3. **Gates already key on this layout.** QG-01's `miri-kernels.yml` watches
   `src/runtime/**`; UK-03..UK-10 write scopes name `src/runtime/*.rs` files.
4. Precedent: PK-02 is separately deciding the runtime/build split; whatever
   it decides (features vs crate) applies to the kernel as part of the
   runtime, not as a third outcome.

Alternative (separate `pbrs-kernel`/`upb-rs` crate): cleaner story if Google
ever vendors the kernel standalone, and smaller `pbrs` API surface. Cost is
items 1–3 above plus duplicated unsafe-audit surface. If the maintainer
prefers this, do it as a mechanical move *after* UK-10 (zero stubs, suites
green), not during the module rewrites.

Either way the kernel's public surface is exactly what gencode names (UK-01
inventory); nothing else in `src/runtime/*` is `pub` to application code.

## Unsafe boundaries

`unsafe` is confined to `src/runtime/*`, behind safe APIs, under the QG-01 new
kernels policy (`SAFETY` comment + pre-registered `unsafe-invariants.md`
entry + PR-time Miri + second reviewer for every site):

| Site | Why unsafe | Safe wrapper |
|---|---|---|
| Arena bump pointer (`UnsafeCell` chunk cursor) | pointer arithmetic, exclusive access | `Arena::alloc_*` taking `&self`, sound by `!Sync` |
| Field access by offset | raw offset → typed slot | one accessor module; `MiniField.offset/presence` checked at link test, not per access |
| `StringView` → `&[u8]` | raw slice construction | `as_ref` with len/null rules (as today) + arena-lifetime tie |
| Fast-dispatch handlers | same as field access, monomorphized | generated-equivalent match arms; generic path is the safe reference |
| Cleanup registry | type-erased drop fns | `register_cleanup` with `SAFETY: fn matches ptr type`, run-once at group drop |
| `adopt_owned_msg` transmute | read `OwnedMsgHead` from gencode type | keep today's size precondition + `mem::forget` discipline |

Everything else — mini-descriptor decoding, table linking, dispatch-index
construction, map/array logic above the element primitives, registry lookup,
size computation — is safe Rust. The generic decode path is the safe semantic
reference the fast path is differentially tested against.

## Miri and fuzz strategy

- **Miri (strict provenance, `-Zmiri-strict-provenance`).** Every UK-03..UK-10
  card ships `tests/upb_kernel/<module>.rs` exercising its unsafe surface
  (arena growth/fusion/aliasing, offset accessors incl. oneof/presence edges,
  dispatch handlers vs generic path, cleanup drops, `Send`/`!Sync` asserts).
  PR-time `miri-kernels.yml` already gates `src/runtime/**`. Reuse the proven
  invocation from `unsafe-invariants.md` §6.3.
- **Differential fuzz vs upb C (UK-15).** Decode/encode/size/equality fuzzers
  with the pinned upb C harness as oracle; corpora seeded from conformance +
  SB-05. Sustained CPU budget needs the same approval as §6.2's 24-CPU-hours
  per target rule.
- **Conformance testee (UK-12).** `rust_out` gencode over this kernel as a
  conformance runner; zero unexpected results is the semantic gate.
- **Sanitizers.** Linux ASan/LSan stay on the scheduled `compatibility.yml`
  lane (per QG-01: too slow for PR gating); cleanups and aliasing get explicit
  LSan/ASan attention because Miri cannot see the C oracle side.
- **Provenance regression.** Keep a Miri test shaped like today's
  `map_and_repeated_message_arena_adoption_fusion` (grow + fuse + read) —
  that test caught a real Stacked Borrows violation once.

## Performance targets (per UK-13)

UK-13 benchmarks identical `rust_out` gencode on this kernel vs C upb on
TestAllTypes, google_message1/2, OTLP and Envoy corpora. Dev-loop bar: **win
or tie every primary cell; any loss gets a filed follow-up card.** No
universal claim — claim-grade confirmation is SB-15's job. Cells:

| Cell | Target vs upb C | Design lever |
|---|---|---|
| A1 fresh encode | win/tie | exact-size reserve, forward write, no temps, direct-to-caller-buffer |
| A3 owned decode | win/tie | 1-byte-tag dispatch, monomorphized handlers, bump arena |
| A5 parse+touch | win | contiguous layout, inline scalars, no lazy re-parse |
| A6 merge | tie | same hot loop as decode |
| A7 size | win/tie | non-allocating walk; no `CachedSize` cell to keep coherent |
| A13 retained memory | tie or better | chunked arena + accounting; aliased-input option |

Known structural advantages over the v4 wrapper (not over upb itself):
no per-`serialize` encode arena + FFI + copy (see `docs/upb.md`), and no
`Box`-per-node parse. The honest comparison target is upb C called
efficiently (`bench/xlang/upb`), not the wrapper's overhead.

## Upstream test portability

Rule: behavior tests that run through generated code or the wire are portable;
tests that name C types, C layout, arenas-as-pointers, or the C build are not.
Vectors and payloads from C-internal tests are still harvestable as differential
inputs.

**Black-box portable (run unmodified — UK-11):**

- `rust/test/shared/*` — the kernel-independent suites, already vendored in
  `rust_out_shared/tests` (19 crates, 233 tests): accessors (incl. proto3,
  map, repeated), bad_names, child_parent, edition2023, enum,
  fields_with_imported_types, import_public, message_copy_merge,
  message_generics, nested_types, package, proto_macro, serialization,
  simple_nested, threading, utf8.
- `rust/test/shared/extensions_test.rs` — portable once UK-08 + CG-14 land
  (a license-only stub at the v35.1 pin; no runnable extension cases).
- Official conformance suites (binary, JSON, text) via the UK-12 testee.
- `rust/test/upb/*` **behavioral** cases, where they assert through the public
  Rust API rather than FFI (triage per file at UK-11; e.g. text/JSON behavior
  yes, `OwnedArenaBox` pointer-identity tests no).

**C-internal (do not port; mine for vectors):**

- `upb/test/*` (C++): mini-table build/layout tests, arena/memblock tests,
  wire codec internals, `upb_Map`/`strtable` internals, reflection C API,
  json/text C printer/parser internals. Our equivalents are
  `tests/upb_kernel/*` plus the UK-04 C-dump comparison.
- `upb/mini_table/`, `upb/mem/`, `upb/message/`, `upb/wire/` unit tests — same
  reason: they assert C struct shape and C aliasing contracts that
  deliberately have no Rust counterpart (see Positioning).
- `rust/upb/*` unit tests that assert FFI glue (`wire.rs` pointer handling) —
  port the behavior cases, drop the FFI-shape cases.
- Fuzz seeds under `upb/` — reusable as corpus inputs only.

**Not applicable at all (documented, not ported):**

- `gtest_matchers_test.rs` (needs `protobuf_gtest_matchers`; skipped per
  `tests/google_shared.rs`).
- `no_internal_access_test.rs` (asserts `__internal == ()`; ours is
  deliberately a module — `docs/upb.md`).
- `ctype_cord_test.rs` (C++ cord/STRING_PIECE storage; ordinary strings here).
- `package_disambiguation_test.rs` (empty upstream).
- cpp-kernel-only suites under `rust/test/cpp/` (different kernel by
  definition; shared-behavior overlap already covered).

## Implementation order (for UK-03..UK-10)

UK-03 (arena) and UK-04 (tables) are independent and can run in parallel;
UK-05 (layout) needs both; UK-06 (decode) and UK-07 (encode) need UK-05 and
can parallelize; UK-09 (maps/arrays) needs UK-05; UK-08 (extensions) needs
UK-06; UK-10 (reflection/eq/debug/formats) needs UK-06+UK-07. Critical path
for the UK-14 drop-in: UK-02 → UK-03 → UK-05 → UK-06 → UK-08 → UK-11 → UK-14.

## Open questions for maintainer

1. **Crate placement** (accept item 1): approve in-`pbrs` kernel
   (`src/runtime/*`) + thin `protobuf` facade crate at UK-14, or require a
   separate kernel crate now?
2. **Facade naming/scope** (feeds UK-14): is a crates.io-shadowing crate
   literally named `protobuf`, usable only via `[patch]`, acceptable? Any
   trademark/packaging objection to prepare for?
3. **`__internal` divergence**: `no_internal_access_test.rs` can never pass
   (module vs `()`). Confirm this stays an approved permanent exclusion.
4. **Threading bounds**: is `Arena: Send + !Sync` (no cross-thread shared
   builds) an acceptable ceiling, or must some multi-threaded build pattern
   work?
5. **Upstream packet ambition (UK-16)**: is the goal still "evidence-led
   upstream proposal, posting needs approval", with `[patch]` usefulness as
   the success floor if Google declines? Any contacts/threads to align with
   before UK-16 drafts?
6. **C harness tolerance**: UK-04/UK-13/UK-15 need pinned upb C + comparators
   as test-only tools (never shipped, per QG-04). Confirm that is approved in
   principle so those cards do not re-litigate it.
