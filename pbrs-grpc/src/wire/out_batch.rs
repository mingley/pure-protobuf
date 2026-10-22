//! Batched stream output: OutBatch.

use super::encode::{EncodeScratch, STREAM_BATCH_BYTES, SegSink, append_frame};
use super::send::{ImmediateSend, send_frame, send_frame_if_ready};
use crate::binlog::CallLogger;
use crate::codec::CodecMessage;
use crate::config::Wire;
use crate::status::Status;
use crate::stream::Framed;
use crate::transport::h2::SendStream;

/// Accumulates encoded stream output and hands it to HTTP/2 in batches.
pub(crate) struct OutBatch {
    sink: SegSink,
    scratch: EncodeScratch,
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
            sink: SegSink::new(),
            scratch: EncodeScratch::new(),
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
    pub(crate) fn encode<T: CodecMessage>(&mut self, item: Framed<T>) -> Result<(), Status> {
        let checkpoint = self.sink.checkpoint();
        if let Err(status) = append_frame(
            &mut self.sink,
            &mut self.scratch,
            &item.message,
            item.compressed.then_some(self.wire.send_codec),
            self.wire.limits,
            self.wire.gzip_level,
        ) {
            // Keep earlier complete frames flushable before the error trailer.
            self.sink.rollback(&checkpoint);
            return Err(status);
        }
        if let Some(tap) = &self.tap {
            let (segs, tail) = self.sink.segments_since(&checkpoint);
            for seg in segs {
                tap.log_written(seg);
            }
            if !tail.is_empty() {
                tap.log_written(tail);
            }
        }
        Ok(())
    }

    /// Whether the batch has reached the size worth writing on its own.
    pub(crate) fn is_full(&self) -> bool {
        self.sink.len() >= STREAM_BATCH_BYTES
    }

    /// Take the accumulated frame, leaving the batch empty.
    /// `None` when there is nothing to write.
    pub(crate) fn take_frame(&mut self) -> Option<super::encode::SegFrame> {
        if self.sink.is_empty() {
            return None;
        }
        let sink = std::mem::replace(&mut self.sink, SegSink::new());
        Some(sink.finish())
    }

    /// Hand whatever has accumulated to HTTP/2.
    pub(crate) async fn flush(&mut self, send: &mut SendStream) -> Result<(), Status> {
        if self.sink.is_empty() {
            return Ok(());
        }
        let sink = std::mem::replace(&mut self.sink, SegSink::new());
        let frame = sink.finish();
        match send_frame_if_ready(send, frame, false, self.wire.send_buffer) {
            ImmediateSend::Done(result) => result,
            ImmediateSend::Blocked(frame) => {
                send_frame(send, frame, false, self.wire.send_buffer).await
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChannelConfig, Code};
    use bytes::Bytes;

    struct PartialEncode(bool);

    impl CodecMessage for PartialEncode {
        fn encoded_len(&self) -> usize {
            1
        }

        fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
            out.put_u8(0xAB);
            if self.0 {
                return Err(Status::internal("partial encode"));
            }
            Ok(())
        }

        fn decode_payload(_: Bytes) -> Result<Self, Status> {
            Ok(Self(false))
        }

        fn empty() -> Self {
            Self(false)
        }
    }

    struct SharedEncode {
        shared: bool,
        fail: bool,
    }

    impl CodecMessage for SharedEncode {
        fn encoded_len(&self) -> usize {
            if self.shared { 34 } else { 8 }
        }

        fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
            out.put_slice(b"start");
            if self.shared {
                out.put_shared(&Bytes::from_static(b"shared-one"));
                out.put_slice(b"middle");
                out.put_shared(&Bytes::from_static(b"shared-two"));
            }
            out.put_slice(b"end");
            if self.fail {
                return Err(Status::internal("partial shared encode"));
            }
            Ok(())
        }

        fn decode_payload(_: Bytes) -> Result<Self, Status> {
            Ok(Self::empty())
        }

        fn empty() -> Self {
            Self {
                shared: false,
                fail: false,
            }
        }
    }

    #[test]
    fn failed_shared_encode_preserves_complete_frames_and_allows_a_later_frame() {
        for prior_shared in [false, true] {
            let wire = ChannelConfig::new().wire();
            let mut batch = OutBatch::new(wire);
            batch
                .encode(Framed::new(SharedEncode::empty()))
                .expect("first complete frame");
            batch
                .encode(Framed::new(SharedEncode {
                    shared: prior_shared,
                    fail: false,
                }))
                .expect("second complete frame");
            let error = batch
                .encode(Framed::new(SharedEncode {
                    shared: true,
                    fail: true,
                }))
                .expect_err("failed message after multiple shared writes");
            assert_eq!(error.code(), Code::Internal);
            batch
                .encode(Framed::new(SharedEncode::empty()))
                .expect("complete frame after rollback");
            let mut bytes = Bytes::from(batch.take_frame().expect("three frames").concat());
            let second = if prior_shared {
                b"startshared-onemiddleshared-twoend".as_slice()
            } else {
                b"startend".as_slice()
            };
            for expected in [b"startend".as_slice(), second, b"startend".as_slice()] {
                let frame = crate::codec::pop_from_chunk(&mut bytes, wire.limits)
                    .expect("valid framing")
                    .expect("complete message");
                assert!(!frame.compressed);
                assert_eq!(frame.payload.as_ref(), expected);
            }
            assert!(bytes.is_empty(), "failed message leaves no frame bytes");
        }
    }

    #[test]
    fn failed_compressed_encode_preserves_prior_frames_and_reuses_scratch_safely() {
        let wire = ChannelConfig::new().wire();
        let mut batch = OutBatch::new(wire);
        batch
            .encode(Framed::compressed(PartialEncode(false)))
            .expect("first frame");
        let status = batch
            .encode(Framed::compressed(PartialEncode(true)))
            .expect_err("partial encode must fail");
        assert_eq!(status.code(), Code::Internal);
        batch
            .encode(Framed::compressed(PartialEncode(false)))
            .expect("frame after failure");
        let frame = batch.take_frame().expect("two complete frames");
        let mut bytes = Bytes::from(frame.concat());
        for _ in 0..2 {
            let frame = crate::codec::pop_from_chunk(&mut bytes, wire.limits)
                .expect("valid framing")
                .expect("complete message");
            assert!(frame.compressed);
            assert_eq!(
                wire.send_codec.decode(&frame.payload).expect("decompress"),
                [0xAB]
            );
        }
        assert!(bytes.is_empty(), "failed message must leave no frame bytes");
        // A flush gives HTTP/2 ownership of the previous output buffer.
        batch
            .encode(Framed::compressed(PartialEncode(false)))
            .expect("frame after flush");
        let next = batch.take_frame().expect("next batch");
        let mut bytes = Bytes::from(next.concat());
        let frame = crate::codec::pop_from_chunk(&mut bytes, wire.limits)
            .expect("valid next framing")
            .expect("next message");
        assert_eq!(
            wire.send_codec
                .decode(&frame.payload)
                .expect("decompress next"),
            [0xAB]
        );
        assert!(bytes.is_empty());
    }
}
