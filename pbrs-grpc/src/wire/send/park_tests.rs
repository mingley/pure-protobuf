//! Cancellation dispatch, runtime ownership, and ready-set regressions.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "focused cancellation assertions"
)]

use super::{
    CachedParker, CancelJob, PARKER_CACHE_SIZE, PARKERS, park_cancel, poll_parked, run_parker,
};
use crate::rpc_cancel::RpcCancel;
use crate::transport::{ClientBuilder, Reason, SendRequest, SendStream};
use atomic_waker::AtomicWaker;
use futures_util::stream::FuturesUnordered;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

#[derive(Default)]
struct ProbeState {
    polls: AtomicUsize,
    ready: AtomicBool,
    waker: AtomicWaker,
}

struct Probe(Arc<ProbeState>);

impl Future for Probe {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.polls.fetch_add(1, Ordering::Relaxed);
        self.0.waker.register(cx.waker());
        if self.0.ready.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[test]
fn one_wake_polls_only_one_of_4096_parked_jobs() {
    let states: Vec<_> = (0..4096).map(|_| Arc::new(ProbeState::default())).collect();
    let mut parked = FuturesUnordered::new();
    for state in &states {
        parked.push(Probe(Arc::clone(state)));
    }
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(poll_parked(&mut parked, &mut cx).is_pending());
    assert!(
        states
            .iter()
            .all(|state| state.polls.load(Ordering::Relaxed) == 1)
    );
    let chosen = states.get(2048).expect("chosen job");
    chosen.ready.store(true, Ordering::Release);
    chosen.waker.wake();
    assert!(poll_parked(&mut parked, &mut cx).is_ready());
    assert_eq!(
        states
            .iter()
            .map(|state| state.polls.load(Ordering::Relaxed))
            .sum::<usize>(),
        4097,
        "a ready job must not cause a scan of unrelated parked jobs"
    );
    assert_eq!(parked.len(), 4095);
}

async fn local_stream() -> (
    crate::transport::h2::SendStream,
    crate::transport::h2::ResponseFuture,
    crate::transport::h2::ClientConnection<tokio::io::DuplexStream>,
    tokio::io::DuplexStream,
) {
    // No peer or driver is polled. A reset is observed directly by the real
    // HTTP/2 response future, so the tests do not infer reset from a timer.
    let (io, peer) = tokio::io::duplex(65536);
    let (mut requests, driver) = crate::transport::h2::ClientBuilder::new()
        .handshake(io)
        .await
        .expect("client handshake");
    let request = http::Request::builder()
        .uri("http://localhost/cancel/probe")
        .body(())
        .expect("request");
    let (response, mut send) = requests.send_request(request, false).expect("stream");
    send.send_data(bytes::Bytes::new(), true)
        .expect("half-close");
    (send, response, driver, peer)
}

async fn expect_local_cancel(response: crate::transport::h2::ResponseFuture) {
    let result = tokio::time::timeout(Duration::from_secs(1), response)
        .await
        .expect("reset must reach the HTTP/2 response future");
    let error = match result {
        Ok(_) => panic!("reset stream returned response headers"),
        Err(error) => error,
    };
    assert_eq!(error.reason(), Some(Reason::CANCEL));
}

async fn assert_no_local_reset(mut response: crate::transport::h2::ResponseFuture) {
    poll_fn(|cx| {
        assert!(
            Pin::new(&mut response).poll(cx).is_pending(),
            "clean completion must not reset"
        );
        Poll::Ready(())
    })
    .await;
}

#[tokio::test]
async fn closing_cancel_senders_releases_without_reset() {
    let (send, response, _driver, _peer) = local_stream().await;
    let cancel = RpcCancel::new();
    let job = CancelJob::new(send, cancel, None, None, false);
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    assert!(sender.try_send(job).is_ok());
    drop(sender);
    tokio::time::timeout(Duration::from_secs(1), run_parker(receiver))
        .await
        .expect("closed registration and flag must drain");
    assert_no_local_reset(response).await;
}

#[tokio::test]
async fn clean_finished_wins_over_ready_cancel_and_deadline() {
    let (send, response, _driver, _peer) = local_stream().await;
    let cancel = RpcCancel::new();
    cancel.signal();
    let (finished, receiver) = tokio::sync::oneshot::channel();
    finished.send(()).expect("finished");
    let job = CancelJob::new(
        send,
        cancel,
        Some(tokio::time::Instant::now()),
        Some(receiver),
        true,
    );
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    assert!(sender.try_send(job).is_ok());
    drop(sender);
    tokio::time::timeout(Duration::from_secs(1), run_parker(receiver))
        .await
        .expect("clean completion drains");
    assert_no_local_reset(response).await;
}

#[tokio::test(start_paused = true)]
async fn parked_deadline_resets_without_reading() {
    let (send, response, _driver, _peer) = local_stream().await;
    let cancel = RpcCancel::new();
    let at = tokio::time::Instant::now() + Duration::from_secs(1);
    let job = CancelJob::new(send, cancel.clone(), Some(at), None, false);
    park_cancel(job);
    tokio::time::advance(Duration::from_secs(1)).await;
    expect_local_cancel(response).await;
    assert!(
        !cancel.is_cancelled(),
        "deadline is distinct from explicit cancellation"
    );
}

#[tokio::test]
async fn a_full_registration_queue_spawns_one_owned_fallback() {
    let (send, response, _driver, _peer) = local_stream().await;
    let cancel = RpcCancel::new();
    let (queued_send, _response, _queued_driver, _queued_peer) = local_stream().await;
    let queued_cancel = RpcCancel::new();
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);
    assert!(
        sender
            .try_send(CancelJob::new(
                queued_send,
                queued_cancel,
                None,
                None,
                false
            ))
            .is_ok()
    );
    let runtime = tokio::runtime::Handle::current();
    PARKERS.with(|slot| {
        let mut cache = slot.borrow_mut();
        cache.clear();
        cache.push(CachedParker {
            runtime: runtime.id(),
            sender,
        });
    });
    let before = runtime.metrics().num_alive_tasks();
    park_cancel(CancelJob::new(send, cancel.clone(), None, None, false));
    assert_eq!(
        runtime.metrics().num_alive_tasks(),
        before + 1,
        "one overflow owns one fallback task"
    );
    cancel.signal();
    expect_local_cancel(response).await;
    PARKERS.with(|slot| slot.borrow_mut().clear());
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

type WireProbe = (
    crate::Streaming<crate::hello::HelloReply>,
    crate::CallHandle,
    tokio::sync::oneshot::Receiver<crate::h2_backend::Reason>,
    tokio::task::JoinHandle<()>,
);

async fn wire_probe() -> WireProbe {
    let (client_io, server_io) = tokio::io::duplex(65536);
    let (reset_tx, reset_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let mut connection = crate::h2_backend::server::Builder::new()
            .handshake::<_, bytes::Bytes>(server_io)
            .await
            .expect("server handshake");
        let (_request, mut response) = connection
            .accept()
            .await
            .expect("request")
            .expect("headers");
        let mut send = response
            .send_response(
                http::Response::builder()
                    .header("content-type", "application/grpc")
                    .body(())
                    .expect("headers"),
                false,
            )
            .expect("response");
        let reset = poll_fn(|cx| send.poll_reset(cx));
        tokio::pin!(reset);
        loop {
            tokio::select! {
                biased;
                reason = &mut reset => {
                    reset_tx.send(reason.expect("peer reset")).ok();
                    break;
                }
                accepted = connection.accept() => {
                    assert!(accepted.is_none(), "only one probe stream");
                    break;
                }
            }
        }
    });
    let channel = crate::Channel::from_io(client_io, "localhost")
        .await
        .expect("channel");
    let client = crate::hello::GreeterClient::new(channel);
    let call = client.server_hello(crate::Request::new(crate::hello::HelloRequest::new()));
    let handle = call.handle();
    let stream = call.await.expect("response headers").into_inner();
    tokio::task::yield_now().await;
    (stream, handle, reset_rx, server)
}

#[test]
fn shutting_down_another_runtime_does_not_lose_wire_cancellation() {
    let first = runtime();
    let second = runtime();
    let _first_probe = first.block_on(wire_probe());
    let (stream, cancel, mut reset, server) = second.block_on(wire_probe());
    first.block_on(tokio::task::yield_now());
    drop(first);
    second.block_on(async {
        assert!(
            matches!(
                reset.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "other runtime shutdown must not cancel this stream"
        );
        cancel.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), reset)
                .await
                .expect("reset survives other runtime shutdown")
                .expect("reset reason"),
            crate::h2_backend::Reason::CANCEL
        );
        drop(stream);
        server.abort();
    });
}

#[test]
fn cache_eviction_drains_existing_wire_jobs() {
    let runtimes: Vec<_> = (0..=PARKER_CACHE_SIZE).map(|_| runtime()).collect();
    let mut probes: Vec<_> = runtimes
        .iter()
        .map(|runtime| runtime.block_on(wire_probe()))
        .collect();
    PARKERS.with(|slot| assert_eq!(slot.borrow().len(), PARKER_CACHE_SIZE));
    let (stream, cancel, reset, server) = probes.remove(0);
    let first = runtimes.first().expect("first runtime");
    first.block_on(async {
        cancel.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), reset)
                .await
                .expect("evicted dispatcher still drains")
                .expect("reset reason"),
            crate::h2_backend::Reason::CANCEL
        );
        drop(stream);
        server.abort();
    });
}
