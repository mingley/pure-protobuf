//! Large compressed sends reuse bounded storage and release it with the RPC.
//! An independent HTTP/2 peer validates every decompressed message.
#![cfg(all(feature = "prost", feature = "copy-counts"))]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "bounded independent HTTP/2 regression fixture and assertions"
)]

use bytes::BytesMut;
use pbrs_grpc::codec::prost::Message;
use pbrs_grpc::{Channel, Code, CompressionAlgorithm, HelloReply, MessageLimits, Request};
use prost::Message as _;
use std::future::Future;
use std::time::Duration;
use tokio::sync::mpsc;

const SIZE: usize = 1024 * 1024;

#[derive(Clone, PartialEq, prost::Message)]
struct Owned {
    #[prost(bytes = "vec", tag = "1")]
    value: Vec<u8>,
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("bounded wire fixture completed")
}

fn counts() -> pbrs_grpc::copy_counts::RetainedEncodeCounts {
    let counts = pbrs_grpc::copy_counts::retained_encode_counts();
    assert!(counts.retained_capacity <= 16 * SIZE);
    assert!(counts.peak_retained_capacity <= 16 * SIZE);
    counts
}

async fn quiescent() {
    bounded(async {
        while counts().retained_capacity != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(counts().retained_buffers, 0);
    assert_eq!(counts().acquired, counts().released);
}

struct Peer {
    channel: Channel,
    seen: mpsc::UnboundedReceiver<usize>,
    task: tokio::task::JoinHandle<()>,
}

impl Peer {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (seen, received) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut connection = h2::server::handshake(socket).await.unwrap();
            let mut handlers = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    finished = handlers.join_next(), if !handlers.is_empty() => {
                        finished.unwrap().unwrap();
                    }
                    incoming = connection.accept() => {
                        let Some(incoming) = incoming else { break };
                        let (request, mut response) = incoming.unwrap();
                        assert_eq!(request.headers().get("grpc-encoding").unwrap(), "gzip");
                        let seen = seen.clone();
                        handlers.spawn(async move {
                            let mut body = request.into_body();
                            let mut buffer = BytesMut::new();
                            while let Some(data) = body.data().await {
                                let Ok(data) = data else { return }; // explicit client cancellation
                                body.flow_control().release_capacity(data.len()).unwrap();
                                buffer.extend_from_slice(&data);
                                while let Some(frame) = pbrs_grpc::codec::pop(&mut buffer).unwrap() {
                                    assert!(frame.compressed);
                                    let raw = CompressionAlgorithm::Gzip.decode(&frame.payload).unwrap();
                                    let message = Owned::decode(raw.as_slice()).unwrap();
                                    assert!(message.value.iter().all(|byte| *byte == 0xAB));
                                    seen.send(message.value.len()).unwrap();
                                }
                            }
                            assert!(buffer.is_empty());
                            let headers = http::Response::builder()
                                .header("content-type", "application/grpc")
                                .body(())
                                .unwrap();
                            let Ok(mut send) = response.send_response(headers, false) else { return };
                            send.send_data(pbrs_grpc::codec::encode(&[], false).unwrap(), false).unwrap();
                            let mut trailers = http::HeaderMap::new();
                            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                            send.send_trailers(trailers).unwrap();
                        });
                    }
                }
            }
        });
        let channel = bounded(Channel::connect(address))
            .await
            .unwrap()
            .send_compressed()
            .message_limits(MessageLimits::unlimited());
        Self {
            channel,
            seen: received,
            task,
        }
    }

    async fn received(&mut self, expected: usize) {
        assert_eq!(bounded(self.seen.recv()).await.unwrap(), expected);
        assert!(!self.task.is_finished(), "peer assertions remain live");
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn message(size: usize) -> Message<Owned> {
    Message(Owned {
        value: vec![0xAB; size],
    })
}

// One test isolates process-wide retention gauges while exercising independent
// streams, cancellation, deadlines, oversized buffers and successful recovery.
#[tokio::test(flavor = "current_thread")]
async fn encoded_storage_is_bounded_reusable_and_owned_by_the_call() {
    quiescent().await;
    let mut peer = Peer::new().await;
    let open = |request| {
        peer.channel
            .client_streaming::<Message<Owned>, HelloReply>("/fixture.Encode/Upload", request)
    };
    let (sender, call) = open(Request::new(()));
    let call = tokio::spawn(call);
    for size in [SIZE, SIZE, 17, 0, SIZE] {
        bounded(sender.send_compressed(message(size)))
            .await
            .unwrap();
        peer.received(size).await;
        assert_eq!(counts().retained_buffers, 1);
    }
    sender.close();
    bounded(call).await.unwrap().unwrap();
    quiescent().await;

    // A buffer larger than the cache policy is still a valid RPC payload.
    let (sender, call) = peer
        .channel
        .client_streaming::<Message<Owned>, HelloReply>("/fixture.Encode/Upload", Request::new(()));
    let call = tokio::spawn(call);
    bounded(sender.send_compressed(message(5 * SIZE)))
        .await
        .unwrap();
    peer.received(5 * SIZE).await;
    assert_eq!(counts().retained_buffers, 0);
    sender.close();
    bounded(call).await.unwrap().unwrap();
    quiescent().await;

    // More live streams than cache slots must make progress without exceeding
    // the aggregate budget. Rejection affects retention, never the wire result.
    let mut uploads = Vec::new();
    for _ in 0..20 {
        let (sender, call) = peer.channel.client_streaming::<Message<Owned>, HelloReply>(
            "/fixture.Encode/Upload",
            Request::new(()),
        );
        let call = tokio::spawn(call);
        bounded(sender.send_compressed(message(SIZE)))
            .await
            .unwrap();
        peer.received(SIZE).await;
        uploads.push((sender, call));
    }
    assert!(counts().retained_buffers > 0);
    assert!(counts().rejected_budget > 0);
    for (sender, call) in uploads {
        sender.close();
        bounded(call).await.unwrap().unwrap();
    }
    quiescent().await;

    for deadline in [false, true] {
        let mut request = Request::new(());
        if deadline {
            request.set_timeout(Duration::from_millis(250));
        }
        let (sender, call) = peer
            .channel
            .client_streaming::<Message<Owned>, HelloReply>("/fixture.Encode/Upload", request);
        let handle = call.handle();
        let call = tokio::spawn(call);
        bounded(sender.send_compressed(message(SIZE)))
            .await
            .unwrap();
        peer.received(SIZE).await;
        assert_eq!(counts().retained_buffers, 1);
        if !deadline {
            handle.cancel();
        }
        let status = bounded(call).await.unwrap().expect_err("terminal call");
        assert_eq!(
            status.code(),
            if deadline {
                Code::DeadlineExceeded
            } else {
                Code::Cancelled
            }
        );
        drop(sender);
        quiescent().await;
    }
}
