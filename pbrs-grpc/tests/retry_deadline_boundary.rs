//! Response headers prevent retry at an attempt deadline for both call shapes.
//! A no-headers control verifies policy retry on the same connection.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unwrap_used,
    reason = "integration test"
)]

mod common;

use common::req;
use http::header::CONTENT_TYPE;
use pbrs_grpc::hello::GreeterClient;
use pbrs_grpc::{Channel, Code, Request};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
use tokio::sync::oneshot;

/// Confirms that a complete response HEADERS frame was written to the duplex.
struct HeaderWriteWitness {
    io: DuplexStream,
    pending: Vec<u8>,
    written: Option<oneshot::Sender<()>>,
}

impl HeaderWriteWitness {
    fn record(&mut self, bytes: &[u8]) {
        if self.written.is_none() {
            return;
        }
        self.pending.extend_from_slice(bytes);
        while self.pending.len() >= 9 {
            let len = (usize::from(self.pending[0]) << 16)
                | (usize::from(self.pending[1]) << 8)
                | usize::from(self.pending[2]);
            if self.pending.len() < 9 + len {
                break;
            }
            let stream = u32::from_be_bytes(self.pending[5..9].try_into().unwrap()) & 0x7fff_ffff;
            if self.pending[3] == 1 && self.pending[4] & 4 != 0 && stream == 1 {
                self.written.take().unwrap().send(()).unwrap();
                self.pending.clear();
                return;
            }
            self.pending.drain(..9 + len);
        }
        assert!(self.pending.len() < 64 * 1024, "unexpected peer frame size");
    }
}

impl AsyncRead for HeaderWriteWitness {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl AsyncWrite for HeaderWriteWitness {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.io).poll_write(cx, bytes);
        if let Poll::Ready(Ok(count)) = result {
            self.record(&bytes[..count]);
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

#[tokio::test(start_paused = true)]
async fn queued_headers_at_attempt_deadline_do_not_replay() {
    for server_stream in [false, true] {
        for send_headers in [false, true] {
            exercise_deadline_boundary(server_stream, send_headers).await;
        }
    }
}

async fn exercise_deadline_boundary(server_stream: bool, send_headers: bool) {
    let (client_io, peer_io) = tokio::io::duplex(64 * 1024);
    let (written_tx, written_rx) = oneshot::channel();
    let peer_io = HeaderWriteWitness {
        io: peer_io,
        pending: Vec::new(),
        written: Some(written_tx),
    };
    let (body_tx, mut body_rx) = oneshot::channel();
    let (go_tx, go_rx) = oneshot::channel();
    let (confirmed_tx, mut confirmed_rx) = oneshot::channel();
    let executions = Arc::new(AtomicUsize::new(0));
    let seen = executions.clone();

    let peer = tokio::spawn(async move {
        let mut conn = h2::server::handshake(peer_io)
            .await
            .expect("HTTP/2 handshake");
        let mut ping = conn.ping_pong().expect("PING handle");
        let (request, mut respond) = conn
            .accept()
            .await
            .expect("first request")
            .expect("valid first request");
        seen.fetch_add(1, Ordering::SeqCst);
        let mut body = request.into_body();
        let mut request_bytes = 0;
        loop {
            tokio::select! {
                biased;
                chunk = body.data() => match chunk {
                    Some(Ok(chunk)) => request_bytes += chunk.len(),
                    Some(Err(error)) => panic!("request DATA failed: {error}"),
                    None => break,
                },
                incoming = conn.accept() => panic!("unexpected early request: {incoming:?}"),
            }
        }
        assert!(request_bytes >= 5, "request must contain a framed message");
        body_tx.send(()).unwrap();
        go_rx.await.expect("release first response");
        let held_send = if send_headers {
            let response = http::Response::builder()
                .status(200)
                .header(CONTENT_TYPE, "application/grpc")
                .body(())
                .unwrap();
            let send = respond
                .send_response(response, false)
                .expect("response headers");
            tokio::select! {
                result = written_rx => result.expect("HEADERS write witness"),
                incoming = conn.accept() => {
                    panic!("unexpected request before HEADERS write: {incoming:?}");
                },
            }
            // HEADERS were written before this PING. Its ACK proves that the
            // client connection processed them before the call is resumed.
            tokio::select! {
                result = ping.ping(h2::Ping::opaque()) => {
                    result.expect("PING acknowledgement");
                },
                incoming = conn.accept() => {
                    panic!("unexpected request before PING acknowledgement: {incoming:?}");
                },
            }
            Some(send)
        } else {
            // Keep the first response unsent as a retry control.
            None
        };
        confirmed_tx.send(()).unwrap();
        while let Some(Ok((request, mut respond))) = conn.accept().await {
            seen.fetch_add(1, Ordering::SeqCst);
            let mut body = request.into_body();
            let mut request_bytes = 0;
            loop {
                tokio::select! {
                    biased;
                    chunk = body.data() => match chunk {
                        Some(Ok(chunk)) => request_bytes += chunk.len(),
                        Some(Err(error)) => panic!("retry DATA failed: {error}"),
                        None => break,
                    },
                    incoming = conn.accept() => panic!("unexpected extra request: {incoming:?}"),
                }
            }
            assert!(request_bytes >= 5, "retry must contain a framed message");
            let response = http::Response::builder()
                .status(200)
                .header(CONTENT_TYPE, "application/grpc")
                .header("grpc-status", "14")
                .body(())
                .unwrap();
            respond
                .send_response(response, true)
                .expect("retry response");
        }
        drop((held_send, respond));
    });

    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("channel")
        .service_config(
            r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
                "maxAttempts":2,"initialBackoff":"0s","maxBackoff":"0s",
                "backoffMultiplier":1,"perAttemptRecvTimeout":"5s",
                "retryableStatusCodes":["UNAVAILABLE"]
            }}]}"#,
        )
        .expect("retry policy");
    let client = GreeterClient::new(channel.clone());
    let mut request = Request::new(req("deadline-boundary"));
    request.set_timeout(Duration::from_secs(30));
    let mut call = Box::pin(async {
        if server_stream {
            client.server_hello(request).await.map(|_| ())
        } else {
            client.say_hello(request).await.map(|_| ())
        }
    });

    // Poll explicitly while the upload completes. After the peer confirms the
    // body, stop polling this call so the timer and any HEADERS can both become ready.
    let started = Instant::now();
    loop {
        let mut cx = Context::from_waker(Waker::noop());
        assert!(call.as_mut().poll(&mut cx).is_pending());
        match body_rx.try_recv() {
            Ok(()) => break,
            Err(oneshot::error::TryRecvError::Empty) => {}
            Err(error) => panic!("peer exited before receiving body: {error}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "body wait stalled"
        );
        tokio::task::yield_now().await;
    }
    let mut cx = Context::from_waker(Waker::noop());
    assert!(call.as_mut().poll(&mut cx).is_pending());
    go_tx.send(()).unwrap();

    let started = Instant::now();
    loop {
        match confirmed_rx.try_recv() {
            Ok(()) => break,
            Err(oneshot::error::TryRecvError::Empty) => {}
            Err(error) => panic!("peer exited before confirming HEADERS: {error}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "HEADERS wait stalled"
        );
        tokio::task::yield_now().await;
    }

    tokio::time::advance(Duration::from_secs(6)).await;
    let status = tokio::time::timeout(Duration::from_secs(30), call)
        .await
        .expect("call must finish")
        .expect_err("attempt must fail");
    let expected_status = if send_headers {
        Code::DeadlineExceeded
    } else {
        Code::Unavailable
    };
    let expected_attempts = if send_headers { 1 } else { 2 };
    let expected_retries = if send_headers { 0 } else { 1 };
    assert_eq!(
        status.code(),
        expected_status,
        "server_stream={server_stream}, send_headers={send_headers}"
    );
    assert_eq!(
        executions.load(Ordering::SeqCst),
        expected_attempts,
        "server_stream={server_stream}, send_headers={send_headers}"
    );
    assert_eq!(channel.retry_stats().policy_retries, expected_retries);
    assert_eq!(channel.retry_stats().transparent_retries, 0);
    peer.abort();
}
