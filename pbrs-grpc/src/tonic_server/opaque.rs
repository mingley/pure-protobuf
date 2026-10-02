//! Cold identity-body framing guard. Encoded DATA remains in original Bytes.

use crate::Status;
use crate::limits::MessageLimits;
use bytes::Bytes;
use http_body::{Body, Frame};
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Clone, Copy)]
pub(super) enum Direction {
    Decode,
    Encode,
}

struct Prefix {
    limits: MessageLimits,
    direction: Direction,
    header: [u8; 5],
    filled: usize,
    admitted: bool,
    remaining: usize,
    data: Bytes,
    fragments: [Bytes; 5],
    queued: usize,
    emitted: usize,
}

impl Prefix {
    fn new(limits: MessageLimits, direction: Direction) -> Self {
        Self {
            limits,
            direction,
            header: [0; 5],
            filled: 0,
            admitted: false,
            remaining: 0,
            data: Bytes::new(),
            fragments: std::array::from_fn(|_| Bytes::new()),
            queued: 0,
            emitted: 0,
        }
    }

    fn next(&mut self) -> Result<Option<Bytes>, Status> {
        loop {
            if self.admitted {
                if self.emitted < self.queued {
                    let fragment = std::mem::take(&mut self.fragments[self.emitted]);
                    self.emitted += 1;
                    return Ok(Some(fragment));
                }
                if self.remaining != 0 {
                    if self.data.is_empty() {
                        return Ok(None);
                    }
                    let n = self.remaining.min(self.data.len());
                    self.remaining -= n;
                    return Ok(Some(self.data.split_to(n)));
                }
                self.admitted = false;
                self.filled = 0;
                self.queued = 0;
                self.emitted = 0;
            }
            if self.data.is_empty() {
                return Ok(None);
            }
            let n = (5 - self.filled).min(self.data.len());
            let fragment = self.data.split_to(n);
            self.header[self.filled..self.filled + n].copy_from_slice(&fragment);
            self.filled += n;
            self.fragments[self.queued] = fragment;
            self.queued += 1;
            if self.filled != 5 {
                return Ok(None);
            }
            match self.header[0] {
                0 => {}
                1 => return Err(compressed_policy()),
                flag => {
                    return Err(Status::internal(format!(
                        "invalid gRPC compressed-flag {flag}"
                    )));
                }
            }
            let length = u32::from_be_bytes([
                self.header[1],
                self.header[2],
                self.header[3],
                self.header[4],
            ]);
            let length =
                usize::try_from(length).map_err(|_| Status::internal("message too large"))?;
            match self.direction {
                Direction::Decode => self.limits.check_decode(length)?,
                Direction::Encode => self.limits.check_encode(length)?,
            }
            5usize
                .checked_add(length)
                .ok_or_else(|| Status::internal("message too large"))?;
            self.remaining = length;
            self.admitted = true;
        }
    }

    fn idle(&self) -> bool {
        self.data.is_empty()
            && if self.admitted {
                self.remaining == 0 && self.emitted == self.queued
            } else {
                self.filled == 0
            }
    }

    fn finish(&self) -> Result<(), Status> {
        if self.idle() {
            Ok(())
        } else {
            Err(Status::internal("gRPC body ended mid-frame"))
        }
    }

    fn clear(&mut self) {
        self.data = Bytes::new();
        self.fragments = std::array::from_fn(|_| Bytes::new());
    }
}

pub(super) fn compressed_policy() -> Status {
    Status::failed_precondition(
        "tonic server transport cannot apply configured native message limits to compressed opaque bodies",
    )
}

fn tonic_status(status: Status) -> tonic::Status {
    // Framing/limit errors have no details or user metadata. Producer errors
    // use tonic::Status::from_error directly below, retaining their contents.
    tonic::Status::new(
        tonic::Code::from_i32(status.code() as i32),
        status.message(),
    )
}

/// Boxed only for explicitly configured native caps; the default body and
/// default call future do not embed this framing state or allocate this box.
pub(super) struct CappedBody<B> {
    inner: Option<Pin<Box<B>>>,
    prefix: Prefix,
}

impl<B> CappedBody<B> {
    pub(super) fn new(inner: B, limits: MessageLimits, direction: Direction) -> Self {
        Self {
            inner: Some(Box::pin(inner)),
            prefix: Prefix::new(limits, direction),
        }
    }

    fn stop(&mut self) {
        self.inner.take();
        self.prefix.clear();
    }
}

impl<B> Body for CappedBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<tonic::codegen::StdError>,
{
    type Data = Bytes;
    type Error = tonic::Status;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        if self.inner.is_none() {
            return Poll::Ready(None);
        }
        loop {
            match self.prefix.next() {
                Ok(Some(data)) => return Poll::Ready(Some(Ok(Frame::data(data)))),
                Ok(None) => {}
                Err(status) => {
                    self.stop();
                    return Poll::Ready(Some(Err(tonic_status(status))));
                }
            }
            let progress = match tokio::task::coop::poll_proceed(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(progress) => progress,
            };
            let Some(inner) = self.inner.as_mut() else {
                return Poll::Ready(None);
            };
            let frame = match inner.as_mut().poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(frame) => {
                    progress.made_progress();
                    frame
                }
            };
            match frame {
                None => {
                    let result = self.prefix.finish();
                    self.stop();
                    return Poll::Ready(result.err().map(|status| Err(tonic_status(status))));
                }
                Some(Err(error)) => {
                    self.stop();
                    return Poll::Ready(Some(Err(tonic::Status::from_error(error.into()))));
                }
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => self.prefix.data = data,
                    Err(frame) => {
                        if let Err(status) = self.prefix.finish() {
                            self.stop();
                            return Poll::Ready(Some(Err(tonic_status(status))));
                        }
                        self.stop();
                        return Poll::Ready(Some(Ok(frame)));
                    }
                },
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(|inner| inner.is_end_stream() && self.prefix.idle())
    }
}

#[cfg(test)]
mod tests {
    use super::{CappedBody, Direction, Prefix};
    use crate::{Code, MessageLimits};
    use bytes::Bytes;
    use http_body::{Body, Frame};
    use std::collections::VecDeque;
    use std::future::poll_fn;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    #[test]
    fn every_segmentation_preserves_original_bytes_and_allocation_spans() {
        // Independently specified empty message followed by one "abc" message.
        let wire = Bytes::from(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 3, b'a', b'b', b'c']);
        for cuts in 0..(1usize << (wire.len() - 1)) {
            let mut parser = Prefix::new(
                MessageLimits::default().with_max_decoding(3),
                Direction::Decode,
            );
            let mut got = Vec::new();
            let mut start = 0;
            for end in 1..=wire.len() {
                if end != wire.len() && cuts & (1 << (end - 1)) == 0 {
                    continue;
                }
                parser.data = wire.slice(start..end);
                while let Some(slice) = parser.next().expect("valid framing") {
                    assert_eq!(slice.as_ptr(), wire.as_ptr().wrapping_add(got.len()));
                    got.extend_from_slice(&slice);
                }
                start = end;
            }
            parser.finish().expect("complete");
            assert_eq!(got, wire, "cut mask {cuts}");
        }
    }

    #[test]
    fn later_oversize_frame_does_not_discard_earlier_complete_message() {
        let first = b"\x00\x00\x00\x00\x01x";
        let mut parser = Prefix::new(
            MessageLimits::default().with_max_encoding(1),
            Direction::Encode,
        );
        parser.data = Bytes::from_static(b"\x00\x00\x00\x00\x01x\x00\x00\x00\x00\x02yz");
        let mut got = Vec::new();
        loop {
            match parser.next() {
                Ok(Some(bytes)) => got.extend_from_slice(&bytes),
                Err(status) => {
                    assert_eq!(status.code(), Code::ResourceExhausted);
                    break;
                }
                Ok(None) => panic!("second frame must fail"),
            }
        }
        assert_eq!(got, first);
    }

    #[test]
    fn uncompressed_cap_is_per_message_and_none_does_not_add_a_cap() {
        for limits in [
            MessageLimits::default().with_max_decoding(1),
            MessageLimits::unlimited(),
        ] {
            let mut parser = Prefix::new(limits, Direction::Decode);
            parser.data = Bytes::from_static(b"\x00\x00\x00\x00\x01x\x00\x00\x00\x00\x01y");
            let mut got = Vec::new();
            while let Some(bytes) = parser.next().expect("each message fits") {
                got.extend_from_slice(&bytes);
            }
            parser.finish().expect("complete");
            assert_eq!(got, b"\x00\x00\x00\x00\x01x\x00\x00\x00\x00\x01y");
        }
    }

    struct Chunks {
        frames: VecDeque<Result<Frame<Bytes>, tonic::Status>>,
        dropped: Arc<AtomicUsize>,
    }

    impl Drop for Chunks {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Body for Chunks {
        type Data = Bytes;
        type Error = tonic::Status;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            Poll::Ready(self.frames.pop_front())
        }
    }

    #[tokio::test]
    async fn invalid_prefix_releases_source_and_emits_one_error_then_eof() {
        for (wire, expected) in [
            (
                b"\x00\xff\xff\xff\xff".as_slice(),
                tonic::Code::ResourceExhausted,
            ),
            (b"\x02\x00\x00\x00\x00".as_slice(), tonic::Code::Internal),
            (
                b"\x01\x00\x00\x00\x00".as_slice(),
                tonic::Code::FailedPrecondition,
            ),
        ] {
            let dropped = Arc::new(AtomicUsize::new(0));
            let mut body = CappedBody::new(
                Chunks {
                    frames: VecDeque::from([Ok(Frame::data(Bytes::copy_from_slice(wire)))]),
                    dropped: dropped.clone(),
                },
                MessageLimits::default().with_max_decoding(8),
                Direction::Decode,
            );
            let error = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                .await
                .expect("error frame")
                .expect_err("no invalid prefix emitted");
            assert_eq!(error.code(), expected);
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
            assert!(body.is_end_stream());
            assert!(
                poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                    .await
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn incomplete_frame_is_not_hidden_by_eof_or_trailers() {
        for wire in [b"\x00\x00".as_slice(), b"\x00\x00\x00\x00\x02x".as_slice()] {
            for trailers in [false, true] {
                let dropped = Arc::new(AtomicUsize::new(0));
                let mut frames = VecDeque::from([Ok(Frame::data(Bytes::copy_from_slice(wire)))]);
                if trailers {
                    frames.push_back(Ok(Frame::trailers(http::HeaderMap::new())));
                }
                let mut body = CappedBody::new(
                    Chunks { frames, dropped },
                    MessageLimits::default(),
                    Direction::Decode,
                );
                loop {
                    let frame = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                        .await
                        .expect("must emit a terminal error");
                    if let Err(error) = frame {
                        assert_eq!(error.code(), tonic::Code::Internal);
                        break;
                    }
                }
                assert!(
                    poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                        .await
                        .is_none()
                );
            }
        }
    }

    #[tokio::test]
    async fn explicit_producer_error_preserves_status_details_and_metadata() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert("x-terminal", "retained".parse().expect("metadata"));
        let failure = tonic::Status::with_details_and_metadata(
            tonic::Code::PermissionDenied,
            "explicit producer failure",
            Bytes::from_static(b"details"),
            metadata,
        );
        let mut body = CappedBody::new(
            Chunks {
                frames: VecDeque::from([
                    Ok(Frame::data(Bytes::from_static(b"\x00\x00"))),
                    Err(failure),
                ]),
                dropped: dropped.clone(),
            },
            MessageLimits::default().with_max_encoding(8),
            Direction::Encode,
        );
        let error = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .expect("error frame")
            .expect_err("withheld partial prefix");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(error.message(), "explicit producer failure");
        assert_eq!(error.details(), b"details");
        assert_eq!(
            error.metadata().get("x-terminal").expect("metadata"),
            "retained"
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(
            poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                .await
                .is_none()
        );
    }
}
