//! Batched stream output: OutBatch.

use super::encode::{STREAM_BATCH_BYTES, append_frame};
use super::send::send_bytes;
use crate::binlog::CallLogger;
use crate::config::Wire;
use crate::status::Status;
use crate::stream::Framed;
use bytes::{Bytes, BytesMut};
use h2::SendStream;
use pbrs::Serialize;

/// Accumulates encoded stream output and hands it to HTTP/2 in batches.
pub(crate) struct OutBatch {
    buf: BytesMut,
    wire: Wire,
    tap: Option<CallLogger>,
}

impl OutBatch {
    /// Most messages to take from a producer in one go.
    ///
    /// Bounded so a fast producer cannot make one batch unboundedly large; the
    /// byte threshold usually fires first.
    pub(crate) const BURST: usize = 64;

    pub(crate) fn new(wire: Wire) -> Self {
        Self {
            buf: BytesMut::new(),
            wire,
            tap: None,
        }
    }

    /// Log every encoded message to `tap` as it joins the batch.
    pub(crate) fn set_tap(&mut self, tap: CallLogger) {
        self.tap = Some(tap);
    }

    /// Encode one message into the batch without writing.
    ///
    /// Encode-cap and serialize failures stay [`Status`] so a server drain
    /// can ship them as trailers instead of treating them as a dead socket.
    pub(crate) fn encode<T: Serialize>(&mut self, item: Framed<T>) -> Result<(), Status> {
        let prior_len = self.buf.len();
        if let Err(status) = append_frame(
            &mut self.buf,
            &item.message,
            item.compressed.then_some(self.wire.send_codec),
            self.wire.limits,
            self.wire.gzip_level,
        ) {
            // Keep earlier complete frames flushable before the error trailer.
            self.buf.truncate(prior_len);
            return Err(status);
        }
        if let Some(tap) = &self.tap {
            tap.log_written(self.buf.get(prior_len..).unwrap_or_default());
        }
        Ok(())
    }

    /// Whether the batch has reached the size worth writing on its own.
    pub(crate) fn is_full(&self) -> bool {
        self.buf.len() >= STREAM_BATCH_BYTES
    }

    /// Hand whatever has accumulated to HTTP/2.
    pub(crate) async fn flush(&mut self, send: &mut SendStream<Bytes>) -> Result<(), Status> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let frame = std::mem::take(&mut self.buf).freeze();
        send_bytes(send, frame, false, self.wire.send_buffer).await
    }
}

/// Give a producer one scheduling turn to refill the queue before writing.
///
/// A producer running ahead of the network is bounded by its channel depth, so
/// draining it yields only that many messages and the write is smaller than it
/// could be. One `yield_now` lets it top the queue up, which halves the writes
/// and the task wakeups for a bulk stream. It costs one re-queue on the same
/// worker when the producer has nothing more, so an interactive stream pays a
/// scheduler turn rather than a timer.
pub(crate) async fn let_producer_catch_up() {
    tokio::task::yield_now().await;
}
