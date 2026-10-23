//! Compressed storage reuse must preserve message ownership and wire validation.
//! An independent HTTP/2 peer controls each message and terminal event over TCP.
#![cfg(feature = "prost")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "bounded independent wire fixtures and regression assertions"
)]

use bytes::Bytes;
use pbrs_grpc::codec::prost::Message;
use pbrs_grpc::{
    CallHandle, Channel, Code, CodecMessage, CompressionAlgorithm, HelloReply, HelloRequest,
    MessageLimits, Request, Status, Streaming,
};
use prost::Message as _;
use std::future::{Future, poll_fn};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

const SIZE: usize = 1024 * 1024;

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("independent wire exchange completed")
}

#[derive(Clone, PartialEq, prost::Message)]
struct Owned {
    #[prost(bytes = "vec", tag = "1")]
    value: Vec<u8>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct Shared {
    #[prost(bytes = "bytes", tag = "1")]
    value: Bytes,
}

// This codec deliberately implements only the original ownership-taking API.
// A new default reusable method must preserve its input and lifetime semantics.
#[derive(Debug, Default)]
struct Retaining(Bytes);

impl CodecMessage for Retaining {
    fn encoded_len(&self) -> usize {
        self.0.len()
    }

    fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
        out.write_contiguous(|buffer| {
            buffer.extend_from_slice(&self.0);
            Ok(())
        })
    }

    fn decode_payload(payload: Bytes) -> Result<Self, Status> {
        Ok(Self(payload))
    }

    fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug)]
enum Action {
    Data(Bytes),
    End(&'static str),
    Reset,
}

#[derive(Debug)]
struct Control {
    actions: mpsc::UnboundedSender<Action>,
    reset: oneshot::Receiver<h2::Reason>,
}

impl Control {
    fn data(&self, payload: &[u8]) {
        self.wire(gzip(payload));
    }

    fn wire(&self, compressed: Vec<u8>) {
        self.actions
            .send(Action::Data(
                pbrs_grpc::codec::encode(&compressed, true).unwrap(),
            ))
            .unwrap();
    }

    fn end(&self, status: &'static str) {
        self.actions.send(Action::End(status)).unwrap();
    }
}

struct Peer {
    channel: Channel,
    accepted: mpsc::UnboundedReceiver<Control>,
    task: tokio::task::JoinHandle<()>,
}

impl Peer {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted_tx, accepted) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut connection = h2::server::handshake(socket).await.unwrap();
            let mut handlers = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    completed = handlers.join_next(), if !handlers.is_empty() => {
                        completed.unwrap().unwrap();
                    }
                    incoming = connection.accept() => {
                        let Some(incoming) = incoming else { break };
                        let (request, mut respond) = incoming.unwrap();
                        let accepted_tx = accepted_tx.clone();
                        handlers.spawn(async move {
                            let mut body = request.into_body();
                            while let Some(data) = body.data().await {
                                let data = data.unwrap();
                                body.flow_control().release_capacity(data.len()).unwrap();
                            }
                            let response = http::Response::builder()
                                .header("content-type", "application/grpc")
                                .header("grpc-encoding", "gzip")
                                .body(())
                                .unwrap();
                            let mut send = respond.send_response(response, false).unwrap();
                            let (actions, mut commands) = mpsc::unbounded_channel();
                            let (reset_tx, reset) = oneshot::channel();
                            accepted_tx.send(Control { actions, reset }).unwrap();
                            loop {
                                tokio::select! {
                                    reason = poll_fn(|cx| send.poll_reset(cx)) => {
                                        let _reset_witness = reset_tx.send(reason.unwrap());
                                        return;
                                    }
                                    action = commands.recv() => match action {
                                        Some(Action::Data(data)) => send.send_data(data, false).unwrap(),
                                        Some(Action::End(status)) => {
                                            let mut trailers = http::HeaderMap::new();
                                            trailers.insert("grpc-status", status.parse().unwrap());
                                            send.send_trailers(trailers).unwrap();
                                            return;
                                        }
                                        Some(Action::Reset) => {
                                            send.send_reset(h2::Reason::CANCEL);
                                            return;
                                        }
                                        None => return,
                                    }
                                }
                            }
                        });
                    }
                }
            }
        });
        let channel = bounded(Channel::connect(address)).await.unwrap();
        Self {
            channel,
            accepted,
            task,
        }
    }

    async fn open<T: CodecMessage + Send + 'static>(
        &mut self,
        limits: MessageLimits,
    ) -> (Streaming<T>, CallHandle, Control) {
        self.open_request(limits, Request::new(HelloRequest::new()))
            .await
    }

    async fn open_request<T: CodecMessage + Send + 'static>(
        &mut self,
        limits: MessageLimits,
        request: Request<HelloRequest>,
    ) -> (Streaming<T>, CallHandle, Control) {
        let call = self
            .channel
            .clone()
            .message_limits(limits)
            .server_streaming::<HelloRequest, T>("/fixture.Storage/Read", request);
        let handle = call.handle();
        let stream = bounded(call).await.unwrap().into_inner();
        let control = bounded(self.accepted.recv()).await.unwrap();
        (stream, handle, control)
    }

    async fn recover(&mut self) {
        let (mut stream, _, control) = self.open::<Message<Owned>>(MessageLimits::new()).await;
        control.data(&encoded(b'r', 17));
        control.end("0");
        assert_eq!(
            bounded(stream.message()).await.unwrap().unwrap().0.value,
            vec![b'r'; 17]
        );
        assert!(bounded(stream.message()).await.unwrap().is_none());
        quiescent().await;
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn encoded(byte: u8, length: usize) -> Vec<u8> {
    Owned {
        value: vec![byte; length],
    }
    .encode_to_vec()
}

fn gzip(payload: &[u8]) -> Vec<u8> {
    CompressionAlgorithm::Gzip.encode(payload).unwrap()
}

#[cfg(feature = "copy-counts")]
fn cached(expected: bool) {
    let counts = pbrs_grpc::copy_counts::retained_decode_counts();
    if expected {
        assert_eq!(counts.retained_buffers, 1);
        assert!(counts.retained_capacity >= SIZE);
        assert!(counts.retained_capacity <= 4 * SIZE);
    } else {
        assert_eq!(counts.retained_buffers, 0);
        assert_eq!(counts.retained_capacity, 0);
    }
    assert!(counts.peak_retained_capacity <= 16 * SIZE);
}

#[cfg(not(feature = "copy-counts"))]
fn cached(_: bool) {}

async fn quiescent() {
    #[cfg(feature = "copy-counts")]
    bounded(async {
        loop {
            let counts = pbrs_grpc::copy_counts::retained_decode_counts();
            if counts.retained_capacity == 0 && counts.retained_buffers == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
}

// One test keeps process-wide cache gauges isolated without test-runner flags.
#[tokio::test(flavor = "current_thread")]
async fn decoded_storage_preserves_ownership_validation_and_cleanup() {
    quiescent().await;
    let mut peer = Peer::new().await;
    let first = encoded(b'a', SIZE);
    let second = encoded(b'b', SIZE);

    // Owned prost fields release the input while application values stay alive.
    let (mut stream, _, control) = peer.open::<Message<Owned>>(MessageLimits::new()).await;
    control.data(&first);
    let held = bounded(stream.message()).await.unwrap().unwrap();
    assert_eq!(held.0.value, vec![b'a'; SIZE]);
    cached(true);
    #[cfg(feature = "copy-counts")]
    let warm_capacity = pbrs_grpc::copy_counts::retained_decode_counts().retained_capacity;
    for payload in [&second, &first, &second] {
        control.data(payload);
        let actual = bounded(stream.message()).await.unwrap().unwrap();
        assert_eq!(actual.0.encode_to_vec(), *payload);
        assert_eq!(held.0.value, vec![b'a'; SIZE]);
        cached(true);
    }
    let mut noise = Vec::with_capacity(64 * 1024);
    let mut state = 0x67ac_4fe2u32;
    for _ in 0..64 * 1024 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        noise.push(u8::try_from(state & 0xff).unwrap());
    }
    for value in [noise, Vec::new(), vec![b's'; 17], vec![b'l'; SIZE]] {
        control.data(
            &Owned {
                value: value.clone(),
            }
            .encode_to_vec(),
        );
        assert_eq!(
            bounded(stream.message()).await.unwrap().unwrap().0.value,
            value
        );
        cached(true);
        #[cfg(feature = "copy-counts")]
        assert_eq!(
            pbrs_grpc::copy_counts::retained_decode_counts().retained_capacity,
            warm_capacity
        );
    }
    control.end("0");
    assert!(bounded(stream.message()).await.unwrap().is_none());
    quiescent().await;
    // Keep the terminated outer stream and the earlier message alive here.
    assert_eq!(held.0.value, vec![b'a'; SIZE]);

    let (mut shared, _, control) = peer.open::<Message<Shared>>(MessageLimits::new()).await;
    control.data(&first);
    let held_shared = bounded(shared.message()).await.unwrap().unwrap();
    let original_pointer = held_shared.0.value.as_ptr();
    cached(false);
    control.data(&second);
    let other = bounded(shared.message()).await.unwrap().unwrap();
    assert_eq!(other.0.value.as_ref(), vec![b'b'; SIZE]);
    assert_eq!(held_shared.0.value.as_ref(), vec![b'a'; SIZE]);
    assert_eq!(held_shared.0.value.as_ptr(), original_pointer);
    cached(false);
    control.end("0");
    assert!(bounded(shared.message()).await.unwrap().is_none());
    quiescent().await;

    let (mut lazy, _, control) = peer.open::<HelloReply>(MessageLimits::new()).await;
    control.data(&first);
    let held_lazy = bounded(lazy.message()).await.unwrap().unwrap();
    control.data(&second);
    let other = bounded(lazy.message()).await.unwrap().unwrap();
    assert_eq!(other.message().to_str().unwrap(), "b".repeat(SIZE));
    assert_eq!(held_lazy.message().to_str().unwrap(), "a".repeat(SIZE));
    cached(false);
    control.end("0");
    assert!(bounded(lazy.message()).await.unwrap().is_none());
    quiescent().await;
    assert_eq!(held_lazy.message().to_str().unwrap(), "a".repeat(SIZE));

    let (mut raw, _, control) = peer.open::<Retaining>(MessageLimits::new()).await;
    control.data(&first);
    let held_raw = bounded(raw.message()).await.unwrap().unwrap();
    let original_pointer = held_raw.0.as_ptr();
    control.data(&second);
    assert_eq!(bounded(raw.message()).await.unwrap().unwrap().0, second);
    assert_eq!(held_raw.0, first);
    assert_eq!(held_raw.0.as_ptr(), original_pointer);
    cached(false);
    control.end("0");
    assert!(bounded(raw.message()).await.unwrap().is_none());
    quiescent().await;

    // Oversized cache candidates are successful messages, not budget errors.
    let (mut uncached, _, control) = peer
        .open::<Message<Owned>>(MessageLimits::new().with_max_decoding(8 * SIZE))
        .await;
    control.data(&encoded(b'o', 4 * SIZE));
    assert_eq!(
        bounded(uncached.message()).await.unwrap().unwrap().0.value,
        vec![b'o'; 4 * SIZE]
    );
    cached(false);
    control.data(&first);
    assert_eq!(
        bounded(uncached.message()).await.unwrap().unwrap().0.value,
        vec![b'a'; SIZE]
    );
    cached(true);
    control.end("0");
    assert!(bounded(uncached.message()).await.unwrap().is_none());
    quiescent().await;

    // Each invalid message follows two valid owned messages, so rejection is
    // exercised after the stream has both populated and reused its workspace.
    let mut bad_crc = gzip(&first);
    let crc = bad_crc.len() - 8;
    *bad_crc.get_mut(crc).unwrap() ^= 1;
    let mut bad_tail = gzip(&first);
    bad_tail.extend_from_slice(b"invalid trailing input");
    let (first_half, second_half) = first.split_at(first.len() / 2);
    let mut bad_later_member = gzip(first_half);
    let mut later = gzip(second_half);
    let crc = later.len() - 8;
    *later.get_mut(crc).unwrap() ^= 1;
    bad_later_member.extend_from_slice(&later);
    let mut truncated_later_member = gzip(first_half);
    let mut later = gzip(second_half);
    later.truncate(later.len() - 1);
    truncated_later_member.extend_from_slice(&later);
    let mut too_many_members = gzip(&first);
    too_many_members.extend_from_slice(&gzip(&[]));
    let exact = first.len();
    let invalid = [
        (bad_crc, MessageLimits::new(), Code::Internal),
        (bad_tail, MessageLimits::new(), Code::Internal),
        (bad_later_member, MessageLimits::new(), Code::Internal),
        (truncated_later_member, MessageLimits::new(), Code::Internal),
        (
            gzip(&encoded(b'c', SIZE + 1)),
            MessageLimits::new().with_max_decoding(exact),
            Code::ResourceExhausted,
        ),
        (
            too_many_members,
            MessageLimits::new().with_max_compression_frames(1),
            Code::ResourceExhausted,
        ),
    ];
    for (wire, limits, expected) in invalid {
        let (mut failed, _, control) = peer.open::<Message<Owned>>(limits).await;
        for payload in [&first, &second] {
            control.data(payload);
            assert_eq!(
                bounded(failed.message())
                    .await
                    .unwrap()
                    .unwrap()
                    .0
                    .encode_to_vec(),
                *payload
            );
            cached(true);
        }
        control.wire(wire);
        assert_eq!(
            bounded(failed.message()).await.unwrap_err().code(),
            expected
        );
        quiescent().await;
        peer.recover().await;
        drop(failed);
    }

    for terminal in ["13", "peer-reset", "cancel", "drop"] {
        let (mut active, handle, control) = peer.open::<Message<Owned>>(MessageLimits::new()).await;
        control.data(&first);
        assert_eq!(
            bounded(active.message()).await.unwrap().unwrap().0.value,
            vec![b'a'; SIZE]
        );
        cached(true);
        match terminal {
            "13" => {
                control.end("13");
                assert_eq!(
                    bounded(active.message()).await.unwrap_err().code(),
                    Code::Internal
                );
                quiescent().await;
            }
            "peer-reset" => {
                control.actions.send(Action::Reset).unwrap();
                assert!(bounded(active.message()).await.is_err());
                quiescent().await;
            }
            "cancel" => {
                handle.cancel();
                assert_eq!(bounded(control.reset).await.unwrap(), h2::Reason::CANCEL);
                assert_eq!(
                    bounded(active.message()).await.unwrap_err().code(),
                    Code::Cancelled
                );
                // The outer Streaming value remains alive after observing cancellation.
                quiescent().await;
            }
            "drop" => {
                drop(active);
                assert_eq!(bounded(control.reset).await.unwrap(), h2::Reason::CANCEL);
                quiescent().await;
            }
            _ => panic!("unknown terminal fixture"),
        }
        peer.recover().await;
    }

    let mut request = Request::new(HelloRequest::new());
    request.set_timeout(Duration::from_secs(1));
    let (mut deadline, _, control) = peer
        .open_request::<Message<Owned>>(MessageLimits::new(), request)
        .await;
    control.data(&first);
    assert_eq!(
        bounded(deadline.message()).await.unwrap().unwrap().0.value,
        vec![b'a'; SIZE]
    );
    cached(true);
    assert_eq!(
        bounded(deadline.message()).await.unwrap_err().code(),
        Code::DeadlineExceeded
    );
    quiescent().await;
    assert_eq!(bounded(control.reset).await.unwrap(), h2::Reason::CANCEL);
    peer.recover().await;

    // Exercise aggregate admission on actual live HTTP/2 streams. Owned
    // application values remain alive, independently of cached decoder output.
    #[cfg(feature = "copy-counts")]
    let before = pbrs_grpc::copy_counts::retained_decode_counts();
    let mut live = Vec::new();
    let mut controls = Vec::new();
    let mut values = Vec::new();
    for _ in 0..17 {
        let (mut stream, _, control) = peer.open::<Message<Owned>>(MessageLimits::new()).await;
        control.data(&first);
        values.push(bounded(stream.message()).await.unwrap().unwrap());
        live.push(stream);
        controls.push(control);
    }
    #[cfg(feature = "copy-counts")]
    {
        let counts = pbrs_grpc::copy_counts::retained_decode_counts();
        assert!(counts.retained_capacity > 0);
        assert!(counts.retained_capacity <= counts.capacity_limit);
        assert!(counts.retained_buffers < 17);
        assert!(counts.rejected_budget > before.rejected_budget);
    }
    for (stream, control) in live.iter_mut().zip(&controls) {
        control.data(&second);
        assert_eq!(
            bounded(stream.message()).await.unwrap().unwrap().0.value,
            vec![b'b'; SIZE]
        );
    }
    assert!(values.iter().all(|value| value.0.value == vec![b'a'; SIZE]));
    drop(live);
    for control in controls {
        assert_eq!(bounded(control.reset).await.unwrap(), h2::Reason::CANCEL);
    }
    quiescent().await;
    peer.recover().await;

    assert_eq!(held_shared.0.value.as_ref(), vec![b'a'; SIZE]);
    assert_eq!(held_lazy.message().to_str().unwrap(), "a".repeat(SIZE));
    assert_eq!(held_raw.0, first);
}
