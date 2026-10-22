#![forbid(unsafe_code)]

//! Request extensions published once, on the first connect-info lookup.
//!
//! The initial map stays immutable during publication, so references obtained
//! before publication remain valid. Exclusive access can modify either map.

use std::sync::OnceLock;

#[derive(Clone)]
pub(crate) struct LazyExt {
    initial: http::Extensions,
    published: OnceLock<http::Extensions>,
}

impl LazyExt {
    pub(crate) fn new() -> Self {
        Self::from_extensions(http::Extensions::new())
    }

    pub(crate) fn from_extensions(extensions: http::Extensions) -> Self {
        Self {
            initial: http::Extensions::new(),
            published: if extensions.is_empty() {
                OnceLock::new()
            } else {
                OnceLock::from(extensions)
            },
        }
    }

    pub(crate) fn get(&self) -> &http::Extensions {
        self.published.get().unwrap_or(&self.initial)
    }

    #[allow(
        clippy::expect_used,
        reason = "publish initializes the cell before exclusive access"
    )]
    pub(crate) fn get_mut(&mut self) -> &mut http::Extensions {
        self.publish(|_| {});
        self.published.get_mut().expect("published extensions")
    }

    /// Existing user context can receive connect info while the request is exclusive.
    pub(crate) fn initialized_mut(&mut self) -> Option<&mut http::Extensions> {
        self.published.get_mut()
    }

    /// Build and publish a map without modifying any previously exposed map.
    pub(crate) fn publish(&self, initialize: impl FnOnce(&mut http::Extensions)) {
        self.published.get_or_init(|| {
            let mut map = http::Extensions::new();
            initialize(&mut map);
            map
        });
    }

    pub(crate) fn into_inner(self) -> http::Extensions {
        self.published.into_inner().unwrap_or(self.initial)
    }
}

impl std::ops::Deref for LazyExt {
    type Target = http::Extensions;

    fn deref(&self) -> &http::Extensions {
        self.get()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "extension publication regressions")]

    use super::LazyExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn publication_preserves_preexisting_shared_references() {
        let extensions = LazyExt::new();
        let shared = extensions.get();
        extensions.publish(|map| {
            map.insert(42_u32);
        });
        assert!(shared.is_empty());
        assert_eq!(shared.get::<u32>(), None);
        assert_eq!(extensions.get().get::<u32>(), Some(&42));
    }

    #[test]
    fn concurrent_publication_initializes_once() {
        let extensions = LazyExt::new();
        let initializers = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for _ in 0..4 {
                workers.push(scope.spawn(|| {
                    let before = extensions.get();
                    extensions.publish(|map| {
                        initializers.fetch_add(1, Ordering::Relaxed);
                        map.insert(42_u32);
                    });
                    assert!(before.is_empty() || before.get::<u32>() == Some(&42));
                    assert_eq!(extensions.get().get::<u32>(), Some(&42));
                }));
            }
            for worker in workers {
                worker.join().expect("publisher");
            }
        });
        assert_eq!(initializers.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn exclusive_updates_and_consumption_use_the_published_map() {
        let mut extensions = LazyExt::new();
        extensions.publish(|map| {
            map.insert(42_u32);
        });
        extensions.get_mut().insert(7_u8);
        extensions.get_mut().insert(9_u8);
        let cloned = extensions.clone().into_inner();
        let owned = extensions.into_inner();
        for map in [cloned, owned] {
            assert_eq!(map.get::<u8>(), Some(&9));
            assert_eq!(map.get::<u32>(), Some(&42));
        }
    }
}
