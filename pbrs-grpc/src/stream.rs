//! Message streams: [`Streaming`] to read, [`StreamSender`] to write.
//!
//! Both halves are used on both sides of an RPC. A client-streaming call
//! hands you a [`StreamSender`] for requests; a server-streaming handler
//! returns a [`Streaming`] of responses. The pair is created with
//! [`Streaming::channel`].

use crate::limits::MessageLimits;
use crate::metadata::Metadata;
use crate::status::Status;
use tokio::sync::mpsc;

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

/// A sequence of decoded gRPC messages.
///
/// Reading is the only way to advance the stream, so a handler that stops
/// reading applies backpressure all the way to the peer's HTTP/2 window.
///
/// ```no_run
/// # use pbrs_grpc::{HelloReply, Status, Streaming};
/// # async fn demo(mut stream: Streaming<HelloReply>) -> Result<(), Status> {
/// while let Some(reply) = stream.message().await? {
///     println!("{}", reply.message());
/// }
/// let trailers = stream.trailers().await;
/// # let _ = trailers;
/// # Ok(())
/// # }
/// ```
pub struct Streaming<T> {
    rx: mpsc::Receiver<Item<T>>,
    trailers: Option<tokio::sync::oneshot::Receiver<Metadata>>,
}

impl<T> Streaming<T> {
    /// A connected [`StreamSender`] / [`Streaming`] pair holding `buffer`
    /// messages in flight.
    ///
    /// This is how a server-streaming handler produces its response: keep the
    /// sender in a spawned task and return the receiver.
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
            },
            Self { rx, trailers: None },
        )
    }

    /// An already-finished stream. Reading it yields `Ok(None)` at once.
    #[must_use]
    pub fn empty() -> Self {
        let (_, stream) = Self::channel(1);
        stream
    }

    /// The next message, `Ok(None)` at end of stream, `Err` on status.
    pub async fn message(&mut self) -> Result<Option<T>, Status> {
        Ok(self.next_framed().await?.map(Framed::into_inner))
    }

    /// [`Self::message`] keeping the Compressed-Flag.
    pub async fn next_framed(&mut self) -> Result<Option<Framed<T>>, Status> {
        match self.rx.recv().await {
            None => Ok(None),
            Some(Ok(item)) => Ok(Some(item)),
            Some(Err(status)) => Err(status),
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

    /// Trailing metadata, available once the stream has ended.
    ///
    /// Returns empty metadata if the stream is still open or carried none.
    pub async fn trailers(&mut self) -> Metadata {
        match self.trailers.take() {
            Some(rx) => rx.await.unwrap_or_default(),
            None => Metadata::new(),
        }
    }

    pub(crate) fn set_trailers(&mut self, rx: tokio::sync::oneshot::Receiver<Metadata>) {
        self.trailers = Some(rx);
    }

    pub(crate) async fn recv(&mut self) -> Option<Item<T>> {
        self.rx.recv().await
    }
}

/// The write half of a message stream.
///
/// Dropping the sender half-closes the stream cleanly; use
/// [`Self::fail`] to end it with an error status instead.
pub struct StreamSender<T> {
    tx: mpsc::Sender<Item<T>>,
    limits: MessageLimits,
}

impl<T> Clone for StreamSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            limits: self.limits,
        }
    }
}

impl<T> StreamSender<T> {
    /// Enforce `limits` on every subsequent [`Self::send`].
    pub(crate) fn with_limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Queue one uncompressed message, waiting if the buffer is full.
    ///
    /// `Err` means the peer is gone or the message exceeds the outbound cap.
    pub async fn send(&self, message: T) -> Result<(), Status>
    where
        T: pbrs::Serialize,
    {
        self.send_framed(Framed::new(message)).await
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
    pub async fn fail(self, status: Status) {
        self.tx.send(Err(status)).await.ok();
    }

    /// Half-close the stream. Equivalent to dropping the sender.
    pub fn close(self) {
        drop(self.tx);
    }

    /// Whether the reader has gone away, so further sends would fail.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    /// Hand on a message decoded off the wire. `false` means the reader is
    /// gone, so the pump should stop.
    ///
    /// Skips the outbound size check: an inbound message was already measured
    /// against the inbound cap before it was parsed.
    pub(crate) async fn send_decoded(&self, item: Framed<T>) -> bool {
        self.tx.send(Ok(item)).await.is_ok()
    }

    pub(crate) async fn send_status(&self, status: Status) {
        self.tx.send(Err(status)).await.ok();
    }
}

#[cfg(test)]
mod tests {
    use super::{Framed, Streaming};
    use crate::status::{Code, Status};

    #[tokio::test]
    async fn channel_round_trips_messages() {
        let (tx, mut stream) = Streaming::<u32>::channel(4);
        assert!(tx.send_decoded(Framed::new(1)).await);
        assert!(tx.send_decoded(Framed::compressed(2)).await);
        tx.close();
        assert_eq!(stream.message().await.expect("recv"), Some(1));
        let second = stream.next_framed().await.expect("recv").expect("item");
        assert!(second.compressed);
        assert_eq!(second.message, 2);
        assert_eq!(stream.message().await.expect("end"), None);
    }

    #[tokio::test]
    async fn buffer_is_never_zero() {
        let (tx, mut stream) = Streaming::<u32>::channel(0);
        assert!(tx.send_decoded(Framed::new(9)).await);
        tx.close();
        assert_eq!(stream.message().await.expect("recv"), Some(9));
    }

    #[tokio::test]
    async fn fail_surfaces_status() {
        let (tx, mut stream) = Streaming::<u32>::channel(1);
        tx.fail(Status::not_found("gone")).await;
        let err = stream.message().await.expect_err("status");
        assert_eq!(err.code(), Code::NotFound);
    }

    #[tokio::test]
    async fn empty_stream_ends_immediately() {
        let mut stream = Streaming::<u32>::empty();
        assert_eq!(stream.message().await.expect("end"), None);
        assert!(stream.trailers().await.is_empty());
    }

    #[tokio::test]
    async fn collect_gathers_all() {
        let (tx, mut stream) = Streaming::<u32>::channel(4);
        for i in 0..3 {
            assert!(tx.send_decoded(Framed::new(i)).await);
        }
        tx.close();
        assert_eq!(stream.collect().await.expect("collect"), vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn dropping_the_reader_closes_the_sender() {
        let (tx, stream) = Streaming::<u32>::channel(1);
        assert!(!tx.is_closed());
        drop(stream);
        assert!(!tx.send_decoded(Framed::new(1)).await);
        assert!(tx.is_closed());
    }
}
