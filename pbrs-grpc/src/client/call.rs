//! Per-call machinery: attempts, commitment, and send/open races.

use super::pool::LiveConn;
use crate::request::{Request, Response};
use crate::status::{Code, Status, TransportEvidence};
use crate::timeout::remaining_timeout;
use crate::wire::{grpc_request, send_bytes, status_from};
use bytes::Bytes;
use h2::Reason;
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

    pub(crate) fn classify_h2(self, err: h2::Error) -> Status {
        match self {
            Self::Uncommitted => Status::from_h2_pre_headers(err),
            Self::BodyStarted | Self::ResponseCommitted => Status::from_h2_post_dispatch(err),
        }
    }
}

pub(crate) async fn prefer_peer_rejection_after_send<T>(
    response: h2::client::ResponseFuture,
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

pub(crate) async fn send_request_frame(
    send: &mut h2::SendStream<Bytes>,
    frame: Bytes,
    send_buffer: usize,
    cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
) -> Result<(), Status> {
    let result = prefer_deadline(
        first_of(
            send_bytes(send, frame, true, send_buffer),
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
pub(crate) async fn open(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    cancel_rx: watch::Receiver<bool>,
    send_gzip: bool,
    accept_gzip: bool,
    user_agent: &HeaderValue,
    https: bool,
) -> Result<(h2::client::ResponseFuture, h2::SendStream<Bytes>), Status> {
    if timeout.is_some_and(|d| d.is_zero()) {
        return Err(Status::deadline_exceeded());
    }
    let mut send_req = prefer_deadline(
        first_of(
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
        send_gzip,
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
    let Some(at) = deadline else {
        return result;
    };
    match &result {
        Err(status)
            if matches!(status.code(), Code::Unavailable | Code::Cancelled)
                && tokio::time::Instant::now() >= at =>
        {
            Err(Status::deadline_exceeded())
        }
        _ => result,
    }
}

/// Race a setup or RPC future against its deadline and cancel signal.
pub(crate) async fn first_of<T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    mut cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
) -> Result<T, Status> {
    if let Some(at) = deadline {
        tokio::select! {
            biased;
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
            _ = tokio::time::sleep_until(at) => Err(Status::deadline_exceeded()),
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

pub(crate) async fn race<T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    cancel_rx: watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
    send: Option<&mut h2::SendStream<Bytes>>,
) -> Result<T, Status> {
    let result = first_of(fut, cancel_rx, deadline).await;
    if let Some(send) = send {
        if matches!(
            &result,
            Err(s) if s.code() == Code::Cancelled || s.code() == Code::DeadlineExceeded
        ) {
            send.send_reset(Reason::CANCEL);
        }
    }
    prefer_deadline(result, deadline)
}

/// HEADERS sent; request DATA has not started. Transparent retry stops here.
pub(crate) struct Opened {
    pub(crate) lease: Option<crate::keepalive::Lease>,
    pub(crate) driver: Option<watch::Sender<bool>>,
    pub(crate) resp_fut: h2::client::ResponseFuture,
    pub(crate) send: h2::SendStream<Bytes>,
}

#[cfg(test)]
mod tests {
    use super::prefer_peer_rejection_after_send;
    use crate::status::{Status, TransportEvidence};
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
                let mut connection = h2::server::handshake(socket).await.expect("handshake");
                let (_, mut respond) = connection
                    .accept()
                    .await
                    .expect("request")
                    .expect("headers");
                respond.send_reset(h2::Reason::REFUSED_STREAM);
                while let Some(result) = connection.accept().await {
                    result.expect("drive reset");
                }
            });

            let socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
            let (sender, connection) = h2::client::handshake(socket).await.expect("handshake");
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
}
impl super::Channel {
    pub(crate) fn apply_response_hooks<T>(
        &self,
        path: &'static str,
        response: Response<T>,
    ) -> Result<Response<T>, Status> {
        crate::interceptor::intercept_response_all(
            response.with_path(Some(path.to_owned())),
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
    pub(crate) async fn grab(
        &self,
        cancel_rx: watch::Receiver<bool>,
        deadline: Option<tokio::time::Instant>,
        wait_for_ready: bool,
    ) -> Result<LiveConn, Status> {
        let _ = remaining_timeout(deadline)?;
        let inner = Arc::clone(&self.inner);
        let obs = self.observer.clone();
        let grabbed = prefer_deadline(
            first_of(
                inner.acquire(wait_for_ready, obs.as_deref()),
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
    pub(crate) async fn open_retrying(
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
            let live = self.grab(cancel_rx.clone(), deadline, wait).await?;
            let (slot, r#gen, lease, driver) = (live.slot, live.r#gen, live.lease, live.driver);
            match open(
                live.send,
                &self.authority,
                path,
                md,
                timeout,
                deadline,
                cancel_rx.clone(),
                compress,
                self.config.accepts_compressed(),
                user_agent,
                self.https,
            )
            .await
            {
                Ok((resp_fut, send)) => {
                    return Ok(Opened {
                        lease,
                        driver,
                        resp_fut,
                        send,
                    });
                }
                Err(status)
                    if !retried
                        && status.is_transparent_retryable()
                        && self.inner.endpoint.can_redial() =>
                {
                    retried = true;
                    self.inner.discard(slot, r#gen).await;
                }
                Err(status) => {
                    if status.is_transport() {
                        self.inner.discard(slot, r#gen).await;
                    }
                    return Err(status);
                }
            }
        }
    }
}
