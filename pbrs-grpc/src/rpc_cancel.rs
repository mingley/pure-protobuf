#![allow(
    clippy::disallowed_types,
    reason = "short synchronous waker registry locks; no guard crosses an await"
)]

//! Per-RPC client cancel flag.
//!
//! One waiter uses the inline atomic waker. Concurrent upload, response and
//! hedge tasks register additional wakers on a cold path. Finished flags can
//! be reused after the previous call has dropped every handle.

use crate::binlog::CallLogger;
use atomic_waker::AtomicWaker;
use std::cell::Cell;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

struct Inner {
    cancelled: AtomicBool,
    log_capable: bool,
    /// Every sender handle is gone. Waiters wake, but this is not a cancel.
    closed: AtomicBool,
    senders: AtomicUsize,
    waker: AtomicWaker,
    primary_claimed: AtomicBool,
    additional: Mutex<Additional>,
}

#[derive(Default)]
struct Additional {
    waiters: Vec<Option<std::task::Waker>>,
    logging: Option<Box<LoggingState>>,
}

#[derive(Default)]
struct LoggingState {
    cancel_log: Option<CancelLog>,
    active_wakes: usize,
    wake_completed: bool,
}

struct CancelLog {
    logger: Option<CallLogger>,
    active_headers: usize,
    header_seen: bool,
    suppress_headers: bool,
    pending: bool,
    emitted: bool,
    finished: bool,
}

fn emit_cancel(logger: Option<CallLogger>) {
    if let Some(logger) = logger {
        // Sink code is user supplied. Never hold the waiter lock while calling
        // it, and do not let a sink panic prevent cancellation from waking.
        drop(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || logger.log_cancel(),
        )));
    }
}

/// Keeps unary cancel logging registered until that call finishes.
pub(crate) struct UnaryCancelLog {
    cancel: RpcCancel,
}

// Called with the registry locked. Claim only after cancellation wakeup has
// been attempted and, if a first header is active, after one header callback
// has returned. Return both owners so user sink/destructor code runs unlocked.
fn claim_cancel(
    inner: &Inner,
    additional: &mut Additional,
) -> (Option<CallLogger>, Option<CallLogger>) {
    let Some(logging) = additional.logging.as_mut() else {
        return (None, None);
    };
    let ready = inner.cancelled.load(Ordering::Acquire)
        && logging.wake_completed
        && logging.active_wakes == 0;
    let Some(log) = logging.cancel_log.as_mut() else {
        return (None, None);
    };
    if log.emitted || (log.finished && !log.pending) || !ready {
        return (None, None);
    }
    if !log.header_seen && log.active_headers != 0 {
        log.pending = true;
        return (None, None);
    }
    if !log.header_seen {
        log.suppress_headers = true;
    }
    log.pending = false;
    log.emitted = true;
    let event = log.logger.clone();
    let release = if log.finished {
        log.logger.take()
    } else {
        None
    };
    (event, release)
}

impl Drop for UnaryCancelLog {
    fn drop(&mut self) {
        let inner = self.cancel.inner();
        let (emit, release) = {
            let mut additional = inner.additional.lock().unwrap_or_else(|p| p.into_inner());
            // The registry lock orders this snapshot with logger setup
            // and the completion of a signal's wake pass.
            let cancelled = inner.cancelled.load(Ordering::Acquire);
            if let Some(log) = additional
                .logging
                .as_mut()
                .and_then(|state| state.cancel_log.as_mut())
            {
                log.finished = true;
                if cancelled && !log.emitted {
                    log.pending = true;
                }
            }
            let (emit, release) = claim_cancel(inner, &mut additional);
            if release.is_some() {
                (emit, release)
            } else if let Some(log) = additional
                .logging
                .as_mut()
                .and_then(|state| state.cancel_log.as_mut())
            {
                if !log.pending {
                    (emit, log.logger.take())
                } else {
                    (emit, None)
                }
            } else {
                (emit, None)
            }
        };
        emit_cancel(emit);
        drop(release);
    }
}

/// Completes one initial-header callback, including when that callback panics.
pub(crate) struct UnaryHeaderLog {
    cancel: RpcCancel,
}

impl Drop for UnaryHeaderLog {
    fn drop(&mut self) {
        let inner = self.cancel.inner();
        let (emit, release) = {
            let mut additional = inner.additional.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(log) = additional
                .logging
                .as_mut()
                .and_then(|state| state.cancel_log.as_mut())
            {
                log.active_headers = log.active_headers.saturating_sub(1);
                log.header_seen = true;
            }
            claim_cancel(inner, &mut additional)
        };
        emit_cancel(emit);
        drop(release);
    }
}

impl Inner {
    fn wake_waiters(&self) {
        self.waker.wake();
        // Always synchronize with secondary registration. Skipping this lock
        // using a relaxed waiter count could race a waiter's terminal check.
        let wakers: Vec<_> = self
            .additional
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .waiters
            .iter_mut()
            .filter_map(Option::take)
            .collect();
        // Executor callbacks may reenter a future; never call them locked.
        for waker in wakers {
            waker.wake();
        }
    }
}

// One finished call's flag. A live call on this thread holds its own clone.
thread_local! {
    static SPARE: Cell<Option<Arc<Inner>>> = const { Cell::new(None) };
    static SPARE_LOGGED: Cell<Option<Arc<Inner>>> = const { Cell::new(None) };
}

/// Cloneable cancel handle for one client RPC.
///
/// The flag is always present. `ManuallyDrop` lets `Drop` move the `Arc`
/// out without a discriminant check on every clone and drop.
pub(crate) struct RpcCancel {
    inner: ManuallyDrop<Arc<Inner>>,
}

impl std::fmt::Debug for RpcCancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcCancel")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl RpcCancel {
    pub(crate) fn new() -> Self {
        Self::new_inner::<false>()
    }

    /// The immutable capability is chosen before a call handle can signal.
    pub(crate) fn new_unary(log_capable: bool) -> Self {
        if log_capable {
            Self::new_inner::<true>()
        } else {
            Self::new()
        }
    }

    fn new_inner<const LOG_CAPABLE: bool>() -> Self {
        let cached = if LOG_CAPABLE {
            SPARE_LOGGED.try_with(Cell::take).ok().flatten()
        } else {
            SPARE.try_with(Cell::take).ok().flatten()
        };
        if let Some(inner) = cached {
            inner.cancelled.store(false, Ordering::Release);
            inner.closed.store(false, Ordering::Release);
            inner.senders.store(1, Ordering::Release);
            if LOG_CAPABLE {
                let previous = {
                    let mut additional = inner.additional.lock().unwrap_or_else(|p| p.into_inner());
                    additional.logging.as_mut().and_then(|state| {
                        state.active_wakes = 0;
                        state.wake_completed = false;
                        state.cancel_log.take()
                    })
                };
                drop(previous);
            }
            return Self {
                inner: ManuallyDrop::new(inner),
            };
        }
        Self {
            inner: ManuallyDrop::new(Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                log_capable: LOG_CAPABLE,
                closed: AtomicBool::new(false),
                senders: AtomicUsize::new(1),
                waker: AtomicWaker::new(),
                primary_claimed: AtomicBool::new(false),
                additional: Mutex::new(Additional::default()),
            })),
        }
    }

    fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }

    /// Mark the call cancelled and wake whoever is waiting.
    pub(crate) fn signal(&self) {
        let inner = self.inner();
        // This store remains before every mutex acquisition for all calls.
        inner.cancelled.store(true, Ordering::Release);
        if inner.log_capable {
            Self::signal_with_log(inner);
        } else {
            inner.wake_waiters();
        }
    }

    /// Signal while dropping a call whose future cannot be polled again.
    ///
    /// Only `Call::drop` may use this: the future cannot be polled again.
    /// The owning unary future is currently the only production registrar,
    /// and it registers before spawning hedges. Once drop begins, that future
    /// cannot be polled again, so an absent registration cannot appear later.
    /// If streaming or a spawned task can register later, this shortcut must
    /// be removed or revalidated with a new registration handoff.
    pub(crate) fn signal_on_drop(&self) {
        let inner = self.inner();
        inner.cancelled.store(true, Ordering::Release);
        if inner.log_capable && Self::drop_has_registration(inner) {
            Self::signal_with_log(inner);
        } else {
            inner.wake_waiters();
        }
    }

    #[cold]
    #[inline(never)]
    fn drop_has_registration(inner: &Inner) -> bool {
        let additional = inner.additional.lock().unwrap_or_else(|p| p.into_inner());
        additional
            .logging
            .as_ref()
            .is_some_and(|state| state.cancel_log.is_some())
    }

    #[cold]
    #[inline(never)]
    fn signal_with_log(inner: &Inner) {
        {
            let mut additional = inner.additional.lock().unwrap_or_else(|p| p.into_inner());
            let logging = additional.logging.get_or_insert_with(Box::default);
            logging.active_wakes = logging.active_wakes.checked_add(1).unwrap_or_else(|| {
                std::process::abort();
            });
            logging.wake_completed = false;
        }
        let woken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inner.wake_waiters();
        }));
        let (emit, release) = {
            let mut additional = inner.additional.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(logging) = additional.logging.as_mut() {
                logging.active_wakes = logging.active_wakes.saturating_sub(1);
                if logging.active_wakes == 0 {
                    logging.wake_completed = true;
                }
            }
            claim_cancel(inner, &mut additional)
        };
        emit_cancel(emit);
        drop(release);
        if let Err(payload) = woken {
            std::panic::resume_unwind(payload);
        }
    }

    /// Register a call-level client logger. A cancellation that already won
    /// the race suppresses the first header and is emitted after wakeup.
    pub(crate) fn observe_unary_cancel(&self, logger: &CallLogger) -> UnaryCancelLog {
        let inner = self.inner();
        debug_assert!(inner.log_capable, "logger requires a logging-capable call");
        let (previous, emit, release) = {
            let mut additional = inner.additional.lock().unwrap_or_else(|p| p.into_inner());
            let cancelled = inner.cancelled.load(Ordering::Acquire);
            let next = CancelLog {
                logger: Some(logger.clone()),
                active_headers: 0,
                header_seen: false,
                suppress_headers: cancelled,
                pending: cancelled,
                emitted: false,
                finished: false,
            };
            let previous = additional
                .logging
                .get_or_insert_with(Box::default)
                .cancel_log
                .replace(next);
            let (emit, release) = claim_cancel(inner, &mut additional);
            (previous, emit, release)
        };
        // A logger or sink destructor may run user code.
        drop(previous);
        emit_cancel(emit);
        drop(release);
        UnaryCancelLog {
            cancel: self.clone(),
        }
    }

    /// Reserve a header callback. A cancellation concurrent with the first
    /// header waits until one callback completes. Hedge callbacks may overlap.
    pub(crate) fn begin_unary_header(&self) -> Option<UnaryHeaderLog> {
        let inner = self.inner();
        let (allowed, emit, release) = {
            let mut additional = inner.additional.lock().unwrap_or_else(|p| p.into_inner());
            let cancelled = inner.cancelled.load(Ordering::Acquire);
            let allowed = match additional
                .logging
                .as_mut()
                .and_then(|state| state.cancel_log.as_mut())
            {
                Some(log) if !log.finished && !log.suppress_headers && !cancelled => {
                    if let Some(next) = log.active_headers.checked_add(1) {
                        log.active_headers = next;
                        true
                    } else {
                        false
                    }
                }
                Some(log) if !log.finished && cancelled && !log.emitted => {
                    log.pending = true;
                    if !log.header_seen && log.active_headers == 0 {
                        log.suppress_headers = true;
                    }
                    false
                }
                _ => false,
            };
            let (emit, release) = claim_cancel(inner, &mut additional);
            (allowed, emit, release)
        };
        emit_cancel(emit);
        drop(release);
        allowed.then(|| UnaryHeaderLog {
            cancel: self.clone(),
        })
    }

    #[inline]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner().cancelled.load(Ordering::Acquire)
    }

    /// Resolves when the call is cancelled or every sender handle is gone.
    pub(crate) fn wait(&self) -> Wait {
        Wait {
            inner: Arc::clone(self.inner()),
            registration: Registration::None,
        }
    }
}

impl Clone for RpcCancel {
    fn clone(&self) -> Self {
        let inner = self.inner();
        inner.senders.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: ManuallyDrop::new(Arc::clone(inner)),
        }
    }
}

impl Drop for RpcCancel {
    #[allow(unsafe_code, reason = "move the flag out of ManuallyDrop exactly once")]
    fn drop(&mut self) {
        // SAFETY: `inner` is initialized by every constructor and this is the
        // only place it is moved out. `ManuallyDrop` does not drop the `Arc`.
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        if inner.senders.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        // Last sender. A waiter still holding the flag must observe close.
        // Nobody waiting: keep the allocation for the next call.
        if Arc::strong_count(&inner) == 1 {
            inner.cancelled.store(false, Ordering::Release);
            inner.closed.store(false, Ordering::Release);
            drop(inner.waker.take());
            let log_capable = inner.log_capable;
            let cache = |cell: &Cell<Option<Arc<Inner>>>| {
                let existing = cell.take();
                if existing.is_none() {
                    cell.set(Some(inner));
                } else {
                    cell.set(existing);
                }
            };
            if log_capable {
                SPARE_LOGGED.try_with(cache).unwrap_or_default();
            } else {
                SPARE.try_with(cache).unwrap_or_default();
            }
            return;
        }
        inner.closed.store(true, Ordering::Release);
        inner.wake_waiters();
    }
}

/// [`RpcCancel::wait`] future.
pub(crate) struct Wait {
    inner: Arc<Inner>,
    registration: Registration,
}

enum Registration {
    None,
    Primary,
    Additional(usize),
}

impl Wait {
    #[inline]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }
}

impl Future for Wait {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if this.inner.cancelled.load(Ordering::Acquire) || this.inner.closed.load(Ordering::Acquire)
        {
            return Poll::Ready(());
        }
        if matches!(this.registration, Registration::None) {
            if this
                .inner
                .primary_claimed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                this.registration = Registration::Primary;
            } else {
                // Cloning or dropping a waker can run executor code. Keep
                // both outside the registry lock so callbacks can cancel.
                let waker = cx.waker().clone();
                let mut waiters = this
                    .inner
                    .additional
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                let waiters = &mut waiters.waiters;
                let index = if let Some((index, slot)) = waiters
                    .iter_mut()
                    .enumerate()
                    .find(|(_, slot)| slot.is_none())
                {
                    *slot = Some(waker);
                    index
                } else {
                    waiters.push(Some(waker));
                    waiters.len() - 1
                };
                this.registration = Registration::Additional(index);
            }
        }
        match this.registration {
            Registration::Primary => this.inner.waker.register(cx.waker()),
            Registration::Additional(index) => {
                let needs_update = {
                    let waiters = this
                        .inner
                        .additional
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    waiters
                        .waiters
                        .get(index)
                        .and_then(Option::as_ref)
                        .is_some_and(|waker| !waker.will_wake(cx.waker()))
                };
                if needs_update {
                    let mut replacement = Some(cx.waker().clone());
                    let previous = {
                        let mut waiters = this
                            .inner
                            .additional
                            .lock()
                            .unwrap_or_else(|p| p.into_inner());
                        match waiters.waiters.get_mut(index) {
                            Some(slot) if slot.is_some() => {
                                std::mem::replace(slot, replacement.take())
                            }
                            _ => None,
                        }
                    };
                    drop(previous);
                    // Cancellation may have removed the registration while
                    // the replacement was cloned. Drop an unused clone here.
                    drop(replacement);
                }
            }
            Registration::None => {}
        }
        if this.inner.cancelled.load(Ordering::Acquire) || this.inner.closed.load(Ordering::Acquire)
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for Wait {
    fn drop(&mut self) {
        match self.registration {
            Registration::None => {}
            Registration::Primary => {
                drop(self.inner.waker.take());
                self.inner.primary_claimed.store(false, Ordering::Release);
            }
            Registration::Additional(index) => {
                let waker = self
                    .inner
                    .additional
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .waiters
                    .get_mut(index)
                    .and_then(Option::take);
                drop(waker);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "cancellation regressions"
    )]
    use super::RpcCancel;
    use std::future::Future;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Context, Wake, Waker};

    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn waker() -> (Arc<Count>, Waker) {
        let count = Arc::new(Count(AtomicUsize::new(0)));
        (count.clone(), Waker::from(count))
    }

    fn client_log() -> (Arc<crate::binlog::VecSink>, crate::binlog::CallLogger) {
        let sink = Arc::new(crate::binlog::VecSink::new());
        let logger = crate::binlog::BinaryLogger::new(
            crate::binlog::BinaryLogFilter::parse("*").expect("filter"),
            sink.clone(),
        );
        let tap = logger
            .start_call("/test.Service/Call", crate::binlog::Logger::Client)
            .expect("log this method");
        (sink, tap)
    }

    fn events(sink: &crate::binlog::VecSink) -> Vec<crate::binlog::EventType> {
        sink.records().iter().map(|r| r.entry.event).collect()
    }

    #[test]
    fn unary_cancel_log_orders_header_and_finalizes() {
        use crate::binlog::EventType;

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let guard = cancel.observe_unary_cancel(&tap);
        let header = cancel.begin_unary_header().expect("header reserved");
        cancel.signal();
        assert!(sink.is_empty(), "cancel waits for the header sink");
        tap.log_client_header(&crate::Metadata::new(), "/test.Service/Call", "host", None);
        drop(header);
        assert_eq!(events(&sink), [EventType::ClientHeader, EventType::Cancel]);
        cancel.signal();
        drop(guard);
        cancel.signal();
        assert_eq!(events(&sink), [EventType::ClientHeader, EventType::Cancel]);

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let guard = cancel.observe_unary_cancel(&tap);
        cancel.signal();
        assert!(cancel.begin_unary_header().is_none());
        assert_eq!(events(&sink), [EventType::Cancel]);
        drop(guard);
        cancel.signal();
        assert_eq!(events(&sink), [EventType::Cancel]);

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        cancel.signal();
        let guard = cancel.observe_unary_cancel(&tap);
        assert!(cancel.begin_unary_header().is_none());
        assert_eq!(events(&sink), [EventType::Cancel]);
        drop(guard);

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let guard = cancel.observe_unary_cancel(&tap);
        drop(guard);
        cancel.signal();
        assert!(sink.is_empty(), "post-completion handle cannot log");
    }

    #[test]
    fn unary_cancel_log_ignores_last_sender_closure() {
        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let mut waiter = Box::pin(cancel.wait());
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        let guard = cancel.observe_unary_cancel(&tap);
        drop(cancel);
        drop(guard);
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready()
        );
        assert!(!waiter.is_cancelled());
        assert!(sink.is_empty(), "sender closure is not cancellation");
    }

    #[test]
    fn unlogged_cancel_is_visible_while_registry_is_locked() {
        let cancel = RpcCancel::new();
        let other = cancel.clone();
        let (started, receiver) = std::sync::mpsc::channel();
        let registry = cancel.inner().additional.lock().expect("registry");
        let thread = std::thread::spawn(move || {
            started.send(()).expect("receiver");
            other.signal();
        });
        receiver.recv().expect("cancel thread started");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !cancel.is_cancelled() {
            assert!(
                std::time::Instant::now() < deadline,
                "cancel flag waited for the registry lock"
            );
            std::thread::yield_now();
        }
        drop(registry);
        thread.join().expect("cancel thread");
    }

    #[test]
    fn unary_cancel_wakes_waiter_before_sink_runs() {
        use crate::binlog::{BinaryLogFilter, BinaryLogger, EventType, LogRecord, Logger, Sink};

        struct Notify(std::sync::mpsc::Sender<()>);
        impl Wake for Notify {
            fn wake(self: Arc<Self>) {
                self.0.send(()).expect("sink receiver");
            }
        }
        #[derive(Debug)]
        struct WaitForWake {
            receiver: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
            observed: std::sync::atomic::AtomicBool,
        }
        impl Sink for WaitForWake {
            fn emit(&self, record: &LogRecord) {
                if record.entry.event == EventType::Cancel {
                    let received = self
                        .receiver
                        .lock()
                        .expect("receiver lock")
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .is_ok();
                    self.observed.store(received, Ordering::SeqCst);
                }
            }
        }

        let (sender, receiver) = std::sync::mpsc::channel();
        let sink = Arc::new(WaitForWake {
            receiver: std::sync::Mutex::new(receiver),
            observed: std::sync::atomic::AtomicBool::new(false),
        });
        let binary = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
        let tap = binary
            .start_call("/test.Service/Call", Logger::Client)
            .expect("tap");
        let cancel = RpcCancel::new_unary(true);
        let _guard = cancel.observe_unary_cancel(&tap);
        drop(cancel.begin_unary_header().expect("header"));
        let waker = Waker::from(Arc::new(Notify(sender)));
        let mut waiter = Box::pin(cancel.wait());
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        cancel.signal();
        assert!(sink.observed.load(Ordering::SeqCst), "sink ran before wake");
    }

    #[test]
    fn unary_pending_cancel_outlives_coordinator_until_header_finishes() {
        use crate::binlog::EventType;

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let call = cancel.observe_unary_cancel(&tap);
        let header = cancel.begin_unary_header().expect("header reserved");
        cancel.signal();
        assert!(sink.is_empty(), "header callback has not returned");
        drop(call);
        assert!(sink.is_empty(), "finalization must not overtake header");
        tap.log_client_header(&crate::Metadata::new(), "/test.Service/Call", "host", None);
        drop(header);
        assert_eq!(events(&sink), [EventType::ClientHeader, EventType::Cancel]);
        cancel.signal();
        assert_eq!(events(&sink), [EventType::ClientHeader, EventType::Cancel]);
    }

    #[test]
    fn unary_overlapping_hedge_headers_keep_cancel_ordered() {
        use crate::binlog::{EventType, LogRecord, Sink, VecSink};

        #[derive(Debug)]
        struct BlockFirst {
            sink: VecSink,
            entered: std::sync::mpsc::Sender<()>,
            release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
            first: std::sync::atomic::AtomicBool,
        }
        impl Sink for BlockFirst {
            fn emit(&self, record: &LogRecord) {
                if record.entry.event == EventType::ClientHeader
                    && !self.first.swap(true, Ordering::SeqCst)
                {
                    self.entered.send(()).expect("first callback witness");
                    self.release
                        .lock()
                        .expect("release lock")
                        .recv_timeout(std::time::Duration::from_secs(3))
                        .expect("release first callback");
                }
                self.sink.emit(record);
            }
        }

        let (entered, first_entered) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let sink = Arc::new(BlockFirst {
            sink: VecSink::new(),
            entered,
            release: std::sync::Mutex::new(released),
            first: std::sync::atomic::AtomicBool::new(false),
        });
        let binary = crate::binlog::BinaryLogger::new(
            crate::binlog::BinaryLogFilter::parse("*").expect("filter"),
            sink.clone(),
        );
        let tap = binary
            .start_call("/test.Service/Call", crate::binlog::Logger::Client)
            .expect("tap");
        let cancel = RpcCancel::new_unary(true);
        let call = cancel.observe_unary_cancel(&tap);
        let first_cancel = cancel.clone();
        let first_tap = tap.clone();
        let first = std::thread::spawn(move || {
            let _header = first_cancel.begin_unary_header().expect("first header");
            first_tap.log_client_header(
                &crate::Metadata::new(),
                "/test.Service/Call",
                "host",
                None,
            );
        });
        first_entered
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("first header is blocked");
        let second = cancel.begin_unary_header().expect("second header");
        tap.log_client_header(&crate::Metadata::new(), "/test.Service/Call", "host", None);
        drop(second);
        cancel.signal();
        assert_eq!(
            events(&sink.sink),
            [EventType::ClientHeader, EventType::Cancel]
        );
        drop(call);
        release.send(()).expect("release header");
        first.join().expect("first header task");
        assert_eq!(
            events(&sink.sink),
            [
                EventType::ClientHeader,
                EventType::Cancel,
                EventType::ClientHeader
            ]
        );
        cancel.signal();
        assert_eq!(
            events(&sink.sink)
                .iter()
                .filter(|event| **event == EventType::Cancel)
                .count(),
            1
        );
    }

    #[test]
    fn unary_cancel_survives_registration_and_finalization_during_wake() {
        use crate::binlog::EventType;

        struct BlockWake {
            entered: std::sync::mpsc::Sender<()>,
            release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl Wake for BlockWake {
            fn wake(self: Arc<Self>) {
                self.entered.send(()).expect("wake entered");
                self.release
                    .lock()
                    .expect("wake lock")
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .expect("release wake");
            }
        }

        for register_during_wake in [false, true] {
            let (sink, tap) = client_log();
            let cancel = RpcCancel::new_unary(true);
            let mut call = if register_during_wake {
                None
            } else {
                let guard = cancel.observe_unary_cancel(&tap);
                drop(cancel.begin_unary_header().expect("header"));
                Some(guard)
            };
            let (entered, waking) = std::sync::mpsc::channel();
            let (release, released) = std::sync::mpsc::channel();
            let waker = Waker::from(Arc::new(BlockWake {
                entered,
                release: std::sync::Mutex::new(released),
            }));
            let mut waiter = Box::pin(cancel.wait());
            assert!(
                waiter
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            let signal = cancel.clone();
            let worker = std::thread::spawn(move || signal.signal());
            waking
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("signal entered wake");
            if register_during_wake {
                call = Some(cancel.observe_unary_cancel(&tap));
            } else {
                drop(call.take());
            }
            assert!(sink.is_empty(), "sink must wait for wake to finish");
            release.send(()).expect("release wake");
            worker.join().expect("signal completed");
            drop(call);
            assert_eq!(events(&sink), [EventType::Cancel]);
        }
    }

    #[test]
    fn unary_header_guard_releases_pending_cancel_during_unwind() {
        use crate::binlog::{EventType, LogRecord, Sink, VecSink};

        #[derive(Debug)]
        struct PanicHeader(VecSink);
        impl Sink for PanicHeader {
            #[allow(clippy::panic, reason = "exercise unwinding from the header sink")]
            fn emit(&self, record: &LogRecord) {
                if record.entry.event == EventType::ClientHeader {
                    panic!("header callback panic");
                }
                self.0.emit(record);
            }
        }

        let sink = Arc::new(PanicHeader(VecSink::new()));
        let binary = crate::binlog::BinaryLogger::new(
            crate::binlog::BinaryLogFilter::parse("*").expect("filter"),
            sink.clone(),
        );
        let tap = binary
            .start_call("/test.Service/Call", crate::binlog::Logger::Client)
            .expect("tap");
        let cancel = RpcCancel::new_unary(true);
        let call = cancel.observe_unary_cancel(&tap);
        let header = cancel.begin_unary_header().expect("header");
        cancel.signal();
        drop(call);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _header = header;
            tap.log_client_header(&crate::Metadata::new(), "/test.Service/Call", "host", None);
        }));
        assert!(result.is_err(), "header sink must panic");
        assert_eq!(events(&sink.0), [EventType::Cancel]);
        cancel.signal();
        assert_eq!(events(&sink.0), [EventType::Cancel]);
    }

    #[test]
    fn unary_cancel_capability_reuses_separate_spares() {
        use crate::binlog::EventType;

        let logged = RpcCancel::new_unary(true);
        assert!(logged.inner().log_capable);
        logged.signal();
        drop(logged);

        let plain = RpcCancel::new();
        assert!(!plain.inner().log_capable);
        assert!(!plain.is_cancelled());
        plain.signal();
        drop(plain);

        let (sink, tap) = client_log();
        let logged = RpcCancel::new_unary(true);
        assert!(logged.inner().log_capable);
        assert!(!logged.is_cancelled());
        let _guard = logged.observe_unary_cancel(&tap);
        logged.signal();
        assert_eq!(events(&sink), [EventType::Cancel]);
    }

    #[test]
    fn nested_unary_wake_finishes_before_cancel_sink_runs() {
        use crate::binlog::{BinaryLogFilter, BinaryLogger, EventType, LogRecord, Logger, Sink};

        struct Reenter {
            cancel: RpcCancel,
            finished: std::sync::mpsc::Sender<()>,
        }
        impl Wake for Reenter {
            fn wake(self: Arc<Self>) {
                self.cancel.signal();
                self.finished.send(()).expect("sink receiver");
            }
        }
        #[derive(Debug)]
        struct WaitForOuterWake {
            receiver: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
            observed: std::sync::atomic::AtomicBool,
        }
        impl Sink for WaitForOuterWake {
            fn emit(&self, record: &LogRecord) {
                if record.entry.event == EventType::Cancel {
                    let received = self
                        .receiver
                        .lock()
                        .expect("receiver lock")
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .is_ok();
                    self.observed.store(received, Ordering::SeqCst);
                }
            }
        }

        let cancel = RpcCancel::new_unary(true);
        let (finished, receiver) = std::sync::mpsc::channel();
        let sink = Arc::new(WaitForOuterWake {
            receiver: std::sync::Mutex::new(receiver),
            observed: std::sync::atomic::AtomicBool::new(false),
        });
        let binary = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
        let tap = binary
            .start_call("/test.Service/Call", Logger::Client)
            .expect("tap");
        let _guard = cancel.observe_unary_cancel(&tap);
        drop(cancel.begin_unary_header().expect("header"));
        let waker = Waker::from(Arc::new(Reenter {
            cancel: cancel.clone(),
            finished,
        }));
        let mut waiter = Box::pin(cancel.wait());
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        cancel.signal();
        assert!(
            sink.observed.load(Ordering::SeqCst),
            "sink ran before the outer reentrant wake finished"
        );
    }

    #[test]
    fn unary_cancel_registration_races_do_not_lose_signals() {
        use crate::binlog::EventType;

        for _ in 0..64 {
            let (sink, tap) = client_log();
            let cancel = RpcCancel::new_unary(true);
            let other = cancel.clone();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let worker_barrier = barrier.clone();
            let worker = std::thread::spawn(move || {
                worker_barrier.wait();
                other.signal();
            });
            barrier.wait();
            let guard = cancel.observe_unary_cancel(&tap);
            worker.join().expect("signal");
            drop(guard);
            assert_eq!(events(&sink), [EventType::Cancel]);
        }
    }

    #[test]
    fn unary_cancel_and_finalization_emit_at_most_once() {
        use crate::binlog::EventType;

        for _ in 0..64 {
            let (sink, tap) = client_log();
            let cancel = RpcCancel::new_unary(true);
            let guard = cancel.observe_unary_cancel(&tap);
            let other = cancel.clone();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let worker_barrier = barrier.clone();
            let worker = std::thread::spawn(move || {
                worker_barrier.wait();
                other.signal();
            });
            barrier.wait();
            drop(guard);
            worker.join().expect("signal");
            assert!(
                events(&sink)
                    .iter()
                    .filter(|event| **event == EventType::Cancel)
                    .count()
                    <= 1
            );
        }
    }

    #[test]
    fn unary_cancel_log_survives_primary_waker_panic() {
        use crate::binlog::EventType;
        struct PanicOnWake;
        impl Wake for PanicOnWake {
            #[allow(clippy::panic, reason = "check a panicking executor callback")]
            fn wake(self: Arc<Self>) {
                panic!("primary waker panic");
            }
        }

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let _guard = cancel.observe_unary_cancel(&tap);
        let header = cancel.begin_unary_header().expect("header reserved");
        tap.log_client_header(&crate::Metadata::new(), "/test.Service/Call", "host", None);
        drop(header);
        let mut waiter = Box::pin(cancel.wait());
        let waker = Waker::from(Arc::new(PanicOnWake));
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cancel.signal())).is_err()
        );
        assert!(cancel.is_cancelled());
        assert_eq!(events(&sink), [EventType::ClientHeader, EventType::Cancel]);
    }

    #[test]
    fn unary_cancel_log_sink_panic_does_not_block_waking() {
        #[derive(Debug)]
        struct PanicSink;
        impl crate::binlog::Sink for PanicSink {
            #[allow(clippy::panic, reason = "check a panicking binary-log sink")]
            fn emit(&self, _record: &crate::binlog::LogRecord) {
                panic!("sink panic");
            }
        }
        let logger = crate::binlog::BinaryLogger::new(
            crate::binlog::BinaryLogFilter::parse("*").expect("filter"),
            Arc::new(PanicSink),
        );
        let tap = logger
            .start_call("/test.Service/Call", crate::binlog::Logger::Client)
            .expect("log this method");
        let cancel = RpcCancel::new_unary(true);
        let _guard = cancel.observe_unary_cancel(&tap);
        drop(cancel.begin_unary_header().expect("header reserved"));
        let (count, waker) = waker();
        let mut waiter = Box::pin(cancel.wait());
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        cancel.signal();
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn unary_cancel_log_sink_can_reenter_cancellation() {
        use crate::binlog::{EventType, LogRecord, Sink, VecSink};

        #[derive(Debug)]
        struct Reentrant {
            cancel: RpcCancel,
            sink: VecSink,
        }
        impl Sink for Reentrant {
            fn emit(&self, record: &LogRecord) {
                self.sink.emit(record);
                if record.entry.event == EventType::Cancel {
                    self.cancel.signal();
                }
            }
        }

        let cancel = RpcCancel::new_unary(true);
        let sink = Arc::new(Reentrant {
            cancel: cancel.clone(),
            sink: VecSink::new(),
        });
        let logger = crate::binlog::BinaryLogger::new(
            crate::binlog::BinaryLogFilter::parse("*").expect("filter"),
            sink.clone(),
        );
        let tap = logger
            .start_call("/test.Service/Call", crate::binlog::Logger::Client)
            .expect("log this method");
        let _guard = cancel.observe_unary_cancel(&tap);
        drop(cancel.begin_unary_header().expect("header reserved"));
        let (done, received) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            cancel.signal();
            done.send(()).expect("receiver");
        });
        received
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("sink callback ran without the waiter lock");
        thread.join().expect("cancel thread");
        assert_eq!(events(&sink.sink), [EventType::Cancel]);
    }

    #[test]
    fn unary_cancel_log_parallel_signals_emit_once() {
        use crate::binlog::EventType;

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let _guard = cancel.observe_unary_cancel(&tap);
        drop(cancel.begin_unary_header().expect("header reserved"));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let cancel = cancel.clone();
                drop(scope.spawn(move || cancel.signal()));
            }
        });
        assert_eq!(events(&sink), [EventType::Cancel]);
    }

    #[test]
    fn public_handle_preserves_unwind_traits_and_cancellation_after_waker_panic() {
        fn public_traits<
            T: Clone + Send + Sync + std::panic::UnwindSafe + std::panic::RefUnwindSafe,
        >() {
        }
        public_traits::<crate::CallHandle>();

        struct PanicOnWake;
        impl Wake for PanicOnWake {
            #[allow(
                clippy::panic,
                reason = "exercise a caller-supplied executor callback panic"
            )]
            fn wake(self: Arc<Self>) {
                panic!("executor wake failed");
            }
        }

        let cancel = RpcCancel::new();
        let call = crate::Call::<()>::new(cancel.clone(), Box::pin(std::future::pending()));
        let handle = call.handle();
        let mut primary = Box::pin(cancel.wait());
        let panic_waker = Waker::from(Arc::new(PanicOnWake));
        assert!(
            primary
                .as_mut()
                .poll(&mut Context::from_waker(&panic_waker))
                .is_pending()
        );
        let (secondary_count, secondary_waker) = waker();
        let mut secondary = Box::pin(cancel.wait());
        assert!(
            secondary
                .as_mut()
                .poll(&mut Context::from_waker(&secondary_waker))
                .is_pending()
        );

        // This compiles without an assertion at the user's catch boundary.
        // A primary executor panic happens before the secondary wake loop.
        assert!(std::panic::catch_unwind(|| handle.cancel()).is_err());
        assert!(handle.is_cancelled());
        assert_eq!(secondary_count.0.load(Ordering::SeqCst), 0);

        // The primary waker was removed before it panicked; retrying cannot
        // encounter it again and still wakes the remaining registered task.
        let cloned = handle.clone();
        std::panic::catch_unwind(|| cloned.cancel()).expect("cancellation remains usable");
        assert!(cloned.is_cancelled());
        assert_eq!(secondary_count.0.load(Ordering::SeqCst), 1);
        assert!(
            primary
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready()
        );
        assert!(
            secondary
                .as_mut()
                .poll(&mut Context::from_waker(&secondary_waker))
                .is_ready()
        );
    }

    #[test]
    fn replacing_secondary_waker_allows_its_drop_to_cancel() {
        struct CancelOnDrop(RpcCancel);
        #[allow(
            clippy::manual_noop_waker,
            reason = "the waker's final owner must run a reentrant cancellation destructor"
        )]
        impl Wake for CancelOnDrop {
            fn wake(self: Arc<Self>) {}
        }
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                self.0.signal();
            }
        }

        let cancel = RpcCancel::new();
        let mut primary = Box::pin(cancel.wait());
        assert!(
            primary
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        let mut secondary = Box::pin(cancel.wait());
        let waker = Waker::from(Arc::new(CancelOnDrop(cancel.clone())));
        assert!(
            secondary
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(waker);

        let (sent, received) = std::sync::mpsc::channel();
        let task = std::thread::spawn(move || {
            assert!(
                secondary
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
                    .is_ready()
            );
            sent.send(()).expect("test receiver exists");
        });
        received
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("waker drop must run outside the registry lock");
        task.join().expect("poll completes");
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn cancellation_wakes_every_registered_task() {
        let cancel = RpcCancel::new();
        let (a, wa) = waker();
        let (b, wb) = waker();
        let mut first = Box::pin(cancel.wait());
        let mut second = Box::pin(cancel.wait());
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_pending()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_pending()
        );
        cancel.signal();
        assert_eq!(a.0.load(Ordering::SeqCst), 1);
        assert_eq!(b.0.load(Ordering::SeqCst), 1);
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_ready()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_ready()
        );
    }

    #[test]
    fn dropping_primary_and_replacing_it_preserves_other_waiters() {
        let cancel = RpcCancel::new();
        let (a, wa) = waker();
        let (b, wb) = waker();
        let (c, wc) = waker();
        let mut first = Box::pin(cancel.wait());
        let mut second = Box::pin(cancel.wait());
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_pending()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_pending()
        );
        drop(first);
        let mut third = Box::pin(cancel.wait());
        assert!(
            third
                .as_mut()
                .poll(&mut Context::from_waker(&wc))
                .is_pending()
        );
        cancel.signal();
        assert_eq!(a.0.load(Ordering::SeqCst), 0);
        assert_eq!(b.0.load(Ordering::SeqCst), 1);
        assert_eq!(c.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn final_sender_closure_wakes_waiters_without_cancelling() {
        let cancel = RpcCancel::new();
        let (a, wa) = waker();
        let (b, wb) = waker();
        let mut first = Box::pin(cancel.wait());
        let mut second = Box::pin(cancel.wait());
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_pending()
        );
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_pending()
        );
        drop(cancel);
        assert_eq!(a.0.load(Ordering::SeqCst), 1);
        assert_eq!(b.0.load(Ordering::SeqCst), 1);
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&wa))
                .is_ready()
        );
        assert!(!first.is_cancelled());
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(&wb))
                .is_ready()
        );
        assert!(!second.is_cancelled());
    }

    #[test]
    fn racing_registration_with_signal_never_loses_a_pending_waiter() {
        for additional in [false, true] {
            for _ in 0..32 {
                let cancel = RpcCancel::new();
                let mut primary = additional.then(|| Box::pin(cancel.wait()));
                if let Some(wait) = &mut primary {
                    assert!(
                        wait.as_mut()
                            .poll(&mut Context::from_waker(Waker::noop()))
                            .is_pending()
                    );
                }
                let peer = cancel.clone();
                let start = Arc::new(std::sync::Barrier::new(2));
                let ready = start.clone();
                let (count, wake) = waker();
                let task = std::thread::spawn(move || {
                    let mut wait = Box::pin(peer.wait());
                    ready.wait();
                    let pending = wait
                        .as_mut()
                        .poll(&mut Context::from_waker(&wake))
                        .is_pending();
                    // Keep the pending future alive until signal has finished.
                    ready.wait();
                    pending
                });
                start.wait();
                cancel.signal();
                start.wait();
                if task.join().expect("waiter") {
                    assert!(count.0.load(Ordering::SeqCst) > 0);
                }
            }
        }
    }

    #[test]
    fn drop_signal_without_registration_wakes_without_initializing_log_state() {
        super::SPARE_LOGGED.with(|cache| drop(cache.take()));
        let cancel = RpcCancel::new_unary(true);
        let (count, waker) = waker();
        let mut waiter = Box::pin(cancel.wait());
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        cancel.signal_on_drop();
        assert!(cancel.is_cancelled());
        assert_eq!(count.0.load(Ordering::SeqCst), 1, "waiter must wake");
        let additional = cancel.inner().additional.lock().expect("registry");
        assert!(
            additional.logging.is_none(),
            "drop without registration must not initialize log state"
        );
    }

    #[test]
    fn drop_signal_is_visible_before_registry_lock() {
        for log_capable in [false, true] {
            let cancel = RpcCancel::new_unary(log_capable);
            let other = cancel.clone();
            let (started, receiver) = std::sync::mpsc::channel();
            let registry = cancel.inner().additional.lock().expect("registry");
            let thread = std::thread::spawn(move || {
                started.send(()).expect("receiver");
                other.signal_on_drop();
            });
            receiver.recv().expect("cancel thread started");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !cancel.is_cancelled() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "drop cancel flag waited for registry lock"
                );
                std::thread::yield_now();
            }
            drop(registry);
            thread.join().expect("cancel thread");
        }
    }

    #[test]
    fn drop_signal_without_registration_races_active_signal_and_reuses_spare() {
        use crate::binlog::EventType;

        struct BlockWake {
            entered: std::sync::mpsc::Sender<()>,
            release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl Wake for BlockWake {
            fn wake(self: Arc<Self>) {
                self.entered.send(()).expect("wake entered");
                self.release
                    .lock()
                    .expect("wake lock")
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .expect("release wake");
            }
        }

        super::SPARE_LOGGED.with(|cache| drop(cache.take()));
        let cancel = RpcCancel::new_unary(true);
        let (entered, waking) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let waker = Waker::from(Arc::new(BlockWake {
            entered,
            release: std::sync::Mutex::new(released),
        }));
        let mut waiter = Box::pin(cancel.wait());
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let detached = cancel.clone();
        let worker = std::thread::spawn(move || detached.signal());
        waking
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("detached signal entered wake");
        {
            let additional = cancel.inner().additional.lock().expect("registry");
            let state = additional
                .logging
                .as_ref()
                .expect("signal initialized state");
            assert!(state.cancel_log.is_none());
            assert_eq!(state.active_wakes, 1);
            assert!(!state.wake_completed);
        }
        cancel.signal_on_drop();
        {
            let additional = cancel.inner().additional.lock().expect("registry");
            let state = additional.logging.as_ref().expect("state retained");
            assert!(state.cancel_log.is_none());
            assert_eq!(state.active_wakes, 1, "active wake must remain tracked");
            assert!(!state.wake_completed);
        }
        release.send(()).expect("release wake");
        worker.join().expect("detached signal");
        drop(waiter);
        let original = Arc::as_ptr(cancel.inner());
        drop(cancel);

        let reused = RpcCancel::new_unary(true);
        assert_eq!(Arc::as_ptr(reused.inner()), original, "reuse logged spare");
        assert!(!reused.is_cancelled());
        {
            let additional = reused.inner().additional.lock().expect("registry");
            let state = additional.logging.as_ref().expect("allocated state reused");
            assert!(state.cancel_log.is_none());
            assert_eq!(state.active_wakes, 0);
            assert!(!state.wake_completed);
        }
        let (sink, tap) = client_log();
        let _guard = reused.observe_unary_cancel(&tap);
        reused.signal();
        assert_eq!(events(&sink), [EventType::Cancel]);
    }

    #[test]
    fn drop_signal_registered_waits_for_active_wake() {
        use crate::binlog::EventType;

        struct BlockWake {
            entered: std::sync::mpsc::Sender<()>,
            release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl Wake for BlockWake {
            fn wake(self: Arc<Self>) {
                self.entered.send(()).expect("wake entered");
                self.release
                    .lock()
                    .expect("wake lock")
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .expect("release wake");
            }
        }

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let _guard = cancel.observe_unary_cancel(&tap);
        drop(cancel.begin_unary_header().expect("header"));
        let (entered, waking) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let waker = Waker::from(Arc::new(BlockWake {
            entered,
            release: std::sync::Mutex::new(released),
        }));
        let mut waiter = Box::pin(cancel.wait());
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let detached = cancel.clone();
        let worker = std::thread::spawn(move || detached.signal());
        waking
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("detached signal entered wake");
        cancel.signal_on_drop();
        assert!(sink.is_empty(), "cancel must wait for active wake");
        release.send(()).expect("release wake");
        worker.join().expect("detached signal");
        assert_eq!(events(&sink), [EventType::Cancel]);
        cancel.signal_on_drop();
        assert_eq!(events(&sink), [EventType::Cancel]);
    }

    #[test]
    fn drop_signal_registered_survives_primary_waker_panic() {
        use crate::binlog::EventType;
        struct PanicOnWake;
        impl Wake for PanicOnWake {
            #[allow(clippy::panic, reason = "check a panicking executor callback")]
            fn wake(self: Arc<Self>) {
                panic!("primary waker panic");
            }
        }

        let (sink, tap) = client_log();
        let cancel = RpcCancel::new_unary(true);
        let _guard = cancel.observe_unary_cancel(&tap);
        let header = cancel.begin_unary_header().expect("header reserved");
        tap.log_client_header(&crate::Metadata::new(), "/test.Service/Call", "host", None);
        drop(header);
        let mut waiter = Box::pin(cancel.wait());
        let waker = Waker::from(Arc::new(PanicOnWake));
        assert!(
            waiter
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cancel.signal_on_drop()))
                .is_err()
        );
        assert!(cancel.is_cancelled());
        assert_eq!(
            events(&sink),
            [EventType::ClientHeader, EventType::Cancel],
            "drop must log despite waker panic"
        );
    }
}
