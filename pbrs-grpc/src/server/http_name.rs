#![allow(
    unsafe_code,
    reason = "own String allocations or borrow process-lifetime HTTP names"
)]
//! Owned or process-lifetime HTTP name.
//!
//! A stock request's path and authority are leaked once and then copied by
//! pointer. Storing another `String` allocated that text on every call and
//! freed it when the handler took the message. `cap == 0` means the bytes
//! outlive the process, so drop does not free them.

use std::borrow::Cow;
use std::ptr::NonNull;

/// Path, authority, or scheme stored like a `String` without a second copy
/// when the bytes are already `'static`.
pub(crate) struct HttpStr {
    ptr: NonNull<u8>,
    len: usize,
    cap: usize,
}

const _: () = {
    assert!(std::mem::size_of::<HttpStr>() == std::mem::size_of::<String>());
    assert!(std::mem::size_of::<Option<HttpStr>>() == std::mem::size_of::<Option<String>>());
};

// SAFETY: the bytes are a `String` allocation this value owns, or `'static`.
// Moving or sharing the pointer does not alias a thread-local buffer.
unsafe impl Send for HttpStr {}
unsafe impl Sync for HttpStr {}

impl HttpStr {
    /// `s` is `'static`. Drop will not free it.
    #[inline]
    pub(crate) fn from_static(s: &'static str) -> Self {
        // SAFETY: a `&str` pointer is never null, including the empty string.
        let ptr = unsafe { NonNull::new_unchecked(s.as_ptr() as *mut u8) };
        Self {
            ptr,
            len: s.len(),
            cap: 0,
        }
    }

    /// Take `s`'s allocation. An empty string has no allocation.
    #[inline]
    pub(crate) fn from_string(s: String) -> Self {
        let mut s = std::mem::ManuallyDrop::new(s);
        let len = s.len();
        let cap = s.capacity();
        // SAFETY: `String::as_mut_ptr` is non-null. `ManuallyDrop` leaves the
        // allocation (when `cap > 0`) for `Drop` to free.
        let ptr = unsafe { NonNull::new_unchecked(s.as_mut_ptr()) };
        Self { ptr, len, cap }
    }

    #[inline]
    pub(crate) fn from_scheme(s: &str) -> Self {
        match s {
            "http" => Self::from_static("http"),
            "https" => Self::from_static("https"),
            other => Self::owned(other),
        }
    }

    /// `leaked` is the process-lifetime copy of `s`, when one exists.
    #[inline]
    pub(crate) fn adopt(s: &str, leaked: Option<&'static str>) -> Self {
        match leaked {
            Some(st) => Self::from_static(st),
            None => Self::owned(s),
        }
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        // SAFETY: `ptr`/`len` came from a `String` or a `&str`, so the bytes
        // are valid UTF-8 for `len` and stay live until drop.
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(self.ptr.as_ptr(), self.len))
        }
    }

    pub(crate) fn into_cow(self) -> Cow<'static, str> {
        if self.cap == 0 {
            // SAFETY: `cap == 0` is only set for `'static` input. Drop does
            // not free those bytes.
            let st: &'static str = unsafe { &*(self.as_str() as *const str) };
            std::mem::forget(self);
            Cow::Borrowed(st)
        } else {
            Cow::Owned(self.into_string())
        }
    }

    fn into_string(self) -> String {
        let this = std::mem::ManuallyDrop::new(self);
        if this.cap == 0 {
            return this.as_str().to_owned();
        }
        // SAFETY: `cap > 0` is the allocation `from_string` took from a
        // `String`. `ManuallyDrop` skipped our `Drop`.
        unsafe { String::from_raw_parts(this.ptr.as_ptr(), this.len, this.cap) }
    }

    #[cold]
    #[inline(never)]
    fn owned(s: &str) -> Self {
        Self::from_string(s.to_owned())
    }
}

impl Clone for HttpStr {
    fn clone(&self) -> Self {
        if self.cap == 0 {
            Self {
                ptr: self.ptr,
                len: self.len,
                cap: 0,
            }
        } else {
            Self::owned(self.as_str())
        }
    }
}

impl Drop for HttpStr {
    #[inline]
    fn drop(&mut self) {
        if self.cap != 0 {
            free_name(self.ptr, self.cap);
        }
    }
}

impl std::ops::Deref for HttpStr {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for HttpStr {
    fn from(s: &str) -> Self {
        Self::owned(s)
    }
}

impl From<String> for HttpStr {
    fn from(s: String) -> Self {
        Self::from_string(s)
    }
}

#[cold]
#[inline(never)]
fn free_name(ptr: NonNull<u8>, cap: usize) {
    // SAFETY: `cap > 0` and `ptr` came from `String`, whose `u8` allocation
    // is `cap` bytes aligned to 1.
    unsafe {
        std::alloc::dealloc(
            ptr.as_ptr(),
            std::alloc::Layout::from_size_align_unchecked(cap, 1),
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "HTTP name ownership regression")]
    use super::HttpStr;

    #[test]
    fn owned_static_and_empty_names_survive_clone_and_consumption() {
        let mut reserved_empty = String::with_capacity(16);
        reserved_empty.clear();
        for name in [
            HttpStr::from_static("/svc/method"),
            HttpStr::from_string(String::from("example.test")),
            HttpStr::from_string(String::new()),
            HttpStr::from_string(reserved_empty),
        ] {
            let expected = name.as_str().to_owned();
            let cloned = name.clone();
            assert_eq!(name.into_cow(), expected);
            assert_eq!(cloned.into_cow(), expected);
        }
    }

    #[test]
    fn owned_name_can_be_shared_and_transferred_between_threads() {
        let name = HttpStr::from_string(String::from("example.test"));
        std::thread::scope(|scope| {
            scope.spawn(|| assert_eq!(name.as_str(), "example.test"));
            scope.spawn(|| assert_eq!(name.as_str(), "example.test"));
        });
        let result = std::thread::spawn(move || name.into_cow());
        assert_eq!(result.join().expect("name consumer"), "example.test");
    }
}
