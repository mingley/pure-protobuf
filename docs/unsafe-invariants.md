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

## 2. MiniTable & Runtime Invariants (`src/runtime.rs`)

`src/runtime.rs` implements the Google protobuf `__internal::runtime` upb kernel ABI in pure Rust. This lets generated code from `protoc --rust_out`, such as `rust_out_person`, link against `pbrs` without C or C++ dependencies.

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
    pub strs: Vec<Vec<u8>>,
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

`adopt_owned_msg<T>` transmutes `value: T` to read `OwnedMsgHead { raw: *mut MsgData, arena: Arena }`.

- **Precondition:** `size_of::<T>() >= size_of::<OwnedMsgHead>()`.
- **Action:** Fuse `head.arena` into the parent arena, record `head.raw` in `FieldKind::Msg`, and call `std::mem::forget(value)`.
- **Invariant:** The message's heap memory transfers ownership safely without invoking `T`'s destructor.

### 2.5 Unsafe Traits

Each unsafe trait has one required invariant:

| Trait | Required invariant |
|---|---|
| `unsafe trait AssociatedMiniTable` | `mini_table()` must return the valid static MiniTable linked for this type. |
| `unsafe trait UpbGetArena` | The returned `Arena` must outlive the message pointer. |
| `unsafe trait UpbGetMessagePtr` | The returned `MessagePtr` must remain valid for `'self`. |
| `unsafe trait UpbGetMessagePtrMut` | The returned `MessagePtr` must be valid for exclusive mutation for `'self`. |

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

`Wire` stores a shared byte frame and a checked window:

```rust
pub struct Wire {
    buf: Arc<[u8]>,
    start: u32,
    end: u32,
}
```

`wire.window(rel_start, rel_end)` must maintain:

```text
start + rel_end <= self.end <= buf.len()
```

The operation is zero-copy. It clones only the `Arc` pointer and updates offsets.

String parse strategy:

- Short strings (`<= 23` bytes) copy into inline `ProtoString` and do not allocate a parent `Wire`.
- Medium strings share the parent `Wire` frame through `Wire::ensure`.
- Long strings (`> 23` bytes) use `Wire::from_utf8_payload`, which allocates a payload-specific `Wire` while running SIMD UTF-8 verification. This avoids pinning unused parent frame bytes.

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

---

## 6. Sustained Fuzz Campaign Recommendations

Ongoing qualification should cover unsafe invariants, parser bounds, and target-dependent behavior.

### 6.1 Fuzz Targets & Scope

| Target | Scope | Invariants checked |
|---|---|---|
| `wire_parse` (`fuzz/fuzz_targets/wire.rs`) | Protobuf binary wire decoder, varint parsing, tag decoding, length-delimited spans, packed repeated fixed/varint scalars, recursion depth limit (100). | No panics, no out-of-bounds reads, no unaligned memory faults, deterministic round-trip for valid messages. |
| `descriptors` (`fuzz/fuzz_targets/descriptors.rs`) | Raw `FileDescriptorSet` binary decoding, unlinking, cyclic imports, extension resolution, edition handling. | Bounded recursion, acyclic traversal, safe failure on corrupt descriptor graphs. |
| `json_parse` (`fuzz/fuzz_targets/json.rs`) | Protobuf JSON parser, arbitrary-precision numbers, exponential formatting, Well-Known Type conversions. | IEEE-754 precision guarantees, non-crashing behavior on deeply nested JSON. |
| `text_parse` (`fuzz/fuzz_targets/text.rs`) | Text format parser, comments, string escapes, `Any` / `Struct` dynamic expansions. | Safe recovery on syntax errors, bounded allocation. |

### 6.2 Resource Allocation & Budget

- **Proposed initial sustained budget:** **24 CPU-hours per major target**, for **96 CPU-hours total** across the four major targets. Do not start this campaign without explicit compute approval and recorded run artifacts.
- **Continuous integration / cadence:**
  - Weekly Miri and ASan/LSan checks run in `compatibility.yml` when the scheduled or manual job completes successfully.
  - Sustained fuzzing is a separate, approval-gated campaign. It is not a claim that the weekly compatibility workflow already runs 96 CPU-hours.
  - Sanitizers use libFuzzer with `-Zsanitizer=address` and `-Zsanitizer=memory`.
- **Corpus management:**
  - Seed corpus comes from differential binary/JSON/text test fixtures in `tests/fixtures/differential/`.
  - Minimized corpus is committed under `fuzz/corpus/`.

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

---

## 7. New Kernels and Engines Policy (QG-01)

The upcoming `pbrs-h2` crate (H2-04), `src/runtime/` kernel modules (UK-03/UK-04), and SIMD/table parse kernels (PK-04/PK-06) land under stricter rules than the historical code above because their `unsafe` has no production track record yet:

1. **`// SAFETY:` comments are required** on every new `unsafe` block, `unsafe fn`, and `unsafe trait` impl. Comments must follow the §1 bar: preconditions, maintained invariants, and why UB is impossible. Reviewers reject unsafe code without one, even if Miri is green.
2. **Pre-register invariants in this document.** The landing PR must add or extend a section naming each new unsafe site, its preconditions, and its layout/aliasing argument. Do this before or alongside the code, not after.
3. **PR-time Miri is blocking.** `.github/workflows/miri-kernels.yml` runs Miri on unit tests for every PR touching a kernel path: `pbrs-h2/**`, `src/runtime/**`, `src/table.rs`, `src/packed.rs`, `src/wire.rs`, and kernel fuzz targets. A red job blocks merge. Enforcement also requires marking the job as a required status check in repository settings; the workflow alone only reports it.
4. **Linux sanitizers stay scheduled.** The `compatibility.yml` `miri-sanitizers` lane runs weekly and on manual dispatch. It covers the new crates when they exist and remains the ASan/LSan proof. PRs are not gated on it because `-Zbuild-std` sanitizer builds are too slow for the PR path.
5. **New `unsafe` needs a second pair of eyes.** At least one reviewer other than the author must approve the `SAFETY` argument. This relates to PB-07, which qualifies the invariants on additional targets.

Until `pbrs-h2` exists, the PR-time job exercises in-crate kernel paths (`pbrs --lib`). The `pbrs-h2` steps activate automatically once the crate lands, using directory guards rather than hardcoded package lists.
