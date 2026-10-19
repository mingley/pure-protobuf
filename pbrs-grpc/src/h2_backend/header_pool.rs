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

thread_local! {
    static REQUEST_MAP: Cell<Option<HeaderMap>> = const { Cell::new(None) };
    static RESPONSE_MAP: Cell<Option<HeaderMap>> = const { Cell::new(None) };
    static TRAILER_MAP: Cell<Option<HeaderMap>> = const { Cell::new(None) };
}

fn slot_map(slot: Slot) -> &'static std::thread::LocalKey<Cell<Option<HeaderMap>>> {
    match slot {
        Slot::Request => &REQUEST_MAP,
        Slot::Response => &RESPONSE_MAP,
        Slot::Trailers => &TRAILER_MAP,
    }
}

pub(crate) fn take(slot: Slot, make: impl FnOnce() -> HeaderMap) -> HeaderMap {
    slot_map(slot).take().unwrap_or_else(make)
}

/// The slot already holds the stock block.
///
/// A replayed frame does not read that map. Callers leave it parked and send
/// an empty block instead of moving the map into the frame and back.
#[inline]
#[allow(
    unsafe_code,
    reason = "the header slot is a thread-local cell; the read does not re-enter it"
)]
pub(crate) fn is_parked(slot: Slot) -> bool {
    let ptr = match slot {
        Slot::Request => REQUEST_MAP.with(Cell::as_ptr),
        Slot::Response => RESPONSE_MAP.with(Cell::as_ptr),
        Slot::Trailers => TRAILER_MAP.with(Cell::as_ptr),
    };
    // SAFETY: the cell is thread-local and this read does not re-enter it.
    unsafe { (*ptr).is_some() }
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

/// `true` when an OK trailer map is already parked for this thread.
#[inline]
#[allow(unsafe_code, reason = "peek the parked trailer map without moving it")]
pub(crate) fn received_ok_trailer_is_parked() -> bool {
    let ptr = RECEIVED_OK_TRAILER.with(Cell::as_ptr);
    // SAFETY: the cell is thread-local and this read does not re-enter it.
    unsafe { (*ptr).is_some() }
}

#[allow(
    unsafe_code,
    reason = "an empty trailer slot is overwritten in place; a full slot is dropped first"
)]
pub(crate) fn recycle_received_ok_trailer(map: HeaderMap) {
    // `Cell::set` copies the previous value out. After `take` that value is
    // `None`, so the copy is only overhead.
    let ptr = RECEIVED_OK_TRAILER.with(Cell::as_ptr);
    unsafe {
        if (*ptr).is_none() {
            std::ptr::write(ptr, Some(map));
        } else {
            std::ptr::drop_in_place(ptr);
            std::ptr::write(ptr, Some(map));
        }
    }
}

#[cfg(not(feature = "zstd"))]
const STOCK_ACCEPT: &[u8] = b"identity,gzip,deflate";
#[cfg(feature = "zstd")]
const STOCK_ACCEPT: &[u8] = b"identity,gzip,deflate,zstd";

/// Content-type plus the standard `grpc-accept-encoding`, and nothing else.
///
/// `Some(false)` is `application/grpc`. `Some(true)` is `application/grpc+proto`.
/// Our encoder writes content-type first. A reversed pair, or any other
/// accept-encoding value, stays an owned map.
#[inline(never)]
pub(crate) fn is_stock_response(fields: &HeaderMap) -> Option<bool> {
    use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
    // The recycled stock map keeps the same content-type buffer. After the
    // first full check, that pointer skips the second header and the compares.
    static VALUE_PTR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
    static VALUE_LEN: AtomicUsize = AtomicUsize::new(0);
    static PROTO: AtomicBool = AtomicBool::new(false);

    if fields.len() != 2 {
        return None;
    }
    let mut headers = fields.iter();
    let Some((name, value)) = headers.next() else {
        return None;
    };
    let bytes = value.as_bytes();
    if name == http::header::CONTENT_TYPE
        && VALUE_PTR.load(Ordering::Acquire) == bytes.as_ptr() as *mut u8
        && VALUE_LEN.load(Ordering::Relaxed) == bytes.len()
    {
        return Some(PROTO.load(Ordering::Relaxed));
    }
    let Some((accept, accept_value)) = headers.next() else {
        return None;
    };
    if name != http::header::CONTENT_TYPE || accept.as_str() != "grpc-accept-encoding" {
        return None;
    }
    if accept_value.as_bytes() != STOCK_ACCEPT {
        return None;
    }
    let proto = if bytes == b"application/grpc" {
        false
    } else if bytes == b"application/grpc+proto" {
        true
    } else {
        return None;
    };
    VALUE_LEN.store(bytes.len(), Ordering::Relaxed);
    PROTO.store(proto, Ordering::Relaxed);
    VALUE_PTR.store(bytes.as_ptr() as *mut u8, Ordering::Release);
    Some(proto)
}

thread_local! {
    // Received stock responses only. The bool is `application/grpc+proto`.
    // The encoder keeps `Slot::Response`.
    static RECEIVED_RESPONSE: Cell<Option<(HeaderMap, bool)>> = const { Cell::new(None) };
}

pub(crate) fn take_received_response() -> Option<(HeaderMap, bool)> {
    RECEIVED_RESPONSE.take()
}

/// `true` when the parked response map is the same protobuf flag.
#[inline]
#[allow(unsafe_code, reason = "peek the parked response map without moving it")]
pub(crate) fn received_response_matches(grpc_proto: bool) -> bool {
    let ptr = RECEIVED_RESPONSE.with(Cell::as_ptr);
    // SAFETY: `as_ref` borrows the map. The cell is thread-local and this
    // read does not re-enter it.
    unsafe {
        match (*ptr).as_ref() {
            Some((_, proto)) => *proto == grpc_proto,
            None => false,
        }
    }
}

/// One heap slot for the `HeaderMap` queued as a stock response.
///
/// The receive path boxes that map and the client poll moves it back out.
/// The slot stays; the next response writes into it.
struct SpareMapBox(std::cell::Cell<*mut HeaderMap>);

#[allow(unsafe_code, reason = "free the parked header-map box at thread exit")]
impl Drop for SpareMapBox {
    fn drop(&mut self) {
        let ptr = self.0.get();
        if !ptr.is_null() {
            // SAFETY: the pointer came from Box::into_raw after the map was moved out.
            unsafe {
                std::alloc::dealloc(ptr.cast(), std::alloc::Layout::new::<HeaderMap>());
            }
        }
    }
}

thread_local! {
    static SPARE_MAP_BOX: SpareMapBox = SpareMapBox(std::cell::Cell::new(std::ptr::null_mut()));
}

#[allow(
    unsafe_code,
    reason = "write a header map into a recycled box without dropping the moved-out bits"
)]
pub(crate) fn box_header_map(fields: HeaderMap) -> Box<HeaderMap> {
    let ptr = SPARE_MAP_BOX.with(|slot| slot.0.replace(std::ptr::null_mut()));
    if ptr.is_null() {
        Box::new(fields)
    } else {
        // SAFETY: `ptr` came from Box::into_raw. The previous map was moved
        // out, so these bits own no heap. `write` installs `fields`.
        unsafe {
            std::ptr::write(ptr, fields);
            Box::from_raw(ptr)
        }
    }
}

#[allow(
    unsafe_code,
    reason = "move a header map out of its box and keep the allocation"
)]
pub(crate) fn unbox_header_map(boxed: Box<HeaderMap>) -> HeaderMap {
    let ptr = Box::into_raw(boxed);
    // SAFETY: `ptr` owns the map. `read` moves it to the caller. The allocation
    // is parked for the next stock response.
    let fields = unsafe { std::ptr::read(ptr) };
    let prev = SPARE_MAP_BOX.with(|slot| slot.0.replace(ptr));
    if !prev.is_null() {
        // SAFETY: `prev` was produced by Box::into_raw after its map was moved out.
        unsafe {
            std::alloc::dealloc(prev.cast(), std::alloc::Layout::new::<HeaderMap>());
        }
    }
    fields
}

#[allow(
    unsafe_code,
    reason = "an empty response slot is overwritten in place; a full slot is dropped first"
)]
pub(crate) fn recycle_received_response(map: HeaderMap, grpc_proto: bool) {
    let ptr = RECEIVED_RESPONSE.with(Cell::as_ptr);
    unsafe {
        if (*ptr).is_none() {
            std::ptr::write(ptr, Some((map, grpc_proto)));
        } else {
            std::ptr::drop_in_place(ptr);
            std::ptr::write(ptr, Some((map, grpc_proto)));
        }
    }
}

/// Return `map` to its slot.
///
/// The pointer is taken first so the map is not moved into the thread-local
/// closure. An empty slot is overwritten in place. A slot that still holds
/// a map drops that map first.
#[inline(always)]
#[allow(
    unsafe_code,
    reason = "an empty header slot is overwritten in place; a full slot is dropped first"
)]
pub(crate) fn recycle(slot: Slot, map: HeaderMap) {
    let ptr = match slot {
        Slot::Request => REQUEST_MAP.with(Cell::as_ptr),
        Slot::Response => RESPONSE_MAP.with(Cell::as_ptr),
        Slot::Trailers => TRAILER_MAP.with(Cell::as_ptr),
    };
    unsafe {
        if (*ptr).is_none() {
            std::ptr::write(ptr, Some(map));
        } else {
            std::ptr::drop_in_place(ptr);
            std::ptr::write(ptr, Some(map));
        }
    }
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

/// The saved stock POST matches this target. The caller can skip copying the
/// `http::Request` and build the header frame from the saved pseudos.
pub(crate) fn stock_post_cached(https: bool, path: &str, authority: &str) -> bool {
    super::frame::Headers::stock_post_cached(https, path, authority)
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

thread_local! {
    // 0 = none, 1 = http, 2 = https. Published by a stock request decode and
    // taken by the same poll before the task can park.
    static STOCK_HTTPS: Cell<u8> = const { Cell::new(0) };
}

pub(crate) fn clear_stock_https() {
    STOCK_HTTPS.set(0);
}

pub(crate) fn publish_stock_https(https: bool) {
    STOCK_HTTPS.set(if https { 2 } else { 1 });
}

pub(crate) fn take_stock_https() -> Option<bool> {
    match STOCK_HTTPS.replace(0) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}
