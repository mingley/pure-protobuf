//! Thread-local caches for canonical gRPC header maps.
//!
//! Requests and trailers own their fields while queued. A frame with an
//! explicit stock kind can reconstruct its fields on a cold encoder or a
//! different worker. Pool availability changes allocation cost, not headers.

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

/// Materialize a known gRPC block when a frame outlives its worker's pool.
pub(crate) fn make_stock(slot: Slot) -> HeaderMap {
    use http::{HeaderValue, header};
    let mut map = HeaderMap::with_capacity(match slot {
        Slot::Request => 4,
        Slot::Response => 2,
        Slot::Trailers => 1,
    });
    if matches!(slot, Slot::Trailers) {
        map.insert("grpc-status", HeaderValue::from_static("0"));
        return map;
    }
    map.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/grpc"),
    );
    map.insert(
        "grpc-accept-encoding",
        HeaderValue::from_static(if cfg!(feature = "zstd") {
            "identity,gzip,deflate,zstd"
        } else {
            "identity,gzip,deflate"
        }),
    );
    if matches!(slot, Slot::Request) {
        map.insert(header::TE, HeaderValue::from_static("trailers"));
        map.insert(
            header::USER_AGENT,
            HeaderValue::from_static(concat!("pbrs-grpc/", env!("CARGO_PKG_VERSION"))),
        );
    }
    map
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

pub(crate) fn recycle_received_ok_trailer(map: HeaderMap) {
    // Install the new owner before dropping the old map: a HeaderValue may
    // own Bytes whose destructor re-enters this thread-local cache.
    drop(RECEIVED_OK_TRAILER.replace(Some(map)));
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
    if fields.len() != 2 {
        return None;
    }
    let mut headers = fields.iter();
    let Some((name, value)) = headers.next() else {
        return None;
    };
    let bytes = value.as_bytes();
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

/// Two heap slots for `HeaderMap`s queued on one connection.
///
/// A stock response and its trailers are both in the receive queue before
/// either is polled. One slot was reused by the response, and the trailer
/// allocated every time. The second slot keeps that trailer box.
struct SpareMapBox(
    std::cell::Cell<*mut HeaderMap>,
    std::cell::Cell<*mut HeaderMap>,
);

#[allow(
    unsafe_code,
    reason = "free the parked header-map boxes at thread exit"
)]
impl Drop for SpareMapBox {
    fn drop(&mut self) {
        for cell in [&self.0, &self.1] {
            let ptr = cell.get();
            if !ptr.is_null() {
                // SAFETY: the pointer came from Box::into_raw after the map was moved out.
                unsafe {
                    std::alloc::dealloc(ptr.cast(), std::alloc::Layout::new::<HeaderMap>());
                }
            }
        }
    }
}

thread_local! {
    static SPARE_MAP_BOX: SpareMapBox = const { SpareMapBox(
        std::cell::Cell::new(std::ptr::null_mut()),
        std::cell::Cell::new(std::ptr::null_mut()),
    ) };
}

#[allow(
    unsafe_code,
    reason = "write a header map into a recycled box without dropping the moved-out bits"
)]
pub(crate) fn box_header_map(fields: HeaderMap) -> Box<HeaderMap> {
    let ptr = SPARE_MAP_BOX.with(|slot| {
        let first = slot.0.replace(std::ptr::null_mut());
        if !first.is_null() {
            first
        } else {
            slot.1.replace(std::ptr::null_mut())
        }
    });
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
    SPARE_MAP_BOX.with(|slot| {
        if slot.0.get().is_null() {
            slot.0.set(ptr);
        } else if slot.1.get().is_null() {
            slot.1.set(ptr);
        } else {
            // SAFETY: `ptr` was produced by Box::into_raw after its map was moved out.
            unsafe {
                std::alloc::dealloc(ptr.cast(), std::alloc::Layout::new::<HeaderMap>());
            }
        }
    });
    fields
}

pub(crate) fn recycle_received_response(map: HeaderMap, grpc_proto: bool) {
    drop(RECEIVED_RESPONSE.replace(Some((map, grpc_proto))));
}

/// Return `map` to its slot.
///
/// Publish the new map before dropping the previous owner, so a header value's
/// destructor can re-enter this cache without observing partially dropped data.
#[inline(always)]
pub(crate) fn recycle(slot: Slot, map: HeaderMap) {
    drop(slot_map(slot).replace(Some(map)));
}

/// Replace `dst` with `src`.
///
/// `HeaderMap::new` does not allocate. Dropping that empty map is only
/// glue, so a zero-capacity map is forgotten instead.
#[inline]
pub(crate) fn replace_map(dst: &mut HeaderMap, src: HeaderMap) {
    let old = std::mem::replace(dst, src);
    if old.capacity() == 0 {
        let _empty = std::mem::ManuallyDrop::new(old);
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
pub(crate) fn stock_caller_hit(https: bool, path: &str, authority: &str) -> bool {
    super::frame::Headers::stock_caller_hit(https, path, authority)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Reenter {
        Trailer,
        Response,
        Request,
    }

    struct ReentrantValue {
        cache: Reenter,
        drops: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl AsRef<[u8]> for ReentrantValue {
        fn as_ref(&self) -> &[u8] {
            b"owned header value"
        }
    }

    impl Drop for ReentrantValue {
        fn drop(&mut self) {
            use std::sync::atomic::Ordering;
            // Bound recursion in the negative control with the old raw TLS
            // replacement: a stale cache must not destroy this owner twice.
            if self.drops.fetch_add(1, Ordering::SeqCst) == 0 {
                let mut map = HeaderMap::new();
                map.insert("x-reentered", http::HeaderValue::from_static("yes"));
                match self.cache {
                    Reenter::Trailer => recycle_received_ok_trailer(map),
                    Reenter::Response => recycle_received_response(map, true),
                    Reenter::Request => recycle(Slot::Request, map),
                }
            }
        }
    }

    #[test]
    fn replacing_a_cached_map_allows_its_value_destructor_to_reenter() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        for cache in [Reenter::Trailer, Reenter::Response, Reenter::Request] {
            let drops = Arc::new(AtomicUsize::new(0));
            let bytes = bytes::Bytes::from_owner(ReentrantValue {
                cache,
                drops: drops.clone(),
            });
            let value = http::HeaderValue::from_maybe_shared(bytes).expect("valid owned value");
            let mut owned = HeaderMap::new();
            owned.insert("x-owned", value);
            match cache {
                Reenter::Trailer => {
                    recycle_received_ok_trailer(owned);
                    recycle_received_ok_trailer(HeaderMap::new());
                    let reentered = RECEIVED_OK_TRAILER.take().expect("reentered trailer");
                    assert_eq!(reentered.get("x-reentered").expect("marker"), "yes");
                }
                Reenter::Response => {
                    recycle_received_response(owned, false);
                    recycle_received_response(HeaderMap::new(), false);
                    let (reentered, proto) = RECEIVED_RESPONSE.take().expect("reentered response");
                    assert!(proto);
                    assert_eq!(reentered.get("x-reentered").expect("marker"), "yes");
                }
                Reenter::Request => {
                    recycle(Slot::Request, owned);
                    recycle(Slot::Request, HeaderMap::new());
                    let reentered = REQUEST_MAP.take().expect("reentered request");
                    assert_eq!(reentered.get("x-reentered").expect("marker"), "yes");
                }
            }
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn stock_response_checks_accept_header_with_reused_content_type() {
        let content_type = http::HeaderValue::from_static("application/grpc");
        let mut standard = HeaderMap::new();
        standard.insert(http::header::CONTENT_TYPE, content_type.clone());
        standard.insert(
            "grpc-accept-encoding",
            http::HeaderValue::from_bytes(STOCK_ACCEPT).unwrap(),
        );
        assert_eq!(is_stock_response(&standard), Some(false));
        let mut other = HeaderMap::new();
        other.insert(http::header::CONTENT_TYPE, content_type);
        other.insert("x-context", http::HeaderValue::from_static("private"));
        assert_eq!(is_stock_response(&other), None);
    }
}
