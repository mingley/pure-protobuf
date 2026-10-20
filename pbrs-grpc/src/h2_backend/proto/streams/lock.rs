#![allow(
    unsafe_code,
    reason = "the stream lock is a futex word and an unsafe cell"
)]

//! Connection stream lock.
//!
//! The hot path is one compare-exchange. A parked waiter uses a private
//! futex. The lock does not track poison: a panicked holder is a bug, and
//! the standard poison check was a global atomic on every acquire.

use std::cell::UnsafeCell;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU32, Ordering};

const UNLOCKED: u32 = 0;
const LOCKED: u32 = 1;
const CONTENDED: u32 = 2;

/// Exclusive lock around connection stream state.
pub(super) struct Mutex<T> {
    state: AtomicU32,
    data: UnsafeCell<T>,
}

// SAFETY: the lock serializes access to `data`. `T: Send` lets the guard
// move the value across threads while it is held.
unsafe impl<T: Send> Send for Mutex<T> {}
// SAFETY: `&Mutex<T>` can be shared. The atomic state is the only shared
// mutable location, and `T: Send` covers the value a guard exposes.
unsafe impl<T: Send> Sync for Mutex<T> {}

/// The lock does not fail. This exists so callers can keep `.unwrap()`.
#[derive(Debug)]
pub(super) struct LockError;

/// Hold on a [`Mutex`].
pub(super) struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

impl<T> Mutex<T> {
    /// Wrap `value` in an unlocked mutex.
    pub(super) fn new(value: T) -> Self {
        Self {
            state: AtomicU32::new(UNLOCKED),
            data: UnsafeCell::new(value),
        }
    }

    /// Acquire the lock, waiting if another guard still holds it.
    #[inline]
    pub(super) fn lock(&self) -> Result<MutexGuard<'_, T>, LockError> {
        if self
            .state
            .compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return Ok(MutexGuard { mutex: self });
        }
        self.lock_slow();
        Ok(MutexGuard { mutex: self })
    }

    /// Acquire the lock only when it is free.
    #[inline]
    pub(super) fn try_lock(&self) -> Result<MutexGuard<'_, T>, LockError> {
        if self
            .state
            .compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Ok(MutexGuard { mutex: self })
        } else {
            Err(LockError)
        }
    }

    #[cold]
    #[inline(never)]
    fn lock_slow(&self) {
        let mut current = LOCKED;
        loop {
            if current == CONTENDED
                || self
                    .state
                    .compare_exchange(LOCKED, CONTENDED, Ordering::Relaxed, Ordering::Relaxed)
                    .is_err()
            {
                // SAFETY: `state` is the futex word. Wait only while it is
                // still contended; a wake or a spurious return retries.
                unsafe {
                    libc::syscall(
                        libc::SYS_futex,
                        self.state.as_ptr(),
                        libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG,
                        CONTENDED,
                        std::ptr::null::<libc::timespec>(),
                    );
                }
            }
            if self
                .state
                .compare_exchange(UNLOCKED, CONTENDED, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
            current = self.state.load(Ordering::Relaxed);
        }
    }

    #[inline]
    fn unlock(&self) {
        if self.state.fetch_sub(1, Ordering::Release) != LOCKED {
            self.unlock_slow();
        }
    }

    #[cold]
    #[inline(never)]
    fn unlock_slow(&self) {
        self.state.store(UNLOCKED, Ordering::Release);
        // SAFETY: `state` is the futex word. Wake one waiter after publishing
        // the unlocked value.
        unsafe {
            libc::syscall(
                libc::SYS_futex,
                self.state.as_ptr(),
                libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG,
                1i32,
            );
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.try_lock() {
            Ok(guard) => f.debug_struct("Mutex").field("data", &&*guard).finish(),
            Err(_) => f.write_str("Mutex { <locked> }"),
        }
    }
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: the guard holds the lock exclusively.
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the guard holds the lock exclusively.
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}
