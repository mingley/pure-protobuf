# Unsafe and Target-Dependent Invariants in pbrs

This document audits all `unsafe` blocks, target-dependent operations, memory models, and unsafe traits across `pbrs`. It details the safety invariants, preconditions, layout representations, and fuzzing requirements that ensure memory safety and correctness.

---

## 1. Safety Policy: Why `unsafe_code = "deny"` Instead of `forbid`

Pure-protobuf enforces `#![deny(unsafe_code)]` at the workspace level, supplemented by `#![deny(unsafe_op_in_unsafe_fn)]` in `src/lib.rs`.

`#![forbid(unsafe_code)]` is intentionally **not** used because `forbid` cannot be relaxed on specific audited modules or items. Achieving wire-speed protobuf parsing and Google `rust_out` ABI compatibility requires low-level unsafe primitives for:

1. **ABI Interoperability (`src/runtime.rs`)**:
   `protoc --rust_out` (Google protobuf v35.1, upb kernel ABI) expects C-compatible struct representations (`#[repr(C)]`), raw message pointers (`*mut MsgData`), and static MiniTable pointers (`MiniTablePtr`).
2. **Zero-Copy Byte/String Projections (`src/string.rs`)**:
   `ProtoStr` is a `#[repr(transparent)]` wrapper over `[u8]` allowing instant reinterpretation of wire byte slices without UTF-8 re-validation or heap allocation.
3. **Hardware-Accelerated Memcpy for Packed Fixed-Width Scalars (`src/packed.rs`)**:
   On little-endian architectures, fixed32/sfixed32/fixed64/sfixed64/float/double wire bytes match the native in-memory layout bit-for-bit, enabling direct `copy_nonoverlapping` memcpy operations.
4. **Fast Zeroed Message Initialization (`src/rt.rs`)**:
   Flat generated message layouts are designed such that an all-zero byte pattern represents a valid default message, allowing `std::mem::zeroed` initialization without per-field branching.

### Policy Enforcement Bar
- Every `unsafe` block or `unsafe fn` must be accompanied by an explicit `// SAFETY:` comment stating:
  - Exact preconditions required before entering the block.
  - Invariants maintained during execution.
  - Proof that undefined behavior (UB), out-of-bounds access, aliasing violations, or use-after-free are impossible.
- Code without a satisfactory `// SAFETY:` rationale is rejected at code review.
- Miri, AddressSanitizer (ASan), and LeakSanitizer (LSan) are scheduled
  qualification tools, not proofs of memory safety. A successful run and its
  exact toolchain/artifact are required before claiming their coverage.

---

## 2. MiniTable & Runtime Invariants (`src/runtime.rs`)

`src/runtime.rs` provides a pure-Rust implementation of the Google protobuf `__internal::runtime` upb kernel ABI, allowing generated code from `protoc --rust_out` (e.g., `rust_out_person`) to link against `pbrs` without C or C++ dependencies.

### 2.1 Struct Representations (`#[repr(C)]`)
- `MessagePtr<T>`:
  ```rust
  #[repr(C)]
  pub struct MessagePtr<T> {
      raw: *mut MsgData,
      _phantom: PhantomData<T>,
  }
  ```
  `MessagePtr<T>` is layout-compatible with a raw C pointer.
- `OwnedMessageInner<T>`:
  ```rust
  #[repr(C)]
  pub struct OwnedMessageInner<T> {
      ptr: MessagePtr<T>,
      arena: Arena,
  }
  ```
  Layout starts with `ptr` (8 bytes on 64-bit targets) followed by `arena` (`Rc<RefCell<ArenaInner>>`).
- `MessageMutInner<'msg, T>`:
  ```rust
  #[repr(C)]
  pub struct MessageMutInner<'msg, T> {
      pub ptr: MessagePtr<T>,
      pub arena: &'msg Arena,
  }
  ```
- `MessageViewInner<'msg, T>`:
  ```rust
  #[repr(C)]
  pub struct MessageViewInner<'msg, T> {
      ptr: MessagePtr<T>,
      _phantom: PhantomData<&'msg ()>,
  }
  ```

### 2.2 Pointer Offsets & Field Slots
- `MsgData`:
  ```rust
  pub struct MsgData {
      pub slots: Vec<FieldKind>,
      pub has: Vec<bool>,
      pub strs: Vec<Vec<u8>>,
      pub unknown: UnknownFields,
      pub mt: MiniTablePtr,
  }
  ```
- **Invariant**: `slots.len() == has.len()`.
- Accessor safety:
  - Getters (`get_i32_at_index`, etc.) query `self.slot(index)`. If `index >= slots.len()`, `FieldKind::Empty` is returned, preventing out-of-bounds reads.
  - Setters (`set_slot`) resize `slots` and `has` dynamically if `index >= slots.len()`.
  - Oneof exclusivity: Setting a field marked with `oneof_group != 0` iterates over all fields sharing that group in `MiniTable`, resetting their `has` flags to `false` and slots to `FieldKind::Empty`.

### 2.3 Arena Allocation & Fused Arenas
- `ArenaInner`:
  ```rust
  pub struct ArenaInner {
      msgs: Vec<Box<MsgData>>,
      arrays: Vec<Box<RawArrayInner>>,
      maps: Vec<Box<RawMapInner>>,
      bytes: Vec<Box<Vec<u8>>>,
  }
  ```
- **Pointer Stability Invariant**:
  All messages, arrays, and maps are wrapped in `Box<_>`. When `ArenaInner.msgs` reallocates or grows, the addresses of the underlying `MsgData` instances on the heap remain strictly pinned. Any raw pointer stored in `MessagePtr<T>` remains valid.
- **String Header Stability**:
  `MsgData.strs` and raw map/repeated string stores hold boxed `Vec<u8>`
  headers, not movable headers in `Vec<Vec<u8>>`. `FieldKind::Bytes` pointers
  therefore remain valid after later field insertions or collection growth.
  Bytes produced by raw parsing or cloning live in `ArenaInner.bytes` and
  move with the arena on fusion. `InnerProtoString::into_raw_parts` returns
  that owning arena with its `StringView`, rather than leaking the buffer.
- **Pointer Provenance Invariant**:
  `Arena::alloc_msg`, `alloc_array`, and `alloc_map` derive raw pointers
  **after** inserting their `Box` into the owning `ArenaInner` vector.
  Taking a reference and raw pointer before moving the `Box` into that
  vector preserves the address but invalidates its Stacked Borrows
  permission on insertion. Miri detected this in the original
  `map_and_repeated_message_arena_adoption_fusion` test. Deriving the pointer
  from the stored owner fixes the invalid retag; the regression also grows
  both arena vectors and reads the pointer after fusion.
- **Arena Fusion (`Arena::fuse`)**:
  When a child message is assigned to a parent message (`message_set_sub_message`) or elements are inserted into repeated arrays/maps (`message_set_repeated_field`, `message_set_map_field`):
  ```rust
  parent.arena.fuse(child.get_arena(Private));
  ```
  `fuse` moves all `Box<MsgData>`, `Box<RawArrayInner>`, and `Box<RawMapInner>` from the child's arena into the parent's arena via `Vec::append`.
  - **No Use-After-Free**: If the original child handle drops, its arena has already transferred heap ownership to the parent. The child data remains alive as long as the parent message lives.
  - **No Memory Leaks**: When the parent message drops, the parent's arena deallocates all child nodes transitively.
  - **Idempotency**: If `Rc::ptr_eq(&self.inner, &other.inner)`, `fuse` is a no-op, avoiding self-append corruption.
- **Raw collection adoption**: Each arena-owned repeated/map collection holds
  a `Weak` link to its owner. A raw mutator that receives an owned submessage
  without an explicit parent upgrades that link and fuses the child's
  allocations; it never leaks a boxed `Arena`. On fusion, moved collections'
  weak links are updated to the new owner, without creating an `Rc` cycle.
  Pointers into the boxed message, array and map allocations remain valid.

### 2.4 Message Adoption (`adopt_owned_msg`)
`adopt_owned_msg<T>` transmutes `value: T` to read `OwnedMsgHead { raw: *mut MsgData, arena: Arena }`:
- Precondition: `size_of::<T>() >= size_of::<OwnedMsgHead>()`.
- Action: Fuses `head.arena` into the parent arena, records `head.raw` into `FieldKind::Msg`, and calls `std::mem::forget(value)`.
- Invariant: Ownership of the message's heap memory is safely transferred without invoking `T`'s destructor.

### 2.5 Unsafe Traits
- `unsafe trait AssociatedMiniTable`: `mini_table()` must return the valid static MiniTable linked for this type.
- `unsafe trait UpbGetArena`: The returned `Arena` must outlive the message pointer.
- `unsafe trait UpbGetMessagePtr`: The returned `MessagePtr` must remain valid for `'self`.
- `unsafe trait UpbGetMessagePtrMut`: The returned `MessagePtr` must be valid for exclusive mutation for `'self`.

---

## 3. String & Bytes Invariants (`src/string.rs`)

### 3.1 `ProtoStr` Layout and UTF-8 Invariants
```rust
#[repr(transparent)]
pub struct ProtoStr([u8]);
```
- **Representation**: `#[repr(transparent)]` guarantees `ProtoStr` has identical memory layout, size, alignment, and ABI to `[u8]`.
- **Casting Safety**:
  `ProtoStr::from_bytes(bytes: &[u8]) -> &ProtoStr` performs:
  ```rust
  unsafe { &*(bytes as *const [u8] as *const ProtoStr) }
  ```
  This is sound because `ProtoStr` has no invalid bit patterns and imposes no validity invariants on the underlying byte slice.
- **Why `[u8]` Instead of `str`**:
  Protobuf specifications require:
  - Proto3: string fields must be verified UTF-8 on the wire.
  - Proto2 & Edition 2023 (`features.(pb.cpp).string_type = VIEW` / `utf8_validation = NONE`): string fields may contain arbitrary binary sequences (e.g., non-UTF-8 bytes like `0xFF` or `0x80`).
  `ProtoStr` accommodates both: proto3 parser validates UTF-8 eagerly at the wire boundary via `simdutf8::basic::from_utf8`, while proto2 permits arbitrary bytes.
- **On-Demand Validation**:
  `to_str(&self) -> Result<&str, Utf8Error>` validates UTF-8 via `std::str::from_utf8` on demand.

### 3.2 `ProtoString` Small-String Optimization (SSO)
```rust
const INLINE_CAP: usize = 23;

#[derive(Clone)]
enum Repr {
    Inline { len: u8, data: [u8; INLINE_CAP] },
    Heap(Vec<u8>),
}
```
- **SSO Invariant**:
  - For strings `<= 23` bytes, bytes are stored inline in `data[..len]`. No heap allocation occurs.
  - `Inline` size: 1 byte (`len`) + 23 bytes (`data`) = 24 bytes, matching `Vec<u8>` on 64-bit systems.
  - For strings `> 23` bytes, storage is elevated to `Repr::Heap(Vec<u8>)`.
  - `clear()` resets to `Repr::Inline { len: 0, data: [0; 23] }`.
  - Complete memory safety is enforced by standard Rust enum invariants (no unsafe untagged unions).

---

## 4. Lazy Parsing & Packed Repeated Scalars (`src/lazy.rs`, `src/packed.rs`)

### 4.1 Wire Buffer Slicing (`src/lazy.rs`)
- `Wire`:
  ```rust
  pub struct Wire {
      buf: Arc<[u8]>,
      start: u32,
      end: u32,
  }
  ```
- Slicing via `wire.window(rel_start, rel_end)`:
  - Invariant: `start + rel_end <= self.end <= buf.len()`.
  - Zero-copy: Clones only the `Arc` pointer and updates offsets.
- Parse strategy:
  - Short strings (`<= 23` bytes): copied into inline `ProtoString` without allocating a parent `Wire`.
  - Medium strings: share the parent `Wire` frame via `Wire::ensure`.
  - Long strings (`> 23` bytes): `Wire::from_utf8_payload` allocates a payload-specific `Wire` while running SIMD UTF-8 verification, avoiding pinning unused parent frame bytes.

### 4.1.1 Arena-backed map and repeated views

Raw map keys are borrowed from the arena-owned entry for the immutable view's
lifetime, not cloned into permanently leaked allocations during every
iteration. Values inserted into raw map/repeated collections own stable
`Box<Vec<u8>>` headers. Overwritten and removed values are released, and
`clear()` and arena drop reclaim the rest. Values parsed or cloned through raw
message paths are owned by the arena instead of leaked. Allocation pointers
are derived only after the boxes enter their owners. Returning a view while
unsafely mutating that same raw collection would violate the view's exclusive
borrow contract; safe generated APIs prevent it. Arena-owned parsed and cloned
bytes are reclaimed when the arena drops, not necessarily when an individual
field is cleared; count that retention in application memory budgets.

### 4.2 Packed Fixed-Width Scalars & Memcpy Preconditions (`src/packed.rs`)
Fixed-width scalar types (`fixed32`, `sfixed32`, `fixed64`, `sfixed64`, `float`, `double`) implement `PackedCodec` with `MEMCPY_SAFE = true`.

#### Preconditions for Memcpy:
1. **Length Divisibility**:
   `buf.len() % width == 0`. Truncated payloads are rejected with `ParseError` prior to any pointer operations.
2. **Target Endianness**:
   Protobuf wire specification mandates **Little-Endian (LE)** integer encoding.
   - On `#[cfg(target_endian = "little")]`: Memory representation matches wire representation bit-for-bit.
   - On `#[cfg(target_endian = "big")]`: Memcpy is disabled. The decoder iterates over `buf.chunks_exact(width)` calling `from_le_bytes`, and the encoder emits `to_le_bytes()`.
3. **Plain Old Data (POD) / Validity Invariant**:
   `u32`, `i32`, `u64`, `i64`, `f32`, and `f64` have no invalid bit patterns. All $2^{32}$ and $2^{64}$ bit states are valid, including floating-point NaNs, subnormals, signed zeros, and infinities.
4. **Aliasing & Capacity**:
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
   - `out.reserve(n)` guarantees allocated capacity for `n` additional elements.
   - `buf` (input slice) and `dest` (new uninitialized capacity in `out`) cannot alias or overlap.
   - `copy_nonoverlapping` writes `buf.len()` bytes into `dest`.
   - `out.set_len(start + n)` is called only after the memory has been fully initialized.
5. **Unaligned Source Pointers**:
   `buf.as_ptr()` may originate from an unaligned offset in a network packet. `std::ptr::copy_nonoverlapping` operates on `*const u8` and `*mut u8`, which require an alignment of 1 byte. Hence, unaligned source buffers cannot trigger alignment faults or UB.

### 4.3 Packed Varints: Canonical Re-encoding
Unlike fixed-width types, packed varint fields (`int32`, `int64`, `uint32`, `uint64`, `sint32`, `sint64`, `bool`) have `MEMCPY_SAFE = false`:
- **Wire Non-Canonicity**: Protobuf decoders must accept overlong varints (e.g., `0x81 0x00` for `1`).
- **Canonical Output**: Protobuf encoders must emit minimal canonical varints. Direct memcpy would propagate non-canonical representations and violate conformance.
- Therefore, varints are parsed and decoded into vector elements, and re-encoded via `encode_varint` or `encode_zigzag32`/`encode_zigzag64`.

### 4.4 Zeroed Message Initialization (`src/rt.rs`)
```rust
pub unsafe fn zeroed_message<T>() -> T {
    unsafe { std::mem::zeroed() }
}
```
- **Safety Invariant**:
  `T` must be a generated message struct whose fields are all valid when represented as all-zero bits (`0u8`).
- **Verification Across Types**:
  - Scalars (`i32`, `f64`, etc.): All zero bits represent numeric zero (`0`, `0.0`).
  - Standard bool: In Rust, `false` is `0u8`.
  - Optional bool (`OptBool`): Standard Rust `Option<bool>` uses niche optimization where `None = 2`, `Some(false) = 0`. An all-zero bit pattern would erroneously represent `Some(false)`. Pure-protobuf avoids this using `OptBool(u8)` where `0 = None`, `1 = Some(false)`, `2 = Some(true)`.
  - Pointers and Boxes (`Option<Box<T>>`): In Rust, `None` for nullable pointers is represented as all zero bits (null pointer).
  - Collections (`Repeated<T>`, `Map<K, V>`, `Packed<C>`, `LazyMsg<T>`): Built on `Option<Box<_>>` with null pointer representing empty state.
  - `CachedSize(AtomicU64)`: Initialized to `0`, which is the valid serialized length of an empty message.

---

## 5. Invalidation Safety (`CachedSize` & Dirty Tracking)

### 5.1 Atomic Size Caching (`src/rt.rs`)
```rust
pub struct CachedSize(AtomicU64);
```
- `DIRTY = u64::MAX`.
- `get()` returns `None` if value is `DIRTY`, or `Some(n)` if a cached length is present.
- `set(n)` stores `n` with `Ordering::Relaxed`.
- `dirty()` stores `DIRTY` with `Ordering::Relaxed`.
- `CachedSize` implements `PartialEq` by always returning `true`, ensuring message equality comparisons are based purely on semantic field content.

### 5.2 Dirty Invalidation Invariants
Every mutation in generated messages must invalidate cached sizes:
1. **Scalar Setters**: Calling `set_field(val)` calls `self.cached_size.dirty()`.
2. **Collection Mutators**: `field_mut()` dirties `self.cached_size` before returning `RepeatedMut` or `MapMut`.
3. **Clearing**: `clear_field()` calls `self.cached_size.dirty()`.
4. **Lazy Objects**:
   - `Packed<C>`: Calling `force_vec()` clears `inner.encoded = OnceLock::new()`, forcing re-encoding.
   - `LazyMsg<T>`: Calling `get_or_insert()` sets `inner.wire = None`, ensuring subsequent serialization encodes the updated message struct rather than stale wire bytes.

---

## 6. Sustained Fuzz Campaign Recommendations

To ensure ongoing qualification of unsafe invariants, parser bounds, and target-dependent behaviors, the following continuous fuzzing schedule is recommended.

### 6.1 Fuzz Targets & Scope
1. **`wire_parse` (`fuzz/fuzz_targets/wire.rs`)**:
   - Target: Protobuf binary wire decoder, varint parsing, tag decoding, length-delimited spans, packed repeated fixed/varint scalars, recursion depth limit (100).
   - Invariants checked: No panics, no out-of-bounds reads, no unaligned memory faults, deterministic round-trip for valid messages.
2. **`descriptors` (`fuzz/fuzz_targets/descriptors.rs`)**:
   - Target: Raw `FileDescriptorSet` binary decoding, unlinking, cyclic imports, extension resolution, edition handling.
   - Invariants checked: Bounded recursion, acyclic traversal, safe failure on corrupt descriptor graphs.
3. **`json_parse` (`fuzz/fuzz_targets/json.rs`)**:
   - Target: Protobuf JSON parser, arbitrary-precision numbers, exponential formatting, WKT conversions.
   - Invariants checked: IEEE-754 precision guarantees, non-crashing on deep nested JSON.
4. **`text_parse` (`fuzz/fuzz_targets/text.rs`)**:
   - Target: Text format parser, comments, string escapes, Any/Struct dynamic expansions.
   - Invariants checked: Safe recovery on syntax errors, bounded allocation.

### 6.2 Resource Allocation & Budget
- **Proposed initial sustained budget**: **24 CPU-hours per major target**
  (total 96 CPU-hours across the 4 major targets). Do not start this campaign
  without explicit compute approval and recorded run artifacts.
- **Continuous Integration / Cadence**:
  - Weekly Miri and ASan/LSan checks in `compatibility.yml`, when the
    scheduled or manual job actually completes successfully.
  - Sustained fuzzing is a separate, approval-gated campaign, not a claim
    that the weekly compatibility workflow already runs 96 CPU-hours.
  - Sanitizers: libFuzzer with `-Zsanitizer=address` and `-Zsanitizer=memory`.
- **Corpus Management**:
  - Seed corpus generated from differential binary/JSON/text test fixtures (`tests/fixtures/differential/`).
  - Minimized corpus committed under `fuzz/corpus/`.

### 6.3 Dated local proof and remaining limits

On 2026-09-23, against the **dirty** local checkout at `cd9d7bd8`, macOS arm64
nightly `rustc 1.100.0-nightly (e7769602a 2026-08-24)` with
`MIRIFLAGS='-Zmiri-disable-isolation -Zmiri-strict-provenance'` passed
`cargo +nightly miri test --offline -p pbrs --lib` (38/38),
`cargo +nightly miri test --offline -p pbrs --test runtime` (14/14), and
`cargo +nightly miri test --manifest-path rust_out_shared/Cargo.toml --offline`
(233/233 across 19 original Google shared suites). These cover pointer use
after arena growth/fusion, raw map iteration and repeated/map ownership,
including the previously leaking paths. This is local
evidence for the current worktree, **not** a CI artifact for the committed
SHA or a big-endian/32-bit proof. A separate local macOS arm64
`RUSTFLAGS='-Zsanitizer=address'` run with nightly `-Zbuild-std` passed the
same 38 core and 14 runtime tests; `ASAN_OPTIONS` requested leak detection,
but LeakSanitizer support on this host was not independently verified. The
repaired `compatibility.yml` scheduled lane now runs the core, runtime and 72
targeted original shared tests under Miri and still needs a successful
Linux ASan/LSan run. That lane retains both sanitizer logs even on failure;
it runs on schedule or explicit dispatch, not on every release SHA.
The proposed 24 CPU-hours per target remain unapproved and unexecuted.
