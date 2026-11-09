//! Real channel regression for a held retry-throttling mutex.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "internal contention test"
)]

use crate::hello::{HelloReply, HelloRequest};
use crate::{Channel, Code, Request};
use bytes::Bytes;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::{Notify, mpsc};

const PATH: &str = "/helloworld.Greeter/SayHello";

thread_local! {
    static ENQUEUED: RefCell<Option<mpsc::UnboundedSender<u32>>> = const { RefCell::new(None) };
}

/// Record a hedged outcome delivered to the coordinator.
pub(super) fn record_enqueue(attempt: u32) {
    ENQUEUED.with(|slot| {
        if let Some(sender) = slot.borrow().as_ref() {
            let _delivered = sender.send(attempt).is_ok();
        }
    });
}

struct Watch;

impl Drop for Watch {
    fn drop(&mut self) {
        ENQUEUED.with(|slot| *slot.borrow_mut() = None);
    }
}

fn watch_enqueue() -> (mpsc::UnboundedReceiver<u32>, Watch) {
    let (sender, receiver) = mpsc::unbounded_channel();
    ENQUEUED.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(sender);
    });
    (receiver, Watch)
}

async fn wait_count(counter: &AtomicUsize, changed: &Notify, expected: usize) {
    tokio::time::timeout(Duration::from_secs(6), async {
        while counter.load(Ordering::SeqCst) < expected {
            changed.notified().await;
        }
    })
    .await
    .expect("expected progress");
}

async fn next_enqueued(receiver: &mut mpsc::UnboundedReceiver<u32>) -> u32 {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("outcome enqueue timed out")
        .expect("outcome watch closed")
}

#[tokio::test(flavor = "current_thread")]
async fn hedge_refusal_is_processed_while_the_real_throttle_lock_is_held() {
    // The hook runs after tx.send, or immediately before a JoinSet task
    // returns. On this current-thread runtime the task completes before the
    // awakened test can resume and release the held lock.
    let (mut enqueued, _watch) = watch_enqueue();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let calls = Arc::new(AtomicUsize::new(0));
    let call_changed = Arc::new(Notify::new());
    let first_response = Arc::new(Notify::new());
    let second_response = Arc::new(Notify::new());

    let peer_calls = calls.clone();
    let peer_changed = call_changed.clone();
    let peer_first = first_response.clone();
    let peer_second = second_response.clone();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        let mut conn = h2::server::handshake(socket).await.expect("handshake");
        while let Some(stream) = conn.accept().await {
            let (request, mut respond) = stream.expect("request");
            let index = peer_calls.fetch_add(1, Ordering::SeqCst) + 1;
            let changed = peer_changed.clone();
            let first = peer_first.clone();
            let second = peer_second.clone();
            drop(tokio::spawn(async move {
                let mut body = request.into_body();
                while let Some(data) = body.data().await {
                    let data: Bytes = data.expect("request DATA");
                    body.flow_control()
                        .release_capacity(data.len())
                        .expect("flow credit");
                }
                changed.notify_one();
                match index {
                    1 => first.notified().await,
                    2 => second.notified().await,
                    _ => {}
                }
                let mut response = http::Response::builder()
                    .header("content-type", "application/grpc")
                    .header("grpc-status", if index > 2 { "13" } else { "14" });
                if index == 1 {
                    response = response.header("grpc-retry-pushback-ms", "-1");
                }
                respond
                    .send_response(response.body(()).expect("response"), true)
                    .expect("response headers");
            }));
        }
    });

    let channel = Channel::connect(address)
        .await
        .expect("connect")
        .service_config(
            r#"{"methodConfig":[{"name":[{}],"hedgingPolicy":{
                "maxAttempts":3,"hedgingDelay":"0.5s",
                "nonFatalStatusCodes":["UNAVAILABLE"]}}],
                "retryThrottling":{"maxTokens":10,"tokenRatio":1}}"#,
        )
        .expect("policy");
    let mut request = Request::new(HelloRequest::new());
    request.set_timeout(Duration::from_secs(15));
    let call = channel.unary::<_, HelloReply>(PATH, request);
    let task = tokio::spawn(call);

    wait_count(&calls, &call_changed, 2).await;
    let state = channel.local.service_config.get().expect("service config");
    let throttler = state.throttler.as_ref().expect("throttler");
    let held = throttler.hold_tokens_for_test().await;

    // The second attempt's nonfatal result requests a third hedge. The
    // first attempt then refuses further hedges while the token lock is held.
    // Wait for both outcomes to enter the coordinator channel, in that order.
    second_response.notify_one();
    assert_eq!(next_enqueued(&mut enqueued).await, 2);
    first_response.notify_one();
    assert_eq!(next_enqueued(&mut enqueued).await, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    drop(held);

    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("call finished")
        .expect("join");
    assert_eq!(
        channel.retry_stats().hedged_sends,
        1,
        "queued refusal must prevent a third hedge"
    );
    assert_eq!(result.expect_err("call failed").code(), Code::Unavailable);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_stops_unset_fanout_while_the_real_throttle_lock_is_held() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        let mut conn = h2::server::handshake(socket).await.expect("handshake");
        let mut seen = Some(seen_tx);
        while let Some(stream) = conn.accept().await {
            let (request, respond) = stream.expect("request");
            if let Some(seen) = seen.take() {
                let _delivered = seen.send(()).is_ok();
            }
            drop(tokio::spawn(async move {
                let _held = (request, respond);
                std::future::pending::<()>().await;
            }));
        }
    });

    let channel = Channel::connect(address)
        .await
        .expect("connect")
        .service_config(
            r#"{"methodConfig":[{"name":[{}],"hedgingPolicy":{
                "maxAttempts":3,"nonFatalStatusCodes":["UNAVAILABLE"]}}],
                "retryThrottling":{"maxTokens":10,"tokenRatio":1}}"#,
        )
        .expect("policy");
    let state = channel.local.service_config.get().expect("service config");
    let throttler = state.throttler.as_ref().expect("throttler");
    let held = throttler.hold_tokens_for_test().await;
    let mut request = Request::new(HelloRequest::new());
    request.set_timeout(Duration::from_secs(15));
    let call = channel.unary::<_, HelloReply>(PATH, request);
    let handle = call.handle();
    let task = tokio::spawn(call);
    tokio::time::timeout(Duration::from_secs(5), seen_rx)
        .await
        .expect("first request timed out")
        .expect("first request not observed");
    handle.cancel();
    drop(held);

    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("call finished")
        .expect("join");
    assert_eq!(
        channel.retry_stats().hedged_sends,
        0,
        "cancellation must stop the initial unset-delay fanout"
    );
    assert_eq!(result.expect_err("call cancelled").code(), Code::Cancelled);
    server.abort();
}
