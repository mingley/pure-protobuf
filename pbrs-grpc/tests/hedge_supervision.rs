//! Supervision and cancellation of hedged unary attempt tasks.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "integration tests"
)]

mod common;

use bytes::Bytes;
use pbrs::Serialize;
use pbrs_grpc::hello::HelloReply;
use pbrs_grpc::{AttemptLabels, Channel, ChannelConfig, LifecycleObserver, Request, Status};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;

const PATH: &str = "/helloworld.Greeter/SayHello";
const BUDGET: Duration = Duration::from_secs(10);
const PANIC_MESSAGE: &str = "hedged decoder panic";
const OBSERVER_PANIC: &str = "hedged observer panic";
static DECODE_ENTERED: OnceLock<Notify> = OnceLock::new();

#[derive(Debug)]
struct PanicDecode;

impl pbrs_grpc::codec::CodecMessage for PanicDecode {
    fn encoded_len(&self) -> usize {
        0
    }

    fn encode_payload<W: pbrs::WireOut>(&self, _out: &mut W) -> Result<(), Status> {
        Ok(())
    }

    fn decode_payload(_payload: Bytes) -> Result<Self, Status> {
        DECODE_ENTERED.get_or_init(Notify::new).notify_one();
        panic!("{PANIC_MESSAGE}");
    }

    fn empty() -> Self {
        Self
    }
}

struct PanicObserver {
    entered: Arc<Notify>,
}

impl LifecycleObserver for PanicObserver {
    fn on_attempt_start(&self, _attempt: &AttemptLabels<'_>) {
        self.entered.notify_one();
        panic!("{OBSERVER_PANIC}");
    }
}

async fn propagated_panic<T: std::fmt::Debug + Send + 'static>(
    task: &mut JoinHandle<Result<T, Status>>,
    expected: &'static str,
) {
    let outcome = match tokio::time::timeout(BUDGET, &mut *task).await {
        Ok(outcome) => outcome,
        Err(_) => {
            task.abort();
            let _ = (&mut *task).await;
            panic!("hedged child panic was not propagated");
        }
    };
    let error = outcome.expect_err("call must panic");
    assert!(
        error.is_panic(),
        "call returned an unexpected join error: {error}"
    );
    let payload = error.into_panic();
    let actual = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&'static str>().copied());
    assert_eq!(actual, Some(expected));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observer_panic_before_headers_propagates() {
    for hedged in [false, true] {
        let (addr, _server) = common::spawn_greeter_server(Default::default()).await;
        let entered = Arc::new(Notify::new());
        let mut channel = Channel::connect(addr)
            .await
            .expect("connect")
            .observer(PanicObserver {
                entered: Arc::clone(&entered),
            });
        if hedged {
            channel = channel
                .service_config(
                    r#"{"methodConfig":[{"name":[{}],"hedgingPolicy":{"maxAttempts":2,"hedgingDelay":"30s"}}]}"#,
                )
                .expect("hedging policy");
        }
        let call = channel.unary::<_, HelloReply>(PATH, Request::new(common::req("panic")));
        let mut task = tokio::spawn(call);
        tokio::time::timeout(BUDGET, entered.notified())
            .await
            .expect("observer entered");
        propagated_panic(&mut task, OBSERVER_PANIC).await;
    }
}

/// A peer accepts two streams and keeps both request bodies behind a small
/// receive window. The parent may request a response on the first stream,
/// or leave both pending for a dropped-call test.
struct Peer {
    channel: Channel,
    accepted: oneshot::Receiver<()>,
    command: oneshot::Sender<bool>,
    resets: oneshot::Receiver<Vec<h2::Reason>>,
    server: common::ServerGuard,
}

async fn peer() -> Peer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let (accepted_tx, accepted) = oneshot::channel();
    let (command, command_rx) = oneshot::channel();
    let (resets_tx, resets) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("connection");
        let mut conn = h2::server::Builder::new()
            .initial_window_size(1_024)
            .handshake::<_, Bytes>(socket)
            .await
            .expect("server handshake");
        let (request_a, mut response_a) = conn
            .accept()
            .await
            .expect("first stream")
            .expect("first headers");
        let (request_b, mut response_b) = conn
            .accept()
            .await
            .expect("second stream")
            .expect("second headers");
        let mut body_a = request_a.into_body();
        let _body_b = request_b.into_body();
        let _driver = common::ServerGuard(tokio::spawn(async move {
            while let Some(next) = conn.accept().await {
                if next.is_err() {
                    break;
                }
            }
        }));
        accepted_tx.send(()).expect("report both streams");
        let respond = command_rx.await.expect("test command");
        let reasons = if respond {
            while let Some(chunk) = body_a.data().await {
                let chunk = chunk.expect("first request data");
                body_a
                    .flow_control()
                    .release_capacity(chunk.len())
                    .expect("return first stream capacity");
            }
            let headers = http::Response::builder()
                .status(http::StatusCode::OK)
                .header(http::header::CONTENT_TYPE, "application/grpc")
                .body(())
                .expect("response headers");
            let mut send = response_a.send_response(headers, false).expect("headers");
            let payload = common::reply("reply").serialize().expect("serialize reply");
            let frame = pbrs_grpc::codec::encode(&payload, false).expect("reply frame");
            send.send_data(frame, false).expect("response data");
            let mut trailers = http::HeaderMap::new();
            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
            send.send_trailers(trailers).expect("response trailers");
            let reason = std::future::poll_fn(|cx| response_b.poll_reset(cx))
                .await
                .expect("sibling reset");
            vec![reason]
        } else {
            let first = std::future::poll_fn(|cx| response_a.poll_reset(cx));
            let second = std::future::poll_fn(|cx| response_b.poll_reset(cx));
            let (first, second) = tokio::join!(first, second);
            vec![first.expect("first reset"), second.expect("second reset")]
        };
        resets_tx.send(reasons).expect("report resets");
    });
    let channel = Channel::connect_with(
        addr,
        ChannelConfig::new()
            .connections(1)
            .max_send_buffer_size(1_024),
    )
    .await
    .expect("connect")
    .byte_budget(2 * 1024 * 1024)
    .service_config(
        r#"{"methodConfig":[{"name":[{}],"hedgingPolicy":{"maxAttempts":2,"hedgingDelay":"0s"}}]}"#,
    )
    .expect("hedging policy");
    Peer {
        channel,
        accepted,
        command,
        resets,
        server: common::ServerGuard(server),
    }
}

async fn budget_released(channel: &Channel) {
    tokio::time::timeout(BUDGET, async {
        while channel.byte_budget_allocated() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("byte permits released");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn decoder_panic_aborts_sibling_and_releases_budget() {
    let Peer {
        channel,
        accepted,
        command,
        resets,
        server,
    } = peer().await;
    // Own the server task explicitly while moving the receiver fields.
    let _server = server;
    let request = common::req(&"x".repeat(256 * 1024));
    let call = channel.unary::<_, PanicDecode>(PATH, Request::new(request));
    let mut task = tokio::spawn(call);
    tokio::time::timeout(BUDGET, accepted)
        .await
        .expect("both streams arrived")
        .expect("accept witness");
    assert!(
        channel.byte_budget_allocated() > 0,
        "uploads hold byte permits"
    );
    command.send(true).expect("send response");
    tokio::time::timeout(BUDGET, DECODE_ENTERED.get_or_init(Notify::new).notified())
        .await
        .expect("decoder entered");
    propagated_panic(&mut task, PANIC_MESSAGE).await;
    let reasons = tokio::time::timeout(BUDGET, resets)
        .await
        .expect("sibling reset arrived")
        .expect("reset witness");
    assert_eq!(reasons, vec![h2::Reason::CANCEL]);
    budget_released(&channel).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_call_aborts_both_hedged_uploads() {
    let Peer {
        channel,
        accepted,
        command,
        resets,
        server,
    } = peer().await;
    let _server = server;
    let request = common::req(&"x".repeat(256 * 1024));
    let call = channel.unary::<_, HelloReply>(PATH, Request::new(request));
    let task = tokio::spawn(call);
    tokio::time::timeout(BUDGET, accepted)
        .await
        .expect("both streams arrived")
        .expect("accept witness");
    assert!(
        channel.byte_budget_allocated() > 0,
        "uploads hold byte permits"
    );
    command.send(false).expect("hold both streams");
    task.abort();
    let error = task.await.expect_err("call task was aborted");
    assert!(error.is_cancelled());
    let reasons = tokio::time::timeout(BUDGET, resets)
        .await
        .expect("both resets arrived")
        .expect("reset witness");
    assert_eq!(reasons, vec![h2::Reason::CANCEL, h2::Reason::CANCEL]);
    budget_released(&channel).await;
}
