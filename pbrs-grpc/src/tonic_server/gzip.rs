//! Finite inbound gzip validation over retained original DATA views.

use crate::{MessageLimits, Status};
use bytes::Bytes;
use flate2::bufread::GzDecoder;
use http_body::{Body, Frame};
use std::io::{self, BufRead, Read};
use std::pin::Pin;
use std::task::{Context, Poll};

// Bounds each synchronous inflater/header-parser invocation before yielding.
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
