//! A tonic `Codec` for pbrs messages using `Parse` and `Serialize`.
//!
//! Keeps tonic transport and middleware while using pbrs-generated messages.
//! Code that requires `prost::Message` needs an adapter.

extern crate self as protobuf_tonic;

use bytes::Buf;
use bytes::Bytes;
use pbrs::{ClearAndParse, Parse, Serialize};
use prost::Message as ProstMessage;
use std::error::Error;
use std::fmt;
use std::marker::PhantomData;
use tonic::Status;
use tonic::codec::{BufferSettings, Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

/// Google Protobuf wire-format message size ceiling (`i32::MAX`, 2 GiB - 1).
const MAX_PROTOBUF_MESSAGE_BYTES: usize = 0x7fff_ffff;

/// tonic [`Codec`] using pbrs [`Serialize`] / [`Parse`] (not `prost::Message`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ProtobufCodec<E, D> {
    _e: PhantomData<fn() -> E>,
    _d: PhantomData<fn() -> D>,
}

impl<E, D> ProtobufCodec<E, D> {
    /// Construct a new [`ProtobufCodec`] with default Tonic buffer settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            _e: PhantomData,
            _d: PhantomData,
        }
    }
}

impl<E, D> ProtobufCodec<E, D>
where
    E: Serialize + Send + 'static,
    D: Parse + Default + ClearAndParse + Send + 'static,
{
    /// Build an encoder half configured with explicit [`BufferSettings`].
    #[must_use]
    pub fn raw_encoder(buffer_settings: BufferSettings) -> <Self as Codec>::Encoder {
        ProtobufEncoder::new(buffer_settings)
    }

    /// Build a decoder half configured with explicit [`BufferSettings`].
    #[must_use]
    pub fn raw_decoder(buffer_settings: BufferSettings) -> <Self as Codec>::Decoder {
        ProtobufDecoder::new(buffer_settings)
    }
}

impl<E, D> Codec for ProtobufCodec<E, D>
where
    E: Serialize + Send + 'static,
    D: Parse + Default + ClearAndParse + Send + 'static,
{
    type Encode = E;
    type Decode = D;
    type Encoder = ProtobufEncoder<E>;
    type Decoder = ProtobufDecoder<D>;

    fn encoder(&mut self) -> Self::Encoder {
        ProtobufEncoder(PhantomData, BufferSettings::default())
    }

    fn decoder(&mut self) -> Self::Decoder {
        ProtobufDecoder(PhantomData, BufferSettings::default())
    }
}

#[inline]
fn reserve_message_len<T: Serialize>(item: &T, dst: &mut EncodeBuf<'_>) {
    let len = item.serialized_len();
    if len <= MAX_PROTOBUF_MESSAGE_BYTES {
        dst.reserve(len);
    }
}

/// Encoder half of [`ProtobufCodec`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ProtobufEncoder<T>(PhantomData<fn() -> T>, BufferSettings);

impl<T> ProtobufEncoder<T> {
    /// Construct an encoder with explicit Tonic [`BufferSettings`].
    #[must_use]
    pub fn new(buffer_settings: BufferSettings) -> Self {
        Self(PhantomData, buffer_settings)
    }
}

impl<T: Serialize> Encoder for ProtobufEncoder<T> {
    type Item = T;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        reserve_message_len(&item, dst);
        Serialize::encode(&item, dst).map_err(|e| Status::internal(e.to_string()))
    }

    fn buffer_settings(&self) -> BufferSettings {
        self.1
    }
}

/// Decoder half of [`ProtobufCodec`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ProtobufDecoder<T>(PhantomData<fn() -> T>, BufferSettings);

impl<T> ProtobufDecoder<T> {
    /// Construct a decoder with explicit Tonic [`BufferSettings`].
    #[must_use]
    pub fn new(buffer_settings: BufferSettings) -> Self {
        Self(PhantomData, buffer_settings)
    }
}

impl<T: Parse + Default + ClearAndParse> Decoder for ProtobufDecoder<T> {
    type Item = T;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        // One copy into an owned buffer (tonic's), then shared parse with no
        // pbrs-side copy (PK-09): 2 copies before, 1 after, either branch.
        let n = src.remaining();
        let bytes = src.copy_to_bytes(n);
        Parse::parse_bytes(bytes)
            .map(Some)
            .map_err(|e| Status::internal(e.to_string()))
    }

    fn buffer_settings(&self) -> BufferSettings {
        self.1
    }
}

pub mod hello;

/// Failure while converting between wire-compatible prost and pbrs messages.
///
/// Conversion deliberately goes through protobuf wire bytes. This keeps the
/// API schema-agnostic. The encoded buffer is moved into the decoder: neither
/// direction copies that buffer after encoding, and the prost-to-pbrs
/// direction retains it as pbrs' shared backing storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversionError(String);

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ConversionError {}

/// Convert a prost message to its wire-compatible pbrs representation.
///
/// This allocates exactly one wire buffer. [`Bytes::from`] takes ownership of
/// prost's `Vec`, and pbrs parses from that shared buffer without copying it.
pub fn prost_to_pbrs<P, R>(message: &P) -> Result<R, ConversionError>
where
    P: ProstMessage,
    R: Parse,
{
    let len = message.encoded_len();
    if len > MAX_PROTOBUF_MESSAGE_BYTES {
        return Err(ConversionError("encoded message exceeds 2 GiB".to_owned()));
    }
    let mut wire = Vec::with_capacity(len);
    message
        .encode(&mut wire)
        .map_err(|error| ConversionError(error.to_string()))?;
    R::parse_bytes(Bytes::from(wire)).map_err(|error| ConversionError(error.to_string()))
}

/// Convert a pbrs message to its wire-compatible prost representation.
///
/// This allocates one wire buffer for pbrs serialization. Prost consumes the
/// owned buffer directly through its [`bytes::Buf`] decoder interface.
pub fn pbrs_to_prost<P, R>(message: &P) -> Result<R, ConversionError>
where
    P: Serialize,
    R: ProstMessage + Default,
{
    let wire = message
        .serialize()
        .map_err(|error| ConversionError(error.to_string()))?;
    R::decode(Bytes::from(wire)).map_err(|error| ConversionError(error.to_string()))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "unit tests assert encode reservation and conversion invariants directly"
)]
mod tests {
    use super::*;
    use bytes::BufMut;
    use prost::encoding::{DecodeContext, WireType};
    use std::future::poll_fn;
    use std::pin::Pin;
    use tokio::sync::mpsc::{self, UnboundedSender};
    use tonic::codec::{CompressionEncoding, EncodeBody, Streaming};

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct ReserveObservation {
        initial_chunk_len: usize,
        reserved_chunk_len: usize,
        written: usize,
        reallocated_mid_write: bool,
    }

    struct ObservingWireOut<'a, 'b> {
        dst: &'a mut EncodeBuf<'b>,
        reserved_ptr_addr: usize,
        written: usize,
        reallocated_mid_write: bool,
    }

    impl pbrs::WireOut for ObservingWireOut<'_, '_> {
        fn put_u8(&mut self, b: u8) {
            let chunk = self.dst.chunk_mut();
            if chunk.as_mut_ptr().addr() != self.reserved_ptr_addr + self.written
                || chunk.len() == 0
            {
                self.reallocated_mid_write = true;
            }
            BufMut::put_u8(self.dst, b);
            self.written += 1;
        }

        fn put_slice(&mut self, data: &[u8]) {
            if data.is_empty() {
                return;
            }
            let chunk = self.dst.chunk_mut();
            if chunk.as_mut_ptr().addr() != self.reserved_ptr_addr + self.written
                || chunk.len() < data.len()
            {
                self.reallocated_mid_write = true;
            }
            BufMut::put_slice(self.dst, data);
            self.written += data.len();
        }
    }

    struct ObservingEncoder<T> {
        settings: BufferSettings,
        observation: UnboundedSender<ReserveObservation>,
        _pd: PhantomData<fn() -> T>,
    }

    impl<T: Serialize> Encoder for ObservingEncoder<T> {
        type Item = T;
        type Error = Status;

        fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
            let initial_chunk_len = dst.chunk_mut().len();
            reserve_message_len(&item, dst);
            let reserved_chunk_len = dst.chunk_mut().len();
            let reserved_ptr_addr = dst.chunk_mut().as_mut_ptr().addr();
            let mut sink = ObservingWireOut {
                dst,
                reserved_ptr_addr,
                written: 0,
                reallocated_mid_write: false,
            };
            Serialize::encode(&item, &mut sink).map_err(|e| Status::internal(e.to_string()))?;
            self.observation
                .send(ReserveObservation {
                    initial_chunk_len,
                    reserved_chunk_len,
                    written: sink.written,
                    reallocated_mid_write: sink.reallocated_mid_write,
                })
                .expect("send observation");
            Ok(())
        }

        fn buffer_settings(&self) -> BufferSettings {
            self.settings
        }
    }

    async fn drain_body_frames<B>(mut body: B) -> Bytes
    where
        B: tonic::codegen::Body<Data = Bytes, Error = Status> + Unpin,
    {
        let mut out = bytes::BytesMut::new();
        while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
            let frame = frame.expect("encode frame");
            if let Ok(data) = frame.into_data() {
                out.extend_from_slice(&data);
            }
        }
        out.freeze()
    }

    #[tokio::test]
    async fn encoder_reserves_full_message_capacity_up_front_without_mid_write_realloc() {
        let payload = "x".repeat(256 * 1024);
        let mut msg = hello::HelloRequest::new();
        msg.set_name(payload.clone());
        let expected_len = msg.serialized_len();
        assert!(expected_len > 256 * 1024);

        let (obs_tx, mut obs_rx) = mpsc::unbounded_channel();
        let encoder = ObservingEncoder::<hello::HelloRequest> {
            settings: BufferSettings::default(),
            observation: obs_tx,
            _pd: PhantomData,
        };
        let body = EncodeBody::new_client(encoder, tokio_stream::iter([Ok(msg)]), None, None);
        let framed = drain_body_frames(body).await;
        assert_eq!(framed.len(), expected_len + 5);

        let obs = obs_rx.recv().await.expect("recorded observation");
        // Tonic's default per-RPC buffer is 8 KiB minus the 5-byte gRPC prefix.
        assert!(obs.initial_chunk_len < 8 * 1024);
        // Single upfront reserve guarantees contiguous capacity for the entire message,
        // and field-by-field serialization never reallocates mid-write.
        assert!(obs.reserved_chunk_len >= expected_len);
        assert_eq!(obs.written, expected_len);
        assert!(!obs.reallocated_mid_write);

        // Verify full ProtobufEncoder + ProtobufDecoder round-trip (identity and gzip).
        for encoding in [None, Some(CompressionEncoding::Gzip)] {
            let mut req = hello::HelloRequest::new();
            req.set_name(payload.clone());
            let encode_body = EncodeBody::new_client(
                ProtobufCodec::<hello::HelloRequest, hello::HelloRequest>::new().encoder(),
                tokio_stream::iter([Ok(req)]),
                encoding,
                Some(1024 * 1024),
            );
            let mut stream = Streaming::new_request(
                ProtobufCodec::<hello::HelloRequest, hello::HelloRequest>::new().decoder(),
                encode_body,
                encoding,
                Some(1024 * 1024),
            );
            let decoded = stream
                .message()
                .await
                .expect("decode status")
                .expect("decoded message");
            assert_eq!(decoded.name().to_str(), Ok(payload.as_str()));
            assert!(stream.message().await.expect("eof").is_none());
        }
    }

    #[tokio::test]
    async fn custom_buffer_settings_propagate_through_raw_encoder() {
        let mut msg = hello::HelloRequest::new();
        msg.set_name("hello".to_owned());

        let (obs_tx, mut obs_rx) = mpsc::unbounded_channel();
        let raw_encoder = ProtobufCodec::<hello::HelloRequest, hello::HelloReply>::raw_encoder(
            BufferSettings::new(64 * 1024, 128 * 1024),
        );
        let encoder = ObservingEncoder::<hello::HelloRequest> {
            settings: raw_encoder.buffer_settings(),
            observation: obs_tx,
            _pd: PhantomData,
        };
        let body = EncodeBody::new_client(encoder, tokio_stream::iter([Ok(msg)]), None, None);
        let _ = drain_body_frames(body).await;

        let obs = obs_rx.recv().await.expect("recorded observation");
        assert!(obs.initial_chunk_len >= 64 * 1024 - 5);
    }

    #[derive(Debug, Default)]
    struct OversizedProstMessage;

    impl ProstMessage for OversizedProstMessage {
        fn encode_raw(&self, _buf: &mut impl BufMut)
        where
            Self: Sized,
        {
            panic!("encode_raw must not be called when encoded_len exceeds 2 GiB");
        }

        fn merge_field(
            &mut self,
            _tag: u32,
            _wire_type: WireType,
            _buf: &mut impl Buf,
            _ctx: DecodeContext,
        ) -> Result<(), prost::DecodeError>
        where
            Self: Sized,
        {
            Ok(())
        }

        fn encoded_len(&self) -> usize {
            MAX_PROTOBUF_MESSAGE_BYTES + 1
        }

        fn clear(&mut self) {}
    }

    #[test]
    fn prost_to_pbrs_rejects_over_2gib_length_without_allocating() {
        let err =
            prost_to_pbrs::<OversizedProstMessage, hello::HelloRequest>(&OversizedProstMessage)
                .expect_err("over-2-GiB prost message must be rejected");
        assert_eq!(err.to_string(), "encoded message exceeds 2 GiB");
    }
}
