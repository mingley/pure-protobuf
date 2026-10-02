//! Burst boundaries and terminal states on the production server stream path.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "transport integration assertions"
)]

mod common;

use common::{ServerGuard, req};
use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Request, Response, Rpc, Server, Service, Status, Streaming};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, oneshot};

#[derive(Clone)]
struct Burst {
    replies: usize,
    capacity: usize,
    fail: bool,
    finish_before_headers: bool,
}

fn reply(index: usize) -> HelloReply {
    let mut reply = HelloReply::new();
    reply.set_message(index.to_string());
    reply
}

impl Service for Burst {
    const NAME: &'static str = "sv09.Burst";

    async fn call(&self, rpc: Rpc) {
        let config = self.clone();
        let finish_before_headers = config.finish_before_headers;
        rpc.server_streaming(move |_: Request<HelloRequest>| async move {
            let (tx, stream) = Streaming::channel(config.capacity);
            let produce = async move {
                for index in 0..config.replies {
                    if tx.send(reply(index)).await.is_err() {
                        return;
                    }
                }
                if config.fail {
                    let mut status = Status::aborted("producer finished with an error");
                    status
                        .metadata_mut()
                        .insert("x-terminal", "retained")
                        .unwrap();
                    tx.fail(status).await;
                }
            };
            if finish_before_headers {
                produce.await;
            } else {
                drop(tokio::spawn(produce));
            }
            Ok::<_, Status>(Response::new(stream))
        })
        .await;
    }
}

async fn connect<S: Service>(service: S) -> (Channel, ServerGuard) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let guard = ServerGuard(tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.unwrap();
    }));
    (Channel::connect(addr).await.unwrap(), guard)
}

async fn check_burst(config: Burst) {
    let expected = config.replies;
    let fail = config.fail;
    let (channel, _server) = connect(config).await;
    let mut stream = channel
        .server_streaming::<HelloRequest, HelloReply>("/sv09.Burst/Read", Request::new(req("")))
        .await
        .unwrap()
        .into_inner();
    for index in 0..expected {
        let value = stream.message().await.unwrap().expect("ordered message");
        assert_eq!(value.message().to_str().unwrap(), index.to_string());
    }
    if fail {
        let status = stream.message().await.unwrap_err();
        assert_eq!(status.code(), Code::Aborted);
        assert_eq!(status.message(), "producer finished with an error");
        assert_eq!(status.metadata().get("x-terminal"), Some("retained"));
    } else {
        assert!(stream.message().await.unwrap().is_none());
        stream.trailers().await.unwrap();
    }
    assert!(stream.message().await.unwrap().is_none());
}

#[tokio::test]
async fn closed_bursts_keep_order_across_the_wire_batch_boundary() {
    for replies in [0, 1, 4, 63, 64, 65, 129, 512] {
        check_burst(Burst {
            replies,
            capacity: replies.max(1),
            fail: false,
            finish_before_headers: true,
        })
        .await;
    }
}

#[tokio::test]
async fn closed_bursts_flush_successes_before_the_terminal_error() {
    for replies in [0, 1, 4, 63, 64, 65, 129] {
        check_burst(Burst {
            replies,
            capacity: replies + 1,
            fail: true,
            finish_before_headers: true,
        })
        .await;
    }
}

#[tokio::test]
async fn live_producers_keep_refilling_small_bounded_channels() {
    for capacity in [1, 4, 8, 64] {
        check_burst(Burst {
            replies: 512,
            capacity,
            fail: false,
            finish_before_headers: false,
        })
        .await;
    }
}

#[expect(
    clippy::disallowed_types,
    reason = "a single synchronous take of the producer completion sender; the lock guard ends before the RPC await"
)]
type CancellationSender = std::sync::Mutex<Option<oneshot::Sender<()>>>;

struct Waiting {
    sent: Arc<Notify>,
    cancelled: CancellationSender,
}

impl Service for Waiting {
    const NAME: &'static str = "sv09.Waiting";

    async fn call(&self, rpc: Rpc) {
        let sent = self.sent.clone();
        let cancelled = self.cancelled.lock().unwrap().take().unwrap();
        rpc.server_streaming(move |_: Request<HelloRequest>| async move {
            let (tx, stream) = Streaming::channel(8);
            drop(tokio::spawn(async move {
                tx.send(reply(0)).await.unwrap();
                sent.notify_one();
                tx.closed().await;
                cancelled.send(()).unwrap();
            }));
            Ok::<_, Status>(Response::new(stream))
        })
        .await;
    }
}

#[tokio::test]
async fn cancellation_wakes_a_producer_waiting_after_its_first_reply() {
    let sent = Arc::new(Notify::new());
    let (cancelled, observed) = oneshot::channel();
    let (channel, _server) = connect(Waiting {
        sent: sent.clone(),
        cancelled: CancellationSender::from(Some(cancelled)),
    })
    .await;
    let mut stream = channel
        .server_streaming::<HelloRequest, HelloReply>("/sv09.Waiting/Read", Request::new(req("")))
        .await
        .unwrap()
        .into_inner();
    sent.notified().await;
    assert_eq!(
        stream
            .message()
            .await
            .unwrap()
            .unwrap()
            .message()
            .to_str()
            .unwrap(),
        "0"
    );
    drop(stream);
    tokio::time::timeout(Duration::from_secs(2), observed)
        .await
        .expect("producer woke on cancellation")
        .unwrap();
}

struct WireEcho;

impl Service for WireEcho {
    const NAME: &'static str = "sv09.WireEcho";

    async fn call(&self, rpc: Rpc) {
        rpc.bidi_streaming(|request: Request<Streaming<HelloRequest>>| async move {
            // Returning the inbound stream directly keeps the server response
            // backed by the wire, rather than an application channel.
            Ok::<_, Status>(Response::new(request.into_inner()))
        })
        .await;
    }
}

#[tokio::test]
async fn wire_backed_bidi_echo_keeps_progressing_and_cancels_its_sender() {
    let (channel, _server) = connect(WireEcho).await;
    let (tx, call) =
        channel.bidi::<HelloRequest, HelloRequest>("/sv09.WireEcho/Echo", Request::new(()));
    let mut inbound = call.await.unwrap().into_inner();
    for index in 0..129 {
        tx.send(req(&index.to_string())).await.unwrap();
        let message = tokio::time::timeout(Duration::from_secs(2), inbound.message())
            .await
            .expect("wire echo made progress")
            .unwrap()
            .unwrap();
        assert_eq!(message.name().to_str().unwrap(), index.to_string());
    }
    drop(tx);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), inbound.message())
            .await
            .expect("half-closed wire echo completed")
            .unwrap()
            .is_none()
    );

    let (tx, call) =
        channel.bidi::<HelloRequest, HelloRequest>("/sv09.WireEcho/Echo", Request::new(()));
    let mut inbound = call.await.unwrap().into_inner();
    tx.send(req("cancel")).await.unwrap();
    assert_eq!(
        inbound
            .message()
            .await
            .unwrap()
            .unwrap()
            .name()
            .to_str()
            .unwrap(),
        "cancel"
    );
    drop(inbound);
    tokio::time::timeout(Duration::from_secs(2), tx.closed())
        .await
        .expect("wire response cancellation closed the request sender");
}
