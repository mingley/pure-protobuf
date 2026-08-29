//! Message streams: [`Streaming`] to read, [`StreamSender`] to write.
//!
//! Both halves are used on both sides of an RPC. A client-streaming call
//! hands you a [`StreamSender`] for requests; a server-streaming handler
//! returns a [`Streaming`] of responses. The pair is created with
//! [`Streaming::channel`].

use crate::limits::MessageLimits;
use crate::metadata::Metadata;
use crate::status::Status;
use futures_core::Stream;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, watch};

/// A message plus the gRPC Compressed-Flag its frame carried.
///
/// Most code ignores the flag and uses [`Streaming::message`] /
/// [`StreamSender::send`]. Reach for [`Framed`] when a per-message
/// compression decision matters, as in the official interop suite.
#[derive(Clone, Debug)]
pub struct Framed<T> {
    /// The protobuf message.
    pub message: T,
    /// Whether the frame's Compressed-Flag was set.
    pub compressed: bool,
}

impl<T> Framed<T> {
    /// A frame with the Compressed-Flag clear.
    #[must_use]
    pub fn new(message: T) -> Self {
        Self {
            message,
            compressed: false,
        }
    }

    /// A frame with the Compressed-Flag set.
    #[must_use]
    pub fn compressed(message: T) -> Self {
        Self {
            message,
            compressed: true,
        }
    }

    /// Discard the flag.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.message
    }
}

type Item<T> = Result<Framed<T>, Status>;

/// Where a [`Streaming`] gets its messages.
enum Source<T> {
    /// Written by application code through a [`StreamSender`].
    Channel(mpsc::Receiver<Item<T>>),
    /// Decoded straight off an HTTP/2 stream, with no pump task in between.
    Wire(Box<crate::wire::WireStream<T>>),
}

/// A sequence of decoded gRPC messages.
///
/// Reading is the only way to advance the stream. For a received stream that is
/// literal: the bytes are decoded on the reading task, so a handler that stops
/// reading stops releasing HTTP/2 capacity and the peer stalls at the window.
/// There is no pump task and no queue in between.
///
/// Pull with [`Self::message`], or as a `futures_core::Stream`
/// (`Item = Result<T, Status>`). Both paths are the same poll: there is no
/// extra buffer and no extra task.
///
/// A stream received from a [`crate::Channel`] holds the HTTP/2 driver, so
/// dropping the client after headers still lets you read to the end.
/// Dropping this `Streaming` before the end resets the HTTP/2 stream, even
/// if a bidi [`StreamSender`] is still held: that is how a client cancels a
/// streaming RPC after it already has headers. A server-side producer waiting
/// on [`crate::Request::cancelled`] or [`StreamSender::closed`] then wakes.
///
/// ```no_run
/// # use pbrs_grpc::{HelloReply, Status, Streaming};
/// # async fn demo(mut stream: Streaming<HelloReply>) -> Result<(), Status> {
/// while let Some(reply) = stream.message().await? {
///     println!("{}", reply.message());
/// }
/// let trailers = stream.trailers().await?;
/// # let _ = trailers;
/// # Ok(())
/// # }
/// ```
pub struct Streaming<T> {
    source: Source<T>,
    /// Client connection lease. `None` on server-produced streams and on
    /// channels that do not idle-close.
    lease: Option<crate::keepalive::Lease>,
    /// Keeps the client HTTP/2 driver alive after [`crate::Channel`] is
    /// dropped. `None` on server-produced streams.
    driver: Option<watch::Sender<bool>>,
    /// Client cancel sender. Dropping a received stream before the end
    /// resets the RPC, including bidi while the send half is still held.
    /// `None` on application channels and server-inbound streams.
    reset: Option<watch::Sender<bool>>,
}

impl<T> Streaming<T> {
    /// A connected [`StreamSender`] / [`Streaming`] pair holding `buffer`
    /// messages in flight.
    ///
    /// This is how a server-streaming handler produces its response: keep the
    /// sender in a spawned task and return the receiver. A producer that waits
    /// on a timer or a status map, rather than on [`StreamSender::send`],
    /// should select on [`StreamSender::closed`] or [`crate::Request::cancelled`]:
    /// drain aborts on client RST so those resolve without another send.
    ///
    /// ```no_run
    /// use pbrs_grpc::{HelloReply, Response, Status, Streaming};
    ///
    /// # async fn handler() -> Result<Response<Streaming<HelloReply>>, Status> {
    /// let (tx, stream) = Streaming::channel(16);
    /// tokio::spawn(async move {
    ///     for i in 0..3 {
    ///         let mut reply = HelloReply::new();
    ///         reply.set_message(format!("tick {i}"));
    ///         if tx.send(reply).await.is_err() {
    ///             break;
    ///         }
    ///     }
    /// });
    /// Ok(Response::new(stream))
    /// # }
    /// ```
    #[must_use]
    pub fn channel(buffer: usize) -> (StreamSender<T>, Self) {
        let (tx, rx) = mpsc::channel(buffer.max(1));
        (
            StreamSender {
                tx,
                limits: MessageLimits::unlimited(),
                compress: false,
            },
            Self {
                source: Source::Channel(rx),
                lease: None,
                driver: None,
                reset: None,
            },
        )
    }

    /// An already-finished stream. Reading it yields `Ok(None)` at once.
    #[must_use]
    pub fn empty() -> Self {
        let (_, stream) = Self::channel(1);
        stream
    }

    pub(crate) fn from_wire(inner: crate::wire::WireStream<T>) -> Self {
        Self {
            source: Source::Wire(Box::new(inner)),
            lease: None,
            driver: None,
            reset: None,
        }
    }

    /// Keep the client HTTP/2 driver (and idle lease) alive while this stream
    /// is read, so dropping the [`crate::Channel`] after headers is safe.
    /// `reset` is the RPC cancel sender: drop before end-of-stream resets
    /// the call, even if a bidi [`StreamSender`] is still held.
    pub(crate) fn bind_conn(
        mut self,
        lease: Option<crate::keepalive::Lease>,
        driver: Option<watch::Sender<bool>>,
        reset: Option<watch::Sender<bool>>,
    ) -> Self {
        self.lease = lease;
        self.driver = driver;
        self.reset = reset;
        self
    }

    fn finished(&self) -> bool {
        match &self.source {
            Source::Channel(_) => true,
            Source::Wire(wire) => wire.finished(),
        }
    }

    /// The next message, `Ok(None)` at end of stream, `Err` on status.
    pub async fn message(&mut self) -> Result<Option<T>, Status> {
        Ok(self.next_framed().await?.map(Framed::into_inner))
    }

    /// [`Self::message`] keeping the Compressed-Flag.
    pub async fn next_framed(&mut self) -> Result<Option<Framed<T>>, Status> {
        poll_fn(|cx| self.poll_framed(cx)).await
    }

    fn poll_framed(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<Framed<T>>, Status>> {
        match &mut self.source {
            Source::Channel(rx) => match rx.poll_recv(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(Ok(None)),
                Poll::Ready(Some(Ok(framed))) => Poll::Ready(Ok(Some(framed))),
                Poll::Ready(Some(Err(status))) => Poll::Ready(Err(status)),
            },
            Source::Wire(wire) => wire.poll_next(cx),
        }
    }

    /// Collect the whole stream. Fails on the first error status.
    pub async fn collect(&mut self) -> Result<Vec<T>, Status> {
        let mut out = Vec::new();
        while let Some(msg) = self.message().await? {
            out.push(msg);
        }
        Ok(out)
    }

    /// Trailing metadata, after the stream has ended.
    ///
    /// On a received stream this waits for end-of-stream, discarding any
    /// unread messages, then returns the trailers that followed the last
    /// DATA frame. Call it before [`Self::message`] when you only need
    /// trailers; call it after a drain and it is cheap. A non-OK trailing
    /// `grpc-status` is `Err`, with the custom trailers on
    /// [`Status::metadata`](crate::Status::metadata) and
    /// `grpc-status-details-bin` on [`Status::error_details`](crate::Status::error_details)
    /// when the peer sent one.
    ///
    /// Application-produced streams ([`Self::channel`]) have no HTTP/2
    /// trailers: this returns empty metadata without consuming remaining
    /// messages.
    pub async fn trailers(&mut self) -> Result<Metadata, Status> {
        match &mut self.source {
            Source::Channel(_) => Ok(Metadata::new()),
            Source::Wire(wire) => {
                while wire.next().await?.is_some() {}
                Ok(wire.trailers().clone())
            }
        }
    }

    /// Wait for at least one message, then take up to `limit` in total.
    ///
    /// This is what lets the wire layer batch: a burst of small messages
    /// becomes one write instead of one per message. Returns the number taken,
    /// or zero at end of stream.
    pub(crate) async fn recv_many(&mut self, out: &mut Vec<Item<T>>, limit: usize) -> usize {
        match &mut self.source {
            Source::Channel(rx) => rx.recv_many(out, limit).await,
            Source::Wire(wire) => match wire.next().await.transpose() {
                None => 0,
                Some(item) => {
                    out.push(item);
                    1
                }
            },
        }
    }

    /// Take up to `limit` already-queued messages without waiting.
    ///
    /// Used to top up a batch after yielding to the producer. A wire-backed
    /// stream has nothing available for free, so it yields nothing.
    pub(crate) fn try_recv_many(&mut self, out: &mut Vec<Item<T>>, limit: usize) -> usize {
        let Source::Channel(rx) = &mut self.source else {
            return 0;
        };
        let mut taken = 0;
        while taken < limit {
            match rx.try_recv() {
                Ok(item) => {
                    out.push(item);
                    taken += 1;
                }
                Err(_) => break,
            }
        }
        taken
    }
}

impl<T> std::fmt::Debug for Streaming<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let source = match &self.source {
            Source::Channel(_) => "channel",
            Source::Wire(_) => "wire",
        };
        f.debug_struct("Streaming")
            .field("source", &source)
            .field("busy", &self.lease.is_some())
            .field("driver", &self.driver.is_some())
            .field("reset", &self.reset.is_some())
            .finish_non_exhaustive()
    }
}

impl<T> Drop for Streaming<T> {
    fn drop(&mut self) {
        if let Some(reset) = self.reset.take() {
            if !self.finished() {
                reset.send(true).ok();
            }
        }
    }
}

impl<T> Stream for Streaming<T> {
    type Item = Result<T, Status>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut().poll_framed(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(None)) => Poll::Ready(None),
            Poll::Ready(Ok(Some(framed))) => Poll::Ready(Some(Ok(framed.message))),
            Poll::Ready(Err(status)) => Poll::Ready(Some(Err(status))),
        }
    }
}

/// The write half of a message stream.
///
/// Dropping the last sender half-closes the stream cleanly; use
/// [`Self::fail`] to end it with an error status instead. [`Clone`] shares
/// the stream: it stays open until every clone is dropped.
/// [`Self::close`] consumes this handle only.
pub struct StreamSender<T> {
    tx: mpsc::Sender<Item<T>>,
    limits: MessageLimits,
    compress: bool,
}

/// [`Clone`] shares the stream. The stream stays open until every clone is
/// dropped. Compress intent is per handle: cloning then
/// [`StreamSender::set_compress`] does not change the original.
impl<T> Clone for StreamSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            limits: self.limits,
            compress: self.compress,
        }
    }
}

impl<T> StreamSender<T> {
    /// Enforce `limits` on every subsequent [`Self::send`].
    pub(crate) fn with_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    pub(crate) fn with_compress(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Whether [`Self::send`] will gzip.
    ///
    /// Set by [`crate::Channel::send_compressed`] when this sender was opened
    /// from a channel, by [`crate::Request::set_compress`] on that RPC, or by
    /// a client interceptor's [`crate::Outgoing::set_compress`]. Overlays and
    /// interceptors run before the sender is returned.
    #[must_use]
    pub fn compress(&self) -> bool {
        self.compress
    }

    /// gzip subsequent [`Self::send`] payloads.
    ///
    /// Does not change already-queued messages. [`Self::send_compressed`]
    /// still gzips a single message regardless of this flag.
    pub fn set_compress(&mut self, compress: bool) {
        self.compress = compress;
    }

    /// Queue one message, waiting if the buffer is full.
    ///
    /// Uncompressed unless this sender was built with channel-wide gzip
    /// ([`crate::ChannelConfig::send_compressed`]), the request called
    /// [`crate::Request::set_compress`], or a client interceptor set
    /// [`crate::Outgoing::set_compress`]. `Err` means the peer is
    /// gone or the message exceeds the outbound cap.
    pub async fn send(&self, message: T) -> Result<(), Status>
    where
        T: pbrs::Serialize,
    {
        if self.compress {
            self.send_framed(Framed::compressed(message)).await
        } else {
            self.send_framed(Framed::new(message)).await
        }
    }

    /// Queue one gzip-compressed message (Compressed-Flag 1).
    pub async fn send_compressed(&self, message: T) -> Result<(), Status>
    where
        T: pbrs::Serialize,
    {
        self.send_framed(Framed::compressed(message)).await
    }

    /// Queue a message with an explicit Compressed-Flag.
    pub async fn send_framed(&self, item: Framed<T>) -> Result<(), Status>
    where
        T: pbrs::Serialize,
    {
        self.limits.check_encode(T::serialized_len(&item.message))?;
        self.tx
            .send(Ok(item))
            .await
            .map_err(|_| Status::cancelled())
    }

    /// End the stream with an error status instead of a clean half-close.
    ///
    /// On a **server response** producer, trailing metadata and
    /// `grpc-status-details-bin` (see [`crate::Status::with_error_details`])
    /// both ship after any messages already sent, the same as a handler
    /// `Err`.
    ///
    /// On a **client request** sender (client-streaming or bidi), gRPC has no
    /// request-side `grpc-status`. This resets the HTTP/2 stream with CANCEL,
    /// matching [`crate::CallHandle::cancel`]. A client-streaming
    /// [`crate::Call`], or a bidi [`crate::Call`] that has not yet seen
    /// headers, resolves with `status`; a bidi call that already has
    /// headers surfaces the reset on the received [`Streaming`].
    pub async fn fail(self, status: Status) {
        self.tx.send(Err(status)).await.ok();
    }

    /// Half-close this handle. Equivalent to dropping it.
    ///
    /// The peer sees end-of-stream and may answer `OK` (an empty
    /// client-stream is a successful empty aggregate) once every clone is
    /// gone. If this sender was cloned, other clones keep the stream open.
    /// To abort, keep a handle and cancel the [`crate::Call`].
    pub fn close(self) {
        drop(self.tx);
    }

    /// Whether the reader has gone away, so further sends would fail.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// Resolves when the reader has gone away.
    ///
    /// Same condition as [`Self::is_closed`], as a future. A producer that
    /// waits on something else (a status map, a timer) should select on this
    /// so it does not sit after the client has cancelled or dropped the stream.
    pub async fn closed(&self) {
        self.tx.closed().await;
    }
}

impl<T> std::fmt::Debug for StreamSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamSender")
            .field("closed", &self.tx.is_closed())
            .field("limits", &self.limits)
            .field("compress", &self.compress)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{Framed, Streaming};
    use crate::hello::HelloReply;
    use crate::status::{Code, Status};

    fn reply(message: &str) -> HelloReply {
        let mut r = HelloReply::new();
        r.set_message(message);
        r
    }

    fn text(reply: &HelloReply) -> String {
        reply.message().to_str().unwrap_or_default().to_owned()
    }

    #[tokio::test]
    async fn channel_round_trips_messages() {
        let (tx, mut stream) = Streaming::<HelloReply>::channel(4);
        let shown_stream = format!("{stream:?}");
        assert!(shown_stream.contains("driver: false"), "{shown_stream}");
        tx.send(reply("one")).await.expect("send");
        tx.send_compressed(reply("two")).await.expect("send");
        assert!(!tx.compress());
        let mut gzip = tx.clone();
        gzip.set_compress(true);
        assert!(gzip.compress());
        assert!(!tx.compress());
        drop(gzip);
        let shown = format!("{tx:?}");
        assert!(shown.contains("compress: false"), "{shown}");
        tx.close();
        let first = stream.message().await.expect("recv").expect("item");
        assert_eq!(text(&first), "one");
        let second = stream.next_framed().await.expect("recv").expect("item");
        assert!(second.compressed);
        assert_eq!(text(&second.message), "two");
        assert!(stream.message().await.expect("end").is_none());
    }

    #[tokio::test]
    async fn streaming_is_a_futures_stream() {
        use futures_core::Stream;
        use std::future::poll_fn;
        use std::pin::Pin;

        let (tx, mut stream) = Streaming::<HelloReply>::channel(4);
        tx.send(reply("one")).await.expect("send");
        tx.close();
        let first = poll_fn(|cx| Pin::new(&mut stream).poll_next(cx))
            .await
            .expect("item")
            .expect("ok");
        assert_eq!(text(&first), "one");
        assert!(poll_fn(|cx| Pin::new(&mut stream).poll_next(cx))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn buffer_is_never_zero() {
        let (tx, mut stream) = Streaming::<HelloReply>::channel(0);
        tx.send(reply("nine")).await.expect("send");
        tx.close();
        let got = stream.message().await.expect("recv").expect("item");
        assert_eq!(text(&got), "nine");
    }

    #[tokio::test]
    async fn fail_surfaces_status() {
        let (tx, mut stream) = Streaming::<HelloReply>::channel(1);
        tx.fail(Status::not_found("gone")).await;
        let err = stream.message().await.expect_err("status");
        assert_eq!(err.code(), Code::NotFound);
    }

    #[tokio::test]
    async fn empty_stream_ends_immediately() {
        let mut stream = Streaming::<HelloReply>::empty();
        assert!(stream.message().await.expect("end").is_none());
        assert!(stream.trailers().await.expect("trailers").is_empty());
    }

    #[tokio::test]
    async fn collect_gathers_all() {
        let (tx, mut stream) = Streaming::<HelloReply>::channel(4);
        for name in ["a", "b", "c"] {
            tx.send(reply(name)).await.expect("send");
        }
        tx.close();
        let got: Vec<String> = stream
            .collect()
            .await
            .expect("collect")
            .iter()
            .map(text)
            .collect();
        assert_eq!(got, ["a", "b", "c"]);
    }

    #[tokio::test]
    async fn dropping_the_reader_closes_the_sender() {
        let (tx, stream) = Streaming::<HelloReply>::channel(1);
        assert!(!tx.is_closed());
        drop(stream);
        assert!(tx.is_closed());
        tx.closed().await;
        assert!(tx.send(reply("late")).await.is_err());
        assert!(tx.is_closed());
    }

    #[tokio::test]
    async fn a_clone_keeps_the_stream_open() {
        let (tx, mut stream) = Streaming::<HelloReply>::channel(4);
        let extra = tx.clone();
        tx.close();
        extra
            .send(reply("still"))
            .await
            .expect("clone keeps it open");
        extra.close();
        let got = stream.message().await.expect("recv").expect("item");
        assert_eq!(text(&got), "still");
        assert!(stream.message().await.expect("end").is_none());
    }

    #[test]
    fn framed_constructors_set_the_flag() {
        assert!(!Framed::new(1u32).compressed);
        assert!(Framed::compressed(1u32).compressed);
        assert_eq!(Framed::new(7u32).into_inner(), 7);
    }
}
