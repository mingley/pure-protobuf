//! PK-12 prototype: opt-in per-request bump arena for owned messages.
//!
//! Status: experiment only. This module is intentionally self-contained
//! (`std` only, no `pbrs` dependency) so it can be measured standalone:
//!
//! ```sh
//! rustc --edition 2021 --test bench/devloop/cells/arena.rs -o /tmp/arena_test
//! /tmp/arena_test
//! ```
//!
//! # What this is
//!
//! A bump arena (`BumpArena`) that hands out message field storage
//! (`bytes`, `str`, `Copy` slices) with no per-value frees: dropping or
//! resetting the arena releases everything at once. The [`probe`] module
//! models one per-RPC owned parse (TAT-like payload) twice — once with the
//! global allocator ([`probe::OwnedMsg`]) and once with the arena
//! ([`probe::ArenaMsg`]) — so allocation counts, parse time, and teardown
//! cost can be compared apples-to-apples.
//!
//! # Safety model (why the owned API cannot dangle)
//!
//! * Every handed-out reference borrows the arena (`&'a BumpArena ->
//!   &'a mut T`). The borrow checker therefore forbids `reset(&mut self)`
//!   and `drop(arena)` while any view into the arena is live.
//! * The arena never reads or writes handed-out ranges after allocation,
//!   so each `&mut` is unique despite the shared `&self` allocator API.
//! * Only `Copy` payloads (bytes, `str`, `Copy` scalars/slices) are
//!   accepted. `reset`/drop runs no per-value destructors, so there is no
//!   drop glue to forget and nothing leaks semantically.
//! * The single-threaded interior uses `UnsafeCell` (`!Sync`); the arena
//!   may move across threads between requests (`Send`) but is never shared.
//!
//! # Prototype limits (do NOT ship as-is)
//!
//! * Maximum supported alignment is 16 (covers all `Copy` scalars through
//!   `u128`; declares, not silently rounds, anything wider).
//! * `reset` retains the first chunk unconditionally (no retention cap).
//! * No `Send + Sync` pool, no thread-local caching, no size-class tuning.
//!
//! # Follow-up wiring (coordinator; outside PK-12 write scope)
//!
//! This file is not referenced by `bench/devloop/src/main.rs` yet: wiring
//! it in requires editing the cell registry (`mod` declaration plus
//! `codec_cells()` entries) and possibly `bench/devloop/Cargo.toml`, all of
//! which PK-12 must not touch. The intended cells, measured standalone for
//! the PK-12 decision record in `docs/decisions/owned-arena.md`, are:
//!
//! * `codec.arena.owned_decode` — [`probe::parse_owned`] + touch + drop.
//! * `codec.arena.arena_fresh_decode` — fresh [`BumpArena`] per op.
//! * `codec.arena.arena_reuse_decode` — one arena, parse + touch + reset.
//! * `codec.arena.owned_teardown` / `codec.arena.arena_teardown` — drop cost.
//!
//! If the prototype grows `unsafe` beyond this file, the QG-01 policy in
//! `docs/unsafe-invariants.md` requires pre-registering the invariants
//! there; that document is outside PK-12 write scope and is reported as a
//! coordinator follow-up instead.

use std::alloc::Layout;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::ptr;
use std::slice;

/// Backing block size and guaranteed base alignment of every chunk, in
/// bytes. Chunks are boxed slices of `MaybeUninit<u128>` (`u128` has size
/// 16 *and* alignment 16), so the allocator hands us a 16-aligned base
/// address by construction — the layout's own alignment — and every bumped
/// offset is rounded up from there. (A `[u8; 16]` backing would *not*
/// suffice: byte arrays carry alignment 1. Miri caught that exact bug in
/// this prototype's first revision.)
const BLOCK: usize = 16;

/// Default chunk size for [`BumpArena::new`]: 8 KiB, enough for several
/// small per-RPC messages per chunk while keeping a cold request's
/// footprint modest.
const DEFAULT_CHUNK_BYTES: usize = 8 * 1024;

/// One pinned backing allocation plus its bump offset.
///
/// The boxed slice never moves after creation, so raw pointers previously
/// handed out of the chunk stay valid when the arena's chunk list grows.
/// Crucially for Stacked Borrows, `_own` is *never borrowed again* after
/// creation: every `as_mut_ptr()` call would push a transient `&mut` over
/// the whole chunk and pop previously handed-out pointers. All access goes
/// through the `base` raw pointer captured once at creation, plus raw
/// arithmetic only.
struct Chunk {
    /// Ownership of the backing allocation; drop frees the chunk.
    /// Never borrowed after the chunk is built (see above).
    _own: Box<[MaybeUninit<u128>]>,
    /// Chunk base as bytes, captured once at creation.
    base: *mut u8,
    /// Byte capacity, cached so stats never touch `_own`.
    cap: usize,
    used: usize,
}

/// Round `x` up to a multiple of `align` (which must be a power of two).
fn align_up(x: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    x.checked_add(align - 1)
        .map(|v| v & !(align - 1))
        .expect("PK-12 arena: bump offset overflow")
}

/// Opt-in per-request bump arena: no individual frees, drop/reset frees all.
///
/// See the module docs for the safety model and prototype limits.
pub struct BumpArena {
    chunks: UnsafeCell<Vec<Chunk>>,
    default_chunk: usize,
}

// `UnsafeCell` makes this `!Sync` (single-threaded use only), while `Send`
// holds: moving the arena to another thread between requests is sound
// because no shared access can exist.

impl BumpArena {
    /// New arena with the default 8 KiB chunk size. Allocates nothing until
    /// the first non-empty allocation.
    pub fn new() -> Self {
        Self::with_chunk_bytes(DEFAULT_CHUNK_BYTES)
    }

    /// New arena whose fresh chunks hold at least `chunk_bytes` bytes.
    /// Oversized single allocations still succeed: the chunk is grown to
    /// fit, so this is a floor, not a ceiling.
    pub fn with_chunk_bytes(chunk_bytes: usize) -> Self {
        BumpArena {
            chunks: UnsafeCell::new(Vec::new()),
            default_chunk: chunk_bytes.max(BLOCK),
        }
    }

    /// Reserve `layout.size()` bytes aligned for `layout` and return a
    /// pointer to the fresh region.
    ///
    /// The region is uninitialized, disjoint from every previously handed
    /// out region, and valid until `reset` or drop.
    fn alloc_raw(&self, layout: Layout) -> *mut u8 {
        assert!(
            layout.align() <= BLOCK,
            "PK-12 arena prototype supports alignment <= 16"
        );
        if layout.size() == 0 {
            // Zero-sized reservation: no memory touched. `without_provenance`
            // documents that this address must never be dereferenced.
            return ptr::without_provenance_mut(layout.align());
        }
        // SAFETY: `BumpArena` is `!Sync` (it contains `UnsafeCell`), so no
        // other thread can touch `chunks` while this runs. No user callback
        // executes between this borrow and the end of `alloc_raw`, so no
        // re-entrant access can observe the temporary `&mut`. The reference
        // never escapes; only disjoint freshly-reserved byte ranges escape
        // as raw pointers, each converted to a caller-owned `&mut` once.
        let chunks = unsafe { &mut *self.chunks.get() };
        if let Some(tail) = chunks.last_mut() {
            let start = align_up(tail.used, layout.align());
            if let Some(end) = start.checked_add(layout.size()) {
                if end <= tail.cap {
                    tail.used = end;
                    // SAFETY: `start..end` lies inside the live chunk and was
                    // just marked used, so it is disjoint from all prior
                    // reservations; `start` is rounded to `layout.align`
                    // from a 16-aligned base with `layout.align() <= 16`.
                    // Raw arithmetic on the stored `base` creates no
                    // intermediate reference, so outstanding pointers keep
                    // their borrow-stack entries (Stacked Borrows).
                    return unsafe { tail.base.add(start) };
                }
            }
        }
        Self::alloc_fresh_chunk(chunks, self.default_chunk, layout)
    }

    /// Push a new chunk sized for `layout` and bump from it. `default_chunk`
    /// is passed by value so this helper needs no access to `self` while the
    /// caller holds the chunks borrow.
    fn alloc_fresh_chunk(chunks: &mut Vec<Chunk>, default_chunk: usize, layout: Layout) -> *mut u8 {
        let need = layout
            .size()
            .checked_add(layout.align())
            .expect("PK-12 arena: request too large");
        let cap_bytes = default_chunk.max(need).max(BLOCK);
        let blocks = cap_bytes.div_ceil(BLOCK);
        let mut mem: Vec<MaybeUninit<u128>> = Vec::with_capacity(blocks);
        // SAFETY: `MaybeUninit<u128>` needs no initialization, so extending
        // the length over `with_capacity` reservation is sound; every
        // element is read only after a later `write`/`copy` through a
        // handed-out pointer covers it. The `u128` element type is what
        // guarantees the chunk base is 16-aligned (see `BLOCK`).
        unsafe {
            mem.set_len(blocks);
        }
        let own: Box<[MaybeUninit<u128>]> = mem.into_boxed_slice();
        // Move the box into place FIRST: moving a `Box` retags its contents
        // (Unique), which would pop a base pointer captured beforehand.
        // Later `Vec` growth moves chunks by raw copy, which preserves tags.
        chunks.push(Chunk {
            _own: own,
            base: ptr::null_mut(),
            cap: blocks * BLOCK,
            used: 0,
        });
        let tail = chunks.last_mut().expect("chunk just pushed");
        // The single borrow of this chunk's backing ever: capture the base
        // from the in-place box; everything afterwards uses the stored raw
        // pointer (see the `Chunk` docs for why reborrowing would pop
        // outstanding pointers). The null placeholder above is overwritten
        // here before any possible use, with no fallible step between.
        tail.base = tail._own.as_mut_ptr() as *mut u8;
        let start = align_up(0, layout.align());
        let end = start + layout.size();
        debug_assert!(end <= tail.cap);
        tail.used = end;
        // SAFETY: same argument as the fast path above; the chunk is fresh,
        // so `start..end` is trivially disjoint.
        unsafe { tail.base.add(start) }
    }

    /// Copy `val` into the arena and return a mutable reference to it.
    ///
    /// `T: Copy` is load-bearing: the arena runs no destructors on
    /// `reset`/drop, which is only sound when values need none.
    #[allow(
        clippy::mut_from_ref,
        reason = "deliberate bump-allocator interior mutability: each &mut covers a disjoint freshly-reserved region (bumpalo pattern), unique by construction"
    )]
    pub fn alloc_copy<T: Copy>(&self, val: T) -> &mut T {
        let ptr = self.alloc_raw(Layout::new::<T>()) as *mut T;
        // SAFETY: `ptr` covers `size_of::<T>()` fresh, correctly aligned
        // bytes in a live chunk (see `alloc_raw`). The region has no other
        // references and the arena never touches handed-out regions again,
        // so `write` followed by a unique reborrow tied to `&self` is sound.
        unsafe {
            ptr.write(val);
            &mut *ptr
        }
    }

    /// Copy `src` into the arena and return it as mutable bytes.
    #[allow(
        clippy::mut_from_ref,
        reason = "deliberate bump-allocator interior mutability: each &mut covers a disjoint freshly-reserved region (bumpalo pattern), unique by construction"
    )]
    pub fn alloc_bytes(&self, src: &[u8]) -> &mut [u8] {
        if src.is_empty() {
            return &mut [];
        }
        let layout = Layout::array::<u8>(src.len()).expect("slice layout");
        let ptr = self.alloc_raw(layout);
        // SAFETY: `ptr` covers `src.len()` fresh bytes (see `alloc_raw`);
        // `src` and the destination cannot overlap (fresh region), so the
        // copy fully initializes exactly the reborrowed slice.
        unsafe {
            ptr.copy_from_nonoverlapping(src.as_ptr(), src.len());
            slice::from_raw_parts_mut(ptr, src.len())
        }
    }

    /// Copy `s` into the arena and return it as mutable `str`.
    #[allow(
        clippy::mut_from_ref,
        reason = "deliberate bump-allocator interior mutability: each &mut covers a disjoint freshly-reserved region (bumpalo pattern), unique by construction"
    )]
    pub fn alloc_str(&self, s: &str) -> &mut str {
        let bytes = self.alloc_bytes(s.as_bytes());
        // SAFETY: `bytes` is an exact copy of a valid `&str`, hence valid
        // UTF-8; the reborrowed region is fresh and uniquely owned.
        unsafe { std::str::from_utf8_unchecked_mut(bytes) }
    }

    /// Copy `src` into the arena and return it as a mutable slice.
    #[allow(
        clippy::mut_from_ref,
        reason = "deliberate bump-allocator interior mutability: each &mut covers a disjoint freshly-reserved region (bumpalo pattern), unique by construction"
    )]
    pub fn alloc_slice<T: Copy>(&self, src: &[T]) -> &mut [T] {
        if src.is_empty() {
            return &mut [];
        }
        let layout = Layout::array::<T>(src.len()).expect("slice layout");
        let ptr = self.alloc_raw(layout) as *mut T;
        // SAFETY: `ptr` covers `src.len()` fresh, correctly aligned `T`
        // slots (see `alloc_raw`); the source cannot overlap the fresh
        // region, so the copy fully initializes exactly the reborrowed
        // slice. `T: Copy` needs no drop glue at `reset`/drop.
        unsafe {
            ptr.copy_from_nonoverlapping(src.as_ptr(), src.len());
            slice::from_raw_parts_mut(ptr, src.len())
        }
    }

    /// Number of live chunks (1 after steady-state reuse; grows while a
    /// request's footprint exceeds one chunk).
    pub fn chunk_count(&self) -> usize {
        // SAFETY: shared read of the chunk list. `BumpArena` is `!Sync` and
        // every mutation runs in a short critical section with no user
        // callbacks, so no mutation can be live across this call on this
        // thread; the returned `usize` is a copy, not a borrow.
        unsafe { (*self.chunks.get()).len() }
    }

    /// Total backing bytes currently reserved from the global allocator.
    pub fn reserved_bytes(&self) -> usize {
        // SAFETY: same shared-read argument as `chunk_count`. Reads only
        // the cached `cap` field, never the chunk backing (see `Chunk`).
        unsafe { (*self.chunks.get()).iter().map(|c| c.cap).sum() }
    }

    /// Bump bytes currently handed out across all chunks.
    pub fn used_bytes(&self) -> usize {
        // SAFETY: same shared-read argument as `chunk_count`.
        unsafe { (*self.chunks.get()).iter().map(|c| c.used).sum() }
    }

    /// Release everything allocated so far, keeping the first chunk for
    /// reuse. Takes `&mut self`, so the borrow checker guarantees no arena
    /// reference is live: reset can never strand a view.
    ///
    /// Prototype policy: the first chunk is retained unconditionally (no
    /// retention cap); production use needs a cap or shrink heuristic.
    pub fn reset(&mut self) {
        let chunks = self.chunks.get_mut();
        if let Some(first) = chunks.first_mut() {
            first.used = 0;
        }
        chunks.truncate(1);
    }
}

impl Default for BumpArena {
    fn default() -> Self {
        Self::new()
    }
}

/// Growable `Copy`-only buffer backed by a [`BumpArena`]: `push` doubles in
/// the arena (abandoned prefixes are reclaimed at `reset`, never freed
/// individually), so single-pass parsing needs no global-allocator scratch.
pub struct ArenaVec<'a, T: Copy> {
    arena: &'a BumpArena,
    ptr: *mut T,
    len: usize,
    cap: usize,
}

impl<'a, T: Copy> ArenaVec<'a, T> {
    /// Empty vector borrowing `arena` for all future growth.
    pub fn new(arena: &'a BumpArena) -> Self {
        ArenaVec {
            arena,
            ptr: ptr::without_provenance_mut(Layout::new::<T>().align()),
            len: 0,
            cap: 0,
        }
    }

    /// Number of elements pushed so far.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when no elements have been pushed.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Append `val`, growing in-arena when full.
    pub fn push(&mut self, val: T) {
        if std::mem::size_of::<T>() == 0 {
            // Zero-sized elements occupy no memory; only the length matters.
            // `without_provenance` in `new` is never dereferenced on this
            // path, and `into_slice` uses a proper dangling pointer.
            self.len = self.len.wrapping_add(1);
            return;
        }
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 4 } else { self.cap * 2 };
            let layout = Layout::array::<T>(new_cap).expect("arena vec capacity");
            let fresh = self.arena.alloc_raw(layout) as *mut T;
            if self.len > 0 {
                // SAFETY: `self.ptr[..self.len]` holds initialized `T`s from
                // prior pushes (or the prior growth copy); `fresh` is a
                // disjoint fresh region of `new_cap > self.len` slots. After
                // the copy the old region is abandoned: no other pointer to
                // it exists, and the arena reclaims it wholesale at reset.
                unsafe {
                    fresh.copy_from_nonoverlapping(self.ptr, self.len);
                }
            }
            self.ptr = fresh;
            self.cap = new_cap;
        }
        // SAFETY: `self.len < self.cap`, so slot `self.len` is a fresh,
        // correctly aligned, initialized-by-`write` `T` slot in the live
        // region; no other reference to it exists.
        unsafe {
            self.ptr.add(self.len).write(val);
        }
        self.len += 1;
    }

    /// Freeze into the pushed slice. Consumes the vector, so no further
    /// push can invalidate the returned borrow.
    pub fn into_slice(self) -> &'a mut [T] {
        if self.len == 0 {
            return &mut [];
        }
        if std::mem::size_of::<T>() == 0 {
            // SAFETY: ZST slice from a proper dangling pointer with the
            // pushed length; no memory is accessed through it.
            return unsafe {
                slice::from_raw_parts_mut(ptr::NonNull::dangling().as_ptr(), self.len)
            };
        }
        // SAFETY: `self.ptr[..self.len]` holds `self.len` initialized `T`s
        // in arena memory live for `'a`; `self` is consumed, so no push can
        // grow (and abandon) the region afterwards, and the arena never
        // touches handed-out regions. The reborrow is unique.
        unsafe { slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

/// Deterministic TAT-like workload: one wire payload parsed two ways.
///
/// The record encoding is deliberately tiny (tag `u8`, length `u32` LE,
/// payload bytes) so the prototype stays dependency-free; field ids mirror
/// a populated `TestAllTypesProto3`-shaped message (string, bytes, nested
/// message, packed ints, map entries, repeated strings, scalar id).
pub mod probe {
    use super::{ArenaVec, BumpArena};

    // Field ids of the toy encoding.
    const F_STRING: u8 = 1;
    const F_BYTES: u8 = 2;
    const F_NESTED: u8 = 3;
    const F_PACKED: u8 = 4;
    const F_MAP: u8 = 5;
    const F_REP_STR: u8 = 6;
    const F_ID: u8 = 7;

    /// Build a wire payload record by record.
    pub struct WireBuilder {
        buf: Vec<u8>,
    }

    impl WireBuilder {
        /// Empty payload under construction.
        pub fn new() -> Self {
            WireBuilder { buf: Vec::new() }
        }

        /// Append one `field` record with a raw payload.
        pub fn field(&mut self, field: u8, payload: &[u8]) {
            self.buf.push(field);
            self.buf
                .extend_from_slice(&(payload.len() as u32).to_le_bytes());
            self.buf.extend_from_slice(payload);
        }

        /// Finished payload bytes.
        pub fn finish(self) -> Vec<u8> {
            self.buf
        }
    }

    impl Default for WireBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    /// TAT-like populated specimen (~200 bytes on the wire).
    pub fn specimen_tat_like() -> Vec<u8> {
        let mut w = WireBuilder::new();
        w.field(F_STRING, b"ada lovelace");
        w.field(F_BYTES, b"notes");
        w.field(F_ID, &42i32.to_le_bytes());
        let mut nested = WireBuilder::new();
        nested.field(1, &9i32.to_le_bytes());
        w.field(F_NESTED, &nested.finish());
        let mut packed = Vec::new();
        for i in 0..8i32 {
            packed.extend_from_slice(&(i * 3).to_le_bytes());
        }
        w.field(F_PACKED, &packed);
        let mut map = Vec::new();
        for i in 0..4i32 {
            map.extend_from_slice(&i.to_le_bytes());
            map.extend_from_slice(&(i * i).to_le_bytes());
        }
        w.field(F_MAP, &map);
        for i in 0..4 {
            w.field(F_REP_STR, format!("tag-{i:02}").as_bytes());
        }
        w.finish()
    }

    /// Packed-heavy specimen: one 80-byte string plus 256 packed ints.
    pub fn specimen_packed_256() -> Vec<u8> {
        let mut w = WireBuilder::new();
        w.field(F_STRING, &[b'x'; 80]);
        w.field(F_ID, &7i32.to_le_bytes());
        let mut packed = Vec::new();
        for i in 0..256i32 {
            packed.extend_from_slice(&i.to_le_bytes());
        }
        w.field(F_PACKED, &packed);
        w.finish()
    }

    /// Cursor over the toy encoding.
    struct Cursor<'w> {
        buf: &'w [u8],
        pos: usize,
    }

    impl<'w> Cursor<'w> {
        fn new(buf: &'w [u8]) -> Self {
            Cursor { buf, pos: 0 }
        }

        /// Next `(field, payload)` record, or `None` at end of input.
        fn next_record(&mut self) -> Result<Option<(u8, &'w [u8])>, &'static str> {
            if self.pos == self.buf.len() {
                return Ok(None);
            }
            if self.pos + 5 > self.buf.len() {
                return Err("truncated record header");
            }
            let field = self.buf[self.pos];
            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&self.buf[self.pos + 1..self.pos + 5]);
            let len = u32::from_le_bytes(len_bytes) as usize;
            let start = self.pos + 5;
            let end = start.checked_add(len).ok_or("record length overflow")?;
            if end > self.buf.len() {
                return Err("truncated record payload");
            }
            self.pos = end;
            Ok(Some((field, &self.buf[start..end])))
        }
    }

    fn get_i32(payload: &[u8]) -> Result<i32, &'static str> {
        if payload.len() != 4 {
            return Err("bad i32 width");
        }
        let mut b = [0u8; 4];
        b.copy_from_slice(payload);
        Ok(i32::from_le_bytes(b))
    }

    /// Global-allocator owned message: every field owns heap storage.
    #[derive(Debug, Default, PartialEq)]
    pub struct OwnedMsg {
        /// Optional string field.
        pub s: String,
        /// Optional bytes field.
        pub b: Vec<u8>,
        /// Nested message scalar (`nested.a`).
        pub nested_a: i32,
        /// Packed int32 field.
        pub packed: Vec<i32>,
        /// Map field as sorted-by-insertion pairs.
        pub map: Vec<(i32, i32)>,
        /// Repeated string field.
        pub rep: Vec<String>,
        /// Scalar id field.
        pub id: i32,
    }

    /// Parse `wire` into a global-allocator owned message.
    pub fn parse_owned(wire: &[u8]) -> Result<OwnedMsg, &'static str> {
        let mut m = OwnedMsg::default();
        let mut cur = Cursor::new(wire);
        while let Some((field, payload)) = cur.next_record()? {
            match field {
                F_STRING => {
                    m.s = std::str::from_utf8(payload)
                        .map_err(|_| "bad string utf8")?
                        .to_owned();
                }
                F_BYTES => m.b = payload.to_vec(),
                F_ID => m.id = get_i32(payload)?,
                F_NESTED => {
                    let mut inner = Cursor::new(payload);
                    match inner.next_record()? {
                        Some((1, p)) => m.nested_a = get_i32(p)?,
                        _ => return Err("bad nested message"),
                    }
                    if inner.next_record()?.is_some() {
                        return Err("bad nested message");
                    }
                }
                F_PACKED => {
                    if payload.len() % 4 != 0 {
                        return Err("bad packed width");
                    }
                    m.packed.reserve(payload.len() / 4);
                    for chunk in payload.as_chunks::<4>().0 {
                        m.packed.push(get_i32(chunk)?);
                    }
                }
                F_MAP => {
                    if payload.len() % 8 != 0 {
                        return Err("bad map width");
                    }
                    m.map.reserve(payload.len() / 8);
                    for entry in payload.as_chunks::<8>().0 {
                        m.map.push((get_i32(&entry[..4])?, get_i32(&entry[4..])?));
                    }
                }
                F_REP_STR => m.rep.push(
                    std::str::from_utf8(payload)
                        .map_err(|_| "bad string utf8")?
                        .to_owned(),
                ),
                _ => {} // Unknown field: skip, mirroring unknown retention.
            }
        }
        Ok(m)
    }

    /// Arena-backed message: every field borrows arena storage, so the
    /// handle itself is `Copy` and teardown is free (no per-field drops).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct ArenaMsg<'a> {
        /// Optional string field (arena storage).
        pub s: &'a str,
        /// Optional bytes field (arena storage).
        pub b: &'a [u8],
        /// Nested message scalar (`nested.a`, inline: no allocation).
        pub nested_a: i32,
        /// Packed int32 field (arena storage).
        pub packed: &'a [i32],
        /// Map field as pairs (arena storage).
        pub map: &'a [(i32, i32)],
        /// Repeated string field (arena storage, both levels).
        pub rep: &'a [&'a str],
        /// Scalar id field (inline: no allocation).
        pub id: i32,
    }

    /// Parse `wire` into `arena`, borrowing all field storage from it.
    ///
    /// Single pass; repeated/packed/map fields grow through [`ArenaVec`],
    /// so no global-allocator scratch is used on this path.
    pub fn parse_arena<'a>(
        arena: &'a BumpArena,
        wire: &[u8],
    ) -> Result<ArenaMsg<'a>, &'static str> {
        let mut s: &'a str = "";
        let mut b: &'a [u8] = &[];
        let mut nested_a = 0i32;
        let mut id = 0i32;
        let mut packed = ArenaVec::new(arena);
        let mut map = ArenaVec::new(arena);
        let mut rep = ArenaVec::new(arena);
        let mut cur = Cursor::new(wire);
        while let Some((field, payload)) = cur.next_record()? {
            match field {
                F_STRING => {
                    s = arena
                        .alloc_str(std::str::from_utf8(payload).map_err(|_| "bad string utf8")?);
                }
                F_BYTES => b = arena.alloc_bytes(payload),
                F_ID => id = get_i32(payload)?,
                F_NESTED => {
                    let mut inner = Cursor::new(payload);
                    match inner.next_record()? {
                        Some((1, p)) => nested_a = get_i32(p)?,
                        _ => return Err("bad nested message"),
                    }
                    if inner.next_record()?.is_some() {
                        return Err("bad nested message");
                    }
                }
                F_PACKED => {
                    if payload.len() % 4 != 0 {
                        return Err("bad packed width");
                    }
                    for chunk in payload.as_chunks::<4>().0 {
                        packed.push(get_i32(chunk)?);
                    }
                }
                F_MAP => {
                    if payload.len() % 8 != 0 {
                        return Err("bad map width");
                    }
                    for entry in payload.as_chunks::<8>().0 {
                        map.push((get_i32(&entry[..4])?, get_i32(&entry[4..])?));
                    }
                }
                F_REP_STR => {
                    let st: &str = arena
                        .alloc_str(std::str::from_utf8(payload).map_err(|_| "bad string utf8")?);
                    rep.push(st);
                }
                _ => {}
            }
        }
        Ok(ArenaMsg {
            s,
            b,
            nested_a,
            packed: packed.into_slice(),
            map: map.into_slice(),
            rep: rep.into_slice(),
            id,
        })
    }

    /// Mixing step shared by both touch walks so checksums must agree.
    fn mix(acc: u64, x: u64) -> u64 {
        acc.wrapping_mul(31).wrapping_add(x)
    }

    /// Touch every scalar/bytes field of a message through plain slices.
    /// Both owned and arena messages reduce to this identical walk.
    fn touch_flat(
        mut acc: u64,
        id: i32,
        s: &str,
        b: &[u8],
        nested_a: i32,
        packed: &[i32],
        map: &[(i32, i32)],
    ) -> u64 {
        acc = mix(acc, id as u64);
        acc = mix(acc, s.len() as u64);
        for byte in s.bytes() {
            acc = mix(acc, byte as u64);
        }
        acc = mix(acc, b.len() as u64);
        for byte in b.iter() {
            acc = mix(acc, *byte as u64);
        }
        acc = mix(acc, nested_a as u64);
        acc = mix(acc, packed.len() as u64);
        for v in packed.iter() {
            acc = mix(acc, *v as u64);
        }
        acc = mix(acc, map.len() as u64);
        for (k, v) in map.iter() {
            acc = mix(acc, *k as u64);
            acc = mix(acc, *v as u64);
        }
        acc
    }

    /// Touch the repeated-string tail through any string iterator.
    fn touch_rep<'x>(mut acc: u64, rep: impl IntoIterator<Item = &'x str>) -> u64 {
        let mut n = 0u64;
        for s in rep {
            n += 1;
            acc = mix(acc, s.len() as u64);
            for byte in s.bytes() {
                acc = mix(acc, byte as u64);
            }
        }
        mix(acc, n)
    }

    /// Walk every field of an owned message; returns a checksum that must
    /// equal [`touch_arena`] on the same wire bytes.
    pub fn touch_owned(m: &OwnedMsg) -> u64 {
        let acc = touch_flat(0, m.id, &m.s, &m.b, m.nested_a, &m.packed, &m.map);
        touch_rep(acc, m.rep.iter().map(|s| s.as_str()))
    }

    /// Walk every field of an arena message; checksum must equal
    /// [`touch_owned`] on the same wire bytes.
    pub fn touch_arena(m: &ArenaMsg) -> u64 {
        let acc = touch_flat(0, m.id, m.s, m.b, m.nested_a, m.packed, m.map);
        touch_rep(acc, m.rep.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::probe::*;
    use super::BumpArena;

    #[test]
    fn alloc_u64_stays_aligned_after_odd_bytes() {
        let arena = BumpArena::new();
        arena.alloc_bytes(b"abc");
        let slot = arena.alloc_copy(0x0102_0304_0506_0708u64);
        assert_eq!(*slot, 0x0102_0304_0506_0708u64);
        assert_eq!((slot as *const u64 as usize) % 8, 0);
        let wide = arena.alloc_copy(0u128);
        assert_eq!(wide as *const u128 as usize % 16, 0);
    }

    #[test]
    fn chunk_growth_keeps_earlier_values_intact() {
        let arena = BumpArena::with_chunk_bytes(64);
        let mut expect = Vec::new();
        for i in 0..200u64 {
            let slot = arena.alloc_copy(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            expect.push((slot as *const u64, *slot));
        }
        assert!(
            arena.chunk_count() > 1,
            "200 u64s must overflow 64-byte chunks"
        );
        assert!(arena.used_bytes() <= arena.reserved_bytes());
        for (ptr, want) in expect {
            // SAFETY: test-only re-read of live arena memory; the arena is
            // still alive and was never reset, so every pointer is valid.
            assert_eq!(unsafe { *ptr }, want);
        }
    }

    #[test]
    fn str_bytes_slice_roundtrip() {
        let arena = BumpArena::new();
        let s = arena.alloc_str("hello arena");
        assert_eq!(s, "hello arena");
        let b = arena.alloc_bytes(b"\x00\xff\x7f");
        assert_eq!(b, b"\x00\xff\x7f");
        let ints = arena.alloc_slice(&[1i32, -2, 3, -4]);
        assert_eq!(ints, &[1, -2, 3, -4]);
        assert!(arena.alloc_bytes(b"").is_empty());
        assert!(arena.alloc_slice::<u64>(&[]).is_empty());
    }

    #[test]
    fn reset_reuses_first_chunk_without_reallocating() {
        let arena = BumpArena::new();
        arena.alloc_bytes(&[7u8; 100]);
        let mut arena = arena;
        let reserved_before = arena.reserved_bytes();
        assert_eq!(arena.chunk_count(), 1);
        arena.reset();
        assert_eq!(arena.used_bytes(), 0);
        arena.alloc_bytes(&[9u8; 100]);
        assert_eq!(arena.reserved_bytes(), reserved_before);
        assert_eq!(arena.chunk_count(), 1);
    }

    #[test]
    fn reset_drops_overflow_chunks() {
        let mut arena = BumpArena::with_chunk_bytes(64);
        for i in 0..50u64 {
            arena.alloc_copy(i);
        }
        assert!(arena.chunk_count() > 1);
        arena.reset();
        assert_eq!(arena.chunk_count(), 1);
        assert_eq!(arena.used_bytes(), 0);
    }

    #[test]
    fn arena_vec_push_grow_preserves_order() {
        let arena = BumpArena::new();
        let mut v = super::ArenaVec::new(&arena);
        assert!(v.is_empty());
        for i in 0..100i32 {
            v.push(i * i - i);
        }
        assert_eq!(v.len(), 100);
        let slice = v.into_slice();
        assert_eq!(slice.len(), 100);
        for (i, got) in slice.iter().enumerate() {
            assert_eq!(*got, (i as i32) * (i as i32) - (i as i32));
        }
    }

    #[test]
    fn parse_equivalence_on_all_specimens() {
        for wire in [specimen_tat_like(), specimen_packed_256(), Vec::new()] {
            let owned = parse_owned(&wire).expect("owned parse");
            let arena = BumpArena::new();
            let amsg = parse_arena(&arena, &wire).expect("arena parse");
            assert_eq!(touch_owned(&owned), touch_arena(&amsg));
            assert_eq!(owned.s, amsg.s);
            assert_eq!(owned.b, amsg.b);
            assert_eq!(owned.nested_a, amsg.nested_a);
            assert_eq!(owned.packed, amsg.packed);
            assert_eq!(owned.map, amsg.map);
            assert_eq!(owned.id, amsg.id);
            let owned_rep: Vec<&str> = owned.rep.iter().map(|s| s.as_str()).collect();
            assert_eq!(owned_rep, amsg.rep);
        }
    }

    #[test]
    fn truncation_and_shape_errors_match_both_paths() {
        let bad_shapes: Vec<Vec<u8>> = vec![
            vec![1u8],                       // header cut off
            vec![1, 4, 0, 0, 0, b'a'],       // payload cut off
            vec![7, 2, 0, 0, 0, 1, 2],       // bad i32 width
            vec![4, 3, 0, 0, 0, 1, 2, 3],    // bad packed width
            vec![5, 4, 0, 0, 0, 1, 2, 3, 4], // bad map width
            vec![1, 1, 0, 0, 0, 0xFF],       // bad string utf8
        ];
        for wire in bad_shapes {
            assert!(parse_owned(&wire).is_err(), "owned accepts {wire:?}");
            let arena = BumpArena::new();
            assert!(
                parse_arena(&arena, &wire).is_err(),
                "arena accepts {wire:?}"
            );
        }
        // Empty input parses to defaults on both paths.
        let owned = parse_owned(&[]).expect("empty owned");
        assert_eq!(owned, OwnedMsg::default());
        let arena = BumpArena::new();
        let amsg = parse_arena(&arena, &[]).expect("empty arena");
        assert_eq!(touch_arena(&amsg), touch_owned(&owned));
    }

    #[test]
    fn alloc_many_small_values() {
        let arena = BumpArena::new();
        let mut sum = 0u64;
        for i in 0..1000u32 {
            sum += *arena.alloc_copy(i) as u64;
        }
        assert_eq!(sum, 1000 * 999 / 2);
        assert_eq!(arena.chunk_count(), 1, "1000 u32s fit one 8 KiB chunk");
    }

    #[test]
    fn reset_then_reuse_sees_only_new_pattern() {
        let mut arena = BumpArena::new();
        for i in 0..64u32 {
            arena.alloc_copy(i);
        }
        arena.reset();
        let mut sum = 0u64;
        for i in 0..64u32 {
            sum += *arena.alloc_copy(1000 + i) as u64;
        }
        assert_eq!(sum, 64 * 1000 + 64 * 63 / 2);
        assert_eq!(arena.chunk_count(), 1);
    }
}
