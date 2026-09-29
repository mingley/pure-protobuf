# Unsafe and Target-Dependent Invariants in pbrs

This page lists every safety invariant that justifies `unsafe`, target-specific layout assumptions, and low-level runtime behavior in `pbrs`. It is for reviewers and maintainers auditing memory safety. Bottom line: each unsafe site must have a local `SAFETY` proof, and this document records the shared invariants that those proofs rely on.

---

## 1. Safety Policy: Why `unsafe_code = "deny"` Instead of `forbid`

Pure-protobuf enforces `#![deny(unsafe_code)]` at the workspace level and `#![deny(unsafe_op_in_unsafe_fn)]` in `src/lib.rs`.

It intentionally does **not** use `#![forbid(unsafe_code)]`. `forbid` cannot be relaxed for specific audited modules or items, but `pbrs` needs small, reviewed unsafe primitives for wire-speed parsing and Google `rust_out` application binary interface (ABI) compatibility.

Those unsafe primitives exist for four reasons:

1. **ABI interoperability (`src/runtime.rs`).** `protoc --rust_out` generated code for Google protobuf v35.1 with the upb kernel ABI expects C-compatible struct representations (`#[repr(C)]`), raw message pointers (`*mut MsgData`), and static MiniTable pointers (`MiniTablePtr`).
2. **Zero-copy byte/string projections (`src/string.rs`).** `ProtoStr` is a `#[repr(transparent)]` wrapper over `[u8]`. It can reinterpret wire byte slices immediately, without UTF-8 revalidation or heap allocation.
3. **Hardware-accelerated memcpy for packed fixed-width scalars (`src/packed.rs`).** On little-endian targets, fixed32/sfixed32/fixed64/sfixed64/float/double wire bytes match native in-memory layout bit-for-bit. That allows direct `copy_nonoverlapping` memcpy operations.
4. **Fast zeroed message initialization (`src/rt.rs`).** Flat generated message layouts are designed so an all-zero byte pattern is a valid default message. This allows `std::mem::zeroed` initialization without per-field branching.

### Policy Enforcement Bar

Every `unsafe` block or `unsafe fn` must have a nearby `// SAFETY:` comment. The comment must state:

- the exact preconditions required before entering the block;
- the invariants maintained while the block runs;
- why undefined behavior (UB), out-of-bounds access, aliasing violations, and use-after-free are impossible.

Code without a satisfactory `// SAFETY:` rationale is rejected in code review.

Miri, AddressSanitizer (ASan), and LeakSanitizer (LSan) are scheduled qualification tools, not proofs of memory safety. A successful run and its exact toolchain/artifact are required before claiming their coverage.

---

## 2. MiniTable & Runtime Invariants (`src/runtime/`)

`src/runtime.rs` re-exports the `src/runtime/` modules (`arena`, `array`, `decode`, `encode`, `extension`, `layout`, `map`, `mini_table`, `reflect`), which implement the Google protobuf `__internal::runtime` upb kernel ABI in pure Rust. This lets generated code from `protoc --rust_out`, such as `rust_out_person`, link against `pbrs` without C or C++ dependencies.

### 2.1 Struct Representations (`#[repr(C)]`)

`MessagePtr<T>`:

```rust
#[repr(C)]
pub struct MessagePtr<T> {
    raw: *mut MsgData,
    _phantom: PhantomData<T>,
}
```

- `MessagePtr<T>` is layout-compatible with a raw C pointer.

`OwnedMessageInner<T>`:

```rust
#[repr(C)]
pub struct OwnedMessageInner<T> {
    ptr: MessagePtr<T>,
    arena: Arena,
}
```

- Layout starts with `ptr`, which is 8 bytes on 64-bit targets.
- `arena` follows `ptr` and is `Rc<RefCell<ArenaInner>>`.

`MessageMutInner<'msg, T>`:

```rust
#[repr(C)]
pub struct MessageMutInner<'msg, T> {
    pub ptr: MessagePtr<T>,
    pub arena: &'msg Arena,
}
```

`MessageViewInner<'msg, T>`:

```rust
#[repr(C)]
pub struct MessageViewInner<'msg, T> {
    ptr: MessagePtr<T>,
    _phantom: PhantomData<&'msg ()>,
}
```

### 2.2 Pointer Offsets & Field Slots

`MsgData` stores dynamic runtime state:

```rust
pub struct MsgData {
    pub slots: Vec<FieldKind>,
    pub has: Vec<bool>,
    pub strs: Vec<Box<Vec<u8>>>,
    pub unknown: UnknownFields,
    pub mt: MiniTablePtr,
}
```

Invariant: `slots.len() == has.len()`.

Accessor rules:

- Getters such as `get_i32_at_index` query `self.slot(index)`. If `index >= slots.len()`, the accessor returns `FieldKind::Empty`, which prevents out-of-bounds reads.
- Setters call `set_slot`, which resizes `slots` and `has` when `index >= slots.len()`.
- Oneof exclusivity is enforced by `MiniTable` metadata. Setting a field with `oneof_group != 0` visits every field in that group, sets its `has` flag to `false`, and resets its slot to `FieldKind::Empty`.

### 2.3 Arena Allocation & Fused Arenas

`ArenaInner` owns all raw runtime allocations:

```rust
pub struct ArenaInner {
    msgs: Vec<Box<MsgData>>,
    arrays: Vec<Box<RawArrayInner>>,
    maps: Vec<Box<RawMapInner>>,
    bytes: Vec<Box<Vec<u8>>>,
}
```

#### Pointer Stability Invariant

All messages, arrays, and maps are wrapped in `Box<_>`. When `ArenaInner.msgs` reallocates or grows, the underlying `MsgData` values stay pinned on the heap. Any raw pointer stored in `MessagePtr<T>` remains valid.

#### String Header Stability

`MsgData.strs` and raw map/repeated string stores hold boxed `Vec<u8>` headers, not movable headers in `Vec<Vec<u8>>`. Therefore, `FieldKind::Bytes` pointers remain valid after later field insertions or collection growth.

Bytes produced by raw parsing or cloning live in `ArenaInner.bytes` and move with the arena during fusion. `InnerProtoString::into_raw_parts` returns that owning arena with its `StringView` instead of leaking the buffer.

#### Pointer Provenance Invariant

`Arena::alloc_msg`, `alloc_array`, and `alloc_map` derive raw pointers **after** inserting their `Box` into the owning `ArenaInner` vector.

Taking a reference and raw pointer before moving the `Box` into that vector preserves the address but invalidates its Stacked Borrows permission on insertion. Miri detected this in the original `map_and_repeated_message_arena_adoption_fusion` test.

Deriving the pointer from the stored owner fixes the invalid retag. The regression also grows both arena vectors and reads the pointer after fusion.

#### Arena Fusion (`Arena::fuse`)

When a child message is assigned to a parent (`message_set_sub_message`) or inserted into repeated arrays/maps (`message_set_repeated_field`, `message_set_map_field`), `pbrs` fuses the arenas:

```rust
parent.arena.fuse(child.get_arena(Private));
```

`fuse` moves all `Box<MsgData>`, `Box<RawArrayInner>`, and `Box<RawMapInner>` values from the child's arena into the parent's arena using `Vec::append`.

- **No use-after-free.** If the original child handle drops, its arena has already transferred heap ownership to the parent. The child data remains alive as long as the parent message lives.
- **No memory leaks.** When the parent message drops, the parent's arena deallocates all child nodes transitively.
- **Idempotency.** If `Rc::ptr_eq(&self.inner, &other.inner)`, `fuse` is a no-op. This avoids self-append corruption.

#### Raw Collection Adoption

Each arena-owned repeated/map collection holds a `Weak` link to its owner.

A raw mutator that receives an owned submessage without an explicit parent upgrades that link and fuses the child's allocations. It never leaks a boxed `Arena`.

On fusion, moved collections' weak links are updated to the new owner without creating an `Rc` cycle. Pointers into boxed message, array, and map allocations remain valid.

### 2.4 Message Adoption (`adopt_owned_msg`)

`adopt_owned_msg<T>` (`src/runtime/layout.rs`) reads `value: T` as `OwnedMsgHead { raw: *mut MsgData, arena: Arena }` via `ptr::read` on the reference cast.

- **Precondition:** `size_of::<T>() >= size_of::<OwnedMsgHead>()`, else `forget(value)` and `FieldKind::Empty`. `rust_out` owned messages are `{ inner: OwnedMessageInner<T> }` with `ptr` first, so the prefix is `(raw, arena)`.
- **Action:** `forget(value)`, fuse `head.arena` into the parent (or the collection owner's upgraded) arena, record `head.raw` in `FieldKind::Msg`. A null `raw` yields `FieldKind::Empty`.
- **Invariant:** The `ptr::read` moves the `Arena` (`Rc`) out without running `T`'s destructor; fusion transfers heap ownership so the message stays alive while the parent lives.
- **Test anchor:** `map_and_repeated_message_arena_adoption_fusion` (`tests/runtime.rs`) fuses a child arena into a parent, drops the child arena, and reads the adopted scalar and string fields back.

### 2.4.1 FieldKind-to-View Transmutes (`src/runtime/reflect.rs`)

`kernel_fieldkind_to_view`, `kernel_msg_ptr_to_view`/`kernel_msg_ptr_to_mut`, and `kernel_bytes_to_view` build generated-API views with `transmute_copy`, gated on `TypeId` or size equality:

- **Scalar views are identity copies.** `View<'msg, i32>` is `i32` (`src/proxied.rs`), so `transmute_copy(&v)` for the `i32`/`i64`/`u32`/`u64`/`bool`/`f32`/`f64` arms copies the same type. Sound provided the `TypeId` gate matches the arm's type exactly.
- **String/bytes views are reference copies.** `View<'msg, ProtoString>` is `&'msg ProtoStr` and `View<'msg, ProtoBytes>` is `&'msg [u8]` (`src/string.rs`); the transmutes copy the fat pointer bit-for-bit. The referent must be arena-owned (`FieldKind::Bytes` points at a boxed `Vec<u8>`; see §2.3) and the caller must keep the arena alive for `'msg`.
- **Message views are layout-gated copies.** `kernel_msg_ptr_to_view` builds a `MessageViewInner<'msg, ()>` and transmutes only when `size_of::<View<'msg, T>>()` equals `size_of::<MessageViewInner>()` or `size_of::<*mut MsgData>()`. This assumes generated message view wrappers share that layout; a wrapper with extra fields or a different representation would be misread. Same-size is necessary, not sufficient — reviewers must check the generated wrapper layout, not just the gate.
- **Size-4 fallback (open review item PB07-F3).** When no `TypeId` arm matches and `size_of::<View>() == 4`, an `i32` is transmuted into the unknown view type. That assumes every 4-byte view accepts all $2^{32}$ bit patterns; a 4-byte view with a niche or validity invariant would produce an invalid value. No such view exists today, but the gate cannot prove it.
- **Lifetime contract.** All three helpers are `pub(crate) unsafe fn`; callers must ensure the arena (or `bytes` slice) outlives `'msg`. Exclusive `Mut` casts additionally require no outstanding view borrows of the same message.

### 2.5 Unsafe Traits

Each unsafe trait has one required invariant:

| Trait | Required invariant |
|---|---|
| `unsafe trait AssociatedMiniTable` | `mini_table()` must return the valid static MiniTable linked for this type. |
| `unsafe trait UpbGetArena` | The returned `Arena` must outlive the message pointer. |
| `unsafe trait UpbGetMessagePtr` | The returned `MessagePtr` must remain valid for `'self`. |
| `unsafe trait UpbGetMessagePtrMut` | The returned `MessagePtr` must be valid for exclusive mutation for `'self`. |

### 2.6 `StringView` Raw Slices (`src/runtime/layout.rs`)

`StringView { ptr: *const u8, len: usize }` is a C-style pointer/length pair. `unsafe fn as_ref<'a>(self) -> &'a [u8]` returns `&[]` when `len == 0` or `ptr` is null (covering `StringView::empty()`), else `slice::from_raw_parts(ptr, len)`.

- **Caller contract:** for a non-empty view the caller must ensure `ptr..ptr+len` is valid, initialized, and alive for `'a`, with no concurrent mutation. Every raw setter copies through this into an arena-owned `Box<Vec<u8>>` immediately, so the view never outlives the call.
- **Test anchor:** `string_view_empty_reads_as_empty_slice` (`tests/runtime.rs`) covers the null guard and a live round-trip.

---

## 3. String & Bytes Invariants (`src/string.rs`)

### 3.1 `ProtoStr` Layout and UTF-8 Invariants

```rust
#[repr(transparent)]
pub struct ProtoStr([u8]);
```

- **Representation.** `#[repr(transparent)]` guarantees that `ProtoStr` has the same memory layout, size, alignment, and ABI as `[u8]`.
- **Casting safety.** `ProtoStr::from_bytes(bytes: &[u8]) -> &ProtoStr` uses this cast:

  ```rust
  unsafe { &*(bytes as *const [u8] as *const ProtoStr) }
  ```

  The cast is sound because `ProtoStr` has no invalid bit patterns and imposes no validity invariant on the underlying byte slice.
- **Why `[u8]`, not `str`.** Protocol Buffers has edition-specific UTF-8 rules:
  - Proto3 string fields must be verified as UTF-8 on the wire.
  - Proto2 and Edition 2023 with `features.(pb.cpp).string_type = VIEW` / `utf8_validation = NONE` may contain arbitrary binary sequences, including non-UTF-8 bytes such as `0xFF` or `0x80`.
  - `ProtoStr` supports both. The proto3 parser validates UTF-8 eagerly at the wire boundary with `simdutf8::basic::from_utf8`; proto2 permits arbitrary bytes.
- **On-demand validation.** `to_str(&self) -> Result<&str, Utf8Error>` validates with `std::str::from_utf8` when called.

### 3.2 `ProtoString` Small-String Optimization (SSO)

```rust
const INLINE_CAP: usize = 23;

#[derive(Clone)]
enum Repr {
    Inline { len: u8, data: [u8; INLINE_CAP] },
    Heap(Vec<u8>),
}
```

SSO invariants:

- Strings `<= 23` bytes are stored inline in `data[..len]`. They do not allocate.
- `Inline` is 24 bytes: 1 byte for `len` plus 23 bytes for `data`.
- That 24-byte size matches `Vec<u8>` on 64-bit systems.
- Strings `> 23` bytes move to `Repr::Heap(Vec<u8>)`.
- `clear()` resets to `Repr::Inline { len: 0, data: [0; 23] }`.
- Standard Rust enum invariants enforce memory safety; no unsafe untagged union is used.

---

## 4. Lazy Parsing & Packed Repeated Scalars (`src/lazy.rs`, `src/packed.rs`)

### 4.1 Wire Buffer Slicing (`src/lazy.rs`)

`Wire` is a refcounted byte frame plus a window, with two backings:

```rust
pub struct Wire {
    inner: WireInner,
}

enum WireInner {
    Private { buf: Arc<[u8]>, start: u32, end: u32 },
    Shared(Bytes),
}
```

- `from_slice` copies into a private `Arc` frame (only `parse_bytes` shares caller memory via `from_bytes`). Empty values normalize to the static empty so they never pin a large frame.
- `window(rel_start, rel_end)` is zero-copy: it clones the `Arc`/`Bytes` handle and narrows offsets. A window therefore outlives its parent handle by construction.
- **Bounds are fail-closed, not `unsafe`.** Callers must maintain `start + rel_end <= end <= buf.len()`, but the enforcement is `debug_assert` in debug builds plus an ordinary panic on the first out-of-range read in release builds (`Private` records the offsets and panics in `as_slice`; `Shared` panics inside `Bytes::slice`). An out-of-bounds window is a panic (a robustness bug), never an out-of-bounds slice or UB.
- **4 GiB single-buffer cap (PB07-F4, fixed).** `Private` offsets are `u32`; `from_slice`/`window` go through the `u32_offset` helper (`checked_add` + `u32::try_from`) and panic fail-closed above `u32::MAX`. Single ambient parses above 4 GiB stay outside the qualified envelope, loudly.
- **Test anchors (`tests/runtime.rs`):** `wire_window_outlives_parent_frame` (both backings + empty normalization), `lazy_str_span_survives_parent_wire_drop` (windowed span vs. inline copy), `wire_window_past_end_panics_private` / `wire_window_past_end_panics_shared` (`should_panic` fail-closed proofs), plus the PK-09 `parse_bytes_*` pinning tests.

String parse strategy:

- Short strings (`<= 23` bytes) copy into inline `ProtoString` and do not allocate a parent `Wire`.
- Medium strings share the parent `Wire` frame through `Wire::ensure` (dense fields in small frames), or copy exact-size storage when the field is near-whole or sparse in a large frame (PK-15).
- Long strings use `Wire::from_utf8_payload`, which allocates a payload-specific `Wire` while running UTF-8 verification. This avoids pinning unused parent frame bytes.

### 4.1.1 Arena-backed map and repeated views

Raw map keys are borrowed from the arena-owned entry for the immutable view's lifetime. They are not cloned into permanently leaked allocations on every iteration.

Values inserted into raw map/repeated collections own stable `Box<Vec<u8>>` headers. Overwritten and removed values are released. `clear()` and arena drop reclaim the remaining values.

Values parsed or cloned through raw message paths are owned by the arena instead of leaked. Allocation pointers are derived only after boxes enter their owners.

Returning a view while unsafely mutating the same raw collection would violate the view's exclusive borrow contract. Safe generated APIs prevent that pattern.

Arena-owned parsed and cloned bytes are reclaimed when the arena drops, not necessarily when an individual field is cleared. Application memory budgets must count that retention.

### 4.2 Packed Fixed-Width Scalars & Memcpy Preconditions (`src/packed.rs`)

Fixed-width scalar types implement `PackedCodec` with `MEMCPY_SAFE = true`:

- `fixed32`
- `sfixed32`
- `fixed64`
- `sfixed64`
- `float`
- `double`

#### Preconditions for Memcpy

1. **Length divisibility.** `buf.len() % width == 0`. Truncated payloads are rejected with `ParseError` before any pointer operation.
2. **Target endianness.** The Protocol Buffers wire specification requires little-endian (LE) integer encoding.
   - On `#[cfg(target_endian = "little")]`, memory representation matches wire representation bit-for-bit.
   - On `#[cfg(target_endian = "big")]`, memcpy is disabled. The decoder iterates through `buf.chunks_exact(width)` and calls `from_le_bytes`; the encoder emits `to_le_bytes()`.
3. **Plain Old Data (POD) / validity invariant.** `u32`, `i32`, `u64`, `i64`, `f32`, and `f64` have no invalid bit patterns. All $2^{32}$ and $2^{64}$ bit states are valid, including floating-point NaNs, subnormals, signed zeros, and infinities.
4. **Aliasing and capacity.**

   ```rust
   let n = buf.len() / $width;
   let start = out.len();
   out.reserve(n);
   unsafe {
       let dest = out.as_mut_ptr().add(start) as *mut u8;
       std::ptr::copy_nonoverlapping(buf.as_ptr(), dest, buf.len());
       out.set_len(start + n);
   }
   ```

   - `out.reserve(n)` guarantees capacity for `n` additional elements.
   - `buf`, the input slice, and `dest`, the new uninitialized capacity in `out`, cannot alias or overlap.
   - `copy_nonoverlapping` writes `buf.len()` bytes into `dest`.
   - `out.set_len(start + n)` runs only after the memory is fully initialized.
5. **Unaligned source pointers.** `buf.as_ptr()` may point to an unaligned offset inside a network packet. `std::ptr::copy_nonoverlapping` operates on `*const u8` and `*mut u8`, which require only 1-byte alignment. Unaligned source buffers cannot trigger alignment faults or UB.

#### Encode-Side Slice (`from_raw_parts`)

The little-endian encoder reinterprets the element slice as bytes:

```rust
let byte_len = packed_byte_len(elems.len(), $width); // checked_mul, fail-closed
// SAFETY: the cast to *const u8 needs only 1-byte alignment ...
let bytes = unsafe {
    std::slice::from_raw_parts(elems.as_ptr() as *const u8, byte_len)
};
out.extend_from_slice(bytes);
```

- The cast to `*const u8` needs only 1-byte alignment, so any element alignment is fine.
- `elems` is a live shared slice, so `elems.len() * width` bytes at that address are valid and initialized.
- **32-bit length guard (PB07-F2, fixed).** `packed_byte_len` computes `elems.len() * width` with `checked_mul` and panics fail-closed on overflow; multi-GiB vectors stay outside the qualified envelope, loudly.
- The encode-side `from_raw_parts` carries a `// SAFETY:` rationale (PB07-F1, fixed).

#### Test Anchors

- `alignment_and_endianness_safety` (`tests/runtime.rs`): LE round-trips, unaligned `read_fixed32/64`, float bitcasts incl. NaN, packed unaligned append, ZigZag extremals.
- `packed_fixed_matches_big_endian_fallback_algorithm` (`tests/runtime.rs`): runs the big-endian `chunks_exact` + `from_le_bytes` fallback explicitly on the LE host and requires bit-equality with the memcpy path, since no big-endian runner exists (see §9).

### 4.3 Packed Varints: Canonical Re-encoding

Packed varint fields have `MEMCPY_SAFE = false`:

- `int32`
- `int64`
- `uint32`
- `uint64`
- `sint32`
- `sint64`
- `bool`

Reason:

- Protobuf decoders must accept overlong varints, such as `0x81 0x00` for `1`.
- Protobuf encoders must emit minimal canonical varints.
- Direct memcpy would preserve non-canonical encodings and violate conformance.

Therefore, varints are parsed into vector elements and re-encoded through `encode_varint` or `encode_zigzag32`/`encode_zigzag64`.

### 4.4 Zeroed Message Initialization (`src/rt.rs`)

```rust
pub unsafe fn zeroed_message<T>() -> T {
    unsafe { std::mem::zeroed() }
}
```

Safety invariant: `T` must be a generated message struct whose fields are all valid when represented as all-zero bits (`0u8`).

Verification across field types:

- Scalars such as `i32` and `f64`: all zero bits represent numeric zero (`0`, `0.0`).
- Standard `bool`: Rust represents `false` as `0u8`.
- Optional bool (`OptBool`): standard Rust `Option<bool>` uses niche optimization where `None = 2` and `Some(false) = 0`. An all-zero bit pattern would incorrectly represent `Some(false)`. Pure-protobuf avoids that with `OptBool(u8)`, where `0 = None`, `1 = Some(false)`, and `2 = Some(true)`.
- Pointers and boxes (`Option<Box<T>>`): Rust represents `None` for nullable pointers as all zero bits, meaning a null pointer.
- Collections (`Repeated<T>`, `Map<K, V>`, `Packed<C>`, `LazyMsg<T>`): built on `Option<Box<_>>`, with a null pointer representing empty state.
- `CachedSize(AtomicU64)`: initialized to `0`, the valid serialized length of an empty message.

**Test anchors (`tests/runtime.rs`):** `tat_default_is_empty_and_zeroed` (generated `new()` equals `default()` and serializes empty), `zeroed_primitives_are_valid_defaults` (`zeroed_message::<OptBool>()` is `NONE`, `zeroed_message::<CachedSize>()` reads `Some(0)`), `empty_eq_and_zeroed` (`src/packed.rs` unit test: zeroed `PackedI32` is empty and droppable).

---

## 5. Invalidation Safety (`CachedSize` & Dirty Tracking)

### 5.1 Atomic Size Caching (`src/rt.rs`)

```rust
pub struct CachedSize(AtomicU64);
```

Rules:

- `DIRTY = u64::MAX`.
- `get()` returns `None` when the value is `DIRTY`, or `Some(n)` when a cached length is present.
- `set(n)` stores `n` with `Ordering::Relaxed`.
- `dirty()` stores `DIRTY` with `Ordering::Relaxed`.
- `CachedSize` implements `PartialEq` by always returning `true`, so message equality compares semantic field content only.

### 5.2 Dirty Invalidation Invariants

Every generated message mutation must invalidate cached sizes:

1. **Scalar setters.** `set_field(val)` calls `self.cached_size.dirty()`.
2. **Collection mutators.** `field_mut()` dirties `self.cached_size` before returning `RepeatedMut` or `MapMut`.
3. **Clearing.** `clear_field()` calls `self.cached_size.dirty()`.
4. **Lazy objects.**
   - `Packed<C>`: `force_vec()` clears `inner.encoded = OnceLock::new()`, forcing re-encoding.
   - `LazyMsg<T>`: `get_or_insert()` sets `inner.wire = None`, so later serialization encodes the updated message struct instead of stale wire bytes.

**Test anchor:** `invalidation_safety_cached_size_and_dirty_tracking` (`tests/runtime.rs`) walks the full contract — `CachedSize` dirty/set/get, `serialized_len` growth on scalar/repeated/string/map/nested mutation, shrink on clear and on proto3-default reset, packed `OnceLock` invalidation on `push`, and nested in-place mutation visibility after re-serialization.

---

## 6. Sustained Fuzz Campaign Recommendations

Ongoing qualification should cover unsafe invariants, parser bounds, and target-dependent behavior.

### 6.1 Fuzz Targets & Scope

Targets are discovered from `fuzz/Cargo.toml` `[[bin]]` entries by `scripts/fuzz-campaign.sh`, so this table must match that manifest (five targets as of PB-07):

| Target | Scope | Invariants checked |
|---|---|---|
| `wire` (`fuzz/fuzz_targets/wire.rs`, 64 KiB cap) | Binary wire decoder over generated `TestAllTypesProto3`/`Person` plus dynamic messages: varints, tags, length-delimited spans, packed fixed/varint scalars, recursion depth limit. | No panics, no OOB reads, serialize→parse→serialize idempotence for valid messages. |
| `descriptors` (`fuzz/fuzz_targets/descriptors.rs`, 64 KiB cap) | Raw `FileDescriptorSet` decoding, `CodeGeneratorRequest` codegen (bounded files/bytes), cyclic imports, extension resolution. | Bounded recursion/output, safe failure on corrupt graphs, no output path escape. |
| `formats` (`fuzz/fuzz_targets/formats.rs`, 64 KiB cap) | JSON and text-format parsers over proto2/proto3/edition descriptors: arbitrary-precision numbers, WKT conversions, escapes, `Any`/`Struct` expansion. | No panics, bounded allocation, recheck round-trip within output cap. |
| `grpc_wire` (`fuzz/fuzz_targets/grpc_wire.rs`, 64 KiB cap) | gRPC frame decode, gzip round-trip, timeouts, status/metadata, `MessageLimits` enforcement (max 64 frames/input). | Limits honored, frame/payload round-trip equality, no panics. |
| `varint_diff` (`fuzz/fuzz_targets/varint_diff.rs`) | Differential check of `decode_varint`/`decode_tag`/validators against an independent scalar reference. | Bit-exact agreement incl. overlong/overflow/truncated edges. |

### 6.2 Sustained Fuzz Budget (Recorded, Not Approved)

- **Recorded budget:** **24 CPU-hours per major target**. Major targets are `wire`, `descriptors`, and `formats` (parser attack surface for unsafe-adjacent code); `grpc_wire` and `varint_diff` are minor (narrower scope, differential oracle). Total at this budget: **72 CPU-hours major + best-effort minor**.
- **Approval status: UNAPPROVED — spend blocked.** PB-07 only records the budget. No sustained campaign, cloud job, or billed compute may start without explicit recorded approval naming the approver, date, budget cap, and target list. A short local smoke (one target, ≤5 min, e.g. `./scripts/fuzz-campaign.sh --seconds 60 --target wire`) is allowed to validate the harness only.
- **Artifacts (when approved):** per-target logs under `target/fuzz-logs/` (uploaded by the `fuzz-campaign` job as `fuzz-campaign-logs`), minimized crashers as `fuzz/corpus/<target>/crash-<sha16>.bin` (uploaded as `fuzz-crashers`), plus toolchain/seed/elapsed-CPU rows appended to §6.3. Every crasher becomes a replayable deterministic regression test; crash files are never ignored and limits are never silently raised.
- **Cadence:** the weekly `compatibility.yml` `fuzz-campaign` lane runs 300 s/target smoke by default — it is a smoke, not the sustained campaign, and must never be described as one.
- **Corpus management:**
  - Seed corpus comes from differential binary/JSON/text test fixtures in `tests/fixtures/differential/`.
  - Minimized corpus and crashers are committed under `fuzz/corpus/<target>/`.
- **History (PB-05/PB-06, resolved):** wire campaign found and fixed 1 real bug (`wire.rs` `usize` overflow → `decode_len`, +3 unit tests + `fuzz_parse` seed; post-fix 4.4M execs/181 s, 0 crashes); descriptors/formats found and fixed 3 panics (codegen char-boundary slice, JSON duration truncate, timestamp checked arithmetic incl. day-0/month-13) + 1 text map-default type bug; post-fix formats campaign 1.09M execs/181 s, 0 crashes. All seeds replay clean via `tests/fuzz_parse.rs`.

### 6.3 Dated local proof and remaining limits

On 2026-09-23, against the **dirty** local checkout at `cd9d7bd8`, macOS arm64 nightly `rustc 1.100.0-nightly (e7769602a 2026-08-24)` with `MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance'` passed:

- `cargo +nightly miri test --offline -p pbrs --lib` (38/38)
- `cargo +nightly miri test --offline -p pbrs --test runtime` (14/14)
- `cargo +nightly miri test --manifest-path rust_out_shared/Cargo.toml --offline` (233/233 across 19 original Google shared suites)

Those runs cover pointer use after arena growth/fusion, raw map iteration, and repeated/map ownership, including previously leaking paths. This is local evidence for the current worktree. It is **not** a CI artifact for the committed SHA, a big-endian proof, or a 32-bit proof.

A separate local macOS arm64 run used `RUSTFLAGS='-Zsanitizer=address'` with nightly `-Zbuild-std` and passed the same 38 core and 14 runtime tests. `ASAN_OPTIONS` requested leak detection, but LeakSanitizer support on this host was not independently verified.

The repaired `compatibility.yml` scheduled lane now runs the core, runtime, and 72 targeted original shared tests under Miri. It still needs a successful Linux ASan/LSan run. The lane retains both sanitizer logs even on failure. It runs on schedule or explicit dispatch, not on every release SHA.

The proposed 24 CPU-hours per target remain unapproved and unexecuted.

On 2026-09-24, a fresh macOS arm64 run against clean tracked `main` at `141d604c` used the same installed nightly `rustc 1.100.0-nightly (e7769602a 2026-08-24)` and `MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance'`.

With `CARGO_BUILD_JOBS=2` and one reused, Miri-specific `CARGO_TARGET_DIR=target/pb07-miri`, serial runs passed:

- `cargo +nightly miri test --locked --offline -p pbrs --lib` (39/39)
- `cargo +nightly miri test --locked --offline -p pbrs --test runtime` (14/14)
- `cargo +nightly miri test --locked --offline --manifest-path rust_out_shared/Cargo.toml` (233/233 across 19 original shared suites)

Each run used `-- --test-threads=1`. This supersedes the older dirty-checkout Miri proof for these source files. It is still a local one-architecture run, not a same-SHA scheduled Miri CI artifact, a Linux ASan/LSan result, a 32-bit/big-endian proof, or approval for sustained fuzzing.

On 2026-09-27, a fresh macOS arm64 run against clean tracked `main` at `edd29d78` with installed nightly `miri 0.1.0 (e7769602ac 2026-08-24)` and `MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance'` passed:

- `cargo +nightly miri test --offline -p pbrs --lib` (43/43, including the PK-11 segmented-send unit tests)

The same limits apply: local one-architecture run, not CI proof.

On 2026-09-29 (PB-07), a macOS arm64 (`aarch64-apple-darwin`) run at base SHA `fd4500c6` with installed `cargo 1.100.0-nightly (e8cb624d5 2026-08-22)` / `miri 0.1.0 (e7769602ac 2026-08-24)` and `MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance'`, `CARGO_BUILD_JOBS=2`, isolated `CARGO_TARGET_DIR=target/pb07-miri`, `-- --test-threads=1`, passed:

- `cargo +nightly miri test --locked --offline -p pbrs --lib` (74/74, clean tree)
- `cargo +nightly miri test --locked --offline -p pbrs --test runtime` (26/26, including the 7 new PB-07 anchors: lazy-lifetime, fail-closed window, BE-fallback, zeroed-primitive, and `StringView` tests)

Same limits as all local runs: one architecture (64-bit LE macOS), not a same-SHA scheduled CI artifact, not a Linux ASan/LSan result, not a 32-bit/big-endian proof. The 24-CPU-hour-per-major-target sustained campaign remains unapproved and unexecuted.

PB-07 harness smoke (2026-09-29, not a campaign): the `wire` target built under cargo-fuzz 0.13.2 + nightly and, invoked directly, completed 1,455,131 execs in 61 s with 0 crashes and no new-unit promotion into the repo. `scripts/fuzz-campaign.sh` mishandled this smoke (a relative `--out` resolves the corpus dir against the wrong cwd, and one absolute-`--out` run exited silently after the build) — runner-script issues for QG-02, not target findings, and outside PB-07 write scope.

---

## 7. New Kernels and Engines Policy (QG-01)

The upcoming `pbrs-h2` crate (H2-04), `src/runtime/` kernel modules (UK-03/UK-04), and SIMD/table parse kernels (PK-04/PK-06) land under stricter rules than the historical code above because their `unsafe` has no production track record yet:

1. **`// SAFETY:` comments are required** on every new `unsafe` block, `unsafe fn`, and `unsafe trait` impl. Comments must follow the §1 bar: preconditions, maintained invariants, and why UB is impossible. Reviewers reject unsafe code without one, even if Miri is green.
2. **Pre-register invariants in this document.** The landing PR must add or extend a section naming each new unsafe site, its preconditions, and its layout/aliasing argument. Do this before or alongside the code, not after.
3. **PR-time Miri is blocking.** `.github/workflows/miri-kernels.yml` runs Miri on unit tests for every PR touching a kernel path: `pbrs-h2/**`, `src/runtime/**`, `src/table.rs`, `src/packed.rs`, `src/wire.rs`, and kernel fuzz targets. A red job blocks merge. Enforcement also requires marking the job as a required status check in repository settings; the workflow alone only reports it.
4. **Linux sanitizers stay scheduled.** The `compatibility.yml` `miri-sanitizers` lane runs weekly and on manual dispatch. It covers the new crates when they exist and remains the ASan/LSan proof. PRs are not gated on it because `-Zbuild-std` sanitizer builds are too slow for the PR path.
5. **New `unsafe` needs a second pair of eyes.** At least one reviewer other than the author must approve the `SAFETY` argument. This relates to PB-07, which qualifies the invariants on additional targets.

Until `pbrs-h2` exists, the PR-time job exercises in-crate kernel paths (`pbrs --lib`). The `pbrs-h2` steps activate automatically once the crate lands, using directory guards rather than hardcoded package lists.

---

## 8. PK-12 Arena Prototype (`bench/devloop/cells/arena.rs`, unregistered)

Bench-only prototype behind the PK-12 REJECT decision; not compiled into
any crate (unregistered until SB-08 wires `cells/`). Its small `unsafe`
core has local `// SAFETY:` proofs and Miri strict-provenance coverage
(10/10). Shared invariants, for audit if the prototype is ever revived:

- **Chunk access through `UnsafeCell`.** `BumpArena` owns a chunk list
  behind `UnsafeCell`; all mutation goes through `&self` methods that
  hand out disjoint slices. The arena is `!Sync` (no cross-thread
  sharing), so no aliasing across threads; within a thread, each
  allocation advances a bump cursor and never reuses live bytes.
- **Bump-pointer arithmetic.** `tail.base.add(start)` stays in-bounds:
  `start + len` is checked against the chunk capacity before the
  pointer is formed, and a fresh chunk is allocated otherwise. Chunk
  backing is `u128`-aligned storage (Miri caught an earlier align-1
  `[u8; 16]` backing), so all derived pointers meet payload alignment
  after `align_up`.
- **Capture-once pointers.** Base pointers are captured once in place
  and never re-derived across a `Box` move (Miri/Stacked-Borrows caught
  a repeated-`as_mut_ptr` pop); no pointer outlives its chunk, and
  `reset`/`drop` invalidates all outstanding borrows by lifetime (refs
  borrow the arena).
- **`from_utf8_unchecked_mut`.** Used only on bytes just validated as
  UTF-8 by the probe parser on the same slice, with no intervening
  mutation.

---

## 9. Target Qualification Matrix (PB-07)

Per-target status for the unsafe and target-dependent invariants in §§2–5, as of base SHA `fd4500c6` (2026-09-29). "Proof" means a named, re-runnable check; a tool limitation is never a safety proof, and every gap below is an explicit exclusion with a repro, never a fake pass.

| Target / tool | Status | Proof or exclusion |
|---|---|---|
| 64-bit LE host tests (`aarch64`/`x86_64`) | ✅ Qualified | `cargo test -p pbrs --test google_shared --test runtime` and `cargo test -p pbrs --test typed --test depth --test fuzz_parse` pass; `tests/runtime.rs` anchors cover arena adoption, lazy lifetimes, invalidation, LE/endian edges, zeroed primitives. |
| Miri strict provenance, 64-bit LE | ✅ Local proof, CI pending | 2026-09-29 local: lib 74/74 + runtime 26/26 (§6.3). Scheduled proof is the `miri-sanitizers` job in `.github/workflows/compatibility.yml` (weekly + manual dispatch); no same-SHA CI artifact exists yet. |
| ASan/LSan (Linux, `-Zbuild-std`) | ⏳ Scheduled-CI-only | Exact job: `compatibility.yml` → `miri-sanitizers` → step "Run AddressSanitizer and LeakSanitizer" (`RUSTFLAGS="-Zsanitizer=address"`, `cargo test -Zbuild-std`, logs → `compatibility-sanitizer-logs` artifact). Not runnable in this macOS worktree: `-Zbuild-std` sanitizer builds are Linux-only in practice and no Linux runner exists here. Repro when on Linux nightly: `RUSTFLAGS="-Zsanitizer=address" cargo test -p pbrs --lib --target x86_64-unknown-linux-gnu -Zbuild-std`. |
| 32-bit LE execution (`i686-unknown-linux-gnu`) | ⏳ Scheduled-CI-only | Exact job: `compatibility.yml` → `target-matrix` → step "Run lib and runtime tests on 32-bit x86". Not installed here (`rustup target list --installed` shows only `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`; toolchains are install-frozen, so this stays an exclusion until CI runs it). Repro: `rustup target add i686-unknown-linux-gnu && cargo test -p pbrs --lib --target i686-unknown-linux-gnu` (Linux runner also needs `gcc-multilib`). |
| Big-endian compile (`s390x-unknown-linux-gnu`) | ⏳ Scheduled-CI-only | Exact job: `compatibility.yml` → `target-matrix` → step "Compile-check big-endian target" (`cargo check -p pbrs --tests --target s390x-unknown-linux-gnu`). Proves the `#[cfg(target_endian = "big")]` branches compile; says nothing about runtime behavior. Same install-freeze exclusion locally. Repro: `rustup target add s390x-unknown-linux-gnu && cargo check -p pbrs --target s390x-unknown-linux-gnu`. |
| Big-endian execution | ❌ Excluded (no QEMU runner) | No BE hardware or QEMU user-mode runner is available locally or in CI, so the BE memcpy-fallback path never executes. Mitigation, not proof: `packed_fixed_matches_big_endian_fallback_algorithm` runs the fallback algorithm explicitly on LE hosts and requires bit-equality. Lifting this needs a `qemu-s390x`-based CI job. |
| Miri on 32-bit / big-endian | ❌ Excluded (upstream) | Miri only supports 64-bit LE targets; there is no Miri coverage for the i686/s390x lanes by construction. |
| MSan (`-Zsanitizer=memory`) | ❌ Excluded (Linux-only, uninstrumented deps) | MSan requires fully instrumented std+deps via `-Zbuild-std` on Linux and is not wired into any lane. No claim is made about uninitialized-memory detection beyond Miri. |
| Sustained fuzz (24 CPU-h/major target) | ❌ Unapproved, spend blocked | Recorded in §6.2 only. No campaign has run at this budget; starting one without recorded approval is blocked. |

---

## 10. Open Findings (PB-07)

Items found while qualifying at `fd4500c6`. All four were fixed by the coordinator on 2026-09-29 in the PB-07 follow-up commit (the table records the fix for audit continuity).

| ID | Location | Finding |
|---|---|---|
| PB07-F1 | `src/packed.rs` (fixed_codec `encode`) | FIXED 2026-09-29: `// SAFETY:` rationale added verbatim from §4.2. |
| PB07-F2 | `src/packed.rs` (fixed_codec `encode`) | FIXED 2026-09-29: `packed_byte_len` helper with `checked_mul`, fail-closed panic; unit tests for passthrough and overflow. |
| PB07-F3 | `src/runtime/reflect.rs` (`kernel_fieldkind_to_view`) | FIXED 2026-09-29: size-4 transmute fallback removed (proven unreachable — all 4-byte views have named arms; full `pbrs` suite green after removal). |
| PB07-F4 | `src/lazy.rs` (`Wire::from_slice`/`window`) | FIXED 2026-09-29: `u32_offset` helper with `checked_add` + `u32::try_from`, fail-closed panic; boundary unit tests. |

Resolved in PB-05/PB-06 (kept for audit continuity): wire `usize` overflow, codegen char-boundary slice, JSON duration truncate, timestamp day-0/month-13 arithmetic, text map-default `value:0` — see §6.2 history. No unresolved fuzz crashers remain: no `crash-*` files exist under `fuzz/corpus/` (seeds only), and the minimized findings are embedded as constants in `tests/fuzz_parse.rs`, which passes (1 test feeding 5 corpus inputs).
