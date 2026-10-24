//! Response draining: handler race, cancel guards, and wire writers.

use super::http_name::HttpStr;
use crate::codec::CodecMessage;
use crate::config::Wire;
use crate::limits::ByteBudgetTracker;
use crate::metadata::Metadata;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use crate::telemetry::{CallLabels, LifecycleObserver};
use crate::transport::{SendResponse, SendStream, h2 as backend};
use crate::wire::{
    ImmediateSend, OutBatch, encode_msg, grpc_trailers, let_producer_catch_up, preferred_codec,
    select_outbound_codec, select_stream_codec, send_frame, send_frame_if_ready, send_ok_headers,
};
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
#[allow(
    clippy::disallowed_types,
    reason = "short waker-registry sections; no await under lock and wakers run after unlocking"
)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

/// Server-side cancel signal.
///
/// A watch channel notifies when its last receiver is dropped. Handlers that
/// never call [`Request::cancelled`] still paid that, so this flag only wakes
/// tasks that have started waiting.
pub(crate) struct CancelFlag {
    pub(crate) deadline: super::deadline::CallDeadline,
    /// Cancellation and waiter registration share one modification order.
    state: AtomicU8,
    #[allow(
        clippy::disallowed_types,
        reason = "short waker-registry sections; no await under lock and wakers run after unlocking"
    )]
    // Slab reuses abandoned registrations in O(1); storage follows peak live
    // waiters rather than the number of cancelled select branches over time.
    waiters: Mutex<slab::Slab<Waker>>,
}

impl CancelFlag {
    const CANCELLED: u8 = 1;
    const ARMED: u8 = 2;

    pub(crate) fn new() -> Arc<Self> {
        if let Some(flag) = take_spare_cancel() {
            // The slot may still be marked cancelled from the call that
            // finished. The next call has not been cancelled.
            flag.state.store(0, Ordering::Release);
            flag.deadline.reset();
            return flag;
        }
        #[allow(
            clippy::disallowed_types,
            reason = "short waker-registry sections; no await under lock and wakers run after unlocking"
        )]
        let waiters = Mutex::new(slab::Slab::new());
        Arc::new(Self {
            deadline: super::deadline::CallDeadline::new(),
            state: AtomicU8::new(0),
            waiters,
        })
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) & Self::CANCELLED != 0
    }

    pub(crate) fn signal(&self) {
        // Either registration observes CANCELLED, or this operation observes
        // ARMED and wakes it. Separate release/acquire flags could both miss.
        if self.state.fetch_or(Self::CANCELLED, Ordering::AcqRel) & Self::ARMED != 0 {
            self.wake_waiters();
        }
    }

    #[cold]
    #[inline(never)]
    fn wake_waiters(&self) {
        let waiters = match self.waiters.lock() {
            Ok(mut waiters) => std::mem::take(&mut *waiters),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };
        for (_, waiter) in waiters {
            waiter.wake();
        }
    }
}

/// Resolves when `flag` is signalled. `None` never resolves.
pub(crate) fn wait_cancel(flag: Option<Arc<CancelFlag>>) -> WaitCancel {
    WaitCancel { flag, slot: None }
}

/// [`wait_cancel`]'s state.
pub(crate) struct WaitCancel {
    flag: Option<Arc<CancelFlag>>,
    slot: Option<usize>,
}

impl Future for WaitCancel {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let Some(flag) = this.flag.as_ref() else {
            return Poll::Pending;
        };
        if flag.is_cancelled() {
            return Poll::Ready(());
        }
        let needs_update = this.slot.is_none_or(|index| {
            let waiters = match flag.waiters.lock() {
                Ok(waiters) => waiters,
                Err(poisoned) => poisoned.into_inner(),
            };
            waiters
                .get(index)
                .is_none_or(|waiter| !waiter.will_wake(cx.waker()))
        });
        if needs_update {
            // RawWaker clone/drop callbacks may reenter signal. Neither runs
            // under the registry lock; an unchanged waker needs no clone.
            let replacement = cx.waker().clone();
            let previous = {
                let mut waiters = match flag.waiters.lock() {
                    Ok(waiters) => waiters,
                    Err(poisoned) => poisoned.into_inner(),
                };
                // No registration can appear after cancellation drains the
                // slab: stale slot handles must never alias a later waiter.
                // The shared RMW also closes the first-registration wake race.
                if flag.state.fetch_or(CancelFlag::ARMED, Ordering::AcqRel) & CancelFlag::CANCELLED
                    != 0
                {
                    Some(replacement)
                } else if let Some(waiter) = this.slot.and_then(|index| waiters.get_mut(index)) {
                    Some(std::mem::replace(waiter, replacement))
                } else {
                    this.slot = Some(waiters.insert(replacement));
                    None
                }
            };
            drop(previous);
        }
        if flag.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for WaitCancel {
    fn drop(&mut self) {
        let (Some(flag), Some(index)) = (&self.flag, self.slot.take()) else {
            return;
        };
        let removed = {
            let mut waiters = match flag.waiters.lock() {
                Ok(waiters) => waiters,
                Err(poisoned) => poisoned.into_inner(),
            };
            waiters.try_remove(index)
        };
        // A waker destructor may signal this same flag.
        drop(removed);
    }
}

/// Wake spawned work as soon as the server deadline wins, not after trailers.
pub(crate) fn notify_deadline<T>(outcome: &Result<T, Status>, cancel: &CancelFlag) {
    if matches!(outcome, Err(s) if s.code() == Code::DeadlineExceeded) {
        cancel.signal();
    }
}

/// Resolve when the client `RST_STREAM`s this RPC.
///
/// Unary handlers that have already read the request (and streaming handlers
/// that are not currently reading) would otherwise run to completion after
/// the caller has gone. `SendResponse::poll_reset` sees the reset without
/// needing the request body.
pub(crate) async fn wait_client_reset(respond: &mut backend::SendResponse) -> Status {
    drop(std::future::poll_fn(|cx| respond.poll_reset(cx)).await);
    Status::cancelled()
}

/// Race the handler against a client reset, signalling spawned work on RST.
///
/// After signalling, poll the handler once so a body awaiting
/// [`Request::cancelled`] can finish. A handler that ignores cancel stays
/// `Pending` and is dropped, the same as before.
pub(crate) async fn run_handler<T>(
    respond: &mut backend::SendResponse,
    on_reset: Arc<CancelFlag>,
    handler: impl Future<Output = Result<T, Status>>,
    tap: Option<&crate::binlog::CallLogger>,
) -> Result<T, Status> {
    tokio::pin!(handler);
    tokio::select! {
        biased;
        result = &mut handler => result,
        gone = wait_client_reset(respond) => {
            on_reset.signal();
            if let Some(tap) = tap {
                tap.log_cancel();
            }
            match poll_fn(|cx| Poll::Ready(handler.as_mut().poll(cx))).await {
                Poll::Ready(result) => result,
                Poll::Pending => Err(gone),
            }
        }
    }
}

/// Owns [`Request::cancelled`] from request setup through the response write.
///
/// Dropping a live handler's owner also signals cancellation. Moving this guard
/// into the response keeps spawned producers alive after the handler returns,
/// until the stream drains, the client resets, or the deadline fires.
pub(crate) struct CancelOnDrop(Option<Arc<CancelFlag>>);

impl CancelOnDrop {
    pub(crate) fn new(flag: Arc<CancelFlag>) -> Self {
        Self(Some(flag))
    }

    #[expect(
        clippy::expect_used,
        reason = "the private guard is constructed live; its flag is only taken inside Drop after its final use"
    )]
    pub(crate) fn flag(&self) -> &Arc<CancelFlag> {
        self.0.as_ref().expect("live cancellation guard")
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let Some(flag) = self.0.take() else {
            return;
        };
        // Other clones (the handler request, an in-flight reset waiter) still
        // need the signal. The last owner is the usual end of a call that
        // nobody waited on: keep the allocation for the next call.
        if Arc::strong_count(&flag) == 1 {
            if flag.state.load(Ordering::Acquire) & CancelFlag::ARMED != 0 {
                flag.wake_waiters();
            }
            flag.state.store(0, Ordering::Release);
            stash_spare_cancel(flag);
            return;
        }
        flag.signal();
    }
}

thread_local! {
    static SPARE_CANCEL: std::cell::Cell<Option<Arc<CancelFlag>>> = const { std::cell::Cell::new(None) };
}

fn take_spare_cancel() -> Option<Arc<CancelFlag>> {
    SPARE_CANCEL.try_with(std::cell::Cell::take).ok().flatten()
}

fn stash_spare_cancel(flag: Arc<CancelFlag>) {
    // Thread shutdown drops the slot; the flag is freed with the closure.
    let _thread_shutdown = SPARE_CANCEL.try_with(|cell| cell.set(Some(flag)));
}

/// Keep [`CancelOnDrop`] alive across `write`.
///
/// Locals not named in the write future can be dropped at `.await` (NLL).
/// A manual future holds the guard as a field so it cannot drop until `write`
/// completes — `write.await; drop(cancel)` is not enough.
pub(crate) fn hold_cancel<F: Future<Output = ()>>(cancel: CancelOnDrop, write: F) -> HoldCancel<F> {
    HoldCancel {
        write: Box::pin(write),
        cancel: Some(cancel),
    }
}

/// [`hold_cancel`]'s state: poll `write`, drop the guard only when it finishes.
pub(crate) struct HoldCancel<F> {
    cancel: Option<CancelOnDrop>,
    write: Pin<Box<F>>,
}

impl<F: Future<Output = ()>> Future for HoldCancel<F> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match self.write.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.cancel.take();
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A handler result plus the response channel it still has to be written to.
pub(crate) struct Prepared<T> {
    pub(crate) respond: backend::SendResponse,
    pub(crate) wire: Wire,
    /// The RPC's deadline, shared by the handler, the inbound stream, and the
    /// response writer, so no stage can outlive it.
    pub(crate) deadline: Option<tokio::time::Instant>,
    pub(crate) outcome: Result<T, Status>,
    pub(crate) prefer_gzip: bool,
    pub(crate) peer_accepts_gzip: bool,
    pub(crate) peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")]
    pub(crate) peer_accepts_zstd: bool,
    pub(crate) cancel: CancelOnDrop,
    /// Kernel-stamped onto the handler [`Response`] before `on_response`.
    pub(crate) path: Option<HttpStr>,
    pub(crate) gzip_level: u32,
    pub(crate) timeout: Option<Duration>,
    pub(crate) peer_timeout: Option<Duration>,
    pub(crate) rpc_timeout: Option<Duration>,
    pub(crate) budget: ByteBudgetTracker,
    pub(crate) observer: Option<Arc<dyn LifecycleObserver>>,
    pub(crate) call_start: tokio::time::Instant,
    pub(crate) binlog: Option<crate::binlog::CallLogger>,
    /// Channelz server owning this RPC, for call counters.
    pub(crate) channelz_server: Option<crate::channelz::ServerId>,
    /// Channelz socket serving this RPC, for stream/message counters.
    pub(crate) channelz_socket: Option<crate::channelz::SocketId>,
    #[cfg(feature = "grpc-web")]
    pub(crate) web: Option<crate::web::Mode>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal write helper with observer and options"
)]
pub(crate) async fn send_unary_response<Resp: CodecMessage>(
    response: Response<Resp>,
    mut respond: backend::SendResponse,
    wire: Wire,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
    call: &CancelFlag,
) -> Status {
    let (msg, headers, trailers, compress) = response.split();
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let codec = select_outbound_codec(compress, prefer_gzip, negotiated);
    let encoded = encode_msg(&msg, codec, wire.limits, wire.gzip_level);
    super::deadline::before_commit(call).await;
    let frame = match encoded {
        Ok(frame) => frame,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            super::deadline::before_commit(call).await;
            return crate::wire::send::try_send_trailers_only(
                &mut respond,
                &status,
                &Metadata::new(),
            )
            .map_or_else(|error| error, |()| status);
        }
    };
    let permit = match budget.acquire(frame.total_len()) {
        Ok(p) => p,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            super::deadline::before_commit(call).await;
            return crate::wire::send::try_send_trailers_only(
                &mut respond,
                &status,
                &Metadata::new(),
            )
            .map_or_else(|error| error, |()| status);
        }
    };
    super::deadline::before_commit(call).await;
    let mut send = match send_ok_headers(&mut respond, &headers, codec, wire.accept_gzip) {
        Ok(send) => send,
        Err(status) => return status,
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
        for seg in frame.segments() {
            tap.log_written(seg);
        }
    }

    if let Some(obs) = observer {
        obs.on_bytes_sent(call_labels, frame.total_len());
    }
    if let Some(socket) = channelz_socket {
        crate::channelz::Registry::global().note_messages(socket, true, 1);
    }
    super::deadline::before_commit(call).await;
    let written = match send_frame_if_ready(&mut send, frame, false, wire.send_buffer) {
        ImmediateSend::Done(result) => result,
        ImmediateSend::Blocked(frame) => {
            send_frame(&mut send, frame, false, wire.send_buffer).await
        }
    };
    if let Err(status) = written {
        return status;
    }
    drop(permit);
    // An empty OK trailer is the pooled map. Building a `Status` and then
    // proving it is bare repeats that work on every successful RPC.
    if trailers.is_empty() {
        if let Some(tap) = tap {
            let status = Status::ok();
            tap.log_trailer(status.metadata(), &status);
        }
        super::deadline::before_commit(call).await;
        return match send.send_trailers(crate::wire::bare_ok_trailer_map()) {
            Ok(()) => Status::ok(),
            Err(error) => Status::from_h2_send(error),
        };
    }
    super::deadline::before_commit(call).await;
    let mut status = Status::ok();
    *status.metadata_mut() = trailers;
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    super::deadline::before_commit(call).await;
    let map = match grpc_trailers(&status) {
        Ok(map) => map,
        Err(error) => return error,
    };
    match send.send_trailers(map) {
        Ok(()) => status,
        Err(error) => Status::from_h2_send(error),
    }
}

#[cfg(feature = "grpc-web")]
struct WebWriter<'a> {
    text: crate::web::TextEncoder,
    call: &'a CancelFlag,
}
#[cfg(feature = "grpc-web")]
impl<'a> WebWriter<'a> {
    fn new(call: &'a CancelFlag) -> Self {
        Self {
            text: crate::web::TextEncoder::new(),
            call,
        }
    }
}
#[cfg(feature = "grpc-web")]
impl std::ops::Deref for WebWriter<'_> {
    type Target = crate::web::TextEncoder;
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}
#[cfg(feature = "grpc-web")]
impl std::ops::DerefMut for WebWriter<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.text
    }
}
#[cfg(feature = "grpc-web")]
impl Drop for WebWriter<'_> {
    fn drop(&mut self) {
        self.call.deadline.save_web_carry(self.text.carry());
    }
}

#[cfg(feature = "grpc-web")]
#[allow(
    clippy::too_many_arguments,
    reason = "internal write helper with observer and options"
)]
pub(crate) async fn send_web_unary_response<Resp: CodecMessage>(
    response: Response<Resp>,
    mut respond: backend::SendResponse,
    web: crate::web::Mode,
    wire: Wire,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
    call: &CancelFlag,
) -> Status {
    let (msg, headers, trailers, compress) = response.split();
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let codec = select_outbound_codec(compress, prefer_gzip, negotiated);
    let encoded = encode_msg(&msg, codec, wire.limits, wire.gzip_level);
    super::deadline::before_commit(call).await;
    let frame = match encoded {
        Ok(frame) => frame,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            super::deadline::before_commit(call).await;
            return crate::web::try_send_trailers_only(
                &mut respond,
                web,
                &status,
                &Metadata::new(),
            )
            .map_or_else(|error| error, |()| status);
        }
    };
    let permit = match budget.acquire(frame.total_len()) {
        Ok(p) => p,
        Err(status) => {
            if let Some(tap) = tap {
                tap.log_trailer(&Metadata::new(), &status);
            }
            super::deadline::before_commit(call).await;
            return crate::web::try_send_trailers_only(
                &mut respond,
                web,
                &status,
                &Metadata::new(),
            )
            .map_or_else(|error| error, |()| status);
        }
    };
    super::deadline::before_commit(call).await;
    let mut send =
        match crate::web::send_ok_headers(&mut respond, web, &headers, codec, wire.accept_gzip) {
            Ok(send) => send,
            Err(status) => return status,
        };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
        for seg in frame.segments() {
            tap.log_written(seg);
        }
    }
    if let Some(obs) = observer {
        obs.on_bytes_sent(call_labels, frame.total_len());
    }
    if let Some(socket) = channelz_socket {
        crate::channelz::Registry::global().note_messages(socket, true, 1);
    }
    let mut text = WebWriter::new(call);
    call.deadline.web_frame_started();
    if let Err(status) =
        crate::web::send_frame(&mut send, web, &mut text, frame, wire.send_buffer).await
    {
        return status;
    }
    call.deadline.web_frame_finished();
    drop(permit);
    super::deadline::before_commit(call).await;
    let mut status = Status::ok();
    if !trailers.is_empty() {
        *status.metadata_mut() = trailers;
    }
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    super::deadline::before_commit(call).await;
    call.deadline.web_frame_started();
    match crate::web::send_trailers(&mut send, web, &mut text, &status, wire.send_buffer).await {
        Ok(()) => {
            call.deadline.web_frame_finished();
            status
        }
        Err(error) => error,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "internal stream write helper with observer and options"
)]
pub(crate) async fn send_stream_response<Resp: CodecMessage + Send>(
    response: Response<Streaming<Resp>>,
    mut respond: backend::SendResponse,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
    call: &CancelFlag,
) -> Status {
    let (mut stream, headers, trailers, compress) = response.split();
    // Headers go out before the first message so a client that only wants
    // initial metadata is not blocked behind handler work.
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let codec = select_outbound_codec(compress, prefer_gzip, negotiated);
    super::deadline::before_commit(call).await;
    let Ok(mut send) = send_ok_headers(&mut respond, &headers, codec, wire.accept_gzip) else {
        return Status::unavailable("failed to send response headers");
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
    }
    let ok_trailers = trailers;
    super::deadline::before_commit(call).await;
    let mut status = Status::ok();
    // The deadline has to cover the whole response, not just the handler
    // future: a producer that stops early because *its* deadline expired must
    // not be reported as a clean end of stream.
    let drained = drain_to_wire(
        &mut stream,
        &mut send,
        wire,
        compress,
        prefer_gzip,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
        budget,
        observer,
        call_labels,
        tap.cloned(),
        channelz_socket,
        call,
        false,
    )
    .await;
    if let Err(err) = drained {
        // A transport failure cannot be reported; a producer failure becomes
        // the stream's trailing status.
        match err {
            DrainError::Transport => return Status::unavailable("transport closed during stream"),
            DrainError::Producer(producer) => status = producer,
        }
    }
    // If the deadline elapsed, the RPC did not finish in time, however the
    // drain ended. A handler reading its request stream sees the deadline as an
    // error on the read and will usually just stop producing, which would
    // otherwise be indistinguishable from a clean end of stream.
    if let Some(at) = deadline {
        if status.is_ok() && tokio::time::Instant::now() >= at {
            status = Status::deadline_exceeded();
        }
    }
    super::deadline::before_commit(call).await;
    if status.is_ok() && ok_trailers.is_empty() {
        if let Some(tap) = tap {
            tap.log_trailer(status.metadata(), &status);
        }
        super::deadline::before_commit(call).await;
        send.send_trailers(crate::wire::bare_ok_trailer_map()).ok();
        return status;
    }
    if status.is_ok() {
        *status.metadata_mut() = ok_trailers;
    }
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    super::deadline::before_commit(call).await;
    if let Ok(map) = grpc_trailers(&status) {
        send.send_trailers(map).ok();
    }
    status
}

#[cfg(feature = "grpc-web")]
#[allow(
    clippy::too_many_arguments,
    reason = "internal stream write helper with observer and options"
)]
pub(crate) async fn send_web_stream_response<Resp: CodecMessage + Send>(
    response: Response<Streaming<Resp>>,
    mut respond: backend::SendResponse,
    web: crate::web::Mode,
    wire: Wire,
    deadline: Option<tokio::time::Instant>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<&crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
    call: &CancelFlag,
) -> Status {
    let (mut stream, headers, trailers, compress) = response.split();
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let codec = select_outbound_codec(compress, prefer_gzip, negotiated);
    super::deadline::before_commit(call).await;
    let Ok(mut send) =
        crate::web::send_ok_headers(&mut respond, web, &headers, codec, wire.accept_gzip)
    else {
        return Status::unavailable("failed to send response headers");
    };
    if let Some(tap) = tap {
        tap.log_server_header(&headers);
    }
    let ok_trailers = trailers;
    super::deadline::before_commit(call).await;
    let mut status = Status::ok();
    let mut text = WebWriter::new(call);
    let drained = if web.is_text() {
        drain_to_web_text(
            &mut stream,
            &mut send,
            web,
            &mut text,
            wire,
            compress,
            prefer_gzip,
            peer_accepts_gzip,
            peer_accepts_deflate,
            #[cfg(feature = "zstd")]
            peer_accepts_zstd,
            budget,
            observer,
            call_labels,
            tap.cloned(),
            channelz_socket,
            call,
        )
        .await
    } else {
        drain_to_wire(
            &mut stream,
            &mut send,
            wire,
            compress,
            prefer_gzip,
            peer_accepts_gzip,
            peer_accepts_deflate,
            #[cfg(feature = "zstd")]
            peer_accepts_zstd,
            budget,
            observer,
            call_labels,
            tap.cloned(),
            channelz_socket,
            call,
            true,
        )
        .await
    };
    if let Err(err) = drained {
        match err {
            DrainError::Transport => return Status::unavailable("transport closed during stream"),
            DrainError::Producer(producer) => status = producer,
        }
    }
    if let Some(at) = deadline {
        if status.is_ok() && tokio::time::Instant::now() >= at {
            status = Status::deadline_exceeded();
        }
    }
    if status.is_ok() && !ok_trailers.is_empty() {
        *status.metadata_mut() = ok_trailers;
    }
    if let Some(tap) = tap {
        tap.log_trailer(status.metadata(), &status);
    }
    super::deadline::before_commit(call).await;
    call.deadline.web_frame_started();
    if crate::web::send_trailers(&mut send, web, &mut text, &status, wire.send_buffer)
        .await
        .is_ok()
    {
        call.deadline.web_frame_finished();
    }
    status
}

#[cfg(feature = "grpc-web")]
#[allow(
    clippy::too_many_arguments,
    reason = "internal stream write helper with observer and options"
)]
pub(crate) async fn drain_to_web_text<Resp: CodecMessage + Send>(
    stream: &mut Streaming<Resp>,
    send: &mut backend::SendStream,
    web: crate::web::Mode,
    text: &mut crate::web::TextEncoder,
    wire: Wire,
    envelope: Option<bool>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
    call: &CancelFlag,
) -> Result<(), DrainError> {
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    let channelz_entry = channelz_socket.and_then(crate::channelz::socket_entry);
    loop {
        super::deadline::before_commit(call).await;
        let item = tokio::select! {
            biased;
            reset = poll_fn(|cx| send.poll_reset(cx)) => {
                drop(reset);
                return Err(DrainError::Transport);
            }
            item = stream.next_framed() => item,
        };
        let item = match item {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(status) => return Err(DrainError::Producer(status)),
        };
        let codec = select_stream_codec(item.compressed, envelope, prefer_gzip, negotiated);
        let frame = encode_msg(&item.message, codec, wire.limits, wire.gzip_level)
            .map_err(DrainError::Producer)?;
        let permit = budget
            .acquire(frame.total_len())
            .map_err(DrainError::Producer)?;
        if let Some(tap) = &tap {
            for seg in frame.segments() {
                tap.log_written(seg);
            }
        }
        if let Some(obs) = observer {
            obs.on_bytes_sent(call_labels, frame.total_len());
        }
        if let Some(entry) = &channelz_entry {
            entry.add_messages(true, 1);
        }
        super::deadline::before_commit(call).await;
        call.deadline.web_frame_started();
        crate::web::send_frame(send, web, text, frame, wire.send_buffer)
            .await
            .map_err(|_| DrainError::Transport)?;
        call.deadline.web_frame_finished();
        drop(permit);
    }
    Ok(())
}

/// Why a stream stopped before its clean end.
pub(crate) enum DrainError {
    /// The wire is gone, so no status can be delivered.
    Transport,
    /// The handler ended the stream with a status.
    Producer(Status),
}

async fn flush_batch(
    batch: &mut OutBatch,
    send: &mut backend::SendStream,
    call: &CancelFlag,
    web: bool,
) -> Result<(), DrainError> {
    super::deadline::before_commit(call).await;
    if web {
        call.deadline.web_frame_started();
    }
    batch.flush(send).await.map_err(|_| DrainError::Transport)?;
    if web {
        call.deadline.web_frame_finished();
    }
    Ok(())
}

async fn flush_queued_before_error(
    batch: &mut OutBatch,
    send: &mut backend::SendStream,
    permits: &mut Vec<crate::limits::BytePermit>,
    status: Status,
    call: &CancelFlag,
    web: bool,
) -> Result<(), DrainError> {
    flush_batch(batch, send, call, web).await?;
    permits.clear();
    Err(DrainError::Producer(status))
}

/// Copy every message from `stream` onto `send`, batching each burst.
#[allow(
    clippy::too_many_arguments,
    reason = "internal stream drain helper with observer and options"
)]
pub(crate) async fn drain_to_wire<Resp: CodecMessage + Send>(
    stream: &mut Streaming<Resp>,
    send: &mut backend::SendStream,
    mut wire: Wire,
    envelope: Option<bool>,
    prefer_gzip: bool,
    peer_accepts_gzip: bool,
    peer_accepts_deflate: bool,
    #[cfg(feature = "zstd")] peer_accepts_zstd: bool,
    budget: &ByteBudgetTracker,
    observer: Option<&dyn LifecycleObserver>,
    call_labels: &CallLabels<'_>,
    tap: Option<crate::binlog::CallLogger>,
    channelz_socket: Option<crate::channelz::SocketId>,
    call: &CancelFlag,
    web: bool,
) -> Result<(), DrainError> {
    // Negotiated once per response: every frame shares the `grpc-encoding`
    // the headers advertised, so per-message selection only decides the
    // Compressed-Flag.
    let negotiated = preferred_codec(
        wire.send_codec,
        peer_accepts_gzip,
        peer_accepts_deflate,
        #[cfg(feature = "zstd")]
        peer_accepts_zstd,
    );
    wire.send_codec = negotiated.unwrap_or_default();
    let mut batch = OutBatch::new(wire);
    if let Some(tap) = tap {
        batch.set_tap(tap);
    }
    // One map lookup for the whole response. Each encoded message then
    // touches the entry's atomics.
    let channelz_entry = channelz_socket.and_then(crate::channelz::socket_entry);
    let mut items = Vec::with_capacity(OutBatch::BURST);
    let mut permits = Vec::with_capacity(OutBatch::BURST);
    loop {
        super::deadline::before_commit(call).await;
        items.clear();
        // A client RST while we wait for the next message must abort: a
        // producer that is itself waiting (Health Watch, a timer) will not
        // send, so the write path would never see the reset.
        let n = tokio::select! {
            biased;
            reset = poll_fn(|cx| send.poll_reset(cx)) => {
                drop(reset);
                return Err(DrainError::Transport);
            }
            n = stream.recv_many(&mut items, OutBatch::BURST) => n,
        };
        if n == 0 {
            break;
        }
        // More than one message queued means the producer is running ahead of
        // the network and is bounded by its channel depth, so one scheduling
        // turn lets it top the queue up and doubles the write size. Exactly one
        // means it is not ahead — a request/response stream, say — and must not
        // pay a turn of latency for nothing.
        let room = OutBatch::BURST - items.len();
        // The sender is already dropped, so every message it produced is queued.
        // Yielding cannot enlarge this batch.
        if items.len() > 1 && room > 0 && !stream.producer_finished() {
            let_producer_catch_up().await;
            stream.try_recv_many(&mut items, room);
        }
        for item in items.drain(..) {
            let mut item = match item {
                Ok(item) => item,
                Err(status) => {
                    return flush_queued_before_error(
                        &mut batch,
                        send,
                        &mut permits,
                        status,
                        call,
                        web,
                    )
                    .await;
                }
            };
            item.compressed =
                select_stream_codec(item.compressed, envelope, prefer_gzip, negotiated).is_some();
            let frame_len = 5 + item.message.encoded_len();
            let permit = match budget.acquire(frame_len) {
                Ok(permit) => permit,
                Err(status) => {
                    return flush_queued_before_error(
                        &mut batch,
                        send,
                        &mut permits,
                        status,
                        call,
                        web,
                    )
                    .await;
                }
            };
            permits.push(permit);
            if let Err(status) = batch.encode(item) {
                return flush_queued_before_error(
                    &mut batch,
                    send,
                    &mut permits,
                    status,
                    call,
                    web,
                )
                .await;
            }
            if let Some(obs) = observer {
                obs.on_bytes_sent(call_labels, frame_len);
            }
            if let Some(entry) = &channelz_entry {
                entry.add_messages(true, 1);
            }
            if batch.is_full() {
                flush_batch(&mut batch, send, call, web).await?;
                permits.clear();
            }
        }
        if !batch.is_full() {
            flush_batch(&mut batch, send, call, web).await?;
            permits.clear();
        }
    }
    flush_batch(&mut batch, send, call, web).await?;
    permits.clear();
    Ok(())
}

#[cfg(test)]
mod cancel_flag_wait_tests {
    use super::{CancelFlag, CancelOnDrop, wait_cancel};
    use std::future::poll_fn;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::task::{Context, Poll, Wake, Waker};

    #[derive(Default)]
    struct WakeCount(AtomicUsize);

    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[tokio::test]
    async fn signal_wakes_a_task_that_is_already_waiting() {
        let flag = CancelFlag::new();
        let waiting = wait_cancel(Some(Arc::clone(&flag)));
        let gate = Arc::clone(&flag);
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            gate.signal();
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .expect("waiter woke");
    }

    #[tokio::test]
    async fn signal_before_wait_is_ready_without_parking() {
        let flag = CancelFlag::new();
        flag.signal();
        let mut waiting = wait_cancel(Some(flag));
        poll_fn(|cx| {
            assert!(Pin::new(&mut waiting).poll(cx).is_ready());
            Poll::Ready(())
        })
        .await;
    }

    #[test]
    fn inbound_owner_abandoned_waiter_churn_releases_wakers_and_slots() {
        let flag = CancelFlag::new();
        let count = Arc::new(WakeCount::default());
        let waker = Waker::from(Arc::clone(&count));
        for _ in 0..10_000 {
            let mut waiting = wait_cancel(Some(Arc::clone(&flag)));
            assert!(
                Pin::new(&mut waiting)
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            drop(waiting);
        }
        assert_eq!(flag.waiters.lock().expect("waiters").len(), 0);
        assert_eq!(Arc::strong_count(&count), 2);
        assert!(flag.waiters.lock().expect("waiters").capacity() <= 4);
    }

    #[test]
    fn inbound_owner_reused_slots_preserve_two_active_waiters() {
        let flag = CancelFlag::new();
        let first_count = Arc::new(WakeCount::default());
        let first_waker = Waker::from(Arc::clone(&first_count));
        let mut first = wait_cancel(Some(Arc::clone(&flag)));
        assert!(
            Pin::new(&mut first)
                .poll(&mut Context::from_waker(&first_waker))
                .is_pending()
        );
        let second_count = Arc::new(WakeCount::default());
        let second_waker = Waker::from(Arc::clone(&second_count));
        let mut second = wait_cancel(Some(Arc::clone(&flag)));
        assert!(
            Pin::new(&mut second)
                .poll(&mut Context::from_waker(&second_waker))
                .is_pending()
        );
        for _ in 0..10_000 {
            let mut abandoned = wait_cancel(Some(Arc::clone(&flag)));
            assert!(
                Pin::new(&mut abandoned)
                    .poll(&mut Context::from_waker(&first_waker))
                    .is_pending()
            );
            drop(abandoned);
        }
        assert_eq!(flag.waiters.lock().expect("waiters").len(), 2);
        assert!(flag.waiters.lock().expect("waiters").capacity() <= 4);
        flag.signal();
        assert_eq!(first_count.0.load(Ordering::Relaxed), 1);
        assert_eq!(second_count.0.load(Ordering::Relaxed), 1);
        assert!(
            Pin::new(&mut first)
                .poll(&mut Context::from_waker(&first_waker))
                .is_ready()
        );
        assert!(
            Pin::new(&mut second)
                .poll(&mut Context::from_waker(&second_waker))
                .is_ready()
        );
    }

    #[test]
    fn inbound_owner_waiter_drop_allows_waker_drop_to_signal() {
        struct SignalOnDrop(Arc<CancelFlag>);
        #[expect(
            clippy::manual_noop_waker,
            reason = "the custom destructor reenters cancellation; Waker::noop would remove the tested callback"
        )]
        impl Wake for SignalOnDrop {
            fn wake(self: Arc<Self>) {}
        }
        impl Drop for SignalOnDrop {
            fn drop(&mut self) {
                self.0.signal();
            }
        }
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let flag = CancelFlag::new();
            let reentrant = Waker::from(Arc::new(SignalOnDrop(Arc::clone(&flag))));
            let mut abandoned = wait_cancel(Some(Arc::clone(&flag)));
            assert!(
                Pin::new(&mut abandoned)
                    .poll(&mut Context::from_waker(&reentrant))
                    .is_pending()
            );
            drop(reentrant);
            let count = Arc::new(WakeCount::default());
            let waker = Waker::from(Arc::clone(&count));
            let mut active = wait_cancel(Some(Arc::clone(&flag)));
            assert!(
                Pin::new(&mut active)
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            drop(abandoned);
            assert!(flag.is_cancelled());
            assert_eq!(count.0.load(Ordering::Relaxed), 1);
            assert!(
                Pin::new(&mut active)
                    .poll(&mut Context::from_waker(&waker))
                    .is_ready()
            );
            done_tx.send(()).expect("observer alive");
        });
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("dropping a waiter failed to release its reentrant waker");
        worker.join().expect("waiter drop worker panicked");
    }

    #[test]
    fn signal_wakes_every_registered_waiter() {
        let flag = CancelFlag::new();
        let mut waiters = Vec::new();
        for _ in 0..8 {
            let count = Arc::new(WakeCount::default());
            let waker = Waker::from(Arc::clone(&count));
            let mut waiting = wait_cancel(Some(Arc::clone(&flag)));
            assert!(
                Pin::new(&mut waiting)
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
            waiters.push((waiting, count, waker));
        }
        std::thread::scope(|scope| {
            scope.spawn(|| flag.signal());
        });
        for (mut waiting, count, waker) in waiters {
            assert_eq!(count.0.load(Ordering::Relaxed), 1);
            assert!(
                Pin::new(&mut waiting)
                    .poll(&mut Context::from_waker(&waker))
                    .is_ready()
            );
        }
    }

    #[test]
    fn replacing_a_waker_allows_its_drop_to_signal_cancellation() {
        struct SignalOnDrop {
            flag: Arc<CancelFlag>,
            dropped: Arc<AtomicUsize>,
        }

        #[allow(
            clippy::manual_noop_waker,
            reason = "the regression exercises cancellation from the waker owner's destructor"
        )]
        impl Wake for SignalOnDrop {
            fn wake(self: Arc<Self>) {}
        }

        impl Drop for SignalOnDrop {
            fn drop(&mut self) {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.flag.signal();
            }
        }

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let flag = CancelFlag::new();
            let dropped = Arc::new(AtomicUsize::new(0));
            let previous = Waker::from(Arc::new(SignalOnDrop {
                flag: Arc::clone(&flag),
                dropped: Arc::clone(&dropped),
            }));
            let mut waiting = wait_cancel(Some(Arc::clone(&flag)));
            assert!(
                Pin::new(&mut waiting)
                    .poll(&mut Context::from_waker(&previous))
                    .is_pending()
            );
            // Leave the registry as the last owner, so replacement runs Drop.
            drop(previous);
            let count = Arc::new(WakeCount::default());
            let replacement = Waker::from(Arc::clone(&count));
            assert!(
                Pin::new(&mut waiting)
                    .poll(&mut Context::from_waker(&replacement))
                    .is_ready()
            );
            assert_eq!(dropped.load(Ordering::Relaxed), 1);
            assert_eq!(count.0.load(Ordering::Relaxed), 1);
            assert!(flag.is_cancelled());
            done_tx.send(()).expect("test observer remains alive");
        });
        // A reentrant lock deadlock fails this test without hanging libtest.
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("waker replacement deadlocked while signaling cancellation");
        worker.join().expect("waker replacement worker panicked");
    }

    #[test]
    fn registration_racing_signal_is_ready_or_woken_for_every_waiter() {
        for round in 0..512 {
            let flag = CancelFlag::new();
            let gate = Arc::new(Barrier::new(2));
            let waiters = std::thread::scope(|scope| {
                let gate_ref = &gate;
                let flag_ref = &flag;
                scope.spawn(move || {
                    gate_ref.wait();
                    if round % 2 == 0 {
                        std::thread::yield_now();
                    }
                    flag_ref.signal();
                });
                let mut waiters = Vec::new();
                gate.wait();
                for _ in 0..8 {
                    let count = Arc::new(WakeCount::default());
                    let waker = Waker::from(Arc::clone(&count));
                    let mut waiting = wait_cancel(Some(Arc::clone(&flag)));
                    let pending = Pin::new(&mut waiting)
                        .poll(&mut Context::from_waker(&waker))
                        .is_pending();
                    waiters.push((waiting, count, waker, pending));
                }
                waiters
            });
            for (mut waiting, count, waker, pending) in waiters {
                if pending {
                    assert!(
                        count.0.load(Ordering::Relaxed) >= 1,
                        "registered waiter was not woken in round {round}"
                    );
                }
                assert!(
                    Pin::new(&mut waiting)
                        .poll(&mut Context::from_waker(&waker))
                        .is_ready()
                );
            }
        }
    }

    #[test]
    fn last_owner_is_reused_and_not_cancelled() {
        let first = CancelFlag::new();
        let ptr = Arc::as_ptr(&first);
        drop(CancelOnDrop::new(first));
        let second = CancelFlag::new();
        assert!(!second.is_cancelled());
        assert_eq!(second.state.load(std::sync::atomic::Ordering::Acquire), 0);
        assert!(std::ptr::eq(Arc::as_ptr(&second), ptr));
    }

    #[test]
    fn drop_still_signals_when_another_clone_is_alive() {
        let flag = CancelFlag::new();
        let watch = Arc::clone(&flag);
        drop(CancelOnDrop::new(flag));
        assert!(watch.is_cancelled());
    }
}

#[cfg(test)]
#[path = "sv09_cancel_tests.rs"]
mod sv09_cancel_tests;

#[cfg(test)]
#[path = "sv09_timeout_tests.rs"]
mod sv09_timeout_tests;
