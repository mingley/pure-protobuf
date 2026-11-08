//! Bounded fuzz and regression driver for the shipping receive codec.
//!
//! This module is compiled only for unit tests or cargo-fuzz's `fuzzing` cfg.
//! It exercises frame parsing and HPACK/CONTINUATION state, not a connection,
//! stream lifecycle, TLS, sockets, or the proposed custom HTTP/2 engine.

use super::codec::Codec;
use super::frame::{Frame, Pseudo, Reason, StreamId};
use super::proto::{Error, Initiator};
use bytes::Bytes;
use futures_core::Stream;
use http::HeaderMap;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const MAX_INPUT: usize = 64 * 1024;
const MAX_FRAME: usize = 16 * 1024;
const MAX_HEADER_LIST: usize = 4096;
const MAX_HEADER_TABLE: usize = 4096;

/// Exercises the actual native HTTP/2 receive codec with finite in-memory I/O.
pub(crate) fn fuzz_h2_frames(data: &[u8]) {
    let input = &data[..data.len().min(MAX_INPUT)];
    let chunk = 1 + usize::from(input.last().copied().unwrap_or(0) % 31);
    check_fragmentation(input, chunk);
}

fn check_fragmentation(input: &[u8], chunk: usize) {
    assert!(input.len() <= MAX_INPUT);
    let mut contiguous = Driver::new(input, MAX_INPUT, false);
    let mut fragmented = Driver::new(input, chunk, true);
    // Every returned frame consumes at least its nine-byte wire header.
    // Unknown frames and CONTINUATIONs can only reduce the event count.
    for _ in 0..=input.len() / 9 {
        let left = contiguous.next();
        let right = fragmented.next();
        assert_eq!(left, right, "HTTP/2 parsing changed with I/O fragmentation");
        if matches!(left, Event::Eof | Event::Error(_)) {
            return;
        }
        // Drop this pair before reading another frame. Do not retain a transcript.
    }
    panic!("HTTP/2 codec returned more frames than the finite input contains");
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Frame(SemanticFrame),
    Eof,
    Error(Failure),
}

#[derive(Debug, PartialEq, Eq)]
enum SemanticFrame {
    Data {
        stream: StreamId,
        end_stream: bool,
        flow_len: usize,
        payload: Bytes,
    },
    Headers {
        stream: StreamId,
        end_stream: bool,
        over_size: bool,
        parts: Box<(Pseudo, HeaderMap)>,
    },
    PushPromise {
        stream: StreamId,
        promised: StreamId,
        over_size: bool,
        parts: Box<(Pseudo, HeaderMap)>,
    },
    // Control frames have no decoded-header cache or pooling bookkeeping.
    Control(Frame),
}

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Reset(StreamId, Reason, Initiator),
    GoAway(Reason, Initiator),
    Io(io::ErrorKind),
}

fn clear_published_headers() {
    drop(super::header_pool::take_shared_request());
    let _ = super::header_pool::take_stock_https();
}

fn request_parts(headers: super::frame::Headers) -> Box<(Pseudo, HeaderMap)> {
    let mut parts = headers.into_parts();
    // The shipping cache-hit path hands these values to the accept loop
    // separately. Consume them now, before polling the other decoder.
    if let Some(shared) = super::header_pool::take_shared_request() {
        parts.1 = shared.as_ref().clone();
    }
    if let Some(https) = super::header_pool::take_stock_https() {
        let request = super::server::cached_simple_post(https);
        parts.0 = Pseudo::request(request.method().clone(), request.uri().clone(), None);
    }
    Box::new(parts)
}

fn normalize(frame: Frame) -> SemanticFrame {
    match frame {
        Frame::Data(data) => {
            let stream = data.stream_id();
            let end_stream = data.is_end_stream();
            let flow_len = data.flow_controlled_len();
            assert!(!stream.is_zero());
            assert!(flow_len <= MAX_FRAME);
            SemanticFrame::Data {
                stream,
                end_stream,
                flow_len,
                payload: data.into_payload(),
            }
        }
        Frame::Headers(headers) => {
            let stream = headers.stream_id();
            let end_stream = headers.is_end_stream();
            let over_size = headers.is_over_size();
            assert!(!stream.is_zero());
            assert!(headers.is_end_headers());
            // A marked oversize list is a valid codec outcome. The connection
            // decides whether to reset that stream while preserving HPACK state.
            SemanticFrame::Headers {
                stream,
                end_stream,
                over_size,
                parts: request_parts(*headers),
            }
        }
        Frame::PushPromise(push) => {
            let stream = push.stream_id();
            let promised = push.promised_id();
            let over_size = push.is_over_size();
            assert!(!stream.is_zero());
            assert!(push.is_end_headers());
            SemanticFrame::PushPromise {
                stream,
                promised,
                over_size,
                parts: Box::new((*push).into_parts()),
            }
        }
        control => SemanticFrame::Control(control),
    }
}

struct Driver<'a> {
    codec: Codec<MemoryIo<'a>, Bytes>,
    polls_left: usize,
}

impl<'a> Driver<'a> {
    fn new(input: &'a [u8], chunk: usize, yield_between_reads: bool) -> Self {
        let io = MemoryIo {
            input,
            position: 0,
            chunk,
            yield_between_reads,
            pending: false,
            written: 0,
        };
        let mut codec = Codec::new(io);
        codec.set_max_recv_frame_size(MAX_FRAME);
        codec.set_max_recv_header_list_size(MAX_HEADER_LIST);
        codec.set_recv_header_table_size(MAX_HEADER_TABLE);
        Self {
            codec,
            // At most one injected Pending per nonempty read, plus frame
            // events and EOF. A stalled codec becomes a reproducible failure.
            polls_left: 2 * input.len() + input.len() / 9 + 16,
        }
    }

    fn next(&mut self) -> Event {
        clear_published_headers();
        let mut cx = Context::from_waker(Waker::noop());
        while self.polls_left != 0 {
            self.polls_left -= 1;
            match Pin::new(&mut self.codec).poll_next(&mut cx) {
                Poll::Pending => {}
                Poll::Ready(None) => {
                    clear_published_headers();
                    return Event::Eof;
                }
                Poll::Ready(Some(Ok(frame))) => return Event::Frame(normalize(frame)),
                Poll::Ready(Some(Err(error))) => {
                    clear_published_headers();
                    return Event::Error(match error {
                        Error::Reset(stream, reason, initiator) => {
                            Failure::Reset(stream, reason, initiator)
                        }
                        Error::GoAway(_, reason, initiator) => Failure::GoAway(reason, initiator),
                        Error::Io(kind, _) => Failure::Io(kind),
                    });
                }
            }
        }
        panic!("HTTP/2 codec exhausted the finite in-memory poll budget");
    }
}

struct MemoryIo<'a> {
    input: &'a [u8],
    position: usize,
    chunk: usize,
    yield_between_reads: bool,
    pending: bool,
    written: usize,
}

impl AsyncRead for MemoryIo<'_> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if output.remaining() == 0 || this.position == this.input.len() {
            return Poll::Ready(Ok(()));
        }
        if this.pending {
            this.pending = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let count = (this.input.len() - this.position)
            .min(this.chunk)
            .min(output.remaining());
        output.put_slice(&this.input[this.position..this.position + count]);
        this.position += count;
        this.pending = this.yield_between_reads;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for MemoryIo<'_> {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if input.len() > MAX_INPUT - this.written {
            return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
        }
        this.written += input.len();
        Poll::Ready(Ok(input.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let length = payload.len();
        assert!(length <= 0x00ff_ffff);
        let mut wire = vec![
            (length >> 16) as u8,
            (length >> 8) as u8,
            length as u8,
            kind,
            flags,
        ];
        wire.extend_from_slice(&stream.to_be_bytes());
        wire.extend_from_slice(payload);
        wire
    }

    fn checked(wire: &[u8]) {
        for chunk in [1, 2, 3, 8, 9, 16, 31] {
            check_fragmentation(wire, chunk);
        }
    }

    fn first(wire: &[u8]) -> Event {
        Driver::new(wire, MAX_INPUT, false).next()
    }

    fn go_away(reason: Reason) -> Event {
        Event::Error(Failure::GoAway(reason, Initiator::Library))
    }

    fn request_pseudos() -> Vec<u8> {
        // Static HPACK indices: :method POST, :scheme http, :path /.
        vec![0x83, 0x86, 0x84]
    }

    fn string(output: &mut Vec<u8>, value: &[u8]) {
        if value.len() < 127 {
            output.push(value.len() as u8);
        } else {
            output.push(127);
            let mut rest = value.len() - 127;
            while rest >= 128 {
                output.push((rest as u8 & 127) | 128);
                rest >>= 7;
            }
            output.push(rest as u8);
        }
        output.extend_from_slice(value);
    }

    fn indexed_field(value: &[u8]) -> Vec<u8> {
        let mut block = request_pseudos();
        block.push(0x40); // Literal with incremental indexing, new field name.
        string(&mut block, b"x-test");
        string(&mut block, value);
        block
    }

    #[test]
    fn valid_control_and_data_frames_survive_fragmentation() {
        let mut wire = frame(4, 1, 0, b"");
        wire.extend(frame(6, 0, 0, b"12345678"));
        wire.extend(frame(0, 1, 1, b"abc"));
        wire.extend(frame(8, 0, 0, &[0, 0, 0, 1]));
        checked(&wire);
        let mut driver = Driver::new(&wire, 1, true);
        assert!(matches!(
            driver.next(),
            Event::Frame(SemanticFrame::Control(Frame::Settings(_)))
        ));
        assert!(matches!(
            driver.next(),
            Event::Frame(SemanticFrame::Control(Frame::Ping(_)))
        ));
        assert_eq!(
            driver.next(),
            Event::Frame(SemanticFrame::Data {
                stream: StreamId::from(1),
                end_stream: true,
                flow_len: 3,
                payload: Bytes::from_static(b"abc"),
            })
        );
        assert!(matches!(
            driver.next(),
            Event::Frame(SemanticFrame::Control(Frame::WindowUpdate(_)))
        ));
        assert_eq!(driver.next(), Event::Eof);
    }

    #[test]
    fn continuation_and_dynamic_table_references_preserve_header_values() {
        let block = indexed_field(b"abc");
        for split in 1..block.len() {
            let mut wire = frame(1, 0, 1, &block[..split]);
            wire.extend(frame(9, 4, 1, &block[split..]));
            let mut repeated = request_pseudos();
            repeated.push(0xbe); // Dynamic HPACK entry 62: x-test: abc.
            wire.extend(frame(1, 4, 3, &repeated));
            checked(&wire);
            let mut driver = Driver::new(&wire, 1, true);
            for stream in [1, 3] {
                match driver.next() {
                    Event::Frame(SemanticFrame::Headers {
                        stream: actual,
                        over_size,
                        parts,
                        ..
                    }) => {
                        assert_eq!(actual, StreamId::from(stream));
                        assert!(!over_size);
                        assert_eq!(parts.0.method, Some(http::Method::POST));
                        assert_eq!(parts.1.get("x-test").unwrap(), "abc");
                    }
                    other => panic!("expected decoded request headers, got {other:?}"),
                }
            }
            assert_eq!(driver.next(), Event::Eof);
        }
    }

    #[test]
    fn malformed_lengths_padding_and_stream_ids_are_protocol_errors() {
        for wire in [
            frame(6, 0, 0, b"1234567"),
            frame(4, 1, 0, &[0; 6]),
            frame(0, 0, 0, b"abc"),
            frame(0, 8, 1, &[2, 0]),
            frame(1, 4, 0, &request_pseudos()),
            frame(8, 0, 0, &[0; 4]),
        ] {
            checked(&wire);
            assert_eq!(first(&wire), go_away(Reason::PROTOCOL_ERROR));
        }
        let wire = frame(2, 0, 1, &[0, 0, 0, 1, 0]);
        checked(&wire);
        assert_eq!(
            first(&wire),
            Event::Error(Failure::Reset(
                StreamId::from(1),
                Reason::PROTOCOL_ERROR,
                Initiator::Library,
            ))
        );
    }

    #[test]
    fn maximum_frame_oversized_length_and_truncation_are_distinct() {
        let wire = frame(0, 1, 1, &vec![0x5a; MAX_FRAME]);
        checked(&wire);
        match first(&wire) {
            Event::Frame(SemanticFrame::Data {
                flow_len, payload, ..
            }) => {
                assert_eq!(flow_len, MAX_FRAME);
                assert_eq!(payload.len(), MAX_FRAME);
            }
            other => panic!("expected the maximum allowed DATA frame, got {other:?}"),
        }
        let oversized = [0, 0x40, 1];
        checked(&oversized);
        assert_eq!(first(&oversized), go_away(Reason::FRAME_SIZE_ERROR));
        for truncated in [&[0, 0, 1][..], &[0, 0, 1, 0, 0, 0, 0, 0, 1][..]] {
            checked(truncated);
            assert!(matches!(first(truncated), Event::Error(Failure::Io(_))));
        }
        assert_eq!(first(b""), Event::Eof);
    }

    #[test]
    fn continuation_order_stream_identity_and_flood_limits_are_enforced() {
        let unexpected = frame(9, 4, 1, b"");
        let mut interrupted = frame(1, 0, 1, b"");
        interrupted.extend(frame(6, 0, 0, b"12345678"));
        let mut wrong_stream = frame(1, 0, 1, b"");
        wrong_stream.extend(frame(9, 4, 3, b""));
        for wire in [unexpected, interrupted, wrong_stream] {
            checked(&wire);
            assert_eq!(first(&wire), go_away(Reason::PROTOCOL_ERROR));
        }
        let mut flooded = frame(1, 0, 1, b"");
        for _ in 0..6 {
            flooded.extend(frame(9, 0, 1, b""));
        }
        checked(&flooded);
        assert_eq!(first(&flooded), go_away(Reason::ENHANCE_YOUR_CALM));
    }

    #[test]
    fn invalid_hpack_index_is_rejected() {
        let wire = frame(1, 4, 1, &[0x80]); // Indexed header zero is invalid.
        checked(&wire);
        assert_eq!(first(&wire), go_away(Reason::PROTOCOL_ERROR));
    }

    #[test]
    fn oversized_header_lists_still_update_the_dynamic_table() {
        // The entry fits in the 4096-byte dynamic table, while its list plus
        // the pseudo-headers exceeds the 4096-byte header-list setting.
        let mut wire = frame(1, 4, 1, &indexed_field(&vec![b'a'; 4000]));
        let mut repeated = request_pseudos();
        repeated.push(0xbe);
        wire.extend(frame(1, 4, 3, &repeated));
        wire.extend(frame(1, 4, 5, &request_pseudos()));
        checked(&wire);
        let mut driver = Driver::new(&wire, 1, true);
        for expected_over_size in [true, true, false] {
            match driver.next() {
                Event::Frame(SemanticFrame::Headers { over_size, .. }) => {
                    assert_eq!(over_size, expected_over_size);
                }
                other => panic!("expected an HPACK-synchronized header block, got {other:?}"),
            }
        }
        assert_eq!(driver.next(), Event::Eof);
    }
    #[test]
    fn cached_requests_materialize_fields_and_clear_publications() {
        let mut block = request_pseudos();
        for (name, value) in [
            (&b":authority"[..], &b"fuzz.invalid"[..]),
            (&b"content-type"[..], &b"application/grpc"[..]),
            (&b"te"[..], &b"trailers"[..]),
            (&b"grpc-accept-encoding"[..], &b"identity,gzip,deflate"[..]),
            (&b"user-agent"[..], &b"fuzz-parser-control"[..]),
        ] {
            block.push(0); // Literal without indexing: a cacheable table-neutral block.
            string(&mut block, name);
            string(&mut block, value);
        }
        let mut wire = frame(1, 4, 1, &block);
        wire.extend(frame(1, 4, 3, &block));
        checked(&wire);
        let mut driver = Driver::new(&wire, 1, true);
        for _ in 0..2 {
            match driver.next() {
                Event::Frame(SemanticFrame::Headers { parts, .. }) => {
                    assert_eq!(parts.0.method, Some(http::Method::POST));
                    assert_eq!(parts.1.len(), 4);
                    assert_eq!(parts.1.get("content-type").unwrap(), "application/grpc");
                    assert_eq!(parts.1.get("user-agent").unwrap(), "fuzz-parser-control");
                }
                other => panic!("expected materialized request headers, got {other:?}"),
            }
        }
        assert_eq!(driver.next(), Event::Eof);
        assert!(super::super::header_pool::take_shared_request().is_none());
        assert!(super::super::header_pool::take_stock_https().is_none());

        // A prior partial/error path must not lend publications to a new run.
        super::super::header_pool::publish_shared_request(std::sync::Arc::new(HeaderMap::new()));
        super::super::header_pool::publish_stock_https(false);
        fuzz_h2_frames(b"");
        assert!(super::super::header_pool::take_shared_request().is_none());
        assert!(super::super::header_pool::take_stock_https().is_none());
    }

}
