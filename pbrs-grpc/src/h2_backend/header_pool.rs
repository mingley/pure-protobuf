//! Reused `HeaderMap`s for the three header blocks a gRPC connection repeats.
//!
//! The benchmark and the default client send the same request headers, the
//! same response headers, and `grpc-status: 0`. Encoding drops that map, and
//! the next RPC allocates another. A hit on the encoded-block cache returns
//! the map here instead. A different header set is not stored, so a later
//! RPC cannot observe someone else's fields.

use http::HeaderMap;
use std::cell::Cell;

#[derive(Copy, Clone)]
pub(crate) enum Slot {
    Request,
    Response,
    Trailers,
}

struct Pool([Cell<Option<HeaderMap>>; 3]);

thread_local! {
    static POOL: Pool = const {
        Pool([
            Cell::new(None),
            Cell::new(None),
            Cell::new(None),
        ])
    };
}

pub(crate) fn take(slot: Slot, make: impl FnOnce() -> HeaderMap) -> HeaderMap {
    POOL.with(|pool| pool.0[slot as usize].take().unwrap_or_else(make))
}

pub(crate) fn recycle(slot: Slot, map: HeaderMap) {
    POOL.with(|pool| {
        drop(pool.0[slot as usize].replace(Some(map)));
    });
}

/// Replace `dst` with `src`.
///
/// `HeaderMap::new` does not allocate. Dropping that empty map is only
/// glue, so a zero-capacity map is forgotten instead.
#[inline]
pub(crate) fn replace_map(dst: &mut HeaderMap, src: HeaderMap) {
    let old = std::mem::replace(dst, src);
    if old.capacity() == 0 {
        std::mem::forget(old);
    }
}

pub(crate) const STOCK_REQUEST: u8 = 1;
pub(crate) const STOCK_RESPONSE: u8 = 2;

thread_local! {
    static PENDING_STOCK: Cell<u8> = const { Cell::new(0) };
    static NEXT_STOCK: Cell<u8> = const { Cell::new(0) };
}

/// The header block just built is stock, but it has not been handed to h2 yet.
pub(crate) fn set_pending_stock(kind: u8) {
    PENDING_STOCK.set(kind);
}

pub(crate) fn clear_pending_stock() {
    PENDING_STOCK.set(0);
}

/// The next `convert_send_message` / trailer build on this worker sends that block.
pub(crate) fn promote_pending_stock() {
    let kind = PENDING_STOCK.replace(0);
    if kind != 0 {
        NEXT_STOCK.set(kind);
    }
}

pub(crate) fn mark_next_stock(kind: u8) {
    NEXT_STOCK.set(kind);
}

/// `true` once, when the pending send was marked `kind`.
pub(crate) fn take_next_stock(kind: u8) -> bool {
    NEXT_STOCK.with(|cell| {
        if cell.get() == kind {
            cell.set(0);
            true
        } else {
            false
        }
    })
}

pub(crate) fn clear_next_stock() {
    NEXT_STOCK.set(0);
}
