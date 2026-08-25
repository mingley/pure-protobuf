//! Inbound message stream and client streaming sender.

use crate::status::Status;
use tokio::sync::mpsc;

/// Sequence of decoded gRPC messages (client-stream inbound or server-stream outbound).
pub struct Inbound<T> {
    rx: mpsc::Receiver<Result<T, Status>>,
}

impl<T: Send> Inbound<T> {
    /// Bounded channel plus the inbound half.
    #[must_use]
    pub fn channel(buffer: usize) -> (mpsc::Sender<Result<T, Status>>, Self) {
        let (tx, rx) = mpsc::channel(buffer);
        (tx, Self { rx })
    }

    /// Next message, `Ok(None)` on half-close, `Err` on status.
    pub async fn message(&mut self) -> Result<Option<T>, Status> {
        match self.rx.recv().await {
            None => Ok(None),
            Some(Ok(v)) => Ok(Some(v)),
            Some(Err(e)) => Err(e),
        }
    }
}

/// Client half of a client-stream or bidi call.
pub struct StreamingSender<T> {
    tx: mpsc::Sender<Result<T, Status>>,
}

impl<T: Send> StreamingSender<T> {
    pub(crate) fn new(tx: mpsc::Sender<Result<T, Status>>) -> Self {
        Self { tx }
    }

    /// Queue one message.
    pub async fn send(&self, msg: T) -> Result<(), Status> {
        self.tx.send(Ok(msg)).await.map_err(|_| Status::cancelled())
    }

    /// Half-close the send side.
    pub fn close(self) {
        drop(self.tx);
    }
}
