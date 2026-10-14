//! Reused `HeaderMap`s for the three header blocks a gRPC connection repeats.
//!
//! The benchmark and the default client send the same request headers, the
//! same response headers, and `grpc-status: 0`. Encoding drops that map, and
//! the next RPC allocates another. A hit on the encoded-block cache returns
//! the map here instead. A different header set is not stored, so a later
//! RPC cannot observe someone else's fields.

use http::HeaderMap;
use std::cell::Cell;
use std::sync::Arc;

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

/// `grpc-status: 0` and nothing else.
pub(crate) fn is_bare_ok_trailer(fields: &HeaderMap) -> bool {
    if fields.len() != 1 {
        return false;
    }
    let Some((name, value)) = fields.iter().next() else {
        return false;
    };
    let bytes = value.as_bytes();
    name.as_str() == "grpc-status" && bytes.len() == 1 && bytes[0] == b'0'
}

thread_local! {
    // Received OK trailers only. The send-side `Slot::Trailers` map stays
    // with the encoder; sharing it made client-stream allocate again.
    static RECEIVED_OK_TRAILER: Cell<Option<HeaderMap>> = const { Cell::new(None) };
}

pub(crate) fn take_received_ok_trailer(make: impl FnOnce() -> HeaderMap) -> HeaderMap {
    RECEIVED_OK_TRAILER.take().unwrap_or_else(make)
}

pub(crate) fn recycle_received_ok_trailer(map: HeaderMap) {
    RECEIVED_OK_TRAILER.set(Some(map));
}

/// Content-type plus `grpc-accept-encoding`, and nothing else.
pub(crate) fn is_stock_response(fields: &HeaderMap) -> bool {
    if fields.len() != 2 {
        return false;
    }
    let mut saw_type = false;
    let mut saw_accept = false;
    for (name, value) in fields {
        if name == http::header::CONTENT_TYPE {
            let bytes = value.as_bytes();
            if bytes != b"application/grpc" && bytes != b"application/grpc+proto" {
                return false;
            }
            saw_type = true;
        } else if name.as_str() == "grpc-accept-encoding" {
            saw_accept = true;
        } else {
            return false;
        }
    }
    saw_type && saw_accept
}

/// Both maps are two-header stock responses in the same order.
///
/// Shared header values compare by address first. A clone of a static or
/// refcounted value keeps that address, so the steady hit does not scan names
/// or memcmp the bytes.
pub(crate) fn same_two_values(left: &HeaderMap, right: &HeaderMap) -> bool {
    let mut left_iter = left.iter();
    let mut right_iter = right.iter();
    let (Some((_, left_a)), Some((_, right_a))) = (left_iter.next(), right_iter.next()) else {
        return false;
    };
    let (Some((_, left_b)), Some((_, right_b))) = (left_iter.next(), right_iter.next()) else {
        return false;
    };
    same_value(left_a.as_bytes(), right_a.as_bytes())
        && same_value(left_b.as_bytes(), right_b.as_bytes())
}

fn same_value(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && (left.as_ptr() == right.as_ptr() || left == right)
}

thread_local! {
    // Received stock responses only. The encoder keeps `Slot::Response`.
    static RECEIVED_RESPONSE: Cell<Option<HeaderMap>> = const { Cell::new(None) };
}

pub(crate) fn take_received_response() -> Option<HeaderMap> {
    RECEIVED_RESPONSE.take()
}

pub(crate) fn recycle_received_response(map: HeaderMap) {
    RECEIVED_RESPONSE.set(Some(map));
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

thread_local! {
    static SHARED_REQUEST: Cell<Option<Arc<HeaderMap>>> = const { Cell::new(None) };
}

/// A decoded stock request is shared with the accept loop. The cache keeps
/// the other handle.
pub(crate) fn publish_shared_request(map: Arc<HeaderMap>) {
    SHARED_REQUEST.set(Some(map));
}

pub(crate) fn take_shared_request() -> Option<Arc<HeaderMap>> {
    SHARED_REQUEST.take()
}
