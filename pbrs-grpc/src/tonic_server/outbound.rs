//! Cold finite outbound transform after tonic identity serialization.
//!
//! This protects forwarding and the transform's own payload buffers. Tonic
//! serialization already happened before a DATA frame reaches this body.
//! Coalesced source DATA, Bytes backing storage and private codec state remain
//! opaque. Compression of one capped message is synchronous; the outer
//! deadline cannot preempt that compressor call.

use crate::compression::CompressionAlgorithm;
use crate::{ByteBudgetTracker, BytePermit, MessageLimits, Rpc, ServerConfig, Status};
use bytes::Bytes;
use http_body::{Body, Frame};
use std::io::{self, Write};
use std::pin::Pin;
use std::task::{Context, Poll};

const QUANTUM: usize = 8 * 1024;

pub(super) fn requested(config: ServerConfig) -> bool {
    let defaults = ServerConfig::default();
    config.compresses_outbound() != defaults.compresses_outbound()
        || config.gzip_level() != defaults.gzip_level()
        || config.send_algorithm() != defaults.send_algorithm()
        || config.send_codec() != defaults.send_codec()
}

fn selected(rpc: &Rpc) -> Option<CompressionAlgorithm> {
    let headers = rpc.request.headers();
    let accepts = |coding| crate::wire::headers::accepts_codec(headers, coding);
    crate::wire::select_outbound_codec(
        None,
        rpc.config.compresses_outbound(),
        crate::wire::preferred_codec(
            rpc.config.send_algorithm(),
            accepts(CompressionAlgorithm::Gzip),
            accepts(CompressionAlgorithm::Deflate),
            #[cfg(feature = "zstd")]
            accepts(CompressionAlgorithm::Zstd),
        ),
    )
}

pub(super) fn supported(rpc: &Rpc) -> bool {
    rpc.limits().max_encoding().is_some()
        && matches!(
            selected(rpc),
            None | Some(CompressionAlgorithm::Gzip | CompressionAlgorithm::Deflate)
        )
}

pub(super) struct Settings {
    pub(super) codec: Option<CompressionAlgorithm>,
    limits: MessageLimits,
    level: u32,
    budget: ByteBudgetTracker,
}

impl Settings {
    pub(super) fn new(rpc: &Rpc) -> Self {
        Self {
            codec: selected(rpc),
            limits: rpc.limits(),
            level: rpc.config.gzip_level(),
            budget: rpc.byte_budget.clone(),
        }
    }
}

fn tonic_status(status: Status) -> tonic::Status {
    tonic::Status::new(
        tonic::Code::from_i32(status.code() as i32),
        status.message(),
    )
}

/// The permit follows every Bytes clone and slice, including an H2 backlog.
struct OwnedFrame {
    bytes: Vec<u8>,
    _permit: BytePermit,
}

impl AsRef<[u8]> for OwnedFrame {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

/// Accounts the capacity reported by Vec, including any reported excess.
/// Allocator size classes and backing-allocation slack are not observable here.
/// Only unlimited trackers reach this leaf; finite opaque budgets reject
/// before readiness. These permits do not account the compressor's state.
struct Buffer {
    bytes: Vec<u8>,
    permit: BytePermit,
    budget: ByteBudgetTracker,
    error: Option<Status>,
}

impl Buffer {
    fn new(budget: ByteBudgetTracker) -> Self {
        Self {
            bytes: Vec::new(),
            permit: BytePermit::empty(),
            budget,
            error: None,
        }
    }

    fn reserve(&mut self, capacity: usize) -> Result<(), Status> {
        if capacity <= self.bytes.capacity() {
            return Ok(());
        }
        let old = self.bytes.capacity();
        let mut permit = self.budget.acquire(capacity - old)?;
        self.bytes
            .try_reserve_exact(capacity - self.bytes.len())
            .map_err(|_| Status::resource_exhausted("opaque outbound buffer allocation failed"))?;
        if self.bytes.capacity() > capacity {
            permit.merge(self.budget.acquire(self.bytes.capacity() - capacity)?);
        }
        self.permit.merge(permit);
        Ok(())
    }

    fn into_frame(self) -> Result<Bytes, Status> {
        let mut bytes = self.bytes;
        let length = bytes
            .len()
            .checked_sub(5)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| Status::internal("message too large"))?;
        bytes
            .get_mut(1..5)
            .ok_or_else(|| Status::internal("invalid outbound frame state"))?
            .copy_from_slice(&length.to_be_bytes());
        Ok(Bytes::from_owner(OwnedFrame {
            bytes,
            _permit: self.permit,
        }))
    }
}

impl Write for Buffer {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let next = self
            .bytes
            .len()
            .checked_add(input.len())
            .ok_or_else(|| io::Error::other("message too large"))?;
        // This is the wire's u32 encoded-length ceiling, never E = N.
        if next
            .checked_sub(5)
            .and_then(|n| u32::try_from(n).ok())
            .is_none()
        {
            return Err(io::Error::other("message too large"));
        }
        if let Err(status) = self.reserve(next) {
            // Native codec helpers translate Write errors to Internal. Retain
            // this buffer's original status so allocation/accounting refusal
            // still reaches the client as ResourceExhausted.
            self.error = Some(status.clone());
            return Err(io::Error::other(status));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct State {
    settings: Settings,
    header: [u8; 5],
    filled: usize,
    remaining: Option<usize>,
    payload: Option<Buffer>,
    data: Bytes,
}

enum Step {
    Data(Bytes),
    NeedData,
    Yield,
}

impl State {
    fn next(&mut self, quota: &mut usize) -> Result<Step, tonic::Status> {
        loop {
            if self.remaining.is_none() {
                if self.data.is_empty() {
                    return Ok(Step::NeedData);
                }
                let n = (5 - self.filled).min(self.data.len());
                self.header
                    .get_mut(self.filled..self.filled + n)
                    .ok_or_else(|| tonic::Status::internal("invalid outbound prefix state"))?
                    .copy_from_slice(&self.data.split_to(n));
                self.filled += n;
                if self.filled != 5 {
                    return Ok(Step::NeedData);
                }
                match self.header[0] {
                    0 => {}
                    1 => return Err(tonic_status(super::opaque::compressed_policy())),
                    flag => {
                        return Err(tonic::Status::internal(format!(
                            "invalid gRPC compressed-flag {flag}"
                        )));
                    }
                }
                let len = usize::try_from(u32::from_be_bytes([
                    self.header[1],
                    self.header[2],
                    self.header[3],
                    self.header[4],
                ]))
                .map_err(|_| tonic::Status::internal("message too large"))?;
                self.settings
                    .limits
                    .check_encode(len)
                    .map_err(tonic_status)?;
                5usize
                    .checked_add(len)
                    .ok_or_else(|| tonic::Status::internal("message too large"))?;
                let mut payload = Buffer::new(self.settings.budget.clone());
                payload.reserve(len).map_err(tonic_status)?;
                self.payload = Some(payload);
                self.remaining = Some(len);
            }
            let remaining = self
                .remaining
                .as_mut()
                .ok_or_else(|| tonic::Status::internal("invalid outbound payload state"))?;
            if *remaining != 0 {
                if self.data.is_empty() {
                    return Ok(Step::NeedData);
                }
                if *quota == 0 {
                    return Ok(Step::Yield);
                }
                let n = (*remaining).min(self.data.len()).min(*quota);
                self.payload
                    .as_mut()
                    .ok_or_else(|| tonic::Status::internal("invalid outbound buffer state"))?
                    .bytes
                    .extend_from_slice(&self.data.split_to(n));
                *remaining -= n;
                *quota -= n;
                if *remaining != 0 {
                    continue;
                }
            }
            let payload = self
                .payload
                .take()
                .ok_or_else(|| tonic::Status::internal("invalid outbound buffer state"))?;
            let mut output = Buffer::new(self.settings.budget.clone());
            output.reserve(5).map_err(tonic_status)?;
            output.bytes.extend_from_slice(&[1, 0, 0, 0, 0]);
            let codec = self
                .settings
                .codec
                .ok_or_else(|| tonic::Status::internal("missing outbound coding"))?;
            if let Err(status) = codec.encode_into(&payload.bytes, self.settings.level, &mut output)
            {
                return Err(tonic_status(output.error.take().unwrap_or(status)));
            }
            let frame = output.into_frame().map_err(tonic_status)?;
            self.remaining = None;
            self.filled = 0;
            return Ok(Step::Data(frame));
        }
    }

    fn idle(&self) -> bool {
        self.filled == 0 && self.remaining.is_none() && self.data.is_empty()
    }

    fn finish(&self) -> Result<(), tonic::Status> {
        if self.idle() {
            Ok(())
        } else {
            Err(tonic::Status::internal("gRPC body ended mid-frame"))
        }
    }

    fn clear(&mut self) {
        self.payload.take();
        self.data = Bytes::new();
    }
}

pub(super) struct TransformBody<B> {
    inner: Option<Pin<Box<B>>>,
    state: State,
}

impl<B> TransformBody<B> {
    pub(super) fn new(inner: B, settings: Settings) -> Self {
        Self {
            inner: Some(Box::pin(inner)),
            state: State {
                settings,
                header: [0; 5],
                filled: 0,
                remaining: None,
                payload: None,
                data: Bytes::new(),
            },
        }
    }

    fn stop(&mut self) {
        self.inner.take();
        self.state.clear();
    }
}

impl<B> Body for TransformBody<B>
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
        // Coalesced DATA may contain many empty messages. Charge each output
        // so compression cannot bypass cooperation without another source poll.
        let output_progress = match tokio::task::coop::poll_proceed(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(progress) => progress,
        };
        let mut quota = QUANTUM;
        loop {
            match self.state.next(&mut quota) {
                Ok(Step::Data(data)) => {
                    output_progress.made_progress();
                    return Poll::Ready(Some(Ok(Frame::data(data))));
                }
                Ok(Step::NeedData) => {}
                Ok(Step::Yield) => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Err(error) => {
                    self.stop();
                    return Poll::Ready(Some(Err(error)));
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
                    let error = self.state.finish().err();
                    self.stop();
                    return Poll::Ready(error.map(Err));
                }
                Some(Err(error)) => {
                    self.stop();
                    return Poll::Ready(Some(Err(tonic::Status::from_error(error.into()))));
                }
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => self.state.data = data,
                    Err(frame) => {
                        let result = self.state.finish();
                        self.stop();
                        return Poll::Ready(Some(result.map(|()| frame)));
                    }
                },
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(|inner| inner.is_end_stream() && self.state.idle())
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "private body fixtures must fail on unexpected framing or codec errors"
)]
mod tests {
    use super::{Settings, TransformBody};
    use crate::compression::CompressionAlgorithm;
    use crate::{ByteBudgetTracker, MessageLimits};
    use bytes::Bytes;
    use http_body::{Body, Frame};
    use std::collections::VecDeque;
    use std::future::poll_fn;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct Chunks {
        frames: VecDeque<Result<Frame<Bytes>, tonic::Status>>,
        pending: bool,
    }

    impl Body for Chunks {
        type Data = Bytes;
        type Error = tonic::Status;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            if let Some(frame) = self.frames.pop_front() {
                Poll::Ready(Some(frame))
            } else if self.pending {
                Poll::Pending
            } else {
                Poll::Ready(None)
            }
        }
    }

    fn body(
        frames: VecDeque<Result<Frame<Bytes>, tonic::Status>>,
        pending: bool,
        cap: usize,
        budget: ByteBudgetTracker,
    ) -> TransformBody<Chunks> {
        TransformBody::new(
            Chunks { frames, pending },
            Settings {
                codec: Some(CompressionAlgorithm::Gzip),
                limits: MessageLimits::default().with_max_encoding(cap),
                level: 1,
                budget,
            },
        )
    }

    #[tokio::test]
    async fn output_clone_and_slice_keep_actual_capacity_permit_after_body_drop() {
        let tracker = ByteBudgetTracker::unlimited();
        let mut body = body(
            VecDeque::from([Ok(Frame::data(Bytes::from_static(b"\0\0\0\0\x03abc")))]),
            false,
            3,
            tracker.clone(),
        );
        let frame = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .expect("frame")
            .expect("valid");
        let bytes = frame.into_data().expect("data");
        assert!(tracker.peak_allocated() >= bytes.len() + 3);
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        let retained = bytes.slice(1..2);
        drop(bytes);
        drop(body);
        assert!(tracker.allocated() >= retained.len());
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        drop(retained);
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
    }

    #[tokio::test]
    async fn complete_message_progresses_without_body_eof_or_next_message() {
        let tracker = ByteBudgetTracker::unlimited();
        let mut body = body(
            VecDeque::from([Ok(Frame::data(Bytes::from_static(b"\0\0\0\0\x01x")))]),
            true,
            1,
            tracker.clone(),
        );
        let frame = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .expect("message")
            .expect("complete");
        let bytes = frame.into_data().expect("data");
        let payload = bytes.get(5..).expect("payload");
        assert_eq!(crate::gzip::decode(payload).expect("gzip"), b"x");
        drop(bytes);
        assert!(
            poll_fn(|cx| Poll::Ready(Pin::new(&mut body).poll_frame(cx)))
                .await
                .is_pending()
        );
        drop(body);
        assert_eq!(tracker.allocated(), 0);
    }

    #[tokio::test]
    async fn incomplete_frame_eof_and_trailers_release_input_and_fuse_error() {
        for trailers in [false, true] {
            let tracker = ByteBudgetTracker::unlimited();
            let mut frames =
                VecDeque::from([Ok(Frame::data(Bytes::from_static(b"\0\0\0\0\x03ab")))]);
            if trailers {
                frames.push_back(Ok(Frame::trailers(http::HeaderMap::new())));
            }
            let mut body = body(frames, false, 3, tracker.clone());
            let error = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                .await
                .expect("error")
                .expect_err("partial message");
            assert_eq!(error.code(), tonic::Code::Internal);
            assert_eq!(tracker.allocated(), 0);
            assert_eq!(tracker.active_byte_permit_tokens(), 0);
            assert!(body.is_end_stream());
            assert!(
                poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                    .await
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn producer_error_preserves_details_and_metadata_and_releases_partial_input() {
        let tracker = ByteBudgetTracker::unlimited();
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert("x-terminal", "preserved".parse().expect("metadata"));
        let error = tonic::Status::with_details_and_metadata(
            tonic::Code::PermissionDenied,
            "producer error",
            Bytes::from_static(b"details"),
            metadata,
        );
        let mut body = body(
            VecDeque::from([
                Ok(Frame::data(Bytes::from_static(b"\0\0\0\0\x03ab"))),
                Err(error),
            ]),
            false,
            3,
            tracker.clone(),
        );
        let error = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .expect("error")
            .expect_err("producer failed");
        assert_eq!(error.code(), tonic::Code::PermissionDenied);
        assert_eq!(error.details(), b"details");
        assert_eq!(
            error.metadata().get("x-terminal").expect("metadata"),
            "preserved"
        );
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
    }

    #[tokio::test]
    async fn cancellation_drop_returns_pending_partial_message_capacity() {
        let tracker = ByteBudgetTracker::unlimited();
        let mut body = body(
            VecDeque::from([Ok(Frame::data(Bytes::from_static(b"\0\0\0\0\x03ab")))]),
            true,
            3,
            tracker.clone(),
        );
        assert!(
            poll_fn(|cx| Poll::Ready(Pin::new(&mut body).poll_frame(cx)))
                .await
                .is_pending()
        );
        assert_eq!(tracker.allocated(), 3);
        assert_eq!(tracker.active_byte_permit_tokens(), 1);
        drop(body);
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
    }

    #[tokio::test]
    async fn coalesced_empty_messages_yield_before_compressing_the_entire_chunk() {
        let tracker = ByteBudgetTracker::unlimited();
        let mut body = body(
            VecDeque::from([Ok(Frame::data(Bytes::from(vec![0; 5 * 1024])))]),
            false,
            0,
            tracker.clone(),
        );
        let mut completed = 0;
        let yielded = poll_fn(|cx| {
            for _ in 0..1024 {
                match Pin::new(&mut body).poll_frame(cx) {
                    Poll::Ready(Some(Ok(frame))) => {
                        assert!(frame.into_data().is_ok());
                        completed += 1;
                    }
                    Poll::Pending => return Poll::Ready(true),
                    _ => return Poll::Ready(false),
                }
            }
            Poll::Ready(false)
        })
        .await;
        assert!(yielded);
        assert!(completed < 1024);
        assert!(!body.state.data.is_empty());
        drop(body);
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
    }

    #[tokio::test]
    async fn writer_budget_refusal_retains_resource_status_and_releases_both_buffers() {
        // Public opaque finite budgets still reject before readiness. Exercise
        // the private writer's failure deterministically, without real OOM:
        // three input bytes plus a five-byte prefix fit, the gzip header does
        // not. The codec itself maps this IO error to Internal; the body must
        // recover the writer's original ResourceExhausted status.
        let tracker = ByteBudgetTracker::with_limit(8);
        let mut body = body(
            VecDeque::from([Ok(Frame::data(Bytes::from_static(b"\0\0\0\0\x03abc")))]),
            false,
            3,
            tracker.clone(),
        );
        let error = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .expect("error")
            .expect_err("writer allocation refused");
        assert_eq!(error.code(), tonic::Code::ResourceExhausted);
        assert!(error.message().contains("transport byte budget exceeded"));
        assert_eq!(tracker.allocated(), 0);
        assert_eq!(tracker.active_byte_permit_tokens(), 0);
        assert!(body.is_end_stream());
        assert!(
            poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                .await
                .is_none()
        );
    }
}
