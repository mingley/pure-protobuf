//! Actual engine/codec retirement controls, including staging unwind and IO loss.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_types,
    clippy::indexing_slicing,
    reason = "bounded internal engine ownership and wire-frame witnesses"
)]
use super::*;
use std::cell::Cell;
use std::pin::Pin;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::io::{AsyncRead, ReadBuf};

#[derive(Default)]
struct Output {
    bytes: StdMutex<Vec<u8>>,
    writes: AtomicUsize,
    flushes: AtomicUsize,
    fail: AtomicBool,
}
struct Io(Arc<Output>);
impl AsyncRead for Io {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}
impl AsyncWrite for Io {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.0.fail.load(Ordering::Acquire) {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        self.0.writes.fetch_add(1, Ordering::AcqRel);
        self.0.bytes.lock().unwrap().extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.flushes.fetch_add(1, Ordering::AcqRel);
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct Probe {
    drops: AtomicUsize,
    unlocked: AtomicUsize,
    available: Box<dyn Fn() -> bool + std::marker::Send + Sync>,
}
struct Owner {
    bytes: Vec<u8>,
    probe: Arc<Probe>,
    panic_on_drop: bool,
}
impl AsRef<[u8]> for Owner {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.probe.drops.fetch_add(1, Ordering::AcqRel);
        if (self.probe.available)() {
            self.probe.unlocked.fetch_add(1, Ordering::AcqRel);
        }
        assert!(!self.panic_on_drop, "controlled queued owner drop panic");
    }
}
#[derive(Default)]
struct Body {
    bytes: Bytes,
    panic: Option<Arc<AtomicBool>>,
    remaining_override: Option<usize>,
    panic_remaining_on: Option<usize>,
    remaining_calls: Cell<usize>,
}
impl Body {
    fn tracked(length: usize, probe: &Arc<Probe>) -> Self {
        Self {
            bytes: Bytes::from_owner(Owner {
                bytes: vec![b'x'; length],
                probe: Arc::clone(probe),
                panic_on_drop: false,
            }),
            ..Self::default()
        }
    }
}
impl Buf for Body {
    fn remaining(&self) -> usize {
        let call = self.remaining_calls.get() + 1;
        self.remaining_calls.set(call);
        assert_ne!(
            self.panic_remaining_on,
            Some(call),
            "controlled payload remaining panic"
        );
        self.remaining_override
            .unwrap_or_else(|| self.bytes.remaining())
    }
    fn chunk(&self) -> &[u8] {
        if self
            .panic
            .as_ref()
            .is_some_and(|armed| armed.load(Ordering::Acquire))
        {
            panic!("controlled second-frame staging panic");
        }
        self.bytes.chunk()
    }
    fn advance(&mut self, count: usize) {
        self.bytes.advance(count);
    }
}
fn available<T>(mutex: &Mutex<T>) -> bool {
    #[cfg(target_os = "linux")]
    {
        mutex.try_lock().is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        match mutex.try_lock() {
            Ok(_) | Err(std::sync::TryLockError::Poisoned(_)) => true,
            Err(std::sync::TryLockError::WouldBlock) => false,
        }
    }
}
fn engine_config(window: u32) -> Config {
    Config {
        initial_max_send_streams: 100,
        local_max_buffer_size: 1024 * 1024,
        local_next_stream_id: StreamId::from(1),
        local_push_enabled: false,
        extended_connect_protocol_enabled: false,
        local_reset_duration: Duration::from_secs(1),
        local_reset_max: 10,
        remote_reset_max: 10,
        remote_init_window_sz: window,
        remote_max_initiated: Some(100),
        local_max_error_reset_streams: Some(100),
        data_frame_budget: 25_600,
    }
}

fn lock_probe<P: Peer>(streams: &Streams<Body, P>) -> Arc<Probe> {
    let engine = Arc::downgrade(&streams.inner);
    let buffer = Arc::downgrade(&streams.send_buffer);
    Arc::new(Probe {
        drops: AtomicUsize::new(0),
        unlocked: AtomicUsize::new(0),
        available: Box::new(move || {
            engine.upgrade().is_some_and(|inner| available(&inner))
                && buffer.upgrade().is_some_and(|send| available(&send.inner))
        }),
    })
}

fn setup(
    window: u32,
) -> (
    Streams<Body, client::Peer>,
    StreamRef<Body>,
    Arc<Probe>,
    Codec<Io, Prioritized<Body>>,
    Arc<Output>,
) {
    let mut streams = Streams::<Body, client::Peer>::new(engine_config(window));
    let (send, pending) = streams
        .send_request(
            Request::builder()
                .uri("http://localhost/retirement")
                .body(())
                .unwrap(),
            false,
            None,
        )
        .unwrap();
    assert!(!pending);
    let probe = lock_probe(&streams);
    let output = Arc::new(Output::default());
    let codec = Codec::new(Io(Arc::clone(&output)));
    (streams, send, probe, codec, output)
}
fn complete<P: Peer>(
    streams: &mut Streams<Body, P>,
    codec: &mut Codec<Io, Prioritized<Body>>,
) -> io::Result<()> {
    match streams.poll_complete(&mut Context::from_waker(Waker::noop()), codec, false) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("in-memory IO must not park"),
    }
}
fn data(output: &Output) -> Vec<(u8, Vec<u8>)> {
    let raw = output.bytes.lock().unwrap();
    let mut bytes = raw.as_slice();
    let mut frames = Vec::new();
    while !bytes.is_empty() {
        let (head, rest) = bytes.split_at_checked(9).unwrap();
        let length =
            (usize::from(head[0]) << 16) | (usize::from(head[1]) << 8) | usize::from(head[2]);
        let (body, tail) = rest.split_at_checked(length).unwrap();
        if head[3] == 0 {
            frames.push((head[4], body.to_vec()));
        }
        bytes = tail;
    }
    frames
}
fn observed(probe: &Probe, count: usize) {
    assert_eq!(probe.drops.load(Ordering::Acquire), count);
    assert_eq!(
        probe.unlocked.load(Ordering::Acquire),
        count,
        "owner must acquire actual engine AND send-buffer locks during Drop"
    );
}

#[derive(Clone, Copy)]
enum Rejection {
    Reset,
    HalfClosed,
    Closed,
}

fn rejected_owner_case(rejection: Rejection, try_send: bool) {
    for length in [0, 64] {
        for end in [false, true] {
            let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
            complete(&mut streams, &mut codec).unwrap();
            match rejection {
                Rejection::Reset => send.send_reset(Reason::CANCEL),
                Rejection::HalfClosed | Rejection::Closed => {
                    send.send_data(Body::default(), true).unwrap();
                    complete(&mut streams, &mut codec).unwrap();
                    if matches!(rejection, Rejection::Closed) {
                        streams.recv_eof(true).unwrap();
                    }
                }
            }
            let body = Body::tracked(length, &probe);
            if try_send {
                assert!(matches!(send.try_send_data(body, end), Ok(Err(_))));
            } else {
                assert!(send.send_data(body, end).is_err());
            }
            observed(&probe, 1);
            streams.recv_eof(true).unwrap();
        }
    }
}

#[test]
fn rejected_direct_reset_payload_is_dropped_after_both_guards() {
    rejected_owner_case(Rejection::Reset, false);
}

#[test]
fn rejected_direct_half_closed_payload_is_dropped_after_both_guards() {
    rejected_owner_case(Rejection::HalfClosed, false);
}

#[test]
fn rejected_direct_closed_payload_is_dropped_after_both_guards() {
    rejected_owner_case(Rejection::Closed, false);
}

#[test]
fn rejected_try_reset_payload_is_dropped_after_both_guards() {
    rejected_owner_case(Rejection::Reset, true);
}

#[test]
fn rejected_try_half_closed_payload_is_dropped_after_both_guards() {
    rejected_owner_case(Rejection::HalfClosed, true);
}

#[test]
fn rejected_try_closed_payload_is_dropped_after_both_guards() {
    rejected_owner_case(Rejection::Closed, true);
}

#[test]
fn rejected_virtual_oversize_keeps_size_error_precedence_without_allocating_its_claim() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    send.send_data(Body::default(), true).unwrap();
    complete(&mut streams, &mut codec).unwrap();
    let mut body = Body::tracked(1, &probe);
    body.remaining_override = Some(0x8000_0000);
    assert_eq!(body.bytes.len(), 1);
    let error = send.send_data(body, true).unwrap_err();
    assert!(error.to_string().contains("payload too big"));
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn rejected_full_try_returns_original_payload_without_dropping_it() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    let mut body = Body::tracked(1, &probe);
    body.remaining_override = Some(1024 * 1024 + 1);
    let pointer = body.bytes.as_ptr();
    let returned = send.try_send_data(body, false).unwrap_err();
    assert_eq!(returned.bytes.as_ptr(), pointer);
    assert_eq!(returned.remaining_override, Some(1024 * 1024 + 1));
    observed(&probe, 0);
    drop(returned);
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

fn rejected_remaining_panic_case(try_send: bool) {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    let mut body = Body::tracked(1, &probe);
    body.panic_remaining_on = Some(if try_send { 2 } else { 1 });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if try_send {
            let _result = send.try_send_data(body, false);
        } else {
            let _result = send.send_data(body, false);
        }
    }));
    #[cfg(not(target_os = "linux"))]
    {
        streams.inner.clear_poison();
        streams.send_buffer.inner.clear_poison();
    }
    assert!(result.is_err());
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn rejected_direct_remaining_panic_drops_payload_after_both_guards() {
    rejected_remaining_panic_case(false);
}

#[test]
fn rejected_try_remaining_panic_drops_payload_after_both_guards() {
    rejected_remaining_panic_case(true);
}

fn rejected_terminal_data_case(before_headers: bool) {
    let mut config = engine_config(65_535);
    config.local_next_stream_id = StreamId::from(2);
    let mut streams = Streams::<Body, server::Peer>::new(config);
    let request = Request::builder()
        .uri("http://localhost/terminal")
        .body(())
        .unwrap();
    streams
        .recv_headers(
            client::Peer::convert_send_message(StreamId::from(1), request, None, false).unwrap(),
        )
        .unwrap();
    let mut send = streams.next_incoming().unwrap();
    let probe = lock_probe(&streams);
    let output = Arc::new(Output::default());
    let mut codec = Codec::new(Io(output));
    if !before_headers {
        send.send_response(Response::new(()), false).unwrap();
        complete(&mut streams, &mut codec).unwrap();
    }
    let mut body = Body::tracked(1, &probe);
    body.remaining_override = Some(0x8000_0000);
    let result = send.finish_response(
        Response::new(()),
        HeaderMap::new(),
        Some(body),
        server::DeadlineEnding::Data,
    );
    assert!(result.unwrap_err().to_string().contains("payload too big"));
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn rejected_terminal_data_before_headers_drops_payload_after_both_guards() {
    rejected_terminal_data_case(true);
}

#[test]
fn rejected_terminal_data_after_headers_drops_payload_after_both_guards() {
    rejected_terminal_data_case(false);
}

#[test]
fn padded_received_data_keeps_payload_and_releases_padding_credit() {
    let (mut streams, mut send, _, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    streams
        .recv_headers(Box::new(frame::Headers::new(
            StreamId::from(1),
            frame::Pseudo::response(http::StatusCode::OK),
            HeaderMap::new(),
        )))
        .unwrap();
    let padded = frame::Data::load(
        frame::Head::new(frame::Kind::Data, 0x9, StreamId::from(1)),
        Bytes::from_static(&[2, b'x', 0, 0]),
    )
    .unwrap();
    streams.recv_data(padded).unwrap();
    let cx = Context::from_waker(Waker::noop());
    assert!(matches!(send.opaque.poll_response(&cx), Poll::Ready(Ok(_))));
    assert!(
        matches!(send.opaque.poll_data(&cx), Poll::Ready(Some(Ok(bytes))) if bytes == b"x"[..])
    );
    send.opaque.release_capacity(1).unwrap();
    assert_eq!(send.opaque.used_recv_capacity(), 0);
    streams.recv_eof(false).unwrap();
}

fn accepted_terminal_case(before_headers: bool, trailers: bool) {
    let mut config = engine_config(65_535);
    config.local_next_stream_id = StreamId::from(2);
    let mut streams = Streams::<Body, server::Peer>::new(config);
    let request = Request::builder()
        .uri("http://localhost/terminal")
        .body(())
        .unwrap();
    streams
        .recv_headers(
            client::Peer::convert_send_message(StreamId::from(1), request, None, false).unwrap(),
        )
        .unwrap();
    let mut send = streams.next_incoming().unwrap();
    let probe = lock_probe(&streams);
    let output = Arc::new(Output::default());
    let mut codec = Codec::new(Io(Arc::clone(&output)));
    if !before_headers {
        send.send_response(Response::new(()), false).unwrap();
        complete(&mut streams, &mut codec).unwrap();
    }
    let body = Body::tracked(64, &probe);
    let ending = if trailers {
        send.send_data(body, false).unwrap();
        None
    } else {
        Some(body)
    };
    let ending_kind = if trailers {
        server::DeadlineEnding::Trailers
    } else {
        server::DeadlineEnding::Data
    };
    assert!(
        send.finish_response(Response::new(()), HeaderMap::new(), ending, ending_kind)
            .unwrap()
    );
    complete(&mut streams, &mut codec).unwrap();
    if trailers {
        assert!(
            data(&output).is_empty(),
            "queued body must be discarded before terminal trailers"
        );
        let raw = output.bytes.lock().unwrap();
        let mut bytes = raw.as_slice();
        let mut headers = Vec::new();
        while !bytes.is_empty() {
            let (head, rest) = bytes.split_at_checked(9).unwrap();
            let length =
                (usize::from(head[0]) << 16) | (usize::from(head[1]) << 8) | usize::from(head[2]);
            if head[3] == 1 {
                headers.push(head[4]);
            }
            bytes = rest.split_at_checked(length).unwrap().1;
        }
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0] & 1, 0);
        assert_eq!(headers[1] & 1, 1);
    } else {
        assert_eq!(data(&output), vec![(1, vec![b'x'; 64])]);
    }
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn accepted_terminal_data_keeps_payload_and_end_stream_before_and_after_headers() {
    accepted_terminal_case(true, false);
    accepted_terminal_case(false, false);
}

#[test]
fn accepted_terminal_trailers_discard_queued_body_and_keep_header_order() {
    accepted_terminal_case(false, true);
}

#[test]
fn excess_reserved_capacity_is_reclaimed_before_end_stream_data() {
    let (mut streams, mut send, probe, mut codec, output) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    send.reserve_capacity(1_024);
    send.send_data(Body::tracked(64, &probe), true).unwrap();
    complete(&mut streams, &mut codec).unwrap();
    let frames = data(&output);
    assert_eq!(frames, vec![(1, vec![b'x'; 64])]);
    observed(&probe, 1);
    streams.recv_eof(false).unwrap();
}

#[test]
fn reset_discards_queued_payload_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    send.send_reset(Reason::CANCEL);
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn peer_reset_discards_queued_payload_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    streams
        .as_dyn()
        .recv_reset(frame::Reset::new(StreamId::from(1), Reason::CANCEL))
        .unwrap();
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn eof_discards_queued_payload_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    streams.recv_eof(true).unwrap();
    observed(&probe, 1);
}

#[test]
fn goaway_discards_only_unprocessed_stream_payloads_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, output) = setup(65_535);
    let (mut later, pending) = streams
        .send_request(
            Request::builder()
                .uri("http://localhost/later")
                .body(())
                .unwrap(),
            false,
            None,
        )
        .unwrap();
    assert!(!pending);
    complete(&mut streams, &mut codec).unwrap();
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    later.send_data(Body::tracked(96, &probe), false).unwrap();
    streams
        .as_dyn()
        .recv_go_away(&frame::GoAway::new(StreamId::from(1), Reason::NO_ERROR))
        .unwrap();
    observed(&probe, 1);
    complete(&mut streams, &mut codec).unwrap();
    observed(&probe, 2);
    assert_eq!(data(&output), vec![(0, vec![b'x'; 64])]);
    streams.recv_eof(true).unwrap();
}

#[test]
fn window_error_discards_queued_payload_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    streams
        .as_dyn()
        .recv_window_update(frame::WindowUpdate::new(StreamId::from(1), 0x7fff_ffff))
        .unwrap();
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn connection_error_discards_all_queued_payloads_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    for _ in 0..17 {
        send.send_data(Body::tracked(64, &probe), false).unwrap();
    }
    streams
        .as_dyn()
        .handle_error(Error::library_go_away(Reason::INTERNAL_ERROR));
    observed(&probe, 17);
    streams.recv_eof(true).unwrap();
}

#[test]
fn queued_owner_panic_drops_remaining_payloads_with_engine_unlocked() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    let panics = Body {
        bytes: Bytes::from_owner(Owner {
            bytes: vec![b'x'; 64],
            probe: Arc::clone(&probe),
            panic_on_drop: true,
        }),
        ..Body::default()
    };
    send.send_data(panics, false).unwrap();
    send.send_data(Body::tracked(96, &probe), false).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        send.send_reset(Reason::CANCEL);
    }));
    assert!(result.is_err(), "controlled owner must panic");
    observed(&probe, 2);
    assert_eq!(send.capacity(), 0);
    let later = Body {
        bytes: Bytes::from_static(b"later"),
        ..Body::default()
    };
    assert!(send.send_data(later, false).is_err());
    observed(&probe, 2);
    streams.recv_eof(true).unwrap();
}

#[test]
fn settings_window_error_discards_queued_payload_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    streams
        .as_dyn()
        .recv_window_update(frame::WindowUpdate::new(
            StreamId::from(1),
            0x7fff_ffff - 65_535,
        ))
        .unwrap();
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    let mut settings = frame::Settings::default();
    settings.set_initial_window_size(Some(65_536));
    assert!(streams.apply_remote_settings(&settings, false).is_err());
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn scheduled_reset_discards_queued_payload_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, output) = setup(65_535);
    complete(&mut streams, &mut codec).unwrap();
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    {
        let mut inner = streams.inner.lock().unwrap();
        let me = &mut *inner;
        let mut stream = me.store.resolve(send.opaque.key);
        me.actions.send.schedule_implicit_reset(
            &mut stream,
            Reason::CANCEL,
            &mut me.counts,
            &mut me.actions.task,
        );
    }
    complete(&mut streams, &mut codec).unwrap();
    observed(&probe, 1);
    assert!(data(&output).is_empty());
    streams.recv_eof(true).unwrap();
}

#[test]
fn pending_open_reset_preserves_headers_before_wire_reset() {
    let (mut streams, mut send, probe, mut codec, output) = setup(65_535);
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    send.send_reset(Reason::CANCEL);
    assert_eq!(probe.drops.load(Ordering::Acquire), 0);
    complete(&mut streams, &mut codec).unwrap();
    observed(&probe, 1);
    let bytes = output.bytes.lock().unwrap();
    let head = bytes.get(..9).unwrap();
    assert_eq!(head[3], 1, "HEADERS opens the idle stream before reset");
    assert_eq!(u32::from_be_bytes(head[5..9].try_into().unwrap()), 1);
    let mut wire = bytes.as_slice();
    let mut kinds = Vec::new();
    let mut resets = 0;
    while !wire.is_empty() {
        let (head, rest) = wire.split_at_checked(9).unwrap();
        let length =
            (usize::from(head[0]) << 16) | (usize::from(head[1]) << 8) | usize::from(head[2]);
        let (payload, rest) = rest.split_at_checked(length).unwrap();
        kinds.push(head[3]);
        if head[3] == 3 {
            resets += 1;
            assert_eq!(u32::from_be_bytes(head[5..9].try_into().unwrap()), 1);
            assert_eq!(payload, &u32::from(Reason::CANCEL).to_be_bytes());
        }
        wire = rest;
    }
    assert_eq!(resets, 1);
    assert_eq!(kinds.last(), Some(&3), "reset must follow opening HEADERS");
    drop(bytes);
    streams.recv_eof(true).unwrap();
}

#[test]
fn multiple_retirement_batches_preserve_one_encoded_write_and_flush() {
    let (mut streams, mut send, probe, mut codec, output) = setup(65_535);
    for index in 0..33 {
        send.send_data(Body::tracked(64, &probe), index == 32)
            .unwrap();
    }
    complete(&mut streams, &mut codec).unwrap();
    observed(&probe, 33);
    let frames = data(&output);
    assert_eq!(frames.len(), 33);
    assert!(frames.iter().all(|(_, bytes)| bytes == &vec![b'x'; 64]));
    assert!(frames.iter().take(32).all(|(flags, _)| flags & 1 == 0));
    assert_eq!(frames.last().unwrap().0 & 1, 1);
    assert_eq!(
        output.writes.load(Ordering::Acquire),
        1,
        "retirement boundaries must not force writes"
    );
    assert_eq!(
        output.flushes.load(Ordering::Acquire),
        1,
        "retirement boundaries must not force flushes"
    );
    eprintln!(
        "retirement scratch: Bytes={} Body={}",
        std::mem::size_of::<RetiredData<Bytes>>(),
        std::mem::size_of::<RetiredData<Body>>()
    );
    streams.recv_eof(true).unwrap();
}

#[test]
fn partial_credit_keeps_owner_and_restores_end_stream_only_on_tail() {
    let (mut streams, mut send, probe, mut codec, output) = setup(16);
    send.send_data(Body::tracked(40, &probe), true).unwrap();
    complete(&mut streams, &mut codec).unwrap();
    observed(&probe, 0);
    let prefix = data(&output);
    assert_eq!(prefix.iter().map(|(_, b)| b.len()).sum::<usize>(), 16);
    assert!(prefix.iter().all(|(flags, _)| flags & 1 == 0));
    complete(&mut streams, &mut codec).unwrap();
    assert_eq!(data(&output), prefix, "no progress before WINDOW_UPDATE");
    streams
        .as_dyn()
        .recv_window_update(frame::WindowUpdate::new(StreamId::from(0), 24))
        .unwrap();
    streams
        .as_dyn()
        .recv_window_update(frame::WindowUpdate::new(StreamId::from(1), 24))
        .unwrap();
    complete(&mut streams, &mut codec).unwrap();
    observed(&probe, 1);
    let frames = data(&output);
    assert_eq!(frames.iter().map(|(_, b)| b.len()).sum::<usize>(), 40);
    assert_eq!(frames.last().unwrap().0 & 1, 1);
    streams.recv_eof(true).unwrap();
}

fn force_postflush(payload: usize, window: u32) {
    let (mut streams, mut send, probe, mut codec, output) = setup(window);
    send.send_data(Body::tracked(payload, &probe), true)
        .unwrap();
    let mut retired = RetiredData::new();
    let mut discarded = Vec::new();
    let status = {
        let mut inner = streams.inner.lock().unwrap();
        inner
            .buffer_pending(
                &streams.send_buffer,
                &mut codec,
                &mut retired,
                &mut discarded,
            )
            .unwrap()
    };
    assert_eq!(status, BufferStatus::CodecFull);
    drop(retired);
    drop(discarded);
    assert!(matches!(
        codec.flush(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    ));
    assert!(codec.has_last_data_frame());
    observed(&probe, 0);
    let mut retired = RetiredData::new();
    let partial = {
        let mut inner = streams.inner.lock().unwrap();
        inner.reclaim_written_frame(&streams.send_buffer, &mut codec, &mut retired)
    };
    assert_eq!(
        partial,
        payload > window as usize,
        "postflush helper must inspect original tail, not exhausted HTTP2 chunk limit"
    );
    observed(&probe, 0);
    drop(retired);
    if partial {
        observed(&probe, 0);
        let prefix = data(&output);
        assert!(prefix.iter().all(|(flags, _)| flags & 1 == 0));
        let remaining = u32::try_from(payload).unwrap() - window;
        streams
            .as_dyn()
            .recv_window_update(frame::WindowUpdate::new(StreamId::from(0), remaining))
            .unwrap();
        streams
            .as_dyn()
            .recv_window_update(frame::WindowUpdate::new(StreamId::from(1), remaining))
            .unwrap();
        complete(&mut streams, &mut codec).unwrap();
    }
    observed(&probe, 1);
    let frames = data(&output);
    assert_eq!(frames.iter().map(|(_, b)| b.len()).sum::<usize>(), payload);
    assert_eq!(frames.last().unwrap().0 & 1, 1);
    streams.recv_eof(true).unwrap();
}
#[test]
fn postflush_consumed_owner_is_retired_outside_engine() {
    force_postflush(16_384, 16_384);
}
#[test]
fn postflush_partial_owner_is_requeued_with_original_eos() {
    force_postflush(20_000, 16_384);
}

#[test]
fn staging_panic_releases_previously_retired_owner_after_both_guards() {
    let (mut streams, mut send, probe, mut codec, _) = setup(65_535);
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    let armed = Arc::new(AtomicBool::new(false));
    send.send_data(
        Body {
            bytes: Bytes::from_static(&[7; 64]),
            panic: Some(Arc::clone(&armed)),
            ..Body::default()
        },
        true,
    )
    .unwrap();
    armed.store(true, Ordering::Release);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        complete(&mut streams, &mut codec)
    }));
    #[cfg(not(target_os = "linux"))]
    {
        streams.inner.clear_poison();
        streams.send_buffer.inner.clear_poison();
    }
    assert!(
        result.is_err(),
        "controlled second frame must panic during staging"
    );
    observed(&probe, 1);
    streams.recv_eof(true).unwrap();
}

#[test]
fn io_failure_and_codec_teardown_release_retired_and_chained_owners_unlocked() {
    let (mut streams, mut send, probe, mut codec, output) = setup(65_535);
    send.send_data(Body::tracked(64, &probe), false).unwrap();
    send.send_data(Body::tracked(16_384, &probe), true).unwrap();
    output.fail.store(true, Ordering::Release);
    assert_eq!(
        complete(&mut streams, &mut codec).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    observed(&probe, 1); // copied owner was drained before failed IO
    drop(codec); // the still-chained codec owner is reclaimed at teardown
    observed(&probe, 2);
    assert!(output.bytes.lock().unwrap().is_empty());
    streams.recv_eof(true).unwrap();
}
