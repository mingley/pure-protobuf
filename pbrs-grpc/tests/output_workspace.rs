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
use pbrs_grpc::{
    Channel, Code, CompressionAlgorithm, HelloReply, HelloRequest, MessageLimits, Request,
    Response, Rpc, Server, Service, Status, StreamSender, Streaming,
};
use prost::Message as _;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;

const SIZE: usize = 1024 * 1024;

#[derive(Clone, PartialEq, prost::Message)]
struct Owned {
    #[prost(bytes = "vec", tag = "1")]
    value: Vec<u8>,
    #[prost(uint32, tag = "2")]
    nonce: u32,
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
    seen: mpsc::UnboundedReceiver<(u32, usize)>,
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
                                    assert!(valid_noise(&message.value, message.nonce));
                                    seen.send((message.nonce, message.value.len())).unwrap();
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

    async fn received(&mut self, nonce: u32, expected: usize) {
        assert_eq!(bounded(self.seen.recv()).await.unwrap(), (nonce, expected));
        assert!(!self.task.is_finished(), "peer assertions remain live");
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn noise(length: usize, nonce: u32) -> Vec<u8> {
    let mut state = noise_seed(nonce);
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            u8::try_from(state & 0xff).unwrap()
        })
        .collect()
}

fn valid_noise(bytes: &[u8], nonce: u32) -> bool {
    let mut state = noise_seed(nonce);
    bytes.iter().all(|&byte| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        byte == u8::try_from(state & 0xff).unwrap()
    })
}

fn noise_seed(nonce: u32) -> u32 {
    0x67ac_4fe2u32.wrapping_add(nonce.wrapping_mul(0x9e37_79b9))
}

fn message(size: usize, nonce: u32) -> Message<Owned> {
    Message(Owned {
        value: noise(size, nonce),
        nonce,
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
    for (index, size) in [SIZE, SIZE, 17, 0, SIZE].into_iter().enumerate() {
        let nonce = u32::try_from(index).unwrap() + 1;
        bounded(sender.send_compressed(message(size, nonce)))
            .await
            .unwrap();
        peer.received(nonce, size).await;
        assert_eq!(counts().retained_buffers, 2);
    }
    sender.close();
    bounded(call).await.unwrap().unwrap();
    quiescent().await;

    // A buffer larger than the cache policy is still a valid RPC payload.
    let (sender, call) = peer
        .channel
        .client_streaming::<Message<Owned>, HelloReply>("/fixture.Encode/Upload", Request::new(()));
    let call = tokio::spawn(call);
    bounded(sender.send_compressed(message(5 * SIZE, 10)))
        .await
        .unwrap();
    peer.received(10, 5 * SIZE).await;
    assert_eq!(counts().retained_buffers, 0);
    sender.close();
    bounded(call).await.unwrap().unwrap();
    quiescent().await;

    // More live streams than cache slots must make progress without exceeding
    // the aggregate budget. Rejection affects retention, never the wire result.
    let mut uploads = Vec::new();
    for index in 0..20u32 {
        let (sender, call) = peer.channel.client_streaming::<Message<Owned>, HelloReply>(
            "/fixture.Encode/Upload",
            Request::new(()),
        );
        let call = tokio::spawn(call);
        bounded(sender.send_compressed(message(SIZE, 20 + index)))
            .await
            .unwrap();
        peer.received(20 + index, SIZE).await;
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
            request.set_timeout(Duration::from_secs(2));
        }
        let (sender, call) = peer
            .channel
            .client_streaming::<Message<Owned>, HelloReply>("/fixture.Encode/Upload", request);
        let handle = call.handle();
        let call = tokio::spawn(call);
        bounded(sender.send_compressed(message(SIZE, if deadline { 61 } else { 60 })))
            .await
            .unwrap();
        peer.received(if deadline { 61 } else { 60 }, SIZE).await;
        assert_eq!(counts().retained_buffers, 2);
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
    drop(peer);
    quiescent().await;
    server_output_lifecycle().await;
}

// This service exposes its producer to the fixture. The native server owns
// framing/drain state; a registry-h2 client independently checks every reply.
struct Download {
    opened: mpsc::UnboundedSender<DownloadControl>,
}

struct DownloadControl {
    sender: StreamSender<Message<Owned>>,
    ended: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl Service for Download {
    const NAME: &'static str = "fixture.Encode";

    async fn call(&self, rpc: Rpc) {
        let opened = self.opened.clone();
        rpc.server_streaming(move |request: Request<HelloRequest>| async move {
            let ended = Box::pin(request.cancelled());
            let (sender, stream) = Streaming::channel(1);
            opened.send(DownloadControl { sender, ended }).unwrap();
            Ok::<_, Status>(Response::new(stream))
        })
        .await;
    }
}

async fn download_message(
    body: &mut h2::RecvStream,
    buffer: &mut BytesMut,
    expected_nonce: u32,
    expected_length: usize,
) {
    loop {
        if let Some(frame) = pbrs_grpc::codec::pop(buffer).unwrap() {
            assert!(frame.compressed);
            let raw = CompressionAlgorithm::Gzip.decode(&frame.payload).unwrap();
            let message = Owned::decode(raw.as_slice()).unwrap();
            assert_eq!(
                (message.nonce, message.value.len()),
                (expected_nonce, expected_length)
            );
            assert!(valid_noise(&message.value, expected_nonce));
            return;
        }
        let data = bounded(body.data()).await.expect("reply frame").unwrap();
        body.flow_control().release_capacity(data.len()).unwrap();
        buffer.extend_from_slice(&data);
    }
}

async fn server_output_lifecycle() {
    quiescent().await;
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (opened, mut accepted) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        Server::new(Download { opened })
            .send_compressed()
            .message_limits(MessageLimits::unlimited())
            .serve_connection(server_io)
            .await
            .expect("native output server");
    });
    let (mut requests, connection) = bounded(h2::client::handshake(client_io)).await.unwrap();
    let driver = tokio::spawn(connection);
    for cancel in [false, true] {
        requests = bounded(requests.ready()).await.unwrap();
        let request = http::Request::builder()
            .method(http::Method::POST)
            .uri("http://localhost/fixture.Encode/Download")
            .header("content-type", "application/grpc")
            .header("te", "trailers")
            .header("grpc-accept-encoding", "gzip")
            .body(())
            .unwrap();
        let (response, mut send) = requests.send_request(request, false).unwrap();
        send.send_data(pbrs_grpc::codec::encode(&[], false).unwrap(), true)
            .unwrap();
        let control = bounded(accepted.recv()).await.unwrap();
        let response = bounded(response).await.unwrap();
        assert_eq!(response.headers().get("grpc-encoding").unwrap(), "gzip");
        let mut body = response.into_body();
        let mut buffer = BytesMut::new();
        let sizes = if cancel {
            vec![SIZE]
        } else {
            vec![SIZE, SIZE, 17, 0, SIZE]
        };
        for (index, size) in sizes.into_iter().enumerate() {
            let nonce = if cancel { 200 } else { 100 } + u32::try_from(index).unwrap();
            bounded(control.sender.send_compressed(message(size, nonce)))
                .await
                .unwrap();
            download_message(&mut body, &mut buffer, nonce, size).await;
            assert_eq!(counts().retained_buffers, 2);
            assert!(!server.is_finished(), "server assertions remain live");
        }
        assert!(buffer.is_empty());
        if cancel {
            send.send_reset(h2::Reason::CANCEL);
            drop(body);
            bounded(control.sender.closed()).await;
            bounded(control.ended).await;
            drop(control.sender);
        } else {
            control.sender.close();
            while let Some(data) = bounded(body.data()).await {
                let data = data.unwrap();
                body.flow_control().release_capacity(data.len()).unwrap();
                assert!(
                    data.is_empty(),
                    "no extra response payload after producer close"
                );
            }
            let trailers = bounded(body.trailers()).await.unwrap().unwrap();
            assert_eq!(trailers.get("grpc-status").unwrap(), "0");
            bounded(control.ended).await;
        }
        quiescent().await;
    }
    drop(requests);
    driver.abort();
    server.abort();
    // Abort releases the connection fixtures too; any unexpected panic still
    // fails this test instead of hiding inside a detached task.
    match bounded(server).await {
        Ok(()) => {}
        Err(error) => assert!(error.is_cancelled()),
    }
    match bounded(driver).await {
        Ok(result) => result.expect("independent HTTP/2 connection"),
        Err(error) => assert!(error.is_cancelled()),
    }
    quiescent().await;
}
