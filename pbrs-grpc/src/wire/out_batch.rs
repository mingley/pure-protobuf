//! Batched stream output: OutBatch.

use super::encode::{
    EncodeScratch, STREAM_BATCH_BYTES, SegSink, SinkCheckpoint, append_frame_with_ready,
};
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
    compressed_only: bool,
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
            compressed_only: true,
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
    pub(crate) fn encode_ready<T: CodecMessage>(
        &mut self,
        item: Framed<T>,
        ready_messages: usize,
    ) -> Result<(), Status> {
        // A failed identity encoder can replace the backing while preserving
        // checkpoint bytes. Invalidation therefore precedes all user code and
        // deliberately survives rollback.
        if !item.compressed {
            self.compressed_only = false;
        }
        let mut guard = SinkEncodeGuard::new(&mut self.sink, &mut self.scratch);
        guard.sink.prepare_encode();
        append_frame_with_ready(
            &mut guard.sink,
            guard.scratch,
            &item.message,
            item.compressed.then_some(self.wire.send_codec),
            self.wire.limits,
            self.wire.gzip_level,
            ready_messages,
        )?;
        if let Some(tap) = &self.tap {
            let (segs, tail) = guard.sink.segments_since(&guard.checkpoint);
            for seg in segs {
                tap.log_written(seg);
            }
            if !tail.is_empty() {
                tap.log_written(tail);
            }
        }
        guard.commit();
        Ok(())
    }

    #[cfg(test)]
    fn encode<T: CodecMessage>(&mut self, item: Framed<T>) -> Result<(), Status> {
        self.encode_ready(item, 4)
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
        Some(
            self.sink
                .take_reusing(std::mem::replace(&mut self.compressed_only, true)),
        )
    }

    /// Hand whatever has accumulated to HTTP/2.
    pub(crate) async fn flush(&mut self, send: &mut SendStream) -> Result<(), Status> {
        if self.sink.is_empty() {
            return Ok(());
        }
        let frame = self
            .sink
            .take_reusing(std::mem::replace(&mut self.compressed_only, true));
        match send_frame_if_ready(send, frame, false, self.wire.send_buffer) {
            ImmediateSend::Done(result) => result,
            ImmediateSend::Blocked(frame) => {
                send_frame(send, frame, false, self.wire.send_buffer).await
            }
        }
    }
}

// Move both mutable ownership and panic cleanup ahead of encoded_len,
// configured limit checks, codecs and compression. Only committed output
// returns normally; errors/unwind preserve prior complete frames, while an
// empty uncharged tail cannot survive in the reusable OutBatch.
struct SinkEncodeGuard<'a> {
    target: &'a mut SegSink,
    sink: SegSink,
    scratch: &'a mut EncodeScratch,
    checkpoint: SinkCheckpoint,
    committed: bool,
}

impl<'a> SinkEncodeGuard<'a> {
    fn new(target: &'a mut SegSink, scratch: &'a mut EncodeScratch) -> Self {
        let sink = std::mem::replace(target, SegSink::new());
        let checkpoint = sink.checkpoint();
        Self {
            target,
            sink,
            scratch,
            checkpoint,
            committed: false,
        }
    }

    fn commit(mut self) {
        *self.target = std::mem::replace(&mut self.sink, SegSink::new());
        self.committed = true;
    }
}

impl Drop for SinkEncodeGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.scratch.discard();
            self.sink.rollback(&self.checkpoint);
            if !self.sink.is_empty() {
                *self.target = std::mem::replace(&mut self.sink, SegSink::new());
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
            out.put_slice(if self.fail { b"bad" } else { b"end" });
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
    fn single_ready_rollback_and_reuse_preserve_plain_shared_and_compressed_frames() {
        for compressed in [false, true] {
            let wire = ChannelConfig::new().wire();
            let mut batch = OutBatch::new(wire);
            for _ in 0..2 {
                let framed = |fail| Framed {
                    message: PartialEncode(fail),
                    compressed,
                };
                batch.encode_ready(framed(false), 1).expect("prefix");
                assert_eq!(
                    batch
                        .encode_ready(framed(true), 1)
                        .expect_err("partial")
                        .code(),
                    Code::Internal
                );
                batch.encode_ready(framed(false), 1).expect("after error");
                let mut bytes = Bytes::from(batch.take_frame().expect("two frames").concat());
                for _ in 0..2 {
                    let frame = crate::codec::pop_from_chunk(&mut bytes, wire.limits)
                        .expect("framing")
                        .expect("complete frame");
                    assert_eq!(frame.compressed, compressed);
                    let payload = if compressed {
                        wire.send_codec.decode(&frame.payload).expect("decode")
                    } else {
                        frame.payload.to_vec()
                    };
                    assert_eq!(payload, [0xAB]);
                }
                assert!(bytes.is_empty());
            }
        }
        let wire = ChannelConfig::new().wire();
        let mut batch = OutBatch::new(wire);
        for _ in 0..2 {
            batch
                .encode_ready(Framed::new(SharedEncode::empty()), 1)
                .expect("complete prefix");
            batch
                .encode_ready(
                    Framed::new(SharedEncode {
                        shared: true,
                        fail: true,
                    }),
                    1,
                )
                .expect_err("partial shared encode");
            batch
                .encode_ready(
                    Framed::new(SharedEncode {
                        shared: true,
                        fail: false,
                    }),
                    1,
                )
                .expect("after shared rollback");
            let mut bytes = Bytes::from(batch.take_frame().expect("two frames").concat());
            for expected in [
                b"startend".as_slice(),
                b"startshared-onemiddleshared-twoend".as_slice(),
            ] {
                let frame = crate::codec::pop_from_chunk(&mut bytes, wire.limits)
                    .expect("framing")
                    .expect("complete frame");
                assert_eq!(frame.payload.as_ref(), expected);
            }
            assert!(bytes.is_empty());
        }
    }

    #[test]
    fn ready_bursts_keep_frame_order_across_batch_flush_boundaries() {
        for count in [1, 2, 3, 4, 64] {
            for compressed in [false, true] {
                let wire = ChannelConfig::new().wire();
                let mut batch = OutBatch::new(wire);
                let mut actual = Vec::new();
                let mut expected = Vec::new();
                for index in 0..count {
                    let mut message = crate::testing::Payload::new();
                    let size = match index % 4 {
                        0 => 1024,
                        1 => 8192,
                        2 => 32768,
                        _ => 65536,
                    };
                    message.set_body(vec![0xAB; size]);
                    expected.extend(
                        crate::wire::encode::encode_msg(
                            &message,
                            compressed.then_some(wire.send_codec),
                            wire.limits,
                            wire.gzip_level,
                        )
                        .expect("independent frame")
                        .concat(),
                    );
                    batch
                        .encode_ready(
                            Framed {
                                message,
                                compressed,
                            },
                            count - index,
                        )
                        .expect("ready frame");
                    if batch.is_full() {
                        actual.extend(batch.take_frame().expect("full frame").concat());
                    }
                }
                if let Some(frame) = batch.take_frame() {
                    actual.extend(frame.concat());
                }
                assert_eq!(actual, expected);
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

    struct NoiseOutput {
        mode: u8,
        bytes: Vec<u8>,
    }
    impl NoiseOutput {
        fn new(mode: u8) -> Self {
            let mut state = 0x67ac_4fe2u32;
            let bytes = (0..131073)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    u8::try_from(state & 0xff).expect("byte")
                })
                .collect();
            Self { mode, bytes }
        }
    }
    impl CodecMessage for NoiseOutput {
        const ENCODE_APPEND_ONLY: bool = true;
        fn encoded_len(&self) -> usize {
            if self.mode == 1 {
                panic!("intentional encoded_len failure")
            }
            if self.mode == 2 {
                usize::MAX
            } else {
                self.bytes.len()
            }
        }
        fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
            out.put_slice(&self.bytes);
            if self.mode == 3 {
                return Err(Status::internal("intentional codec error"));
            }
            if self.mode == 4 {
                panic!("intentional codec panic")
            }
            Ok(())
        }
        fn decode_payload(_: Bytes) -> Result<Self, Status> {
            Ok(Self::empty())
        }
        fn empty() -> Self {
            Self {
                mode: 0,
                bytes: Vec::new(),
            }
        }
    }

    fn warmed_output() -> (OutBatch, super::super::encode::SegFrame, Vec<u8>) {
        let mut batch = OutBatch::new(ChannelConfig::new().wire());
        batch
            .encode_ready(Framed::compressed(NoiseOutput::new(0)), 1)
            .expect("warm output");
        let first = batch.take_frame().expect("complete warm frame");
        let saved = first.clone().concat();
        assert!(
            batch
                .sink
                .output_cache_state()
                .1
                .expect("charged warm backing")
                > 65536
        );
        (batch, first, saved)
    }

    #[test]
    fn warmed_output_errors_and_caught_panics_drop_empty_uncharged_storage() {
        for mode in [1, 2, 3, 4] {
            let (mut batch, first, saved) = warmed_output();
            batch.wire.limits = batch.wire.limits.with_max_encoding(1024 * 1024);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                batch.encode_ready(Framed::compressed(NoiseOutput::new(mode)), 1)
            }));
            if mode == 1 || mode == 4 {
                assert!(result.is_err());
            } else {
                assert!(result.expect("error rather than panic").is_err());
            }
            assert!(batch.sink.is_empty());
            assert_eq!(batch.sink.output_cache_state(), (0, None, false));
            assert_eq!(first.clone().concat(), saved);
            batch
                .encode_ready(Framed::compressed(PartialEncode(false)), 1)
                .expect("reuse after failure");
            let mut frame = Bytes::from(batch.take_frame().expect("later frame").concat());
            let frame = crate::codec::pop_from_chunk(&mut frame, batch.wire.limits)
                .expect("frame")
                .expect("one");
            assert_eq!(
                batch
                    .wire
                    .send_codec
                    .decode(&frame.payload)
                    .expect("valid compression"),
                [0xAB]
            );
        }
    }

    #[test]
    fn caught_codec_panic_restores_prior_complete_output() {
        let (mut batch, first, saved) = warmed_output();
        batch
            .encode_ready(Framed::compressed(PartialEncode(false)), 1)
            .expect("complete prefix");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            batch.encode_ready(Framed::compressed(NoiseOutput::new(4)), 1)
        }));
        assert!(result.is_err());
        assert!(!batch.sink.is_empty());
        assert_eq!(first.clone().concat(), saved);
        let mut bytes = Bytes::from(batch.take_frame().expect("preserved frame").concat());
        let frame = crate::codec::pop_from_chunk(&mut bytes, batch.wire.limits)
            .expect("frame")
            .expect("one");
        assert_eq!(
            batch
                .wire
                .send_codec
                .decode(&frame.payload)
                .expect("payload"),
            [0xAB]
        );
        assert!(bytes.is_empty());
    }

    struct IdentitySuffix<const APPEND: bool> {
        fail: bool,
    }
    impl<const APPEND: bool> CodecMessage for IdentitySuffix<APPEND> {
        const ENCODE_APPEND_ONLY: bool = APPEND;
        fn encoded_len(&self) -> usize {
            131073
        }
        fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
            out.write_contiguous(|buffer| {
                let mut backing = bytes::BytesMut::with_capacity(8 * 1024 * 1024);
                backing.resize(7 * 1024 * 1024, 0);
                backing.extend_from_slice(buffer);
                backing.extend_from_slice(&vec![0xCD; 131073]);
                drop(backing.split_to(7 * 1024 * 1024));
                *buffer = backing;
            });
            if self.fail {
                Err(Status::internal("intentional identity suffix error"))
            } else {
                Ok(())
            }
        }
        fn decode_payload(_: Bytes) -> Result<Self, Status> {
            Ok(Self::empty())
        }
        fn empty() -> Self {
            Self { fail: false }
        }
    }

    fn suffix_control<const APPEND: bool>(fail: bool) {
        let (mut batch, first, saved) = warmed_output();
        batch
            .encode_ready(Framed::compressed(PartialEncode(false)), 1)
            .expect("compressed prefix");
        let result = batch.encode_ready(Framed::new(IdentitySuffix::<APPEND> { fail }), 1);
        assert_eq!(result.is_err(), fail);
        assert!(
            !batch.compressed_only,
            "identity attempts permanently invalidate this batch"
        );
        let mut bytes = Bytes::from(batch.take_frame().expect("complete output").concat());
        assert_eq!(batch.sink.output_cache_state(), (0, None, false));
        let prefix = crate::codec::pop_from_chunk(&mut bytes, batch.wire.limits)
            .expect("prefix")
            .expect("one");
        assert!(prefix.compressed);
        assert_eq!(
            batch
                .wire
                .send_codec
                .decode(&prefix.payload)
                .expect("payload"),
            [0xAB]
        );
        if !fail {
            let identity = crate::codec::pop_from_chunk(&mut bytes, batch.wire.limits)
                .expect("identity")
                .expect("two");
            assert!(!identity.compressed);
            assert_eq!(identity.payload.as_ref(), vec![0xCD; 131073]);
        }
        assert!(bytes.is_empty());
        assert_eq!(first.clone().concat(), saved);
    }

    #[test]
    fn identity_suffix_backing_is_never_cached_including_failed_mixed_batches() {
        for fail in [false, true] {
            suffix_control::<false>(fail);
            suffix_control::<true>(fail);
        }
    }
}
