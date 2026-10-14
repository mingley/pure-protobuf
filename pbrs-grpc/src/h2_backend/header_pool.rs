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
