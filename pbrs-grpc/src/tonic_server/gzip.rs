//! Finite inbound gzip validation over retained original DATA views.

use crate::{MessageLimits, Status};
use bytes::Bytes;
use flate2::bufread::GzDecoder;
use http_body::{Body, Frame};
use std::io::{self, BufRead, Read};
use std::pin::Pin;
use std::task::{Context, Poll};

// Caps newly consumed input and produced output per call before yielding.
// Flate2 can allocate/checksum a complete optional u16-sized header field in
// one call; those separate finite bounds are not reduced by this quota.
const QUANTUM: usize = 8 * 1024;

struct Segments {
    views: Vec<Bytes>,
    cursor: usize,
    offset: usize,
    replay: usize,
    quota: usize,
    complete: bool,
    invalid: bool,
}
impl Segments {
    fn new() -> Self {
        Self {
            views: Vec::new(),
            cursor: 0,
            offset: 0,
            replay: 0,
            quota: 0,
            complete: false,
            invalid: false,
        }
    }
    fn push(&mut self, bytes: Bytes) -> Result<(), tonic::Status> {
        if !bytes.is_empty() {
            self.views.try_reserve(1).map_err(|_| {
                tonic::Status::resource_exhausted("gzip fragment allocation failed")
            })?;
            self.views.push(bytes);
        }
        Ok(())
    }
    fn replay(&mut self) -> Option<Bytes> {
        let view = self.views.get_mut(self.replay)?;
        self.replay += 1;
        Some(std::mem::take(view))
    }
}
fn invalid_state() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid gzip segment state")
}
impl BufRead for Segments {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.invalid {
            return Err(invalid_state());
        }
        let Some(view) = self.views.get(self.cursor) else {
            return if self.complete {
                Ok(&[])
            } else {
                Err(io::ErrorKind::WouldBlock.into())
            };
        };
        if self.quota == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let tail = view.get(self.offset..).ok_or_else(invalid_state)?;
        tail.get(..tail.len().min(self.quota))
            .ok_or_else(invalid_state)
    }
    fn consume(&mut self, amount: usize) {
        if let Some(view) = self.views.get(self.cursor) {
            if amount > self.quota || amount > view.len().saturating_sub(self.offset) {
                self.invalid = true;
                return;
            }
            self.offset += amount;
            self.quota -= amount;
            if self.offset == view.len() {
                self.cursor += 1;
                self.offset = 0;
            }
        }
    }
}
impl Read for Segments {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let input = self.fill_buf()?;
        let n = input.len().min(output.len());
        output
            .get_mut(..n)
            .ok_or_else(invalid_state)?
            .copy_from_slice(input.get(..n).ok_or_else(invalid_state)?);
        self.consume(n);
        Ok(n)
    }
}

enum Payload {
    Identity(Segments),
    Gzip {
        decoder: Box<GzDecoder<Segments>>,
        inflated: usize,
        done: bool,
    },
}
enum Step {
    Data(Bytes),
    NeedData,
    Yield,
}
impl Payload {
    fn store(&mut self) -> &mut Segments {
        match self {
            Self::Identity(store) => store,
            Self::Gzip { decoder, .. } => decoder.get_mut(),
        }
    }
    fn validate(
        &mut self,
        scratch: &mut [u8; QUANTUM],
        max: usize,
        input_left: &mut usize,
    ) -> Result<Option<Step>, tonic::Status> {
        match self {
            Self::Identity(store) => Ok((!store.complete).then_some(Step::NeedData)),
            Self::Gzip {
                decoder,
                inflated,
                done,
            } => {
                if *done {
                    return Ok((!decoder.get_ref().complete).then_some(Step::NeedData));
                }
                decoder.get_mut().quota = *input_left;
                let read_len = max.saturating_sub(*inflated).saturating_add(1).min(QUANTUM);
                let output = scratch
                    .get_mut(..read_len)
                    .ok_or_else(|| tonic::Status::internal("invalid gzip scratch state"))?;
                let result = decoder.read(output);
                *input_left = decoder.get_ref().quota;
                match result {
                    Ok(0) => {
                        *done = true;
                        Ok((!decoder.get_ref().complete).then_some(Step::NeedData))
                    }
                    Ok(n) => {
                        let next =
                            inflated
                                .checked_add(n)
                                .filter(|&n| n <= max)
                                .ok_or_else(|| {
                                    tonic::Status::resource_exhausted(format!(
                                        "decompressed message exceeds limit {max}"
                                    ))
                                })?;
                        *inflated = next;
                        Ok(Some(Step::Yield))
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        Ok(Some(if decoder.get_ref().quota == 0 {
                            Step::Yield
                        } else {
                            Step::NeedData
                        }))
                    }
                    Err(error) => Err(tonic::Status::internal(format!("gzip decode: {error}"))),
                }
            }
        }
    }
}

struct State {
    limits: MessageLimits,
    max: usize,
    header: [u8; 5],
    filled: usize,
    prefix: [Bytes; 5],
    queued: usize,
    emitted: usize,
    remaining: usize,
    payload: Option<Payload>,
    data: Bytes,
    scratch: [u8; QUANTUM],
}
fn native_status(status: Status) -> tonic::Status {
    tonic::Status::new(
        tonic::Code::from_i32(status.code() as i32),
        status.message(),
    )
}
impl State {
    fn new(limits: MessageLimits, max: usize) -> Self {
        Self {
            limits,
            max,
            header: [0; 5],
            filled: 0,
            prefix: std::array::from_fn(|_| Bytes::new()),
            queued: 0,
            emitted: 0,
            remaining: 0,
            payload: None,
            data: Bytes::new(),
            scratch: [0; QUANTUM],
        }
    }
    fn next(&mut self, input_left: &mut usize) -> Result<Step, tonic::Status> {
        loop {
            if self.payload.is_none() {
                if self.data.is_empty() {
                    return Ok(Step::NeedData);
                }
                let n = (5 - self.filled).min(self.data.len());
                let view = self.data.split_to(n);
                self.header
                    .get_mut(self.filled..self.filled + n)
                    .ok_or_else(|| tonic::Status::internal("invalid gzip prefix state"))?
                    .copy_from_slice(&view);
                *self
                    .prefix
                    .get_mut(self.queued)
                    .ok_or_else(|| tonic::Status::internal("invalid gzip prefix state"))? = view;
                self.filled += n;
                self.queued += 1;
                if self.filled != 5 {
                    return Ok(Step::NeedData);
                }
                let len = usize::try_from(u32::from_be_bytes([
                    self.header[1],
                    self.header[2],
                    self.header[3],
                    self.header[4],
                ]))
                .map_err(|_| tonic::Status::internal("message too large"))?;
                match self.header[0] {
                    0 | 1 => {}
                    flag => {
                        return Err(tonic::Status::internal(format!(
                            "invalid gRPC compressed-flag {flag}"
                        )));
                    }
                }
                self.limits.check_decode(len).map_err(native_status)?;
                5usize
                    .checked_add(len)
                    .ok_or_else(|| tonic::Status::internal("message too large"))?;
                self.remaining = len;
                self.payload = Some(if self.header[0] == 0 {
                    Payload::Identity(Segments::new())
                } else {
                    // An empty unfinished reader yields WouldBlock during
                    // construction; optional gzip headers are parsed in quota.
                    Payload::Gzip {
                        decoder: Box::new(GzDecoder::new(Segments::new())),
                        inflated: 0,
                        done: false,
                    }
                });
            }
            let Some(payload) = self.payload.as_mut() else {
                return Err(tonic::Status::internal("invalid gzip payload state"));
            };
            if self.remaining != 0 && !self.data.is_empty() {
                let n = self.remaining.min(self.data.len());
                payload.store().push(self.data.split_to(n))?;
                self.remaining -= n;
            }
            if self.remaining == 0 {
                payload.store().complete = true;
            }
            if let Some(step) = payload.validate(&mut self.scratch, self.max, input_left)? {
                return Ok(step);
            }
            if self.emitted < self.queued {
                let view = std::mem::take(
                    self.prefix
                        .get_mut(self.emitted)
                        .ok_or_else(|| tonic::Status::internal("invalid gzip prefix state"))?,
                );
                self.emitted += 1;
                return Ok(Step::Data(view));
            }
            if let Some(view) = payload.store().replay() {
                return Ok(Step::Data(view));
            }
            self.payload = None;
            self.filled = 0;
            self.queued = 0;
            self.emitted = 0;
        }
    }
    fn idle(&self) -> bool {
        self.data.is_empty() && self.filled == 0 && self.payload.is_none()
    }
    fn finish(&self) -> Result<(), tonic::Status> {
        if self.idle() {
            Ok(())
        } else {
            Err(tonic::Status::internal("gRPC body ended mid-frame"))
        }
    }
}

/// Exists only for changed, finite decoding caps and gzip request encoding.
/// Retained logical bytes/descriptor count are bounded by the wire cap, but
/// Bytes backing allocations and tonic's private decoded buffers are opaque.
pub(super) struct GzipBody<B> {
    inner: Option<Pin<Box<B>>>,
    state: Option<Box<State>>,
}
impl<B> GzipBody<B> {
    pub(super) fn new(inner: B, limits: MessageLimits, max: usize) -> Self {
        Self {
            inner: Some(Box::pin(inner)),
            state: Some(Box::new(State::new(limits, max))),
        }
    }
    fn stop(&mut self) {
        self.inner.take();
        self.state.take();
    }
}
impl<B> Body for GzipBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: Into<tonic::codegen::StdError>,
{
    type Data = Bytes;
    type Error = tonic::Status;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        let mut input_left = QUANTUM;
        loop {
            let Some(state) = this.state.as_mut() else {
                return Poll::Ready(None);
            };
            match state.next(&mut input_left) {
                Ok(Step::Data(data)) => return Poll::Ready(Some(Ok(Frame::data(data)))),
                Ok(Step::Yield) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Ok(Step::NeedData) => {}
                Err(status) => {
                    this.stop();
                    return Poll::Ready(Some(Err(status)));
                }
            }
            let progress = match tokio::task::coop::poll_proceed(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(progress) => progress,
            };
            let Some(inner) = this.inner.as_mut() else {
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
                    let result = state.finish();
                    this.stop();
                    return Poll::Ready(result.err().map(Err));
                }
                Some(Err(error)) => {
                    this.stop();
                    return Poll::Ready(Some(Err(tonic::Status::from_error(error.into()))));
                }
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => state.data = data,
                    Err(frame) => {
                        let result = state.finish();
                        this.stop();
                        return Poll::Ready(Some(result.map(|()| frame)));
                    }
                },
            }
        }
    }
    fn is_end_stream(&self) -> bool {
        self.inner.as_ref().is_none_or(|inner| {
            inner.is_end_stream() && self.state.as_ref().is_none_or(|state| state.idle())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{GzipBody, QUANTUM};
    use crate::MessageLimits;
    use bytes::Bytes;
    use http_body::{Body, Frame};
    use std::collections::VecDeque;
    use std::future::poll_fn;
    use std::io::Write;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};

    struct Frames(
        VecDeque<Result<Frame<Bytes>, tonic::Status>>,
        Arc<AtomicUsize>,
    );
    impl Drop for Frames {
        fn drop(&mut self) {
            self.1.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Body for Frames {
        type Data = Bytes;
        type Error = tonic::Status;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            Poll::Ready(self.0.pop_front())
        }
    }
    fn wire(payload: &[u8]) -> Bytes {
        let encoded = crate::gzip::encode(payload).expect("gzip");
        frame(encoded)
    }
    fn frame(encoded: Vec<u8>) -> Bytes {
        let mut wire = vec![1];
        wire.extend_from_slice(
            &u32::try_from(encoded.len())
                .expect("small fixture")
                .to_be_bytes(),
        );
        wire.extend_from_slice(&encoded);
        Bytes::from(wire)
    }
    fn body(
        frames: Vec<Result<Frame<Bytes>, tonic::Status>>,
        max: usize,
        dropped: Arc<AtomicUsize>,
    ) -> GzipBody<Frames> {
        GzipBody::new(
            Frames(frames.into(), dropped),
            MessageLimits::default().with_max_decoding(max),
            max,
        )
    }
    #[tokio::test]
    async fn fragmented_mixed_messages_preserve_original_spans_and_per_message_cap() {
        let first = wire(&[7; 64]);
        let zero = wire(b"");
        let original = Bytes::from([first.as_ref(), b"\0\0\0\0\x03abc", zero.as_ref()].concat());
        let ptr = original.as_ptr() as usize;
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut frames: Vec<_> = (0..original.len())
            .map(|n| Ok(Frame::data(original.slice(n..n + 1))))
            .collect();
        frames.insert(0, Ok(Frame::data(Bytes::new())));
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
        trailers.insert("x-terminal", http::HeaderValue::from_static("retained"));
        frames.push(Ok(Frame::trailers(trailers)));
        let mut body = body(frames, 64, dropped.clone());
        let mut got = Vec::new();
        while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
            let frame = frame.expect("valid frame");
            let bytes = match frame.into_data() {
                Ok(bytes) => bytes,
                Err(frame) => {
                    let trailers = frame.into_trailers().expect("trailers");
                    assert_eq!(trailers.get("x-terminal").expect("metadata"), "retained");
                    continue;
                }
            };
            let start = bytes.as_ptr() as usize;
            assert!(start >= ptr && start + bytes.len() <= ptr + original.len());
            got.extend_from_slice(&bytes);
        }
        assert_eq!(got, original);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn later_bomb_preserves_first_message_and_fuses_after_source_drop() {
        let first = wire(&[7; 64]);
        let second = wire(&[7; 65]);
        let original = Bytes::from([first.as_ref(), second.as_ref()].concat());
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut body = body(vec![Ok(Frame::data(original))], 64, dropped.clone());
        let mut got = Vec::new();
        loop {
            match poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                .await
                .expect("error or data")
            {
                Ok(frame) => got.extend_from_slice(&frame.into_data().expect("data")),
                Err(error) => {
                    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
                    break;
                }
            }
        }
        assert_eq!(got, first);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(
            poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                .await
                .is_none()
        );
    }
    #[tokio::test]
    async fn single_member_and_encoded_tail_match_existing_decoders() {
        let first = crate::gzip::encode(b"one").expect("first");
        for tail in [
            crate::gzip::encode(b"two").expect("second"),
            vec![255, 0, 123],
        ] {
            let original = frame([first.as_slice(), tail.as_slice()].concat());
            let mut body = body(
                vec![Ok(Frame::data(original.clone()))],
                64,
                Arc::new(AtomicUsize::new(0)),
            );
            let mut got = Vec::new();
            while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
                got.extend_from_slice(&frame.expect("accepted tail").into_data().expect("data"));
            }
            assert_eq!(got, original);
        }
    }
    #[tokio::test]
    async fn partial_gzip_producer_error_keeps_status_details_and_metadata() {
        let mut error = tonic::Status::with_details(
            tonic::Code::PermissionDenied,
            "producer",
            Bytes::from_static(b"details"),
        );
        error
            .metadata_mut()
            .insert("x-terminal", "retained".parse().expect("metadata"));
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut body = body(
            vec![
                Ok(Frame::data(Bytes::from_static(b"\x01\0\0\0\x20\x1f\x8b"))),
                Err(error),
            ],
            64,
            dropped.clone(),
        );
        let error = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .expect("error frame")
            .expect_err("prefix withheld");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(error.details(), b"details");
        assert_eq!(
            error.metadata().get("x-terminal").expect("metadata"),
            "retained"
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn optional_header_work_yields_and_cancellation_drops_retained_source() {
        let mut encoder = flate2::GzBuilder::new()
            .comment(vec![b'a'; 2 * QUANTUM + 17])
            .write(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b"abc").expect("write");
        let original = frame(encoder.finish().expect("finish"));
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut body = body(
            vec![Ok(Frame::data(original))],
            4 * QUANTUM,
            dropped.clone(),
        );
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
        drop(body);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn optional_header_resumes_across_input_quotas_before_replaying() {
        let mut encoder = flate2::GzBuilder::new()
            .extra(vec![9; 2 * QUANTUM + 17])
            .filename(vec![b'n'; QUANTUM + 11])
            .comment(vec![b'c'; QUANTUM + 3])
            .write(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b"abc").expect("write");
        let original = frame(encoder.finish().expect("finish"));
        let mut body = body(
            vec![Ok(Frame::data(original.clone()))],
            8 * QUANTUM,
            Arc::new(AtomicUsize::new(0)),
        );
        let mut cx = Context::from_waker(Waker::noop());
        let mut got = Vec::new();
        let mut yields = 0;
        for _ in 0..32 {
            match Pin::new(&mut body).poll_frame(&mut cx) {
                Poll::Pending => yields += 1,
                Poll::Ready(Some(Ok(frame))) => {
                    got.extend_from_slice(&frame.into_data().expect("data"))
                }
                Poll::Ready(Some(Err(error))) => panic!("valid gzip: {error}"),
                Poll::Ready(None) => {
                    assert!(yields >= 4);
                    assert_eq!(got, original);
                    return;
                }
            }
        }
        panic!("bounded fixture failed to finish");
    }
    #[tokio::test]
    async fn header_and_payload_checksums_validate_before_encoded_replay() {
        let original = crate::gzip::encode(b"abc").expect("gzip");
        let mut header = original.get(..10).expect("fixed header").to_vec();
        *header.get_mut(3).expect("flags") |= 2;
        let mut crc = !0u32;
        for &byte in &header {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0u32.wrapping_sub(crc & 1) & 0xedb88320);
            }
        }
        let mut with_header_crc = header;
        with_header_crc.extend_from_slice((!crc).to_le_bytes().get(..2).expect("header CRC"));
        with_header_crc.extend_from_slice(original.get(10..).expect("body and trailer"));
        let mut bad_header = with_header_crc.clone();
        *bad_header.get_mut(10).expect("checksum") ^= 1;
        let mut bad_crc = original.clone();
        let crc_offset = bad_crc.len() - 8;
        *bad_crc.get_mut(crc_offset).expect("CRC") ^= 1;
        for (encoded, valid) in [
            (with_header_crc, true),
            (bad_header, false),
            (bad_crc, false),
        ] {
            let original = frame(encoded);
            let dropped = Arc::new(AtomicUsize::new(0));
            let frames = (0..original.len())
                .map(|n| Ok(Frame::data(original.slice(n..n + 1))))
                .collect();
            let mut body = body(frames, 64, dropped.clone());
            let mut got = Vec::new();
            let mut error = None;
            while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
                match frame {
                    Ok(frame) => got.extend_from_slice(&frame.into_data().expect("data")),
                    Err(e) => error = Some(e),
                }
            }
            if valid {
                assert!(error.is_none());
                assert_eq!(got, original);
            } else {
                assert_eq!(
                    error.expect("checksum rejected").code(),
                    tonic::Code::Internal
                );
                assert!(got.is_empty());
            }
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test]
    async fn trailers_cannot_turn_partial_gzip_into_success() {
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
        let mut body = body(
            vec![
                Ok(Frame::data(Bytes::from_static(b"\x01\0\0\0\x20\x1f\x8b"))),
                Ok(Frame::trailers(trailers)),
            ],
            64,
            Arc::new(AtomicUsize::new(0)),
        );
        let error = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .expect("terminal error")
            .expect_err("prefix withheld");
        assert_eq!(error.code(), tonic::Code::Internal);
        assert!(
            poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                .await
                .is_none()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inflating_yields_runtime_progress_before_prefix_and_replay_drop_frees_source() {
        let original = wire(&vec![7; 1024 * 1024]);
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut body = body(
            vec![Ok(Frame::data(original))],
            2 * 1024 * 1024,
            dropped.clone(),
        );
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
        let progress = Arc::new(AtomicUsize::new(0));
        let seen = progress.clone();
        let pulse = tokio::spawn(async move {
            seen.fetch_add(1, Ordering::SeqCst);
        });
        let first = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)),
        )
        .await
        .expect("bounded inflation finishes")
        .expect("prefix")
        .expect("valid");
        assert!(first.is_data());
        assert_eq!(
            progress.load(Ordering::SeqCst),
            1,
            "runtime progressed before prefix replay"
        );
        pulse.await.expect("pulse");
        drop(body);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dropping_during_inflate_releases_owned_original_backing() {
        struct Owner(Vec<u8>, Arc<AtomicUsize>);
        impl AsRef<[u8]> for Owner {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }
        impl Drop for Owner {
            fn drop(&mut self) {
                self.1.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicUsize::new(0));
        let backing = Arc::new(AtomicUsize::new(0));
        let original =
            Bytes::from_owner(Owner(wire(&vec![7; 1024 * 1024]).to_vec(), backing.clone()));
        let mut body = body(
            vec![Ok(Frame::data(original))],
            2 * 1024 * 1024,
            dropped.clone(),
        );
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
        assert_eq!(backing.load(Ordering::SeqCst), 0);
        drop(body);
        assert_eq!(backing.load(Ordering::SeqCst), 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
}
