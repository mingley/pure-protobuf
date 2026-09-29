//! Per-call machinery: attempts, commitment, and send/open races.

use super::pool::LiveConn;
use crate::request::{Request, Response};
use crate::rt::{Runtime, TokioRuntime};
use crate::status::{Code, Status, TransportEvidence};
use crate::timeout::remaining_timeout;
use crate::transport::{
    Error as TransportError, Reason, RecvStream, SendRequest, SendStream, h2 as backend,
};
use crate::wire::{SegFrame, grpc_request, send_frame, status_from};
use http::HeaderValue;
use http::uri::Authority;
use std::sync::Arc;
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

pub(crate) async fn prefer_peer_rejection_after_send<T>(
    response: backend::ResponseFuture,
    send_error: Status,
) -> Result<T, Status> {
    if send_error.is_transport() {
        match response.await {
            Ok(response) => {
                let grpc_code = response
                    .headers()
                    .get("grpc-status")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<i32>().ok());
                // An early trailers-only error can close the request body with
                // RST_STREAM(NO_ERROR). Never turn an incomplete upload into OK.
                if response.status() == http::StatusCode::OK
                    && response.body().is_end_stream()
                    && grpc_code.is_some_and(|code| code != 0)
                {
                    return Err(status_from(response.headers(), None));
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

pub(crate) fn send_request_frame(
    send: &mut backend::SendStream,
    frame: SegFrame,
    send_buffer: usize,
    cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
) -> impl std::future::Future<Output = Result<(), Status>> {
    send_request_frame_in::<TokioRuntime>(send, frame, send_buffer, cancel_rx, deadline)
}

/// [`send_request_frame`] on runtime `R`; production callers use Tokio.
pub(crate) async fn send_request_frame_in<R: Runtime>(
    send: &mut backend::SendStream,
    frame: SegFrame,
    send_buffer: usize,
    cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
) -> Result<(), Status> {
    let result = prefer_deadline_in::<R, _>(
        first_of_in::<R, _>(
            send_frame(send, frame, true, send_buffer),
            cancel_rx,
            deadline,
        )
        .await,
        deadline,
    );
    if matches!(
        &result,
        Err(status) if matches!(status.code(), Code::Cancelled | Code::DeadlineExceeded)
    ) {
        send.send_reset(Reason::CANCEL);
    }
    result
}

#[allow(
    clippy::too_many_arguments,
    reason = "one HTTP/2 stream open plus headers, timeout, encoding, and scheme"
)]
pub(crate) fn open(
    send_req: backend::SendRequest,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    cancel_rx: watch::Receiver<bool>,
    send_codec: Option<crate::compression::Codec>,
    accept_gzip: bool,
    user_agent: &HeaderValue,
    https: bool,
) -> impl std::future::Future<Output = Result<(backend::ResponseFuture, backend::SendStream), Status>>
{
    open_in::<TokioRuntime>(
        send_req,
        authority,
        path,
        md,
        timeout,
        deadline,
        cancel_rx,
        send_codec,
        accept_gzip,
        user_agent,
        https,
    )
}

/// [`open`] on runtime `R`; production callers use Tokio.
#[allow(
    clippy::too_many_arguments,
    reason = "one HTTP/2 stream open plus headers, timeout, encoding, and scheme"
)]
pub(crate) async fn open_in<R: Runtime>(
    send_req: backend::SendRequest,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    cancel_rx: watch::Receiver<bool>,
    send_codec: Option<crate::compression::Codec>,
    accept_gzip: bool,
    user_agent: &HeaderValue,
    https: bool,
) -> Result<(backend::ResponseFuture, backend::SendStream), Status> {
    if timeout.is_some_and(|d| d.is_zero()) {
        return Err(Status::deadline_exceeded());
    }
    let mut send_req = prefer_deadline_in::<R, _>(
        first_of_in::<R, _>(
            async { send_req.ready().await.map_err(Status::from_h2_pre_headers) },
            cancel_rx,
            deadline,
        )
        .await,
        deadline,
    )?;
    let remaining = match (timeout, remaining_timeout(deadline)?) {
        (Some(initial), Some(rem)) => {
            if initial > rem && initial - rem < Duration::from_millis(20) {
                Some(initial)
            } else {
                Some(rem)
            }
        }
        (None, rem) => rem,
        (Some(initial), None) => Some(initial),
    };
    let http_req = grpc_request(
        authority,
        path,
        md,
        remaining,
        send_codec,
        accept_gzip,
        user_agent,
        https,
    )?;
    send_req
        .send_request(http_req, false)
        .map_err(Status::from_h2_pre_headers)
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

pub(crate) fn first_of<T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
) -> impl std::future::Future<Output = Result<T, Status>> {
    first_of_in::<TokioRuntime, T>(fut, cancel_rx, deadline)
}

/// [`first_of`] on runtime `R`; production callers use Tokio.
pub(crate) async fn first_of_in<R: Runtime, T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    mut cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
) -> Result<T, Status> {
    if let Some(at) = deadline {
        tokio::select! {
            biased;
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
            _ = R::sleep_until(at) => Err(Status::deadline_exceeded()),
            r = fut => r,
        }
    } else {
        tokio::select! {
            biased;
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
            r = fut => r,
        }
    }
}

pub(crate) fn race<T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
    send: Option<&mut backend::SendStream>,
) -> impl std::future::Future<Output = Result<T, Status>> {
    race_in::<TokioRuntime, T>(fut, cancel_rx, deadline, send)
}

/// [`race`] on runtime `R`; production callers use Tokio.
pub(crate) async fn race_in<R: Runtime, T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
    send: Option<&mut backend::SendStream>,
) -> Result<T, Status> {
    let result = first_of_in::<R, _>(fut, cancel_rx, deadline).await;
    if let Some(send) = send {
        if matches!(
            &result,
            Err(s) if s.code() == Code::Cancelled || s.code() == Code::DeadlineExceeded
        ) {
            send.send_reset(Reason::CANCEL);
        }
    }
    prefer_deadline_in::<R, _>(result, deadline)
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

#[cfg(test)]
mod tests {
    use super::{first_of_in, prefer_deadline_in, prefer_peer_rejection_after_send};
    use crate::rt::manual::{ManualGuard, ManualRuntime};
    use crate::rt::{Runtime, TokioRuntime};
    use crate::status::{Code, Status, TransportEvidence};
    use crate::transport::h2 as backend;
    use crate::transport::{
        ClientBuilder, Reason, SendRequest, SendResponse, ServerBuilder, ServerConnection,
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
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let expired =
            first_of_in::<ManualRuntime, ()>(std::future::pending(), cancel_rx, Some(now)).await;
        assert_eq!(expired.unwrap_err().code(), Code::DeadlineExceeded);
        // The rewrite reads the manual clock too.
        let rewritten =
            prefer_deadline_in::<ManualRuntime, ()>(Err(Status::unavailable("raced")), Some(now));
        assert_eq!(rewritten.unwrap_err().code(), Code::DeadlineExceeded);
        // A future deadline parks until the manual clock advances.
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
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
}
impl super::Channel {
    pub(crate) fn apply_response_hooks<T>(
        &self,
        path: &'static str,
        response: Response<T>,
    ) -> Result<Response<T>, Status> {
        crate::interceptor::intercept_response_all(
            response.with_static_path(path),
            &self.response_interceptors,
        )
    }

    pub(crate) fn prepare_outbound<T>(
        &self,
        path: &'static str,
        req: &mut Request<T>,
    ) -> Result<(), Status> {
        if let Some(method) = self.method_config_for(path) {
            if req.timeout().is_none() {
                if let Some(timeout) = method.timeout {
                    req.set_timeout(timeout);
                }
            }
            if !req.wait_for_ready_is_set() && method.wait_for_ready == Some(true) {
                req.set_wait_for_ready(true);
            }
        }
        if req.timeout().is_none() {
            if let Some(timeout) = self.config.rpc_timeout() {
                req.set_timeout(timeout);
            }
        }
        if !req.wait_for_ready_is_set() && self.config.waits_for_ready() {
            req.set_wait_for_ready(true);
        }
        if !req.compress_is_set() && self.config.compresses_outbound() {
            req.set_compress(true);
        }
        self.apply_interceptors(path, req)
    }

    fn apply_interceptors<T>(
        &self,
        path: &'static str,
        req: &mut Request<T>,
    ) -> Result<(), Status> {
        for hook in self.interceptors.iter() {
            hook.intercept(
                &mut req
                    .outgoing(
                        path,
                        self.authority(),
                        self.https,
                        self.grpc_user_agent(),
                        self.config,
                    )
                    .with_connected(self.connected()),
            )?;
        }
        Ok(())
    }

    pub(crate) fn take_rpc_slot(&self) -> Result<Option<OwnedSemaphorePermit>, Status> {
        match &self.rpc_slots {
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
        cancel_rx: watch::Receiver<bool>,
        deadline: Option<tokio::time::Instant>,
        wait_for_ready: bool,
        md: Option<&crate::metadata::Metadata>,
    ) -> impl std::future::Future<Output = Result<LiveConn, Status>> {
        self.grab_in::<TokioRuntime>(cancel_rx, deadline, wait_for_ready, md)
    }

    /// [`grab`](Self::grab) on runtime `R`; production callers use Tokio.
    pub(crate) async fn grab_in<R: Runtime>(
        &self,
        cancel_rx: watch::Receiver<bool>,
        deadline: Option<tokio::time::Instant>,
        wait_for_ready: bool,
        md: Option<&crate::metadata::Metadata>,
    ) -> Result<LiveConn, Status> {
        let _ = remaining_timeout(deadline)?;
        let inner = Arc::clone(&self.inner);
        let obs = self.observer.clone();
        let health = self.health_directive();
        let hash = md.and_then(|md| self.ring_request_hash(md));
        let grabbed = prefer_deadline_in::<R, _>(
            first_of_in::<R, _>(
                inner.acquire(wait_for_ready, obs.as_deref(), health, hash),
                cancel_rx,
                deadline,
            )
            .await,
            deadline,
        )?;
        let _ = remaining_timeout(deadline)?;
        Ok(grabbed)
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
        cancel_rx: watch::Receiver<bool>,
        timeout: Option<Duration>,
        deadline: Option<tokio::time::Instant>,
        wait: bool,
        path: &'static str,
        md: &crate::metadata::Metadata,
        compress: bool,
        user_agent: &http::HeaderValue,
    ) -> impl std::future::Future<Output = Result<Opened, Status>> {
        self.open_retrying_in::<TokioRuntime>(
            cancel_rx, timeout, deadline, wait, path, md, compress, user_agent,
        )
    }

    /// [`open_retrying`](Self::open_retrying) on runtime `R`; production
    /// callers use Tokio.
    #[allow(
        clippy::too_many_arguments,
        reason = "path, headers, timeout, encoding, and the grab race"
    )]
    pub(crate) async fn open_retrying_in<R: Runtime>(
        &self,
        cancel_rx: watch::Receiver<bool>,
        timeout: Option<Duration>,
        deadline: Option<tokio::time::Instant>,
        wait: bool,
        path: &'static str,
        md: &crate::metadata::Metadata,
        compress: bool,
        user_agent: &http::HeaderValue,
    ) -> Result<Opened, Status> {
        let mut retried = false;
        loop {
            let _ = remaining_timeout(deadline)?;
            let live = self
                .grab_in::<R>(cancel_rx.clone(), deadline, wait, Some(md))
                .await?;
            let (slot, r#gen, lease, driver, rr_addr, channelz_socket) = (
                live.slot,
                live.r#gen,
                live.lease,
                live.driver,
                live.rr_addr,
                live.channelz_socket,
            );
            let load = live.load;
            match open_in::<R>(
                live.send,
                &self.authority,
                path,
                md,
                timeout,
                deadline,
                cancel_rx.clone(),
                compress.then_some(self.config.send_codec()),
                self.config.accepts_compressed(),
                user_agent,
                self.https,
            )
            .await
            {
                Ok((resp_fut, send)) => {
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
                    self.inner.discard_conn(slot, r#gen, rr_addr.as_ref()).await;
                }
                Err(status) => {
                    if status.is_transport() {
                        self.inner.discard_conn(slot, r#gen, rr_addr.as_ref()).await;
                    }
                    return Err(status);
                }
            }
        }
    }
}
