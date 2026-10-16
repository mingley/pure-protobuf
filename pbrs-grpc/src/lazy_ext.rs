#![allow(unsafe_code, reason = "the cell is the same size as http::Extensions")]

//! Same-sized stand-in for [`http::Extensions`].
//!
//! Connect info is inserted on first read. The steady RPC never reads the
//! map, so accept does not allocate or hash it. The wrapper is transparent,
//! so a [`crate::Request`] does not grow.

use std::cell::UnsafeCell;
use std::sync::Mutex;

#[repr(transparent)]
pub(crate) struct LazyExt {
    inner: UnsafeCell<http::Extensions>,
}

impl LazyExt {
    pub(crate) fn new() -> Self {
        Self {
            inner: UnsafeCell::new(http::Extensions::new()),
        }
    }

    pub(crate) fn from_extensions(extensions: http::Extensions) -> Self {
        Self {
            inner: UnsafeCell::new(extensions),
        }
    }

    pub(crate) fn get(&self) -> &http::Extensions {
        // The map lives in this value for the value's whole lifetime. Callers
        // only get a shared borrow, and `Request` is not shared across threads.
        unsafe { &*self.inner.get() }
    }

    pub(crate) fn get_mut(&mut self) -> &mut http::Extensions {
        self.inner.get_mut()
    }

    /// Insert connect info through a shared borrow. The returned reference
    /// is the same map `get` hands out.
    pub(crate) fn with_mut<R>(&self, f: impl FnOnce(&mut http::Extensions) -> R) -> R {
        // One publication lock for every request. Two threads can both hold
        // `&Request` (it is `Sync`) and both read the map. The first call
        // inserts; later calls only look the key up. The lock orders that
        // write before any later lookup.
        static PUBLISH: Mutex<()> = Mutex::new(());
        let _guard = match PUBLISH.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        f(unsafe { &mut *self.inner.get() })
    }

    pub(crate) fn into_inner(self) -> http::Extensions {
        self.inner.into_inner()
    }
}

// SAFETY: the only write is the first `with_mut`, and that call holds
// `PUBLISH`. Later lookups only read the map. `get` does not write.
unsafe impl Sync for LazyExt {}

impl Clone for LazyExt {
    fn clone(&self) -> Self {
        Self::from_extensions(self.get().clone())
    }
}

impl std::ops::Deref for LazyExt {
    type Target = http::Extensions;

    fn deref(&self) -> &http::Extensions {
        self.get()
    }
}
