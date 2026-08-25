//! Inbound message stream and client streaming sender.

use crate::status::Status;
use tokio::sync::mpsc;

/// One inbound data frame after parse.
pub struct InItem<T> {
    /// Protobuf message.
    pub message: T,
    /// Compressed-Flag on the wire (after gzip decode of the payload).
    pub compressed: bool,
}

/// One outbound data frame.
pub struct OutItem<T> {
    /// Protobuf message.
    pub message: T,
    /// Set Compressed-Flag and gzip the payload.
    pub compress: bool,
}

/// Sequence of decoded gRPC messages (client-stream inbound or server-stream outbound).
pub struct Inbound<T> {
    rx: mpsc::Receiver<Result<InItem<T>, Status>>,
    trailers: Option<tokio::sync::oneshot::Receiver<crate::metadata::Metadata>>,
}

impl<T: Send> Inbound<T> {
    /// Bounded channel plus the inbound half.
    #[must_use]
    pub fn channel(buffer: usize) -> (mpsc::Sender<Result<InItem<T>, Status>>, Self) {
        let (tx, rx) = mpsc::channel(buffer);
        (tx, Self { rx, trailers: None })
    }

    pub(crate) fn set_trailers(
        &mut self,
        rx: tokio::sync::oneshot::Receiver<crate::metadata::Metadata>,
    ) {
        self.trailers = Some(rx);
    }

    /// Trailing metadata after the stream ends.
    pub async fn trailers(&mut self) -> crate::metadata::Metadata {
        match self.trailers.take() {
            Some(rx) => rx.await.unwrap_or_default(),
            None => crate::metadata::Metadata::new(),
        }
    }

    /// Next message, `Ok(None)` on half-close, `Err` on status.
    pub async fn message(&mut self) -> Result<Option<T>, Status> {
        Ok(self.next_item().await?.map(|i| i.message))
    }

    /// Next message plus Compressed-Flag.
    pub async fn next_item(&mut self) -> Result<Option<InItem<T>>, Status> {
        match self.rx.recv().await {
            None => Ok(None),
            Some(Ok(v)) => Ok(Some(v)),
            Some(Err(e)) => Err(e),
        }
    }
}

/// Client half of a client-stream or bidi call.
pub struct StreamingSender<T> {
    tx: mpsc::Sender<Result<OutItem<T>, Status>>,
}

impl<T: Send> StreamingSender<T> {
    pub(crate) fn new(tx: mpsc::Sender<Result<OutItem<T>, Status>>) -> Self {
        Self { tx }
    }

    /// Queue one uncompressed message.
    pub async fn send(&self, msg: T) -> Result<(), Status> {
        self.send_item(OutItem {
            message: msg,
            compress: false,
        })
        .await
    }

    /// Queue one gzip-compressed message (Compressed-Flag 1).
    pub async fn send_compressed(&self, msg: T) -> Result<(), Status> {
        self.send_item(OutItem {
            message: msg,
            compress: true,
        })
        .await
    }

    async fn send_item(&self, item: OutItem<T>) -> Result<(), Status> {
        self.tx
            .send(Ok(item))
            .await
            .map_err(|_| Status::cancelled())
    }

    /// Half-close the send side.
    pub fn close(self) {
        drop(self.tx);
    }
}
