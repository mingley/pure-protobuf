//! Verify buffer ownership at the public Tonic codec boundary.

#![allow(
    clippy::expect_used,
    reason = "missing frames and fixture fields should fail these integration tests"
)]

use bytes::{Buf, BufMut};
use pbrs::{Parse, Serialize, gencode::TestAllTypesProto3};
use protobuf_tonic::{ProtobufCodec, ProtobufDecoder, ProtobufEncoder};
use std::future::poll_fn;
use std::pin::Pin;
use tonic::Status;
use tonic::codec::{
    BufferSettings, Codec, CompressionEncoding, DecodeBuf, Decoder, EncodeBody, EncodeBuf, Encoder,
    Streaming,
};
use tonic::codegen::Body;

type TestMessage = TestAllTypesProto3;

struct ObserveDecoder {
    decoder: ProtobufDecoder<TestMessage>,
    payload_offsets: [usize; 3],
}

impl Decoder for ObserveDecoder {
    type Item = (TestMessage, [usize; 3]);
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Status> {
        let addresses = self
            .payload_offsets
            .map(|offset| src.chunk().as_ptr().wrapping_add(offset).addr());
        self.decoder
            .decode(src)
            .map(|item| item.map(|message| (message, addresses)))
    }
}

fn append_field(wire: &mut Vec<u8>, tag: u32, payload: &[u8]) -> usize {
    prost::encoding::encode_varint(u64::from((tag << 3) | 2), wire);
    prost::encoding::encode_varint(payload.len() as u64, wire);
    let offset = wire.len();
    wire.extend_from_slice(payload);
    offset
}

#[tokio::test]
async fn large_fields_keep_the_tonic_decode_buffer_alive() {
    let text = "é".repeat(24 * 1024);
    let bytes = vec![0x4a; 48 * 1024];
    let nested_bytes = vec![0xb3; 48 * 1024];
    let mut nested = Vec::new();
    let nested_offset = append_field(&mut nested, 2, &nested_bytes);

    let mut wire = Vec::new();
    let text_offset = append_field(&mut wire, 14, text.as_bytes());
    let bytes_offset = append_field(&mut wire, 15, &bytes);
    let any_offset = append_field(&mut wire, 305, &nested);
    let payload_offsets = [text_offset, bytes_offset, any_offset + nested_offset];

    for encoding in [None, Some(CompressionEncoding::Gzip)] {
        let mut codec = ProtobufCodec::<TestMessage, TestMessage>::new();
        let first = TestMessage::parse(&wire).expect("fixture parses");
        let mut second = first.clone();
        second.set_optional_int32(42);
        second.set_optional_bytes(vec![0x71; bytes.len()]);
        let body = EncodeBody::new_client(
            codec.encoder(),
            tokio_stream::iter([Ok(first), Ok(second)]),
            encoding,
            None,
        );
        let mut stream = Streaming::new_request(
            ObserveDecoder {
                decoder: codec.decoder(),
                payload_offsets,
            },
            body,
            encoding,
            None,
        );
        let (first, addresses) = stream
            .message()
            .await
            .expect("decode first frame")
            .expect("first message");
        let (second, _) = stream
            .message()
            .await
            .expect("decode second frame")
            .expect("second message");
        assert_eq!(second.optional_int32(), 42);
        assert!(second.optional_bytes().iter().all(|&byte| byte == 0x71));
        assert!(stream.message().await.expect("stream EOF").is_none());
        drop(stream);
        drop(second);

        assert_eq!(first.optional_int32(), 0);
        let text_view = first.optional_string();
        let bytes_view = first.optional_bytes();
        let nested = first.optional_any();
        let nested_view = nested.value();
        assert_eq!(text_view.to_str(), Ok(text.as_str()));
        assert_eq!(bytes_view, bytes);
        assert_eq!(nested_view, nested_bytes);
        assert_eq!(
            [
                text_view.as_bytes().as_ptr().addr(),
                bytes_view.as_ptr().addr(),
                nested_view.as_ptr().addr(),
            ],
            addresses
        );
    }
}

struct ObserveEncoder {
    encoder: ProtobufEncoder<TestMessage>,
    payload_address: Option<tokio::sync::oneshot::Sender<usize>>,
}

impl Encoder for ObserveEncoder {
    type Item = TestMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        let address = dst.chunk_mut().as_mut_ptr().addr();
        self.encoder.encode(item, dst)?;
        self.payload_address
            .take()
            .expect("single message")
            .send(address)
            .expect("address receiver");
        Ok(())
    }

    fn buffer_settings(&self) -> BufferSettings {
        self.encoder.buffer_settings()
    }
}

#[tokio::test]
async fn tonic_emits_the_buffer_written_by_the_encoder() {
    let mut message = TestMessage::default();
    message.set_optional_bytes(vec![0xff; 48 * 1024]);
    let expected = message.serialize().expect("serialize fixture");
    let (send_address, address) = tokio::sync::oneshot::channel();
    let encoder = ObserveEncoder {
        encoder: ProtobufEncoder::new(BufferSettings::new(64 * 1024, 64 * 1024)),
        payload_address: Some(send_address),
    };
    let mut body = EncodeBody::new_client(encoder, tokio_stream::iter([Ok(message)]), None, None);
    let frame = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
        .await
        .expect("encoded frame")
        .expect("encode status")
        .into_data()
        .expect("data frame");
    let payload = frame.get(5..).expect("gRPC prefix");
    assert_eq!(payload, expected);
    assert_eq!(
        payload.as_ptr().addr(),
        address.await.expect("encoder address")
    );
}
