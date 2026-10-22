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

/// Clone a cached request, retaining any dynamically allocated URI storage.
#[inline(always)]
pub(crate) fn clone_http_request(parsed: &http::Request<()>) -> http::Request<()> {
    parsed.clone()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "static HTTP copy regression")]
    use super::*;

    #[test]
    fn static_copies_remain_valid_after_independent_drops() {
        let path = http::uri::PathAndQuery::from_static("/svc/method?x=1");
        let authority = http::uri::Authority::from_static("example.test:443");
        let mut request = http::Request::new(());
        *request.method_mut() = http::Method::POST;
        *request.uri_mut() = http::Uri::builder()
            .scheme(http::uri::Scheme::HTTPS)
            .authority(copy_static_authority(&authority))
            .path_and_query(copy_static_path(&path))
            .build()
            .expect("static URI");
        for _ in 0..8 {
            let copy = clone_http_request(&request);
            assert_eq!(copy.uri().path(), "/svc/method");
            assert_eq!(copy.uri().authority(), Some(&authority));
            assert!(copy.headers().is_empty());
            drop(copy);
        }
        drop(request);
        assert_eq!(copy_static_path(&path), path);
        assert_eq!(copy_static_authority(&authority), authority);
    }
    #[test]
    fn cached_request_owns_dynamic_uri_after_original_is_dropped() {
        let request = http::Request::builder()
            .uri(String::from("http://owned.example.test:443/service/method"))
            .body(())
            .expect("owned URI");
        let cloned = clone_http_request(&request);
        drop(request);
        assert_eq!(
            cloned.uri().authority().expect("authority").as_str(),
            "owned.example.test:443"
        );
        assert_eq!(cloned.uri().path(), "/service/method");
        let second = clone_http_request(&cloned);
        drop(cloned);
        assert_eq!(second.uri().path(), "/service/method");
    }
}
