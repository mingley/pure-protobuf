//! Bitwise copy of a [`http::uri::PathAndQuery`] built with `from_static`.
//!
//! That value's bytes live for the process lifetime. Clone and drop only
//! copy the words, so a read is the same as `clone` without the virtual call.

#![allow(
    unsafe_code,
    reason = "a static PathAndQuery has no refcount; copying its words matches clone"
)]

/// Copy a path that was built with [`http::uri::PathAndQuery::from_static`].
#[inline(always)]
pub(crate) fn copy_static_path(parsed: &http::uri::PathAndQuery) -> http::uri::PathAndQuery {
    // SAFETY: caller passes only `PathAndQuery::from_static` values. Drop of
    // those bytes is a no-op, so an extra bitwise copy does not free them.
    unsafe { std::ptr::read(parsed) }
}

/// Copy an authority that was built with [`http::uri::Authority::from_static`].
#[inline(always)]
pub(crate) fn copy_static_authority(parsed: &http::uri::Authority) -> http::uri::Authority {
    // SAFETY: caller passes only `Authority::from_static` values. Drop of
    // those bytes is a no-op, so an extra bitwise copy does not free them.
    unsafe { std::ptr::read(parsed) }
}

/// Copy a POST whose URI bytes are static and whose header map is empty.
#[inline(always)]
pub(crate) fn copy_static_http_request(parsed: &http::Request<()>) -> http::Request<()> {
    // SAFETY: caller passes only requests built from `Scheme::HTTP`/`HTTPS`,
    // `Authority::from_static`, `PathAndQuery::from_static`, and an empty map.
    // Drop of that request does not free those bytes or a header allocation.
    unsafe { std::ptr::read(parsed) }
}
