//! Oversized codec lengths fail without panicking, leaking permits or losing prior replies.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "bounded integration fixture and regression assertions"
)]

use bytes::Bytes;
use pbrs::WireOut;
use pbrs_grpc::{
    ByteBudgetTracker, Channel, Code, CodecMessage, HelloReply, HelloRequest, MessageLimits,
    Request, Response, Router, Rpc, Service, Status, Streaming,
};
use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::net::TcpListener;

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("bounded RPC completed")
}

#[derive(Debug)]
struct Message {
    huge: bool,
    encoded_huge: Arc<AtomicBool>,
}

impl Message {
    fn small(encoded_huge: &Arc<AtomicBool>) -> Self {
        Self {
            huge: false,
            encoded_huge: Arc::clone(encoded_huge),
        }
    }
    fn huge(encoded_huge: &Arc<AtomicBool>) -> Self {
        Self {
            huge: true,
            encoded_huge: Arc::clone(encoded_huge),
        }
    }
}

impl CodecMessage for Message {
    fn encoded_len(&self) -> usize {
        if self.huge { usize::MAX } else { 1 }
    }
    fn encode_payload<W: WireOut>(&self, out: &mut W) -> Result<(), Status> {
        if self.huge {
            self.encoded_huge.store(true, Ordering::SeqCst);
            return Err(Status::internal("huge encoder must not run"));
        }
        out.put_u8(b'x');
        Ok(())
    }
    fn decode_payload(payload: Bytes) -> Result<Self, Status> {
        if payload.as_ref() != b"x" {
            return Err(Status::internal("unexpected payload"));
        }
        Ok(Self::small(&Arc::new(AtomicBool::new(false))))
    }
    fn empty() -> Self {
        Self::small(&Arc::new(AtomicBool::new(false)))
    }
}

struct Fixture {
    encoded_huge: Arc<AtomicBool>,
}

impl Service for Fixture {
    const NAME: &'static str = "fixture.Overflow";
    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Upload" => {
                rpc.client_streaming(|request: Request<Streaming<Message>>| async move {
                    let mut input = request.into_inner();
                    while input.message().await?.is_some() {}
                    Ok::<_, Status>(Response::new(HelloReply::new()))
                })
                .await
            }
            "Response" => {
                let encoded_huge = Arc::clone(&self.encoded_huge);
                rpc.server_streaming(move |_: Request<HelloRequest>| async move {
                    let (sender, stream) = Streaming::channel(2);
                    sender.send(Message::small(&encoded_huge)).await?;
                    sender.send(Message::huge(&encoded_huge)).await?;
                    sender.close();
                    Ok::<_, Status>(Response::new(stream))
                })
                .await;
            }
            "Recovery" => {
                rpc.unary(|_: Request<HelloRequest>| async move {
                    Ok::<_, Status>(Response::new(HelloReply::new()))
                })
                .await
            }
            _ => rpc.unimplemented(),
        }
    }
}

fn limits(capped: bool) -> MessageLimits {
    let limits = MessageLimits::unlimited();
    if capped {
        limits.with_max_encoding(8)
    } else {
        limits
    }
}

async fn quiescent(budget: &ByteBudgetTracker) {
    bounded(async {
        while budget.allocated() != 0 || budget.active_byte_permit_tokens() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

async fn fixture(
    capped: bool,
) -> (
    Channel,
    ByteBudgetTracker,
    ByteBudgetTracker,
    Arc<AtomicBool>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let encoded_huge = Arc::new(AtomicBool::new(false));
    let witness = Arc::clone(&encoded_huge);
    let server_budget = ByteBudgetTracker::with_limit(1024);
    let server_observer = server_budget.clone();
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(Fixture { encoded_huge })
            .message_limits(limits(capped))
            .with_byte_budget_tracker(server_budget)
            .serve_with_shutdown(listener, std::future::pending())
            .await
            .unwrap();
    });
    let client_budget = ByteBudgetTracker::with_limit(1024);
    let channel = bounded(Channel::connect(address))
        .await
        .unwrap()
        .message_limits(limits(capped))
        .with_byte_budget_tracker(client_budget.clone());
    (channel, client_budget, server_observer, witness, server)
}

#[tokio::test]
async fn overflowing_uploads_return_status_and_release_permits() {
    for capped in [false, true] {
        for closed in [false, true] {
            let (channel, budget, server_budget, encoded_huge, server) = fixture(capped).await;
            let (sender, call) = channel.client_streaming::<Message, HelloReply>(
                "/fixture.Overflow/Upload",
                Request::new(()),
            );
            sender.send(Message::small(&encoded_huge)).await.unwrap();
            let queued = sender.send(Message::huge(&encoded_huge)).await;
            if capped {
                assert_eq!(
                    queued.expect_err("producer rejects configured cap").code(),
                    Code::ResourceExhausted
                );
            } else {
                queued.expect("unlimited producer queues the message");
            }
            let held_sender = if closed {
                sender.close();
                None
            } else {
                Some(sender)
            };
            let status = bounded(call).await.expect_err("oversized upload rejected");
            assert_eq!(
                status.code(),
                if capped {
                    Code::ResourceExhausted
                } else {
                    Code::Internal
                }
            );
            assert!(!encoded_huge.load(Ordering::SeqCst));
            drop(held_sender);
            quiescent(&budget).await;
            quiescent(&server_budget).await;
            bounded(channel.unary::<HelloRequest, HelloReply>(
                "/fixture.Overflow/Recovery",
                Request::new(HelloRequest::new()),
            ))
            .await
            .expect("channel recovers after overflow");
            server.abort();
        }
    }
}

#[tokio::test]
async fn overflowing_server_reply_preserves_previous_frame_and_error_trailer() {
    for capped in [false, true] {
        let (channel, budget, server_budget, encoded_huge, server) = fixture(capped).await;
        let mut stream = bounded(channel.server_streaming::<HelloRequest, Message>(
            "/fixture.Overflow/Response",
            Request::new(HelloRequest::new()),
        ))
        .await
        .expect("response headers")
        .into_inner();
        assert!(
            !bounded(stream.message())
                .await
                .expect("first message")
                .expect("message present")
                .huge
        );
        let status = bounded(stream.message())
            .await
            .expect_err("overflow trailer");
        assert_eq!(
            status.code(),
            if capped {
                Code::ResourceExhausted
            } else {
                Code::Internal
            }
        );
        assert!(!encoded_huge.load(Ordering::SeqCst));
        quiescent(&budget).await;
        quiescent(&server_budget).await;
        server.abort();
    }
}

#[tokio::test]
async fn oversized_unary_is_rejected_before_reserving_or_calling_codec() {
    for capped in [false, true] {
        let (channel, budget, server_budget, encoded_huge, server) = fixture(capped).await;
        let status = bounded(channel.unary::<Message, HelloReply>(
            "/fixture.Overflow/Recovery",
            Request::new(Message::huge(&encoded_huge)),
        ))
        .await
        .expect_err("unrepresentable unary frame rejected");
        assert_eq!(
            status.code(),
            if capped {
                Code::ResourceExhausted
            } else {
                Code::Internal
            }
        );
        assert!(!encoded_huge.load(Ordering::SeqCst));
        quiescent(&budget).await;
        quiescent(&server_budget).await;
        server.abort();
    }
}
