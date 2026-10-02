//! Prost message adapter for the native transport.
//!
//! Rust coherence prevents a blanket `CodecMessage for T: prost::Message`
//! alongside the pbrs blanket implementation. `Message<T>` is the explicit
//! prost transport wrapper; the generated prost stubs hide it at RPC
//! boundaries so handlers still work with their prost-build message types.

use crate::{CodecMessage, Framed, MessageLimits, Status, StreamSender as NativeSender};
use bytes::Bytes;
use futures_core::Stream as FuturesStream;
use prost::Message as ProstMessage;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A prost message carried over the native pbrs-grpc transport.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Message<T>(pub T);

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
    fn encoded_len(&self) -> usize {
        self.0.encoded_len()
    }

    fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
        out.put_slice(&self.encode_to_vec()?);
        Ok(())
    }

    fn encode_to_vec(&self) -> Result<Vec<u8>, Status> {
        let mut out = Vec::with_capacity(self.0.encoded_len());
        self.0
            .encode(&mut out)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(out)
    }

    fn decode_payload(payload: Bytes) -> Result<Self, Status> {
        T::decode(payload)
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
