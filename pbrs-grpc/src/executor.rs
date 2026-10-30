//! Scheduling of background HTTP/2 connection drivers.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// An owned HTTP/2 connection-driver task handed to an [`Executor`].
pub type ExecutorFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Schedule background HTTP/2 connection drivers.
///
/// Set this on [`crate::Endpoint::executor`], [`crate::Server::executor`] or
/// [`crate::Router::executor`]. [`tokio::runtime::Handle`] implements the trait,
/// so a dedicated Tokio runtime can own transport polling. A custom executor
/// can wrap a task to attach tracing or scheduling policy before running it.
///
/// `execute` must arrange to poll the future to completion or drop it. Keeping
/// an unpolled task indefinitely also retains its connection. Dropping a task
/// closes the connection and releases its admission and drain guards.
///
/// This selects the connection-driver scheduler, including the server handlers
/// polled inside that driver. It does not replace Tokio socket IO or timers, or
/// the Tokio scheduling of retry, upload, resolver and keepalive helper tasks.
/// An executor must provide the Tokio context those facilities require.
pub trait Executor: Send + Sync + 'static {
    /// Arrange to run `future` without blocking the caller.
    fn execute(&self, future: ExecutorFuture);
}

impl Executor for tokio::runtime::Handle {
    fn execute(&self, future: ExecutorFuture) {
        drop(self.spawn(future));
    }
}

impl<E: Executor + ?Sized> Executor for Arc<E> {
    fn execute(&self, future: ExecutorFuture) {
        self.as_ref().execute(future);
    }
}

/// Preserve the direct, unboxed Tokio path when no override is configured.
#[derive(Clone, Default)]
pub(crate) struct TaskExecutor(Option<Arc<dyn Executor>>);

pub(crate) static TOKIO_EXECUTOR: TaskExecutor = TaskExecutor(None);

pin_project_lite::pin_project! {
    struct Scheduled<F> {
        #[pin]
        future: F,
        // Fields drop in declaration order: transport state before scheduler
        // resources, including when a task is discarded before its first poll.
        _owner: Arc<dyn Executor>,
    }
}

impl<F: Future<Output = ()>> Future for Scheduled<F> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.project().future.poll(cx)
    }
}

impl TaskExecutor {
    pub(crate) fn new(executor: impl Executor) -> Self {
        Self(Some(Arc::new(executor)))
    }

    pub(crate) fn is_custom(&self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        match &self.0 {
            Some(executor) => {
                executor.execute(Box::pin(Scheduled {
                    future,
                    // A response stream can outlive its last Channel clone.
                    _owner: Arc::clone(executor),
                }));
            }
            None => drop(tokio::spawn(future)),
        }
    }
}

impl std::fmt::Debug for TaskExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskExecutor")
            .field("custom", &self.is_custom())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "drop-order and ownership assertions")]

    use super::*;
    use tokio::sync::Mutex;

    struct Queued {
        queue: Arc<Mutex<Vec<ExecutorFuture>>>,
        drops: Arc<Mutex<Vec<&'static str>>>,
    }
    impl Executor for Queued {
        fn execute(&self, future: ExecutorFuture) {
            self.queue.try_lock().unwrap().push(future);
        }
    }
    impl Drop for Queued {
        fn drop(&mut self) {
            self.drops.try_lock().unwrap().push("executor");
        }
    }
    struct Transport(Arc<Mutex<Vec<&'static str>>>);
    impl Future for Transport {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            Poll::Pending
        }
    }
    impl Drop for Transport {
        fn drop(&mut self) {
            self.0.try_lock().unwrap().push("transport");
        }
    }

    #[test]
    fn discarded_unpolled_driver_releases_transport_before_scheduler() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let drops = Arc::new(Mutex::new(Vec::new()));
        let executor = TaskExecutor::new(Queued {
            queue: Arc::clone(&queue),
            drops: Arc::clone(&drops),
        });
        executor.spawn(Transport(Arc::clone(&drops)));
        drop(executor);
        assert!(
            drops.try_lock().unwrap().is_empty(),
            "queued driver retains executor"
        );
        let task = queue.try_lock().unwrap().pop().unwrap();
        drop(task);
        assert_eq!(*drops.try_lock().unwrap(), ["transport", "executor"]);
    }
}
