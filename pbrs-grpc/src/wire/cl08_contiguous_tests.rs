//! Direct-frame codec behavior, including the existing batch rollback boundary.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "bounded codec regression assertions"
)]

use super::{SegSink, append_frame, encode_msg};
use crate::codec::{self, CodecMessage};
use crate::compression::CompressionAlgorithm;
use crate::config::ChannelConfig;
use crate::limits::MessageLimits;
use crate::status::{Code, Status};
use crate::stream::Framed;
use crate::transport::{
    ClientBuilder, FlowControl, RecvStream, SendRequest, SendStream, ServerBuilder,
    ServerConnection, h2 as backend,
};
use crate::wire::OutBatch;
use bytes::{Bytes, BytesMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::task::JoinHandle;

#[derive(Default)]
struct Encodes {
    direct: AtomicUsize,
    fallback: AtomicUsize,
}

struct Direct {
    payload: Bytes,
    fail: bool,
    calls: Arc<Encodes>,
}

impl Direct {
    fn new(payload: &'static [u8]) -> Self {
        Self {
            payload: Bytes::from_static(payload),
            fail: false,
            calls: Arc::new(Encodes::default()),
        }
    }
}

impl CodecMessage for Direct {
    fn encoded_len(&self) -> usize {
        self.payload.len()
    }

    fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
        self.calls.fallback.fetch_add(1, Ordering::SeqCst);
        out.put_slice(&self.payload);
        Ok(())
    }

    fn encode_contiguous(&self, out: &mut BytesMut) -> Option<Result<(), Status>> {
        self.calls.direct.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            out.extend_from_slice(b"partial");
            Some(Err(Status::internal("injected partial encode failure")))
        } else {
            out.extend_from_slice(&self.payload);
            Some(Ok(()))
        }
    }

    fn decode_payload(payload: Bytes) -> Result<Self, Status> {
        Ok(Self {
            payload,
            fail: false,
            calls: Arc::new(Encodes::default()),
        })
    }

    fn empty() -> Self {
        Self::new(b"")
    }
}

#[test]
fn direct_payload_appends_after_prefix_and_tracks_actual_bytes() {
    let message = Direct::new(b"payload");
    let mut sink = SegSink::new();
    append_frame(&mut sink, &message, None, MessageLimits::new(), 1).expect("append");
    assert_eq!(sink.len(), codec::HEADER_LEN + message.payload.len());
    let frame = sink.finish();
    assert_eq!(frame.seg_count(), 1);
    assert_eq!(
        frame.concat(),
        codec::encode(b"payload", false).expect("frame")
    );
    assert_eq!(message.calls.direct.load(Ordering::SeqCst), 1);
    assert_eq!(message.calls.fallback.load(Ordering::SeqCst), 0);
}

#[test]
fn outbound_limit_rejects_before_either_encode_path() {
    let message = Direct::new(b"payload");
    let limits = MessageLimits::new().with_max_encoding(message.payload.len() - 1);
    let error = encode_msg(&message, None, limits, 1).expect_err("over limit");
    assert_eq!(error.code(), Code::ResourceExhausted);
    let mut sink = SegSink::new();
    let error = append_frame(&mut sink, &message, None, limits, 1).expect_err("batch over limit");
    assert_eq!(error.code(), Code::ResourceExhausted);
    assert!(sink.is_empty());
    assert_eq!(message.calls.direct.load(Ordering::SeqCst), 0);
    assert_eq!(message.calls.fallback.load(Ordering::SeqCst), 0);
}

#[test]
fn gzip_stays_on_materialized_payload_path() {
    let message = Direct::new(b"compress this payload");
    let frame = encode_msg(
        &message,
        Some(CompressionAlgorithm::Gzip),
        MessageLimits::new(),
        1,
    )
    .expect("gzip frame");
    let mut bytes = BytesMut::from(frame.concat().as_slice());
    let framed = codec::pop(&mut bytes)
        .expect("framing")
        .expect("complete frame");
    assert!(framed.compressed);
    assert_eq!(
        crate::gzip::decode(&framed.payload).expect("gzip"),
        message.payload
    );
    assert!(bytes.is_empty());
    assert_eq!(message.calls.direct.load(Ordering::SeqCst), 0);
    assert_eq!(message.calls.fallback.load(Ordering::SeqCst), 1);
}

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn partial_failure_preserves_previous_batch_and_later_good_append() {
    let (client_io, server_io) = tokio::io::duplex(65536);
    let (send, conn) = backend::ClientBuilder::new()
        .handshake(client_io)
        .await
        .expect("client handshake");
    let _client_driver = AbortOnDrop(tokio::spawn(conn));
    let mut server = backend::ServerBuilder::new()
        .handshake(server_io)
        .await
        .expect("server handshake");
    let mut peer = AbortOnDrop(tokio::spawn(async move {
        let (request, _response) = server.accept().await.expect("stream").expect("request");
        let _server_driver = AbortOnDrop(tokio::spawn(async move {
            while server.accept().await.is_some() {}
        }));
        let mut body = request.into_body();
        let mut data = Vec::new();
        while let Some(chunk) = std::future::poll_fn(|cx| body.poll_data(cx)).await {
            let chunk = chunk.expect("DATA");
            body.flow_control()
                .release_capacity(chunk.len())
                .expect("window");
            data.extend_from_slice(&chunk);
        }
        data
    }));
    let mut send = send.ready().await.expect("ready");
    let (response, mut stream) = send
        .send_request(
            http::Request::builder()
                .method("POST")
                .uri("http://localhost/cl08/Batch")
                .body(())
                .expect("request"),
            false,
        )
        .expect("open");
    let mut batch = OutBatch::new(ChannelConfig::new().wire());
    batch
        .encode(Framed::new(Direct::new(b"earlier")))
        .expect("first frame");
    let mut failing = Direct::new(b"must not escape");
    failing.fail = true;
    let error = batch
        .encode(Framed::new(failing))
        .expect_err("partial failure");
    assert_eq!(error.code(), Code::Internal);
    assert_eq!(error.message(), "injected partial encode failure");
    batch
        .encode(Framed::new(Direct::new(b"later")))
        .expect("later frame");
    batch
        .flush(&mut stream)
        .await
        .expect("flush retained frames");
    stream.send_data(Bytes::new(), true).expect("end stream");
    let bytes = tokio::time::timeout(Duration::from_secs(2), &mut peer.0)
        .await
        .expect("bounded receiver")
        .expect("receiver task");
    let mut expected = codec::encode(b"earlier", false).expect("earlier").to_vec();
    expected.extend_from_slice(&codec::encode(b"later", false).expect("later"));
    assert_eq!(bytes, expected);
    drop(response);
}

#[cfg(feature = "prost")]
#[derive(Clone, PartialEq, prost::Message)]
struct ProstFields {
    #[prost(bytes = "vec", tag = "1")]
    bytes: Vec<u8>,
    #[prost(string, tag = "2")]
    text: String,
    #[prost(sint64, tag = "3")]
    number: i64,
}

#[cfg(feature = "prost")]
#[test]
fn prost_direct_frames_match_reference_at_length_boundaries() {
    use crate::codec::prost::Message;
    use prost::Message as _;
    for length in [0, 1, 127, 128, 1024, 16383, 16384, 65536, 1024 * 1024] {
        let message = Message(ProstFields {
            bytes: vec![0xab; length],
            text: "boundary".into(),
            number: -129,
        });
        let payload = message.0.encode_to_vec();
        let expected = codec::encode(&payload, false).expect("reference frame");
        let framed = encode_msg(&message, None, MessageLimits::new(), 1).expect("direct frame");
        assert_eq!(framed.concat(), expected, "payload length {length}");
        assert_eq!(framed.total_len(), expected.len());
        assert_eq!(framed.seg_count(), 1);
    }
}
