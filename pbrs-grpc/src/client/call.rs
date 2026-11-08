//! Per-call machinery: attempts, commitment, and send/open races.

use super::channel::RequestRouting;
use super::pool::{ChannelInner, HealthDirective, LiveConn};
use crate::request::{Request, Response};
use crate::rt::{Runtime, TokioRuntime};
use crate::status::{Code, Status, TransportEvidence};
use crate::telemetry::LifecycleObserver;
use crate::timeout::remaining_timeout;
use crate::transport::{
    Error as TransportError, Reason, RecvStream, SendRequest, SendStream, h2 as backend,
};
use crate::wire::{
    ImmediateSend, SegFrame, begin_stock_request, grpc_request, grpc_request_with_scheme,
    send_frame, send_frame_if_ready, status_from,
};
use futures_util::future::Either;
use http::HeaderValue;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, watch};

/// Tracks commitment state of an RPC attempt according to gRFC A6.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum AttemptCommitment {
    /// Request HEADERS have not been sent (or stream open failed).
    Uncommitted,
    /// Request DATA transmission has started or completed.
    BodyStarted,
    /// Initial response HEADERS have been received; attempt is committed to response.
    ResponseCommitted,
}

impl AttemptCommitment {
    pub(crate) fn classify(self, status: Status) -> Status {
        match self {
            Self::Uncommitted => status,
            Self::BodyStarted | Self::ResponseCommitted => {
                if let Some(ev) = status.transport_evidence() {
                    if !ev.is_transparent_retryable() {
                        let mut s = status;
                        s.mark_transport(TransportEvidence::AmbiguousLoss);
                        return s;
                    }
                }
                status
            }
        }
    }

    pub(crate) fn classify_h2(self, err: TransportError) -> Status {
        match self {
            Self::Uncommitted => Status::from_h2_pre_headers(err),
            Self::BodyStarted | Self::ResponseCommitted => Status::from_h2_post_dispatch(err),
        }
    }
}

#[cfg(test)]
pub(crate) async fn prefer_peer_rejection_after_send<T>(
    response: impl std::future::Future<Output = ResponseHead>,
    send_error: Status,
) -> Result<T, Status> {
    prefer_peer_rejection_with_commitment(response, send_error, &mut false, None).await
}

pub(crate) fn response_commits(response: &http::Response<backend::RecvStream>) -> bool {
    // A valid trailers-only application rejection remains retry eligible.
    !(response.status() == http::StatusCode::OK
        && response.body().is_end_stream()
        && response
            .headers()
            .get("grpc-status")
            .and_then(|code| code.to_str().ok())
            .and_then(|code| code.parse::<i32>().ok())
            .is_some_and(|code| matches!(code, 1..=16)))
}

/// Parse a trailers-only rejection, retaining a pre-reset EOS classification.
pub(crate) fn peer_rejection_from_head(
    response: &http::Response<backend::RecvStream>,
    cached_commitment: Option<bool>,
) -> Option<Status> {
    let grpc_code = response
        .headers()
        .get("grpc-status")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i32>().ok());
    if response.status() == http::StatusCode::OK
        && (cached_commitment == Some(false) || response.body().is_end_stream())
        && grpc_code.is_some_and(|code| code != 0)
    {
        Some(status_from(response.headers(), None))
    } else {
        None
    }
}

pub(crate) async fn prefer_peer_rejection_with_commitment<T>(
    response: impl std::future::Future<
        Output = Result<http::Response<backend::RecvStream>, TransportError>,
    >,
    send_error: Status,
    response_committed: &mut bool,
    cached_commitment: Option<bool>,
) -> Result<T, Status> {
    let response = if send_error.is_transport() {
        Some(response.await)
    } else if std::error::Error::source(&send_error)
        .is_some_and(|cause| cause.is::<TransportError>())
    {
        // A peer rejection can retire the send stream before send_data runs.
        // The resulting InactiveStreamId is a backend user error, without
        // transport retry evidence. Inspect queued headers, but never wait
        // for a response after a local send error.
        tokio::pin!(response);
        poll_now(response.as_mut())
    } else {
        None
    };
    if let Some(response) = response {
        match response {
            Ok(response) => {
                *response_committed =
                    cached_commitment.unwrap_or_else(|| response_commits(&response));
                // Never turn an incomplete upload into OK.
                if let Some(status) = peer_rejection_from_head(&response, cached_commitment) {
                    return Err(status);
                }
            }
            // A stream reset can close the send half before its explicit refusal
            // reaches the response future. Only REFUSED_STREAM proves no execution.
            Err(error) if error.reason() == Some(Reason::REFUSED_STREAM) => {
                return Err(Status::from_h2_post_dispatch(error));
            }
            Err(_) => {}
        }
    }
    Err(send_error)
}

pub(crate) type ResponseHead = Result<http::Response<backend::RecvStream>, TransportError>;

/// Response evidence observed while sending one request frame.
#[derive(Default)]
pub(crate) struct UploadHead {
    pub(crate) head: Option<ResponseHead>,
    /// Classification saved before any reset can change the body's EOS state.
    pub(crate) committed: Option<bool>,
    /// A reset was sent; a later-discovered head must count as committed.
    pub(crate) reset: bool,
}

impl UploadHead {
    fn capture(&mut self, head: ResponseHead) {
        self.committed = head
            .as_ref()
            .ok()
            .map(|response| self.reset || response_commits(response));
        self.head = Some(head);
    }
}

pub(crate) fn send_request_frame(
    send: &mut backend::SendStream,
    frame: SegFrame,
    send_buffer: usize,
    cancel_rx: &crate::rpc_cancel::RpcCancel,
    deadline: Option<tokio::time::Instant>,
    response: &mut backend::ResponseFuture,
    received: &mut UploadHead,
) -> impl std::future::Future<Output = Result<(), Status>> {
    send_request_frame_in::<TokioRuntime>(
        send,
        frame,
        send_buffer,
        cancel_rx,
        deadline,
        response,
        received,
    )
}

/// [`send_request_frame`] on runtime `R`; production callers use Tokio.
/// Poll `fut` once without arming cancellation. Callers fall back to
/// [`first_of_in`] on `None`, so the race (and its waiter registration)
/// is paid only when the operation actually waits.
pub(crate) fn poll_now<F: std::future::Future>(fut: Pin<&mut F>) -> Option<F::Output> {
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    match fut.poll(&mut cx) {
        Poll::Ready(value) => Some(value),
        Poll::Pending => None,
    }
}

// Observe a queued head before resetting a failed upload. Do not poll a
// response future twice once it has returned Ready.
fn capture_ready_response(response: &mut backend::ResponseFuture, received: &mut UploadHead) {
    if received.head.is_none() {
        if let Some(head) = poll_now(Pin::new(response)) {
            received.capture(head);
        }
    }
}

/// `true` when a cancel watch has been set.
///
/// An unchanged version is still the initial `false`, so the steady path is
/// one atomic load. The value lock runs only after a send or a dropped sender.
#[inline]
pub(crate) fn cancel_requested(rx: &crate::rpc_cancel::RpcCancel) -> bool {
    rx.is_cancelled()
}

pub(crate) async fn send_request_frame_in<R: Runtime>(
    send: &mut backend::SendStream,
    frame: SegFrame,
    send_buffer: usize,
    cancel_rx: &crate::rpc_cancel::RpcCancel,
    deadline: Option<tokio::time::Instant>,
    response: &mut backend::ResponseFuture,
    received: &mut UploadHead,
) -> Result<(), Status> {
    // Optimistic: a send with capacity completes without arming the race.
    // The pre-checks mirror `first_of_in`'s biased order (cancel, then
    // deadline) so an already-fired signal still wins, with the same reset.
    if cancel_requested(cancel_rx) {
        reset_request_send(send, response, received);
        return Err(Status::cancelled());
    }
    if remaining_timeout(deadline).is_err() {
        reset_request_send(send, response, received);
        return Err(Status::deadline_exceeded());
    }
    let frame = match send_frame_if_ready(send, frame, true, send_buffer) {
        ImmediateSend::Done(result) => {
            return finish_request_send::<R>(send, result, deadline, response, received);
        }
        ImmediateSend::Blocked(frame) => frame,
    };
    let raced = {
        let send_fut = send_frame(send, frame, true, send_buffer);
        tokio::pin!(send_fut);
        match poll_now(send_fut.as_mut()) {
            Some(result) => result,
            None => {
                let upload = poll_fn(|cx| {
                    if let Poll::Ready(sent) = send_fut.as_mut().poll(cx) {
                        return Poll::Ready(sent);
                    }
                    let head = received
                        .head
                        .is_none()
                        .then(|| Pin::new(&mut *response).poll(cx));
                    if let Some(Poll::Ready(head)) = head {
                        // A server can reject without granting upload credit
                        // or resetting the request half. Only a valid error
                        // ends an incomplete upload; ordinary response
                        // headers are saved while the send keeps waiting.
                        received.capture(head);
                        let rejection = match (&received.head, received.committed) {
                            (Some(Ok(head)), Some(false)) => {
                                Some(status_from(head.headers(), None))
                            }
                            _ => None,
                        };
                        if let Some(status) = rejection {
                            return Poll::Ready(Err(status));
                        }
                    }
                    Poll::Pending
                });
                first_of_in::<R, _>(upload, cancel_rx.clone(), deadline).await
            }
        }
    };
    finish_request_send::<R>(send, raced, deadline, response, received)
}

// Probe on either side of the reset lock: HEADERS queued between the two
// probes must also prevent replay. A head found only after reset is treated
// as committed because reset may have erased its END_STREAM state.
fn reset_request_send(
    send: &mut backend::SendStream,
    response: &mut backend::ResponseFuture,
    received: &mut UploadHead,
) {
    capture_ready_response(response, received);
    send.send_reset(Reason::CANCEL);
    received.reset = true;
    capture_ready_response(response, received);
}

/// Apply the deadline preference and reset the send half on cancel.
fn finish_request_send<R: Runtime>(
    send: &mut backend::SendStream,
    raced: Result<(), Status>,
    deadline: Option<tokio::time::Instant>,
    response: &mut backend::ResponseFuture,
    received: &mut UploadHead,
) -> Result<(), Status> {
    let result = if deadline.is_none() {
        raced
    } else {
        prefer_deadline_in::<R, _>(raced, deadline)
    };
    if result.is_err() {
        capture_ready_response(response, received);
    }
    if matches!(
        &result,
        Err(status) if matches!(status.code(), Code::Cancelled | Code::DeadlineExceeded)
    ) {
        reset_request_send(send, response, received);
    }
    result
}

#[allow(
    clippy::too_many_arguments,
    reason = "one HTTP/2 stream open plus headers, timeout, encoding, and scheme"
)]
pub(crate) fn open(
    send_req: backend::SendRequest,
    routing: &impl RequestRouting,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    cancel_rx: &crate::rpc_cancel::RpcCancel,
    send_codec: Option<crate::compression::CompressionAlgorithm>,
    accept_gzip: bool,
    user_agent: &HeaderValue,
) -> impl std::future::Future<Output = Result<(backend::ResponseFuture, backend::SendStream), Status>>
{
    open_in::<TokioRuntime>(
        send_req,
        routing,
        path,
        md,
        timeout,
        deadline,
        cancel_rx,
        send_codec,
        accept_gzip,
        user_agent,
    )
}

/// [`open`] on runtime `R`; production callers use Tokio.
#[allow(
    clippy::too_many_arguments,
    reason = "one HTTP/2 stream open plus headers, timeout, encoding, and scheme"
)]
pub(crate) async fn open_in<R: Runtime>(
    mut send_req: backend::SendRequest,
    routing: &impl RequestRouting,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    cancel_rx: &crate::rpc_cancel::RpcCancel,
    send_codec: Option<crate::compression::CompressionAlgorithm>,
    accept_gzip: bool,
    user_agent: &HeaderValue,
) -> Result<(backend::ResponseFuture, backend::SendStream), Status> {
    let scheme = routing.request_scheme()?;
    let https = *scheme == http::uri::Scheme::HTTPS;
    let standard_scheme = https || *scheme == http::uri::Scheme::HTTP;
    let authority = routing.authority();
    if timeout.is_some_and(|d| d.is_zero()) {
        return Err(Status::deadline_exceeded());
    }
    if cancel_requested(cancel_rx) {
        return Err(Status::cancelled());
    }
    let remaining = outgoing_timeout(timeout, remaining_timeout(deadline)?);
    // No deadline: the stock block does not carry grpc-timeout, so the
    // request head is not needed. Copying it only to throw it away costs
    // two 216-byte moves.
    let stock_admission = standard_scheme
        && deadline.is_none()
        && begin_stock_request(
            authority,
            path,
            md,
            remaining,
            send_codec,
            accept_gzip,
            user_agent,
            https,
        );
    let mut admission = if stock_admission {
        send_req.send_stock_when_ready(false)
    } else {
        let http_req = if standard_scheme {
            grpc_request(
                authority,
                path,
                md,
                remaining,
                send_codec,
                accept_gzip,
                user_agent,
                https,
            )?
        } else {
            grpc_request_with_scheme(
                authority,
                path,
                md,
                remaining,
                send_codec,
                accept_gzip,
                user_agent,
                scheme,
            )?
        };
        send_req.send_request_when_ready(http_req, false)
    };
    // Stock admission has no deadline and no grpc-timeout, and cancel was
    // checked above. A ready stream does not need the poll wrapper.
    if stock_admission {
        if let Some(result) = poll_now(std::pin::Pin::new(&mut admission)) {
            return result.map_err(Status::from_h2_pre_headers);
        }
    }
    // grpc_request already encoded this duration. In particular, the existing
    // sub-20ms policy leaves it unchanged on a warm call's first guarded poll.
    let mut last_timeout = remaining;
    // Every possible admission poll checks the same biased order as the outer
    // race. Request headers remain untouched by h2 while pending; refresh the
    // relative timeout here, outside its stream lock, immediately before poll.
    let guarded = poll_fn(|cx| {
        if cancel_requested(cancel_rx) {
            return Poll::Ready(Err(Status::cancelled()));
        }
        // A warm call has no deadline and no grpc-timeout. The refresh stays
        // out of this poll.
        if timeout.is_some() || deadline.is_some() {
            if let Err(status) =
                refresh_admission_timeout(&mut admission, &mut last_timeout, timeout, deadline)
            {
                return Poll::Ready(Err(status));
            }
        }
        Pin::new(&mut admission)
            .poll(cx)
            .map(|result| result.map_err(Status::from_h2_pre_headers))
    });
    tokio::pin!(guarded);
    // Preserve the optimistic warm path, but never poll admission without the
    // cancellation/deadline/header checks above.
    // A copied request is marked only for this poll. A pending admission has
    // not built the HEADERS frame, so the mark must not leak to a later send.
    // The stock frame is built from the saved pseudos and does not use it.
    if !stock_admission {
        crate::h2_backend::header_pool::promote_pending_stock();
    }
    let raced = match poll_now(guarded.as_mut()) {
        Some(result) => {
            if !stock_admission {
                crate::h2_backend::header_pool::clear_next_stock();
            }
            // No deadline: return the halves from this arm so they are not
            // stored and moved again after the match.
            if deadline.is_none() {
                return result;
            }
            result
        }
        None => {
            if !stock_admission {
                crate::h2_backend::header_pool::clear_next_stock();
            }
            first_of_in::<R, _>(guarded, cancel_rx.clone(), deadline).await
        }
    };
    // The warm poll above already returned. This is the path that waited,
    // still with no deadline, so `prefer_deadline_in` would only move the pair.
    if deadline.is_none() {
        return raced;
    }
    prefer_deadline_in::<R, _>(raced, deadline)
}

#[cold]
#[inline(never)]
fn refresh_admission_timeout(
    admission: &mut backend::Admission<'_>,
    last_timeout: &mut Option<Duration>,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
) -> Result<(), Status> {
    let remaining = outgoing_timeout(timeout, remaining_timeout(deadline)?);
    if let Some(remaining) = remaining {
        if *last_timeout != Some(remaining) {
            let value = HeaderValue::from_str(&crate::timeout::encode_timeout(remaining))
                .map_err(|error| Status::internal(error.to_string()))?;
            if let Some(request) = admission.request_mut() {
                request.headers_mut().insert("grpc-timeout", value);
                *last_timeout = Some(remaining);
            }
        }
    }
    Ok(())
}

fn outgoing_timeout(initial: Option<Duration>, remaining: Option<Duration>) -> Option<Duration> {
    match (initial, remaining) {
        (Some(initial), Some(remaining)) => {
            if initial > remaining && initial - remaining < Duration::from_millis(20) {
                Some(initial)
            } else {
                Some(remaining)
            }
        }
        (None, remaining) => remaining,
        (Some(initial), None) => Some(initial),
    }
}

/// Report an expired deadline as `DEADLINE_EXCEEDED`, whatever the transport
/// said.
///
/// A server enforcing the same `grpc-timeout` resets the stream at the
/// deadline, and that reset can reach us before our own timer fires. Reporting
/// it as `UNAVAILABLE` or `CANCELLED` would tell the caller the connection
/// failed when in fact their deadline elapsed, so the deadline wins. Real
/// statuses from the peer are left alone.
pub(crate) fn prefer_deadline<T>(
    result: Result<T, Status>,
    deadline: Option<tokio::time::Instant>,
) -> Result<T, Status> {
    prefer_deadline_in::<TokioRuntime, T>(result, deadline)
}

/// [`prefer_deadline`] on runtime `R`; production callers use Tokio.
pub(crate) fn prefer_deadline_in<R: Runtime, T>(
    result: Result<T, Status>,
    deadline: Option<tokio::time::Instant>,
) -> Result<T, Status> {
    let Some(at) = deadline else {
        return result;
    };
    match &result {
        Err(status)
            if matches!(status.code(), Code::Unavailable | Code::Cancelled) && R::now() >= at =>
        {
            Err(Status::deadline_exceeded())
        }
        _ => result,
    }
}

pub(crate) fn first_of<'a, T>(
    fut: impl Future<Output = Result<T, Status>> + Send + 'a,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    deadline: Option<tokio::time::Instant>,
) -> impl Future<Output = Result<T, Status>> + Send + 'a
where
    T: Send + 'a,
{
    first_of_in::<TokioRuntime, T>(fut, cancel_rx, deadline)
}

/// [`first_of`] on runtime `R`.
///
/// A deadline and a plain cancel are different futures. The deadline arm is
/// boxed so a call with no deadline does not carry the timer.
pub(crate) fn first_of_in<'a, R, T>(
    fut: impl Future<Output = Result<T, Status>> + Send + 'a,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    deadline: Option<tokio::time::Instant>,
) -> impl Future<Output = Result<T, Status>> + Send + 'a
where
    R: Runtime + 'a,
    T: Send + 'a,
{
    match deadline {
        None => Either::Left(first_of_cancel(fut, cancel_rx)),
        Some(at) => Either::Right(Box::pin(first_of_deadline::<R, T, _>(fut, cancel_rx, at))
            as Pin<Box<dyn Future<Output = Result<T, Status>> + Send + 'a>>),
    }
}

async fn first_of_cancel<T>(
    fut: impl Future<Output = Result<T, Status>>,
    cancel_rx: crate::rpc_cancel::RpcCancel,
) -> Result<T, Status> {
    tokio::select! {
        biased;
        _ = cancel_rx.wait() => Err(Status::cancelled()),
        r = fut => r,
    }
}

async fn first_of_deadline<R, T, F>(
    fut: F,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    at: tokio::time::Instant,
) -> Result<T, Status>
where
    R: Runtime,
    F: Future<Output = Result<T, Status>>,
{
    tokio::select! {
        biased;
        _ = cancel_rx.wait() => Err(Status::cancelled()),
        _ = R::sleep_until(at) => Err(Status::deadline_exceeded()),
        r = fut => r,
    }
}

async fn grab_wait<R: Runtime>(
    inner: Arc<ChannelInner>,
    obs: Option<Arc<dyn LifecycleObserver>>,
    health: Option<HealthDirective>,
    hash: Option<u64>,
    cancel_rx: crate::rpc_cancel::RpcCancel,
    deadline: Option<tokio::time::Instant>,
    wait_for_ready: bool,
) -> Result<LiveConn, Status> {
    let acquire = inner.acquire(wait_for_ready, obs.as_deref(), health, hash);
    tokio::pin!(acquire);
    let raced = match poll_now(acquire.as_mut()) {
        Some(result) if deadline.is_none() => return result,
        Some(result) => result,
        None => first_of_in::<R, _>(acquire, cancel_rx, deadline).await,
    };
    if deadline.is_none() {
        return raced;
    }
    let grabbed = prefer_deadline_in::<R, _>(raced, deadline)?;
    let _ = remaining_timeout(deadline)?;
    Ok(grabbed)
}

pub(crate) fn reset_send_on_cancel<T>(
    send: Option<&mut backend::SendStream>,
    result: &Result<T, Status>,
) {
    if let Some(send) = send {
        if matches!(
            result,
            Err(status) if status.code() == Code::Cancelled || status.code() == Code::DeadlineExceeded
        ) {
            send.send_reset(Reason::CANCEL);
        }
    }
}

/// HEADERS sent; request DATA has not started. Transparent retry stops here.
pub(crate) struct Opened {
    pub(crate) load: Option<super::pool::SlotLoadGuard>,
    pub(crate) lease: Option<crate::keepalive::Lease>,
    pub(crate) driver: Option<watch::Sender<bool>>,
    pub(crate) resp_fut: backend::ResponseFuture,
    pub(crate) send: backend::SendStream,
    /// Channelz socket serving the stream, for stream/message counters.
    pub(crate) channelz_socket: Option<crate::channelz::SocketId>,
}

/// Finish an optional attempt guard. The guard exists only when an
/// observer is attached (it has no other effects), so call sites hold
/// `Option<AttemptGuard>` and skip the 144-byte guard otherwise.
pub(crate) fn finish_attempt(guard: &mut Option<crate::telemetry::AttemptGuard>, status: &Status) {
    if let Some(guard) = guard {
        guard.finish(status);
    }
}

/// Cancel an optional attempt guard; see [`finish_attempt`].
pub(crate) fn cancel_attempt(
    guard: &mut Option<crate::telemetry::AttemptGuard>,
    reason: crate::telemetry::CancellationReason,
) {
    if let Some(guard) = guard {
        guard.cancel(reason);
    }
}

/// Reject an optional attempt guard; see [`finish_attempt`].
pub(crate) fn reject_attempt(
    guard: &mut Option<crate::telemetry::AttemptGuard>,
    reason: crate::telemetry::RejectionReason,
    status: &Status,
) {
    if let Some(guard) = guard {
        guard.reject(reason, status);
    }
}

impl super::Channel {
    pub(crate) fn apply_response_hooks<T>(
        &self,
        path: &'static str,
        response: Response<T>,
    ) -> Result<Response<T>, Status> {
        let response = response.with_static_path(path);
        if self.local.response_interceptors.is_empty() {
            return Ok(response);
        }
        crate::interceptor::intercept_response_all(response, &self.local.response_interceptors)
    }

    pub(crate) fn prepare_outbound<T>(
        &self,
        path: &'static str,
        req: &mut Request<T>,
        state: Option<&crate::service_config::ServiceConfigState>,
    ) -> Result<(), Status> {
        // No document: there is no method entry. The lookup is its own call
        // and a large frame; the channel defaults do not need either.
        if state.is_some() {
            return self.prepare_outbound_with_method(path, req, state);
        }
        self.finish_outbound_defaults(path, req)
    }

    #[inline(never)]
    fn prepare_outbound_with_method<T>(
        &self,
        path: &'static str,
        req: &mut Request<T>,
        state: Option<&crate::service_config::ServiceConfigState>,
    ) -> Result<(), Status> {
        if let Some(method) = super::Channel::method_entry(state, path) {
            if req.timeout().is_none() {
                if let Some(timeout) = method.timeout {
                    req.set_timeout(timeout);
                }
            }
            if !req.wait_for_ready_is_set() && method.wait_for_ready == Some(true) {
                req.set_wait_for_ready(true);
            }
        }
        self.finish_outbound_defaults(path, req)
    }

    #[inline(always)]
    fn finish_outbound_defaults<T>(
        &self,
        path: &'static str,
        req: &mut Request<T>,
    ) -> Result<(), Status> {
        // Reject a malformed endpoint origin before hooks, encoding or dialing.
        self.local.request_scheme()?;
        if req.timeout().is_none() {
            if let Some(timeout) = self.local.config.rpc_timeout() {
                req.set_timeout(timeout);
            }
        }
        if !req.wait_for_ready_is_set() && self.local.config.waits_for_ready() {
            req.set_wait_for_ready(true);
        }
        if !req.compress_is_set() && self.local.config.compresses_outbound() {
            req.set_compress(true);
        }
        self.apply_interceptors(path, req)
    }

    fn apply_interceptors<T>(
        &self,
        path: &'static str,
        req: &mut Request<T>,
    ) -> Result<(), Status> {
        for hook in self.local.interceptors.iter() {
            hook.intercept(
                &mut req
                    .outgoing(
                        path,
                        self.authority(),
                        self.local.https,
                        self.grpc_user_agent(),
                        *self.local.config,
                    )
                    .with_connected(self.connected()),
            )?;
        }
        Ok(())
    }

    pub(crate) fn take_rpc_slot(&self) -> Result<Option<OwnedSemaphorePermit>, Status> {
        match &self.local.rpc_slots {
            None => Ok(None),
            Some(slots) => match slots.clone().try_acquire_owned() {
                Ok(permit) => Ok(Some(permit)),
                Err(_) => Err(Status::resource_exhausted("too many concurrent RPCs")),
            },
        }
    }

    /// Wait for a live HTTP/2 sender, redialing this slot if the current one
    /// is dead. Raced against the RPC's deadline and cancel signal so a
    /// hanging reconnect cannot outlive the call. `wait_for_ready` retries
    /// a failed handshake until that race fires.
    pub(crate) fn grab(
        &self,
        cancel_rx: &crate::rpc_cancel::RpcCancel,
        deadline: Option<tokio::time::Instant>,
        wait_for_ready: bool,
        md: Option<&crate::metadata::Metadata>,
    ) -> impl std::future::Future<Output = Result<LiveConn, Status>> {
        self.grab_in::<TokioRuntime>(cancel_rx, deadline, wait_for_ready, md)
    }

    /// [`grab`](Self::grab) on runtime `R`; production callers use Tokio.
    ///
    /// Not inlined: the acquire state machine must not become part of the
    /// caller's future. A warm pool hit returns a ready value.
    #[inline(never)]
    pub(crate) fn grab_in<R: Runtime + 'static>(
        &self,
        cancel_rx: &crate::rpc_cancel::RpcCancel,
        deadline: Option<tokio::time::Instant>,
        wait_for_ready: bool,
        md: Option<&crate::metadata::Metadata>,
    ) -> impl Future<Output = Result<LiveConn, Status>> + Send {
        if let Err(status) = remaining_timeout(deadline) {
            return Either::Left(std::future::ready(Err(status)));
        }
        if cancel_requested(cancel_rx) {
            return Either::Left(std::future::ready(Err(Status::cancelled())));
        }
        // A warm direct connection does not acquire. Cloning the pool, the
        // observer, and the ring hash only to drop them was an atomic pair
        // on every RPC.
        if deadline.is_none() {
            if let Some(conn) = self.inner.try_warm() {
                return Either::Left(std::future::ready(Ok(conn)));
            }
        }
        let inner = Arc::clone(&self.inner);
        let obs = self.local.observer.clone();
        let hash = md.and_then(|md| self.ring_request_hash(md));
        // A pending acquire may already have picked a backend or started a
        // health/dial operation. Keep that same future in grab_wait instead
        // of dropping a speculative poll and repeating those side effects.
        Either::Right(Box::pin(grab_wait::<R>(
            inner,
            obs,
            self.health_directive(),
            hash,
            cancel_rx.clone(),
            deadline,
            wait_for_ready,
        ))
            as Pin<Box<dyn Future<Output = Result<LiveConn, Status>> + Send>>)
    }

    /// Grab a slot and send request HEADERS, retrying once on a raced
    /// connection death. Distinct from unary / server-streaming: those replay
    /// the already-encoded request frame after HEADERS. After this returns,
    /// request DATA may start and this RPC is not retried.
    #[allow(
        clippy::too_many_arguments,
        reason = "path, headers, timeout, encoding, and the grab race"
    )]
    pub(crate) fn open_retrying(
        &self,
        cancel_rx: crate::rpc_cancel::RpcCancel,
        timeout: Option<Duration>,
        deadline: Option<tokio::time::Instant>,
        wait: bool,
        path: &'static str,
        md: &crate::metadata::Metadata,
        compress: bool,
        user_agent: &http::HeaderValue,
        tap: Option<&crate::binlog::CallLogger>,
    ) -> impl std::future::Future<Output = Result<Opened, Status>> {
        self.open_retrying_in::<TokioRuntime>(
            cancel_rx, timeout, deadline, wait, path, md, compress, user_agent, tap,
        )
    }

    /// [`open_retrying`](Self::open_retrying) on runtime `R`; production
    /// callers use Tokio.
    #[allow(
        clippy::too_many_arguments,
        reason = "path, headers, timeout, encoding, and the grab race"
    )]
    pub(crate) async fn open_retrying_in<R: Runtime + 'static>(
        &self,
        cancel_rx: crate::rpc_cancel::RpcCancel,
        timeout: Option<Duration>,
        deadline: Option<tokio::time::Instant>,
        wait: bool,
        path: &'static str,
        md: &crate::metadata::Metadata,
        compress: bool,
        user_agent: &http::HeaderValue,
        tap: Option<&crate::binlog::CallLogger>,
    ) -> Result<Opened, Status> {
        let mut retried = false;
        loop {
            let _ = remaining_timeout(deadline)?;
            let live = self
                .grab_in::<R>(&cancel_rx, deadline, wait, Some(md))
                .await?;
            let (slot, r#gen, lease, driver, route, channelz_socket) = (
                live.slot,
                live.r#gen,
                live.lease,
                live.driver,
                live.route,
                live.channelz_socket,
            );
            let load = live.load;
            let routing = self.local.for_attempt(route.authority());
            match open_in::<R>(
                live.send,
                &routing,
                path,
                md,
                timeout,
                deadline,
                &cancel_rx,
                compress.then_some(self.local.config.send_algorithm()),
                self.local.config.accepts_compressed(),
                user_agent,
            )
            .await
            {
                Ok((resp_fut, send)) => {
                    if let Some(tap) = tap {
                        tap.log_client_header(md, path, routing.authority().as_str(), timeout);
                    }
                    return Ok(Opened {
                        load,
                        lease,
                        driver,
                        resp_fut,
                        send,
                        channelz_socket,
                    });
                }
                Err(status)
                    if !retried
                        && status.is_transparent_retryable()
                        && self.inner.endpoint.can_redial() =>
                {
                    retried = true;
                    self.inner
                        .discard_conn(slot, r#gen, route.resolved_address())
                        .await;
                }
                Err(status) => {
                    if status.is_transport() {
                        self.inner
                            .discard_conn(slot, r#gen, route.resolved_address())
                            .await;
                    }
                    return Err(status);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{first_of_in, prefer_deadline_in, prefer_peer_rejection_after_send};
    use crate::rt::manual::{ManualGuard, ManualRuntime};
    use crate::rt::{Runtime, TokioRuntime};
    use crate::status::{Code, Status, TransportEvidence};
    use crate::transport::h2 as backend;
    use crate::transport::{
        ClientBuilder, Reason, RecvStream, SendRequest, SendResponse, SendStream, ServerBuilder,
        ServerConnection,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn refused_response_overrides_ambiguous_request_send_error() {
        let status = tokio::time::timeout(Duration::from_secs(3), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            let peer = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.expect("accept");
                let mut connection = backend::ServerBuilder::new()
                    .handshake(socket)
                    .await
                    .expect("handshake");
                let (_, mut respond) = connection
                    .accept()
                    .await
                    .expect("request")
                    .expect("headers");
                respond.send_reset(Reason::REFUSED_STREAM);
                while let Some(result) = connection.accept().await {
                    result.expect("drive reset");
                }
            });

            let socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
            let (sender, connection) = backend::ClientBuilder::new()
                .handshake(socket)
                .await
                .expect("handshake");
            let driver = tokio::spawn(async move { drop(connection.await) });
            let mut sender = sender.ready().await.expect("ready");
            let (response, _send) = sender
                .send_request(
                    http::Request::builder()
                        .uri("http://localhost/first")
                        .body(())
                        .expect("request"),
                    false,
                )
                .expect("send headers");
            let result =
                prefer_peer_rejection_after_send::<()>(response, Status::stream_closed()).await;
            driver.abort();
            peer.abort();
            result.expect_err("REFUSED_STREAM must not be reported as an ambiguous send loss")
        })
        .await
        .expect("HTTP/2 refusal stalled");
        assert_eq!(
            status.transport_evidence(),
            Some(TransportEvidence::RefusedStream)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn manual_runtime_drives_deadlines_without_wall_waits() {
        let now = TokioRuntime::now();
        let _guard = ManualGuard::install(now);
        // An already-expired deadline resolves on first poll, off the manual
        // clock rather than a wall wait.
        let cancel_rx = crate::rpc_cancel::RpcCancel::new();
        let expired =
            first_of_in::<ManualRuntime, ()>(std::future::pending(), cancel_rx, Some(now)).await;
        assert_eq!(expired.unwrap_err().code(), Code::DeadlineExceeded);
        // The rewrite reads the manual clock too.
        let rewritten =
            prefer_deadline_in::<ManualRuntime, ()>(Err(Status::unavailable("raced")), Some(now));
        assert_eq!(rewritten.unwrap_err().code(), Code::DeadlineExceeded);
        // A future deadline parks until the manual clock advances.
        let cancel_rx = crate::rpc_cancel::RpcCancel::new();
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();
        ManualRuntime::spawn(async move {
            let outcome = first_of_in::<ManualRuntime, ()>(
                std::future::pending(),
                cancel_rx,
                Some(now + Duration::from_secs(10)),
            )
            .await;
            done_tx.send(outcome).ok();
        });
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        done_rx
            .try_recv()
            .expect_err("deadline race must not finish before the clock moves");
        ManualRuntime::advance(Duration::from_secs(10));
        let outcome = done_rx.await.expect("deadline race must finish");
        assert_eq!(outcome.unwrap_err().code(), Code::DeadlineExceeded);
    }

    async fn response_after_closed_upload(
        http_status: http::StatusCode,
        grpc_status: Option<&str>,
    ) -> (http::Response<backend::RecvStream>, Status) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (headers_tx, headers_rx) = tokio::sync::oneshot::channel();
        let grpc_status = grpc_status.map(str::to_owned);
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            let mut connection = backend::ServerBuilder::new()
                .handshake(socket)
                .await
                .expect("handshake");
            let (request, mut respond) = connection
                .accept()
                .await
                .expect("request")
                .expect("headers");
            let mut response = http::Response::builder().status(http_status);
            if let Some(code) = grpc_status {
                response = response
                    .header("grpc-status", code)
                    .header("grpc-message", "busy");
            }
            let send = respond
                .send_response(response.body(()).expect("response"), true)
                .expect("send headers");
            tokio::select! {
                result = headers_rx => result.expect("client received headers"),
                request = connection.accept() => panic!("unexpected request while flushing response: {request:?}"),
            }
            respond.send_reset(Reason::NO_ERROR);
            drop(send);
            drop(request);
            drop(respond);
            while let Some(result) = connection.accept().await {
                result.expect("drive reset");
            }
        });
        let socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (sender, connection) = backend::ClientBuilder::new()
            .handshake(socket)
            .await
            .expect("handshake");
        let driver = tokio::spawn(async move { drop(connection.await) });
        let mut sender = sender.ready().await.expect("ready");
        let (response, mut send) = sender
            .send_request(
                http::Request::builder()
                    .uri("http://localhost/rejected")
                    .body(())
                    .expect("request"),
                false,
            )
            .expect("request headers");
        let response = response.await.expect("response headers");
        headers_tx.send(()).expect("release peer");
        std::future::poll_fn(|cx| send.poll_reset(cx))
            .await
            .expect("reset");
        let error = send
            .send_data(bytes::Bytes::from_static(b"unfinished"), true)
            .expect_err("closed send half");
        let status = Status::from_h2_send(error);
        assert_eq!(status.code(), Code::Internal);
        assert!(!status.is_transport());
        driver.abort();
        peer.abort();
        (response, status)
    }

    #[tokio::test]
    async fn cached_trailers_only_classification_survives_local_reset() {
        tokio::time::timeout(Duration::from_secs(3), async {
            let (client, server) = tokio::io::duplex(64 * 1024);
            let peer = tokio::spawn(async move {
                let mut connection = backend::ServerBuilder::new()
                    .handshake(server)
                    .await
                    .expect("server handshake");
                let (request, mut respond) = connection
                    .accept()
                    .await
                    .expect("request")
                    .expect("headers");
                let send = respond
                    .send_response(
                        http::Response::builder()
                            .header("grpc-status", "1")
                            .header("grpc-message", "peer%20cancelled")
                            .header("x-terminal", "kept")
                            .body(())
                            .expect("response"),
                        true,
                    )
                    .expect("response headers");
                let held = (request, respond, send);
                while let Some(result) = connection.accept().await {
                    result.expect("drive peer connection");
                }
                drop(held);
            });
            let (sender, connection) = backend::ClientBuilder::new()
                .handshake(client)
                .await
                .expect("client handshake");
            let driver = tokio::spawn(async move { drop(connection.await) });
            let mut sender = sender.ready().await.expect("ready");
            let (mut response, mut send) = sender
                .send_request(
                    http::Request::builder()
                        .uri("http://localhost/rejected")
                        .body(())
                        .expect("request"),
                    false,
                )
                .expect("request headers");
            let head = (&mut response).await.expect("response headers");
            let mut saved = super::UploadHead::default();
            saved.capture(Ok(head));
            assert_eq!(saved.committed, Some(false));

            send.send_reset(Reason::CANCEL);
            saved.reset = true;
            assert!(
                !saved
                    .head
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .body()
                    .is_end_stream(),
                "local reset must erase the live EOS state for this regression"
            );
            // The response future has already returned Ready. These probes
            // must not poll it again, including after a reset.
            super::capture_ready_response(&mut response, &mut saved);
            super::capture_ready_response(&mut response, &mut saved);

            let cached = saved.committed;
            let mut post_reset = super::UploadHead {
                reset: true,
                ..Default::default()
            };
            post_reset.capture(saved.head.take().expect("saved response"));
            assert_eq!(post_reset.committed, Some(true));
            let mut committed = true;
            let status = super::prefer_peer_rejection_with_commitment::<()>(
                std::future::ready(post_reset.head.take().expect("response")),
                Status::stream_closed(),
                &mut committed,
                cached,
            )
            .await
            .expect_err("peer rejection");
            assert_eq!(status.code(), Code::Cancelled);
            assert_eq!(status.message(), "peer cancelled");
            assert_eq!(status.metadata().get("x-terminal"), Some("kept"));
            assert!(!committed, "pre-reset trailers-only decision must survive");

            // A completed error is also a consumed response future.
            let mut sender = sender.ready().await.expect("ready for another stream");
            let (mut response, mut send) = sender
                .send_request(
                    http::Request::builder()
                        .uri("http://localhost/cancelled")
                        .body(())
                        .expect("request"),
                    false,
                )
                .expect("request headers");
            send.send_reset(Reason::CANCEL);
            let error = (&mut response).await.expect_err("local reset");
            let mut terminal = super::UploadHead::default();
            terminal.capture(Err(error));
            assert_eq!(terminal.committed, None);
            super::capture_ready_response(&mut response, &mut terminal);
            super::capture_ready_response(&mut response, &mut terminal);
            assert!(matches!(terminal.head, Some(Err(_))));

            driver.abort();
            peer.abort();
        })
        .await
        .expect("trailers-only reset test stalled");
    }

    #[tokio::test]
    async fn queued_peer_rejection_overrides_inactive_upload_error() {
        tokio::time::timeout(Duration::from_secs(3), async {
            let (response, send_error) =
                response_after_closed_upload(http::StatusCode::OK, Some("8")).await;
            let mut committed = false;
            let status = super::prefer_peer_rejection_with_commitment::<()>(
                std::future::ready(Ok(response)),
                send_error,
                &mut committed,
                None,
            )
            .await
            .expect_err("peer rejection");
            assert_eq!(status.code(), Code::ResourceExhausted);
            assert_eq!(status.message(), "busy");
            assert!(
                !committed,
                "trailers-only rejection remains policy retry eligible"
            );
            assert!(!status.is_transport());
        })
        .await
        .expect("closed upload stalled");
    }

    #[tokio::test]
    async fn closed_upload_never_becomes_success_or_waits_for_local_send_error() {
        tokio::time::timeout(Duration::from_secs(3), async {
            for (http_status, grpc_status) in [
                (http::StatusCode::OK, Some("0")),
                (http::StatusCode::OK, None),
                (http::StatusCode::OK, Some("bad")),
                (http::StatusCode::SERVICE_UNAVAILABLE, Some("8")),
            ] {
                let (response, send_error) =
                    response_after_closed_upload(http_status, grpc_status).await;
                let message = send_error.message().to_owned();
                let mut committed = false;
                let status = super::prefer_peer_rejection_with_commitment::<()>(
                    std::future::ready(Ok(response)),
                    send_error,
                    &mut committed,
                    None,
                )
                .await
                .expect_err("incomplete upload must fail");
                assert_eq!(status.code(), Code::Internal);
                assert_eq!(status.message(), message);
                assert!(std::error::Error::source(&status).is_some());
            }
            let (_, send_error) =
                response_after_closed_upload(http::StatusCode::OK, Some("8")).await;
            let status = super::prefer_peer_rejection_with_commitment::<()>(
                std::future::pending(),
                send_error,
                &mut false,
                None,
            )
            .await
            .expect_err("no queued response");
            assert_eq!(status.code(), Code::Internal);
            let status = super::prefer_peer_rejection_with_commitment::<()>(
                std::future::pending(),
                Status::internal("local validation"),
                &mut false,
                None,
            )
            .await
            .expect_err("local error must not await response");
            assert_eq!(status.message(), "local validation");
        })
        .await
        .expect("local send error waited for headers");
    }
}
