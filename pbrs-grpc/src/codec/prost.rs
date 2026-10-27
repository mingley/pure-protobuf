//! Prost message adapter for the native transport.
//!
//! Rust coherence prevents a blanket `CodecMessage for T: prost::Message`
//! alongside the pbrs blanket implementation. `Message<T>` is the explicit
//! prost transport wrapper; the generated prost stubs hide it at RPC
//! boundaries so handlers still work with their prost-build message types.

use crate::{CodecMessage, Framed, MessageLimits, Status, StreamSender as NativeSender};
use bytes::{Buf, Bytes};
use futures_core::Stream as FuturesStream;
use prost::Message as ProstMessage;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A prost message carried over the native pbrs-grpc transport.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Message<T>(pub T);

// Bytes::copy_to_bytes moves the backing allocation out when the requested
// range is the entire remaining input. Prost uses it even for owned Vec fields,
// then copies and drops that temporary. Keep an exhausted owner in the cursor
// so the transport can reclaim it after decoding. Partial splits keep Bytes'
// existing behavior; there is no unconditional keeper clone per message.
struct DecodeCursor<'a>(&'a mut Bytes);

impl Buf for DecodeCursor<'_> {
    fn remaining(&self) -> usize {
        self.0.len()
    }

    fn chunk(&self) -> &[u8] {
        self.0.chunk()
    }

    fn advance(&mut self, count: usize) {
        self.0.advance(count);
    }

    fn copy_to_bytes(&mut self, count: usize) -> Bytes {
        if count == 0 {
            Bytes::new()
        } else if count == self.0.len() {
            let bytes = self.0.clone();
            self.0.advance(count);
            bytes
        } else {
            self.0.split_to(count)
        }
    }
}

impl<T> Message<T> {
    /// Wrap a prost message for transport.
    #[must_use]
    pub fn new(message: T) -> Self {
        Self(message)
    }

    /// Recover the prost message.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> CodecMessage for Message<T>
where
    T: ProstMessage + Default,
{
    const ENCODE_APPEND_ONLY: bool = true;

    fn encoded_len(&self) -> usize {
        self.0.encoded_len()
    }

    fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
        // Prost writes straight into the frame buffer. The previous path
        // encoded into a `Vec` and copied that into the frame.
        out.write_contiguous(|buf| {
            let before = buf.len();
            match self.0.encode(buf) {
                Ok(()) => Ok(()),
                Err(err) => {
                    buf.truncate(before);
                    Err(Status::internal(err.to_string()))
                }
            }
        })
    }

    fn encode_to_vec(&self) -> Result<Vec<u8>, Status> {
        let mut out = Vec::with_capacity(self.0.encoded_len());
        self.0
            .encode(&mut out)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(out)
    }

    fn encode_payload_bufmut<B: bytes::BufMut>(&self, out: &mut B) -> Result<(), Status> {
        self.0
            .encode(out)
            .map_err(|e| Status::internal(e.to_string()))
    }

    fn decode_payload(payload: Bytes) -> Result<Self, Status> {
        T::decode(payload)
            .map(Self)
            .map_err(|e| Status::internal(e.to_string()))
    }

    fn decode_payload_reusable(payload: &mut Bytes) -> Result<Self, Status> {
        T::decode(DecodeCursor(payload))
            .map(Self)
            .map_err(|e| Status::internal(e.to_string()))
    }

    fn empty() -> Self {
        Self(T::default())
    }
}

/// Prost-friendly stream wrapper used by generated stubs.
#[derive(Debug)]
pub struct Streaming<T> {
    inner: crate::Streaming<Message<T>>,
}

impl<T> Streaming<T> {
    /// Wrap a native prost-message stream.
    #[must_use]
    pub fn from_native(inner: crate::Streaming<Message<T>>) -> Self {
        Self { inner }
    }

    /// Recover the native wrapped stream.
    #[must_use]
    pub fn into_native(self) -> crate::Streaming<Message<T>> {
        self.inner
    }

    /// Create an application stream carrying prost messages.
    #[must_use]
    pub fn channel(buffer: usize) -> (StreamSender<T>, Self) {
        let (tx, stream) = crate::Streaming::channel(buffer);
        (StreamSender { inner: tx }, Self { inner: stream })
    }

    /// Read the next prost message.
    pub async fn message(&mut self) -> Result<Option<T>, Status> {
        Ok(self.inner.message().await?.map(Message::into_inner))
    }

    /// Collect all prost messages.
    pub async fn collect(&mut self) -> Result<Vec<T>, Status> {
        let mut out = Vec::new();
        while let Some(message) = self.message().await? {
            out.push(message);
        }
        Ok(out)
    }

    /// Trailing metadata after end-of-stream.
    pub async fn trailers(&mut self) -> Result<crate::Metadata, Status> {
        self.inner.trailers().await
    }
}

impl<T> FuturesStream for Streaming<T> {
    type Item = Result<T, Status>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        Pin::new(&mut this.inner)
            .poll_next(cx)
            .map(|item| item.map(|result| result.map(Message::into_inner)))
    }
}

/// Prost-friendly write half used by generated stubs.
#[derive(Clone)]
pub struct StreamSender<T> {
    inner: NativeSender<Message<T>>,
}

impl<T> StreamSender<T> {
    /// Wrap a native sender.
    #[must_use]
    pub fn from_native(inner: NativeSender<Message<T>>) -> Self {
        Self { inner }
    }

    /// Recover the native sender.
    #[must_use]
    pub fn into_native(self) -> NativeSender<Message<T>> {
        self.inner
    }

    /// Queue one prost message.
    pub async fn send(&self, message: T) -> Result<(), Status>
    where
        Message<T>: CodecMessage,
    {
        self.inner.send(Message(message)).await
    }

    /// Queue one compressed prost message.
    pub async fn send_compressed(&self, message: T) -> Result<(), Status>
    where
        Message<T>: CodecMessage,
    {
        self.inner.send_compressed(Message(message)).await
    }

    /// Queue one framed prost message.
    pub async fn send_framed(&self, item: Framed<T>) -> Result<(), Status>
    where
        Message<T>: CodecMessage,
    {
        self.inner
            .send_framed(Framed {
                message: Message(item.message),
                compressed: item.compressed,
            })
            .await
    }

    /// End the stream with an error.
    pub async fn fail(self, status: Status) {
        self.inner.fail(status).await;
    }

    /// Half-close this sender.
    pub fn close(self) {
        self.inner.close();
    }

    /// Whether the reader has gone away.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Resolves when the reader has gone away.
    pub async fn closed(&self) {
        self.inner.closed().await;
    }
}

/// Wrap a prost response for the native wire.
#[must_use]
pub fn response_to_native<T>(response: crate::Response<T>) -> crate::Response<Message<T>> {
    response.map(Message)
}

/// Unwrap a native response into prost messages.
#[must_use]
pub fn response_from_native<T>(response: crate::Response<Message<T>>) -> crate::Response<T> {
    response.map(Message::into_inner)
}

/// Wrap a prost streaming response for the native wire.
#[must_use]
pub fn streaming_response_to_native<T>(
    response: crate::Response<Streaming<T>>,
) -> crate::Response<crate::Streaming<Message<T>>> {
    response.map(Streaming::into_native)
}

/// Unwrap a native streaming response into prost messages.
#[must_use]
pub fn streaming_response_from_native<T>(
    response: crate::Response<crate::Streaming<Message<T>>>,
) -> crate::Response<Streaming<T>> {
    response.map(Streaming::from_native)
}

/// Apply encode limits before handing a prost message to a native sender.
#[must_use]
pub fn limits() -> MessageLimits {
    MessageLimits::unlimited()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, prost::Message)]
    struct Sample {
        #[prost(bytes = "vec", tag = "1")]
        payload: Vec<u8>,
        #[prost(int32, tag = "2")]
        n: i32,
    }

    #[test]
    fn direct_encode_matches_the_buffered_payload() {
        let msg = Message(Sample {
            payload: vec![0x11; 1000],
            n: -7,
        });
        let buffered = msg.encode_to_vec().expect("buffered");
        let mut direct = Vec::new();
        msg.encode_payload(&mut direct).expect("direct");
        assert_eq!(direct, buffered);
    }

    #[test]
    fn reusable_decode_preserves_owned_and_shared_field_semantics() {
        #[derive(Clone, PartialEq, prost::Message)]
        struct LastBytes {
            #[prost(bytes = "vec", tag = "1")]
            payload: Vec<u8>,
        }

        #[derive(Clone, PartialEq, prost::Message)]
        struct Nested {
            #[prost(message, optional, tag = "1")]
            value: Option<LastBytes>,
        }

        #[derive(Clone, PartialEq, prost::Message)]
        struct SharedLast {
            #[prost(bytes = "bytes", tag = "1")]
            payload: Bytes,
        }

        #[derive(Clone, PartialEq, prost::Message)]
        struct Shared {
            #[prost(bytes = "bytes", tag = "1")]
            payload: Bytes,
            #[prost(int32, tag = "2")]
            n: i32,
        }

        let expected = Sample {
            payload: vec![0x71; 64 * 1024],
            n: -19,
        };
        let encoded = expected.encode_to_vec();
        let mut owned_input = Bytes::from(encoded.clone());
        let owned = Message::<Sample>::decode_payload_reusable(&mut owned_input).expect("owned");
        assert_eq!(owned.0, expected);
        assert!(owned_input.is_empty());
        assert!(
            owned_input.try_into_mut().is_ok(),
            "owned fields release input"
        );

        let last = LastBytes {
            payload: expected.payload.clone(),
        };
        let last_buffer = last.encode_to_vec();
        let last_capacity = last_buffer.capacity();
        let last_pointer = last_buffer.as_ptr();
        let mut last_input = Bytes::from(last_buffer);
        let actual =
            Message::<LastBytes>::decode_payload_reusable(&mut last_input).expect("last field");
        assert_eq!(actual.0, last);
        assert!(last_input.is_empty());
        let recovered: Vec<u8> = last_input
            .try_into_mut()
            .expect("last bytes field preserves unique ownership")
            .into();
        assert_eq!(
            recovered.capacity(),
            last_capacity,
            "last bytes field returns original storage"
        );
        assert_eq!(recovered.as_ptr(), last_pointer);

        let nested = Nested {
            value: Some(last.clone()),
        };
        let nested_buffer = nested.encode_to_vec();
        let nested_capacity = nested_buffer.capacity();
        let nested_pointer = nested_buffer.as_ptr();
        let mut nested_input = Bytes::from(nested_buffer);
        let actual = Message::<Nested>::decode_payload_reusable(&mut nested_input)
            .expect("nested last field");
        assert_eq!(actual.0, nested);
        assert!(nested_input.is_empty());
        let recovered: Vec<u8> = nested_input
            .try_into_mut()
            .expect("nested last field preserves unique ownership")
            .into();
        assert_eq!(recovered.capacity(), nested_capacity);
        assert_eq!(recovered.as_ptr(), nested_pointer);

        let mut shared_last_input = Bytes::from(last.encode_to_vec());
        let actual = Message::<SharedLast>::decode_payload_reusable(&mut shared_last_input)
            .expect("shared last field");
        assert_eq!(actual.0.payload.as_ref(), last.payload);
        assert!(shared_last_input.is_empty());
        assert!(shared_last_input.try_into_mut().is_err());

        let mut empty = Bytes::new();
        assert_eq!(
            Message::<LastBytes>::decode_payload_reusable(&mut empty)
                .expect("empty payload")
                .0,
            LastBytes::default()
        );
        let mut truncated_last = last.encode_to_vec();
        truncated_last.pop();
        let mut truncated_last = Bytes::from(truncated_last);
        let ordinary = Message::<LastBytes>::decode_payload(truncated_last.clone()).unwrap_err();
        let reusable =
            Message::<LastBytes>::decode_payload_reusable(&mut truncated_last).unwrap_err();
        assert_eq!(ordinary.code(), reusable.code());
        assert_eq!(ordinary.message(), reusable.message());

        let mut shared_input = Bytes::from(encoded.clone());
        let shared = Message::<Shared>::decode_payload_reusable(&mut shared_input).expect("shared");
        assert!(shared_input.is_empty());
        assert!(
            shared_input.try_into_mut().is_err(),
            "shared field owns backing"
        );
        assert_eq!(shared.0.payload.as_ref(), expected.payload);
        assert_eq!(shared.0.n, expected.n);

        let mut truncated = Bytes::from(encoded[..encoded.len() - 1].to_vec());
        let ordinary_error = Message::<Sample>::decode_payload(truncated.clone()).unwrap_err();
        let reusable_error =
            Message::<Sample>::decode_payload_reusable(&mut truncated).unwrap_err();
        assert_eq!(reusable_error.code(), ordinary_error.code());
        assert_eq!(reusable_error.message(), ordinary_error.message());
    }
}
