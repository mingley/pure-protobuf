//! One accepted deadline and terminal owner for the entire service future.

use super::dispatch::Dispatch;
use super::drain::{CancelFlag, CancelOnDrop};
use super::rpc::{Rpc, end_call, start_call};
use crate::metadata::Metadata;
use crate::rt::Runtime;
use crate::status::Status;
use crate::telemetry::{
    CallLabels, CallRole, CancellationEvent, CancellationReason, LifecycleObserver, OwnedCallLabels,
};
use crate::transport::h2::{DeadlineEnding, DeadlineResponse, SendResponse};
use atomic_waker::AtomicWaker;
use std::future::{Future, poll_fn};
use std::sync::Arc;
#[allow(
    clippy::disallowed_types,
    reason = "brief call-state transfers, no callbacks or await under lock"
)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context, Poll};
use tokio::time::Instant;

/// Reuses the call's cancellation allocation. Unlimited calls do not retain a
/// backend response capability or allocate a timer.
pub(crate) struct CallDeadline {
    flags: AtomicU8,
    wake: AtomicWaker,
    #[allow(
        clippy::disallowed_types,
        reason = "brief ownership transfers; no await or callbacks under this lock"
    )]
    inner: Mutex<DeadlineState>,
}

struct DeadlineState {
    at: Option<Instant>,
    response: Option<DeadlineResponse>,
    binlog: Option<crate::binlog::CallLogger>,
    web_carry: ([u8; 2], usize),
}

impl CallDeadline {
    const ACTIVE: u8 = 1;
    const STARTED: u8 = 2;
    const ENDED: u8 = 4;
    const CLOSED: u8 = 8;
    const WEB_PARTIAL: u8 = 16;
    const CANCELLATION: u8 = 32;

    pub(crate) fn new() -> Self {
        #[allow(
            clippy::disallowed_types,
            reason = "brief ownership transfers; no await or callbacks under this lock"
        )]
        let inner = Mutex::new(DeadlineState {
            at: None,
            response: None,
            binlog: None,
            web_carry: ([0; 2], 0),
        });
        Self {
            flags: AtomicU8::new(0),
            wake: AtomicWaker::new(),
            inner,
        }
    }

    /// Reset only when the cancellation allocation is uniquely owned.
    pub(crate) fn reset(&self) {
        self.close();
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).at = None;
        self.flags.store(0, Ordering::Release);
    }

    pub(crate) fn at(&self) -> Option<Instant> {
        if self.flags.load(Ordering::Acquire) & Self::ACTIVE == 0 {
            return None;
        }
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).at
    }

    pub(crate) fn tighten(&self, at: Option<Instant>, respond: &SendResponse) -> Option<Instant> {
        let Some(at) = at else { return self.at() };
        let flags = self.flags.load(Ordering::Acquire);
        if flags & Self::CLOSED != 0 {
            return self.at();
        }
        if self.at().is_some_and(|previous| previous <= at) {
            return self.at();
        }
        // Cloning/dropping a backend capability can acquire engine locks, so
        // neither operation belongs inside the call-state lock.
        let mut capability = (flags & Self::ACTIVE == 0)
            .then(|| respond.deadline_response())
            .flatten();
        let result = {
            let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            if self.flags.load(Ordering::Acquire) & Self::CLOSED == 0 {
                state.at = Some(state.at.map_or(at, |previous| previous.min(at)));
                if state.response.is_none() {
                    state.response = capability.take();
                }
                self.flags.fetch_or(Self::ACTIVE, Ordering::Release);
            }
            state.at
        };
        drop(capability);
        self.wake.wake();
        result
    }

    fn poll_at(&self, cx: &Context<'_>) -> Option<Instant> {
        self.wake.register(cx.waker());
        self.at()
    }

    pub(crate) fn start(&self) -> bool {
        self.flags.fetch_or(Self::STARTED, Ordering::AcqRel) & Self::STARTED == 0
    }

    pub(crate) fn end(&self) -> bool {
        self.flags.fetch_or(Self::ENDED, Ordering::AcqRel) & Self::ENDED == 0
    }

    fn started(&self) -> bool {
        self.flags.load(Ordering::Acquire) & Self::STARTED != 0
    }

    fn ended(&self) -> bool {
        self.flags.load(Ordering::Acquire) & Self::ENDED != 0
    }

    pub(crate) fn set_binlog(&self, logger: &crate::binlog::CallLogger) {
        let previous = {
            let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            if self.flags.load(Ordering::Acquire) & Self::CLOSED == 0 {
                state.binlog.replace(logger.clone())
            } else {
                None
            }
        };
        drop(previous);
    }

    pub(crate) fn record_cancellation(&self) -> bool {
        self.flags.fetch_or(Self::CANCELLATION, Ordering::AcqRel) & Self::CANCELLATION == 0
    }

    pub(crate) fn expired(&self) -> bool {
        self.at().is_some_and(|at| Instant::now() >= at)
    }

    pub(crate) fn web_frame_started(&self) {
        self.flags.fetch_or(Self::WEB_PARTIAL, Ordering::Release);
    }

    pub(crate) fn web_frame_finished(&self) {
        self.flags.fetch_and(!Self::WEB_PARTIAL, Ordering::Release);
    }

    #[cfg(feature = "grpc-web")]
    pub(crate) fn save_web_carry(&self, carry: ([u8; 2], usize)) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .web_carry = carry;
    }

    /// Detach resources even when an application retains Request/cancel handles.
    pub(crate) fn close(&self) {
        let resources = {
            let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
            self.flags.fetch_or(Self::CLOSED, Ordering::AcqRel);
            state.web_carry = ([0; 2], 0);
            (state.response.take(), state.binlog.take())
        };
        drop(self.wake.take());
        drop(resources);
    }

    fn take_terminal(
        &self,
    ) -> (
        Option<DeadlineResponse>,
        Option<crate::binlog::CallLogger>,
        ([u8; 2], usize),
        bool,
    ) {
        let mut state = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        (
            state.response.take(),
            state.binlog.take(),
            state.web_carry,
            self.flags.load(Ordering::Acquire) & Self::WEB_PARTIAL != 0,
        )
    }
}

struct Owner {
    cancel: CancelOnDrop,
    observer: Option<Arc<dyn LifecycleObserver>>,
    labels: Option<OwnedCallLabels>,
    channelz_server: Option<crate::channelz::ServerId>,
    channelz_socket: Option<crate::channelz::SocketId>,
    accepted_at: Instant,
}

/// An encoding/interceptor may consume the remaining budget in one synchronous
/// poll. Yield to the terminal owner before committing more response bytes.
pub(crate) async fn before_commit(call: &CancelFlag) {
    if call.deadline.expired() {
        std::future::pending::<()>().await;
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.cancel.flag().deadline.close();
        if self.cancel.flag().deadline.started() && !self.cancel.flag().deadline.ended() {
            // Preserve registry accounting on connection abort and panic. A
            // second user callback panic during unwinding could abort the process.
            let observer = if std::thread::panicking() {
                None
            } else {
                self.observer.as_deref()
            };
            let labels = self.labels.as_ref().map(|labels| labels.as_borrowed());
            end_call(
                self.cancel.flag(),
                observer,
                labels.as_ref(),
                self.channelz_server,
                self.channelz_socket,
                &Status::cancelled(),
                self.accepted_at.elapsed(),
            );
        }
    }
}

// Field order also preserves destruction order if the returned future is never
// polled: application state goes away before the terminal/lifecycle owner.
struct ServiceState<F> {
    service: Option<F>,
    owner: Owner,
}

pub(crate) fn serve<R: Runtime, D: Dispatch>(
    dispatch: &D,
    rpc: Rpc,
) -> impl Future<Output = ()> + Send {
    let observer = dispatch.observer().cloned();
    let labels = observer
        .as_ref()
        .map(|_| CallLabels::new(rpc.path(), rpc.authority(), CallRole::Server).to_owned());
    let channelz_server = rpc.channelz_server;
    let channelz_socket = rpc.channelz_socket;
    let accepted_at = rpc.accepted_at;
    let owner = Owner {
        cancel: CancelOnDrop::new(Arc::clone(&rpc.call_cancel)),
        observer,
        labels,
        channelz_server,
        channelz_socket,
        accepted_at,
    };
    let start_labels = owner.labels.as_ref().map(|labels| labels.as_borrowed());
    start_call(
        owner.cancel.flag(),
        owner.observer.as_deref(),
        start_labels.as_ref(),
        channelz_server,
        channelz_socket,
    );
    #[cfg(feature = "grpc-web")]
    let web = rpc.web;
    // Construct the selected dispatch before the returned async state exists;
    // otherwise its initial arm reserves another full Rpc-sized record.
    let service = if owner.cancel.flag().deadline.expired() {
        drop(rpc);
        None
    } else {
        Some(dispatch.dispatch(rpc))
    };
    let state = ServiceState { service, owner };
    async move {
        let ServiceState { owner, service } = state;
        let flag = owner.cancel.flag();
        let expired = {
            let service = async move {
                match service {
                    Some(service) => service.await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::pin!(service);
            let mut sleep = None;
            poll_fn(|cx| {
                let at = flag.deadline.poll_at(cx);
                if at.is_some_and(|at| R::now() >= at) {
                    flag.signal();
                    return Poll::Ready(true);
                }
                if service.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(false);
                }
                if let Some(at) = flag.deadline.poll_at(cx) {
                    if sleep.as_ref().is_none_or(|(previous, _)| *previous != at) {
                        sleep = Some((at, Box::pin(R::sleep_until(at))));
                    }
                    if let Some((_, sleep)) = &mut sleep {
                        if sleep.as_mut().poll(cx).is_ready() {
                            flag.signal();
                            return Poll::Ready(true);
                        }
                    }
                }
                Poll::Pending
            })
            .await
        }; // Destroy service state before recovering/sending its terminal response.
        if expired {
            let status = Status::deadline_exceeded();
            let (capability, logger, carry, partial_web_frame) = flag.deadline.take_terminal();
            #[cfg(not(feature = "grpc-web"))]
            let _carry = carry;
            let labels = owner.labels.as_ref().map(|labels| labels.as_borrowed());
            let observer = owner.observer.as_deref();
            if !flag.deadline.ended() {
                start_call(
                    flag,
                    observer,
                    labels.as_ref(),
                    channelz_server,
                    channelz_socket,
                );
            }
            if !flag.deadline.ended() && flag.deadline.record_cancellation() {
                if let (Some(observer), Some(labels)) = (observer, labels.as_ref()) {
                    observer.on_cancellation(&CancellationEvent {
                        call: *labels,
                        reason: CancellationReason::DeadlineExceeded,
                    });
                }
            }
            if !flag.deadline.ended() {
                if let Some(logger) = logger {
                    logger.log_trailer(&Metadata::new(), &status);
                }
            }
            if let Some(mut capability) = capability {
                #[cfg(feature = "grpc-web")]
                let terminal = match web {
                    Some(mode) => match carry.0.get(..carry.1) {
                        Some(carry) => crate::web::deadline_response(mode, &status, carry),
                        None => Err(Status::internal("invalid retained grpc-web text carry")),
                    },
                    None => native_terminal(&status),
                };
                #[cfg(not(feature = "grpc-web"))]
                let terminal = native_terminal(&status);
                if let Ok((response, trailers, body)) = terminal {
                    let ending = if body.is_some() {
                        if partial_web_frame {
                            DeadlineEnding::Reset
                        } else {
                            DeadlineEnding::Data
                        }
                    } else {
                        DeadlineEnding::Trailers
                    };
                    capability.finish(response, trailers, body, ending).ok();
                }
            }
            end_call(
                flag,
                observer,
                labels.as_ref(),
                channelz_server,
                channelz_socket,
                &status,
                accepted_at.elapsed(),
            );
        }
    }
}

fn native_terminal(
    status: &Status,
) -> Result<(http::Response<()>, http::HeaderMap, Option<bytes::Bytes>), Status> {
    let trailers = crate::wire::grpc_trailers(status)?;
    let mut response = http::Response::new(());
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/grpc"),
    );
    response.headers_mut().extend(trailers.clone());
    Ok((response, trailers, None))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "bounded ownership and pool regression witnesses"
    )]
    use super::*;
    use crate::transport::{ServerBuilder as _, ServerConnection as _};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Wake, Waker};
    use std::time::Duration;

    struct WakeCount(AtomicUsize);
    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct LayoutService;
    impl super::super::dispatch::Service for LayoutService {
        const NAME: &'static str = "deadline.Layout";
        async fn call(&self, rpc: Rpc) {
            std::future::pending::<()>().await;
            drop(rpc);
        }
    }

    struct LargeLayoutService;
    impl super::super::dispatch::Service for LargeLayoutService {
        const NAME: &'static str = "deadline.LargeLayout";
        async fn call(&self, rpc: Rpc) {
            let payload = [7_u8; 16 * 1024];
            tokio::time::sleep(Duration::from_secs(1)).await;
            std::hint::black_box(payload);
            drop(rpc);
        }
    }

    fn returned_future_size<A, B, F>(_: impl FnOnce(A, B) -> F) -> usize {
        std::mem::size_of::<F>()
    }

    #[tokio::test]
    async fn retained_application_cancel_handle_does_not_retain_terminal_engine_or_waker() {
        println!(
            "deadline layout diagnostic: Rpc={} CancelFlag={} CallDeadline={} Owner={} service_driver={}",
            std::mem::size_of::<Rpc>(),
            std::mem::size_of::<CancelFlag>(),
            std::mem::size_of::<CallDeadline>(),
            std::mem::size_of::<Owner>(),
            returned_future_size(
                super::serve::<
                    crate::rt::TokioRuntime,
                    super::super::dispatch::Single<LayoutService>,
                >
            )
        );
        let (client_io, server_io) = tokio::io::duplex(4096);
        let (accepted, receiver) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let mut connection = crate::transport::h2::ServerBuilder::new()
                .handshake(server_io)
                .await
                .unwrap();
            let (request, response) = connection.accept().await.unwrap().unwrap();
            accepted.send((request, response)).unwrap();
            while let Some(request) = connection.accept().await {
                drop(request.unwrap());
            }
        });
        let (sender, connection) = h2::client::handshake(client_io).await.unwrap();
        let client = tokio::spawn(async move {
            drop(connection.await);
        });
        let mut sender = sender.ready().await.unwrap();
        let (response, _) = sender
            .send_request(
                http::Request::builder()
                    .uri("http://local/deadline/Owned")
                    .header("grpc-timeout", "20m")
                    .body(())
                    .unwrap(),
                true,
            )
            .unwrap();
        let (request, respond) = tokio::time::timeout(Duration::from_secs(2), receiver)
            .await
            .unwrap()
            .unwrap();
        let accepted_at = Instant::now();
        let mut rpc = super::super::connection::incoming_rpc(
            request,
            respond,
            accepted_at,
            Arc::new(crate::ServerConfig::default()),
            super::super::connection::ConnectionInfo::new(),
            crate::limits::ByteBudgetTracker::default(),
            None,
            None,
            crate::wire::headers::StockFacts::not_stock(),
            None,
        );
        assert_eq!(rpc.peer_timeout(), Some(Duration::from_millis(20)));
        assert_eq!(
            rpc.deadline(),
            Some(accepted_at + Duration::from_millis(20))
        );
        let mut replacement = http::HeaderMap::new();
        replacement.insert("grpc-timeout", http::HeaderValue::from_static("5m"));
        rpc.metadata = Metadata::from_owned_headers(replacement);
        assert_eq!(
            rpc.peer_timeout(),
            Some(Duration::from_millis(20)),
            "inspection preserves the actual received peer budget"
        );
        assert_eq!(rpc.deadline(), Some(accepted_at + Duration::from_millis(5)));
        rpc.metadata = Metadata::new();
        assert_eq!(
            rpc.effective_timeout(),
            Some(Duration::from_millis(5)),
            "removing edited metadata cannot renew the accepted inspection cap"
        );
        assert_eq!(rpc.deadline(), Some(accepted_at + Duration::from_millis(5)));
        let flag = Arc::clone(&rpc.call_cancel);
        let wake = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake));
        flag.deadline.poll_at(&Context::from_waker(&waker));
        assert_eq!(Arc::strong_count(&wake), 3);
        let owner = Owner {
            cancel: CancelOnDrop::new(Arc::clone(&flag)),
            observer: None,
            labels: None,
            channelz_server: None,
            channelz_socket: None,
            accepted_at: Instant::now(),
        };
        drop(owner);
        // The retained application signal stays observable, without retaining
        // the driver's waker or its extra HTTP/2 response reference.
        assert!(flag.is_cancelled());
        assert_eq!(Arc::strong_count(&wake), 2);
        drop(rpc);
        let error = tokio::time::timeout(Duration::from_secs(2), response)
            .await
            .expect("no terminal capability remains hidden in the retained signal")
            .unwrap_err();
        assert_eq!(error.reason(), Some(h2::Reason::CANCEL));
        drop(flag);
        client.abort();
        server.abort();
    }

    #[tokio::test]
    async fn unpolled_router_owner_drop_closes_lifecycle_and_terminal_capability() {
        struct Counts {
            starts: AtomicUsize,
            ends: AtomicUsize,
            polls: AtomicUsize,
        }
        struct Observed(Arc<Counts>);
        impl LifecycleObserver for Observed {
            fn on_server_call_start(&self, _: &CallLabels<'_>) {
                self.0.starts.fetch_add(1, Ordering::SeqCst);
            }
            fn on_server_call_end(&self, _: &CallLabels<'_>, status: &Status, _: Duration) {
                assert_eq!(status.code(), crate::Code::Cancelled);
                self.0.ends.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct Unpolled(Arc<Counts>);
        impl super::super::dispatch::Service for Unpolled {
            const NAME: &'static str = "deadline.Unpolled";
            async fn call(&self, rpc: Rpc) {
                self.0.polls.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<()>().await;
                drop(rpc);
            }
        }
        let counts = Arc::new(Counts {
            starts: AtomicUsize::new(0),
            ends: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
        });
        let router = crate::Router::new()
            .add_service(Unpolled(Arc::clone(&counts)))
            .observer(Observed(Arc::clone(&counts)));
        let (client_io, server_io) = tokio::io::duplex(4096);
        let (accepted, receiver) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let mut connection = crate::transport::h2::ServerBuilder::new()
                .handshake(server_io)
                .await
                .unwrap();
            let (request, response) = connection.accept().await.unwrap().unwrap();
            accepted.send((request, response)).unwrap();
            while let Some(request) = connection.accept().await {
                drop(request.unwrap());
            }
        });
        let (sender, connection) = h2::client::handshake(client_io).await.unwrap();
        let client = tokio::spawn(async move {
            drop(connection.await);
        });
        let mut sender = sender.ready().await.unwrap();
        let (response, _) = sender
            .send_request(
                http::Request::builder()
                    .uri("http://local/deadline.Unpolled/Owned")
                    .header("grpc-timeout", "20m")
                    .body(())
                    .unwrap(),
                true,
            )
            .unwrap();
        let (request, respond) = tokio::time::timeout(Duration::from_secs(2), receiver)
            .await
            .unwrap()
            .unwrap();
        let accepted_at = Instant::now();
        let rpc = super::super::connection::incoming_rpc(
            request,
            respond,
            accepted_at,
            Arc::new(crate::ServerConfig::default()),
            super::super::connection::ConnectionInfo::new(),
            crate::limits::ByteBudgetTracker::default(),
            None,
            None,
            crate::wire::headers::StockFacts::not_stock(),
            None,
        );
        let _deadline = rpc.deadline();
        let flag = Arc::clone(&rpc.call_cancel);
        let driver = super::serve::<crate::rt::TokioRuntime, _>(&router, rpc);
        assert_eq!(counts.starts.load(Ordering::SeqCst), 1);
        assert_eq!(counts.polls.load(Ordering::SeqCst), 0);
        drop(driver); // Deliberately never polled.
        assert_eq!(counts.polls.load(Ordering::SeqCst), 0);
        assert_eq!(counts.ends.load(Ordering::SeqCst), 1);
        assert!(flag.is_cancelled());
        assert!(flag.deadline.ended());
        let error = tokio::time::timeout(Duration::from_secs(2), response)
            .await
            .expect("retained cancellation flag must not retain terminal capability")
            .unwrap_err();
        assert_eq!(error.reason(), Some(h2::Reason::CANCEL));
        client.abort();
        server.abort();
        let router_size =
            returned_future_size(super::serve::<crate::rt::TokioRuntime, crate::Router>);
        let small_size = returned_future_size(
            super::serve::<crate::rt::TokioRuntime, super::super::dispatch::Single<LayoutService>>,
        );
        let large_size = returned_future_size(
            super::serve::<
                crate::rt::TokioRuntime,
                super::super::dispatch::Single<LargeLayoutService>,
            >,
        );
        println!(
            "router layout diagnostic: router={router_size} single={small_size} large_single={large_size}"
        );
        assert_eq!(
            large_size, small_size,
            "boxed user payload must not enlarge the driver"
        );
        assert!(
            router_size <= small_size + 4 * std::mem::size_of::<usize>(),
            "routed dispatch should retain only its selected future pointer"
        );
    }

    #[test]
    fn pooled_call_does_not_carry_deadline_lifecycle_or_web_state_into_the_next_call() {
        let flag = CancelFlag::new();
        let previous = Arc::as_ptr(&flag);
        flag.deadline.inner.lock().unwrap().at = Some(Instant::now());
        flag.deadline.flags.store(
            CallDeadline::ACTIVE
                | CallDeadline::STARTED
                | CallDeadline::ENDED
                | CallDeadline::WEB_PARTIAL,
            Ordering::Release,
        );
        flag.deadline.inner.lock().unwrap().web_carry = ([1, 2], 2);
        drop(CancelOnDrop::new(flag));
        let next = CancelFlag::new();
        assert_eq!(
            Arc::as_ptr(&next),
            previous,
            "exercise the actual reused allocation"
        );
        assert!(!next.is_cancelled());
        assert!(next.deadline.at().is_none());
        assert!(!next.deadline.started());
        assert!(!next.deadline.ended());
        assert_eq!(next.deadline.inner.lock().unwrap().web_carry.1, 0);
        assert_eq!(next.deadline.flags.load(Ordering::Acquire), 0);
    }
}
