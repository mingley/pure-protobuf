//! Tonic-shaped API shims over the native `pbrs-grpc` transport.

use futures_core::Stream;
use std::collections::VecDeque;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::{Context, Poll};

pub use crate::{Code, Request, Response, Status, StreamSender, Streaming};
pub use futures_core::Stream as ResponseStream;

/// Convert a value into a tonic-shaped unary request envelope.
pub trait IntoRequest<T> {
    /// Convert into a [`Request`].
    fn into_request(self) -> Request<T>;
}

impl<T> IntoRequest<T> for T {
    fn into_request(self) -> Request<T> {
        Request::new(self)
    }
}

impl<T> IntoRequest<T> for Request<T> {
    fn into_request(self) -> Request<T> {
        self
    }
}

/// Convert a stream into a tonic-shaped streaming request envelope.
pub trait IntoStreamingRequest<T> {
    /// Stream type carrying request messages.
    type Stream: Stream<Item = T> + Send + 'static;

    /// Convert into a [`Request`] around the stream.
    fn into_streaming_request(self) -> Request<Self::Stream>;
}

impl<T, S> IntoStreamingRequest<T> for S
where
    S: Stream<Item = T> + Send + 'static,
{
    type Stream = S;

    fn into_streaming_request(self) -> Request<Self::Stream> {
        Request::new(self)
    }
}

/// A small dependency-free stream for examples and tests.
pub struct Iter<T> {
    items: VecDeque<T>,
}

impl<T> Unpin for Iter<T> {}

impl<T> Stream for Iter<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.items.pop_front())
    }
}

/// Build a stream from an iterator without adding `tokio-stream`.
pub fn iter<T>(items: impl IntoIterator<Item = T>) -> Iter<T> {
    Iter {
        items: items.into_iter().collect(),
    }
}

/// Forward a client request stream into the native write half.
pub fn spawn_request_stream<T, S>(stream: S, sender: StreamSender<T>)
where
    T: crate::CodecMessage + Send + 'static,
    S: Stream<Item = T> + Send + 'static,
{
    drop(tokio::spawn(async move {
        let mut stream = Box::pin(stream);
        loop {
            let next = poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
            let Some(message) = next else {
                break;
            };
            if sender.send(message).await.is_err() {
                return;
            }
        }
        sender.close();
    }));
}

/// Convert a tonic-shaped response stream into the native streaming response.
pub fn response_stream<T, S>(response: Response<S>) -> Response<Streaming<T>>
where
    T: crate::CodecMessage + Send + 'static,
    S: Stream<Item = Result<T, Status>> + Send + 'static,
{
    let (stream, parts) = response.into_message_and_parts();
    let (sender, outbound) = Streaming::channel(crate::DEFAULT_STREAM_BUFFER);
    drop(tokio::spawn(async move {
        let mut stream = Box::pin(stream);
        loop {
            match poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
                Some(Ok(message)) => {
                    if sender.send(message).await.is_err() {
                        return;
                    }
                }
                Some(Err(status)) => {
                    sender.fail(status).await;
                    return;
                }
                None => {
                    sender.close();
                    return;
                }
            }
        }
    }));
    Response::from_message_and_parts(outbound, parts)
}
