use crate::h2_backend::codec::UserError;
use crate::h2_backend::codec::UserError::*;
use crate::h2_backend::frame::{self, Frame, FrameSize};
use crate::h2_backend::hpack;
use crate::h2_backend::tracing;

use bytes::{Buf, BufMut, BytesMut};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::io::poll_write_buf;

use std::io::{self, Cursor, IoSlice};

// A macro to get around a method needing to borrow &mut self
macro_rules! limited_write_buf {
    ($self:expr) => {{
        let limit = $self.max_frame_size() + frame::HEADER_LEN;
        $self.buf.get_mut().limit(limit)
    }};
}

#[derive(Debug)]
pub struct FramedWrite<T, B> {
    /// Upstream `AsyncWrite`
    inner: T,
    final_flush_done: bool,
    /// Set when a frame is buffered. Cleared after `poll_flush` succeeds.
    /// An empty, already-flushed codec does not touch the socket again.
    needs_flush: bool,

    encoder: Encoder<B>,
}

#[derive(Debug)]
struct Encoder<B> {
    /// HPACK encoder
    hpack: hpack::Encoder,

    /// Write buffer
    ///
    /// TODO: Should this be a ring buffer?
    buf: Cursor<BytesMut>,

    /// Next frame to encode
    next: Option<Next<B>>,

    /// Last data frame
    last_data_frame: Option<frame::Data<B>>,

    /// Max frame size, this is specified by the peer
    max_frame_size: FrameSize,

    /// Chain payloads bigger than this.
    chain_threshold: usize,

    vectored: bool,

    /// Min buffer required to attempt to write a frame
    min_buffer_capacity: usize,
}

#[derive(Debug)]
enum Next<B> {
    Data(frame::Data<B>),
    Continuation(frame::Continuation),
}

/// Initialize the connection with this amount of write buffer.
///
/// The minimum MAX_FRAME_SIZE is 16kb, so always be able to send a HEADERS
/// frame that big.
const DEFAULT_BUFFER_CAPACITY: usize = 16 * 1_024;

/// Chain payloads bigger than this when vectored I/O is enabled.
///
/// Payloads up to 4 KiB are copied into the 16 KiB frame buffer so bursts of
/// small gRPC DATA frames coalesce into a single write syscall while payloads
/// at or above 4 KiB stay zero-copy via `writev`.
const CHAIN_THRESHOLD: usize = 4096;

/// Chain payloads bigger than this when vectored I/O is **not** enabled.
/// A larger value in this scenario will reduce the number of small and
/// fragmented data being sent, and hereby improve the throughput.
const CHAIN_THRESHOLD_WITHOUT_VECTORED_IO: usize = 4096;

/// A short protobuf prefix can join the DATA header before the large tail.
const MAX_VECTORED_PREFIX: usize = 256;

// TODO: Make generic
impl<T, B> FramedWrite<T, B>
where
    T: AsyncWrite + Unpin,
    B: Buf,
{
    pub fn new(inner: T) -> FramedWrite<T, B> {
        let vectored = inner.is_write_vectored();
        let chain_threshold = if vectored {
            CHAIN_THRESHOLD
        } else {
            CHAIN_THRESHOLD_WITHOUT_VECTORED_IO
        };
        FramedWrite {
            inner,
            final_flush_done: false,
            needs_flush: false,
            encoder: Encoder {
                hpack: hpack::Encoder::default(),
                buf: Cursor::new(BytesMut::with_capacity(DEFAULT_BUFFER_CAPACITY)),
                next: None,
                last_data_frame: None,
                max_frame_size: frame::DEFAULT_MAX_FRAME_SIZE,
                chain_threshold,
                vectored,
                min_buffer_capacity: chain_threshold + frame::HEADER_LEN,
            },
        }
    }

    /// Returns `Ready` when `send` is able to accept a frame
    ///
    /// Calling this function may result in the current contents of the buffer
    /// to be flushed to `T`.
    pub fn poll_ready(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if !self.encoder.has_capacity() {
            // Try flushing
            ready!(self.flush(cx))?;

            if !self.encoder.has_capacity() {
                return Poll::Pending;
            }
        }

        Poll::Ready(Ok(()))
    }

    /// Returns whether a frame can be buffered without first flushing the
    /// underlying I/O object.
    pub(crate) fn has_capacity(&self) -> bool {
        self.encoder.has_capacity()
    }

    /// A previous `buffer` has not been flushed yet.
    #[inline]
    pub(crate) fn has_pending_flush(&self) -> bool {
        self.needs_flush
    }

    /// Buffer a frame.
    ///
    /// `poll_ready` must be called first to ensure that a frame may be
    /// accepted.
    pub fn buffer(&mut self, item: Frame<B>) -> Result<(), UserError> {
        self.needs_flush = true;
        self.encoder.buffer(item)
    }

    /// Buffer a DATA frame the caller already matched.
    pub(crate) fn buffer_data(&mut self, item: frame::Data<B>) -> Result<(), UserError> {
        self.needs_flush = true;
        self.encoder.buffer_data(item)
    }

    /// One frame, at least `chain_threshold` bytes. The head is stored and
    /// the body stays beside it for `writev`, so this does not take the
    /// small-frame copy path.
    #[inline(always)]
    pub(crate) fn wants_chain(&self, len: usize) -> bool {
        len >= self.encoder.chain_threshold && len <= self.max_frame_size()
    }

    /// Same bytes `buffer_data` would write for a chained payload. Inlined
    /// so the frame is stored into `next` once.
    #[inline(always)]
    pub(crate) fn buffer_chained_data(&mut self, mut v: frame::Data<B>) {
        self.needs_flush = true;
        let len = v.payload().remaining();
        debug_assert!(self.wants_chain(len));
        let head = v.head();
        head.encode_spare(len, self.encoder.buf.get_mut());
        self.encoder.copy_data_prefix(v.payload_mut());
        self.encoder.next = Some(Next::Data(v));
    }

    /// Buffer a HEADERS frame the caller already matched.
    pub(crate) fn buffer_headers(&mut self, item: Box<frame::Headers>) -> Result<(), UserError> {
        self.needs_flush = true;
        self.encoder.buffer_headers(item)
    }

    /// Flush buffered data to the wire
    pub fn flush(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if !self.needs_flush {
            return Poll::Ready(Ok(()));
        }
        loop {
            while !self.encoder.is_empty() {
                let n = match self.encoder.next {
                    Some(Next::Data(ref mut frame)) => {
                        // One contiguous payload is two slices: the encoded
                        // head and the body. Building those directly skips the
                        // cursor chain and its 64-slot vectored scratch.
                        let pos = self.encoder.buf.position() as usize;
                        let head_len = self.encoder.buf.get_ref().len().saturating_sub(pos);
                        let body_all =
                            frame.payload().chunk().len() == frame.payload().remaining();
                        if body_all && self.inner.is_write_vectored() {
                            let n = {
                                let head = &self.encoder.buf.get_ref()[pos..pos + head_len];
                                let body = frame.payload().chunk();
                                if head.is_empty() {
                                    ready!(Pin::new(&mut self.inner).poll_write(cx, body))?
                                } else if body.is_empty() {
                                    ready!(Pin::new(&mut self.inner).poll_write(cx, head))?
                                } else {
                                    let bufs = [IoSlice::new(head), IoSlice::new(body)];
                                    ready!(Pin::new(&mut self.inner).poll_write_vectored(
                                        cx, &bufs
                                    ))?
                                }
                            };
                            let head_wrote = n.min(head_len);
                            if head_wrote > 0 {
                                self.encoder.buf.set_position((pos + head_wrote) as u64);
                            }
                            let body_wrote = n - head_wrote;
                            if body_wrote > 0 {
                                frame.payload_mut().advance(body_wrote);
                            }
                            n
                        } else {
                            let mut buf = (&mut self.encoder.buf).chain(frame.payload_mut());
                            ready!(poll_write_buf(Pin::new(&mut self.inner), cx, &mut buf))?
                        }
                    }
                    _ => {
                        let pos = self.encoder.buf.position() as usize;
                        let n = {
                            let rest = &self.encoder.buf.get_ref()[pos..];
                            if rest.is_empty() {
                                0
                            } else {
                                ready!(Pin::new(&mut self.inner).poll_write(cx, rest))?
                            }
                        };
                        if n > 0 {
                            self.encoder.buf.set_position((pos + n) as u64);
                        }
                        n
                    }
                };
                if n == 0 {
                    // No progress is possible; retrying would busy-loop.
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
            }

            match self.encoder.unset_frame() {
                ControlFlow::Continue => (),
                ControlFlow::Break => break,
            }
        }

        // Flush the upstream
        ready!(Pin::new(&mut self.inner).poll_flush(cx))?;
        self.needs_flush = false;

        Poll::Ready(Ok(()))
    }

    /// Close the codec
    pub fn shutdown(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if !self.final_flush_done {
            ready!(self.flush(cx))?;
            self.final_flush_done = true;
        }
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[must_use]
enum ControlFlow {
    Continue,
    Break,
}

impl<B> Encoder<B>
where
    B: Buf,
{
    /// Clear the bytes just written. A finished DATA frame is parked for
    /// reclaim; a header write leaves `next` empty. Continuation is the only
    /// arm that encodes again, so it stays out of this call.
    #[inline]
    fn unset_frame(&mut self) -> ControlFlow {
        self.buf.set_position(0);
        self.buf.get_mut().clear();

        match self.next.take() {
            Some(Next::Data(frame)) => {
                self.last_data_frame = Some(frame);
                debug_assert!(self.is_empty());
                ControlFlow::Break
            }
            Some(Next::Continuation(frame)) => self.queue_continuation(frame),
            None => ControlFlow::Break,
        }
    }

    #[cold]
    #[inline(never)]
    fn queue_continuation(&mut self, frame: frame::Continuation) -> ControlFlow {
        let mut buf = limited_write_buf!(self);
        if let Some(continuation) = frame.encode(&mut buf) {
            self.next = Some(Next::Continuation(continuation));
        }
        ControlFlow::Continue
    }

    fn buffer(&mut self, item: Frame<B>) -> Result<(), UserError> {
        // The caller already checked. Release builds do not repeat it.
        debug_assert!(self.has_capacity());

        match item {
            Frame::Data(v) => self.buffer_data(v)?,
            Frame::Headers(v) => self.buffer_headers(v)?,
            Frame::PushPromise(v) => {
                let mut buf = limited_write_buf!(self);
                if let Some(continuation) = (*v).encode(&mut self.hpack, &mut buf) {
                    self.next = Some(Next::Continuation(continuation));
                }
            }
            Frame::Settings(v) => {
                v.encode(self.buf.get_mut());
                tracing::trace!(rem = self.buf.remaining(), "encoded settings");
            }
            Frame::GoAway(v) => {
                v.encode(self.buf.get_mut());
                tracing::trace!(rem = self.buf.remaining(), "encoded go_away");
            }
            Frame::Ping(v) => {
                v.encode(self.buf.get_mut());
                tracing::trace!(rem = self.buf.remaining(), "encoded ping");
            }
            Frame::WindowUpdate(v) => {
                v.encode(self.buf.get_mut());
                tracing::trace!(rem = self.buf.remaining(), "encoded window_update");
            }

            Frame::Priority(_) => {
                /*
                v.encode(self.buf.get_mut());
                tracing::trace!("encoded priority; rem={:?}", self.buf.remaining());
                */
                unimplemented!();
            }
            Frame::Reset(v) => {
                v.encode(self.buf.get_mut());
                tracing::trace!(rem = self.buf.remaining(), "encoded reset");
            }
        }

        Ok(())
    }

    fn buffer_data(&mut self, mut v: frame::Data<B>) -> Result<(), UserError> {
        // Ensure that the payload is not greater than the max frame.
        let len = v.payload().remaining();

        if len > self.max_frame_size() {
            return Err(PayloadTooBig);
        }

        if len >= self.chain_threshold {
            let head = v.head();

            // Room for this head was proved by `has_capacity`.
            head.encode_spare(len, self.buf.get_mut());

            self.copy_data_prefix(v.payload_mut());

            // Save the data frame
            self.next = Some(Next::Data(v));
        } else {
            v.encode_chunk(self.buf.get_mut());

            // The chunk has been fully encoded, so there is no need to
            // keep it around
            assert_eq!(v.payload().remaining(), 0, "chunk not fully encoded");

            // Save off the last frame...
            self.last_data_frame = Some(v);
        }
        Ok(())
    }

    #[inline]
    fn copy_data_prefix(&mut self, payload: &mut B) {
        if self.buf.get_ref().remaining() >= self.chain_threshold {
            return;
        }
        let first = payload.chunk().len();
        if first == payload.remaining() {
            return;
        }
        let headroom = self.chain_threshold - self.buf.remaining();
        // For vectored writes, absorbing just the leading protobuf prefix
        // lets a contiguous large tail use the existing two-slice write.
        // Preserve the bounded prefill for nonvectored or larger chunks.
        let copy_len = if self.vectored && first <= MAX_VECTORED_PREFIX {
            headroom.min(first)
        } else {
            headroom
        };
        self.buf.get_mut().put(payload.take(copy_len));
    }

    fn buffer_headers(&mut self, v: Box<frame::Headers>) -> Result<(), UserError> {
        let mut buf = limited_write_buf!(self);
        if let Some(continuation) = frame::Headers::encode_box(v, &mut self.hpack, &mut buf) {
            self.next = Some(Next::Continuation(continuation));
        }
        Ok(())
    }

    fn has_capacity(&self) -> bool {
        self.next.is_none()
            && (self.buf.get_ref().capacity() - self.buf.get_ref().len()
                >= self.min_buffer_capacity)
    }

    fn is_empty(&self) -> bool {
        match self.next {
            Some(Next::Data(ref frame)) => !frame.payload().has_remaining(),
            _ => !self.buf.has_remaining(),
        }
    }
}

impl<B> Encoder<B> {
    fn max_frame_size(&self) -> usize {
        self.max_frame_size as usize
    }
}

impl<T, B> FramedWrite<T, B> {
    /// Returns the max frame size that can be sent
    pub fn max_frame_size(&self) -> usize {
        self.encoder.max_frame_size()
    }

    /// Set the peer's max frame size.
    pub fn set_max_frame_size(&mut self, val: usize) {
        assert!(val <= frame::MAX_MAX_FRAME_SIZE as usize);
        self.encoder.max_frame_size = val as FrameSize;
    }

    /// Set the peer's header table size.
    pub fn set_header_table_size(&mut self, val: usize) {
        self.encoder.hpack.update_max_size(val);
    }

    /// Retrieve the last data frame that has been sent
    pub fn take_last_data_frame(&mut self) -> Option<frame::Data<B>> {
        self.encoder.last_data_frame.take()
    }

    /// Whether flush left a data frame for the prioritizer to reclaim.
    pub fn has_last_data_frame(&self) -> bool {
        self.encoder.last_data_frame.is_some()
    }

    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: AsyncRead + Unpin, B> AsyncRead for FramedWrite<T, B> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

// We never project the Pin to `B`.
impl<T: Unpin, B> Unpin for FramedWrite<T, B> {}

#[cfg(any())]
mod unstable {
    use super::*;

    impl<T, B> FramedWrite<T, B> {
        pub fn get_ref(&self) -> &T {
            &self.inner
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "check encoded HTTP/2 bytes and leading-copy lengths"
)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::future::poll_fn;

    struct ShortWriter {
        bytes: Vec<u8>,
        vectored: bool,
    }

    impl AsyncWrite for ShortWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            let len = bytes.len().min(11);
            self.bytes.extend(bytes.iter().take(len));
            Poll::Ready(Ok(len))
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            slices: &[IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            let start = self.bytes.len();
            self.bytes
                .extend(slices.iter().flat_map(|slice| slice.iter()).take(11));
            Poll::Ready(Ok(self.bytes.len() - start))
        }

        fn is_write_vectored(&self) -> bool {
            self.vectored
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn data_prefill_retains_large_tail_with_a_short_vectored_prefix() {
        const PREFIX: &[u8] = b"gRPC and protobuf prefix";
        let tail = Bytes::from(vec![0xab; 12 * 1024]);
        for optimized in [false, true] {
            for vectored in [false, true] {
                for prefix in [b"".as_slice(), PREFIX] {
                    let io = ShortWriter {
                        bytes: Vec::new(),
                        vectored,
                    };
                    let mut writer = FramedWrite::new(io);
                    let body = Bytes::from_static(prefix).chain(tail.clone());
                    let mut data = frame::Data::new(1_u32.into(), body);
                    data.set_end_stream(true);
                    if optimized {
                        writer.buffer_chained_data(data);
                    } else {
                        writer.buffer(Frame::Data(data)).unwrap();
                    }
                    let expected_head = if vectored || prefix.is_empty() {
                        frame::HEADER_LEN + prefix.len()
                    } else {
                        CHAIN_THRESHOLD_WITHOUT_VECTORED_IO
                    };
                    assert_eq!(writer.encoder.buf.get_ref().len(), expected_head);
                    poll_fn(|cx| writer.flush(cx)).await.unwrap();
                    let encoded = &writer.inner.bytes;
                    let payload = encoded.get(frame::HEADER_LEN..).expect("DATA header");
                    assert_eq!(encoded.get(4).copied(), Some(1), "END_STREAM");
                    assert_eq!(payload.len(), prefix.len() + tail.len());
                    assert!(payload.starts_with(prefix));
                    assert_eq!(payload.get(prefix.len()..).unwrap(), tail.as_ref());
                }
            }
        }
    }
}
