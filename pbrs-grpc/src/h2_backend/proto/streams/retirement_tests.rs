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
    }
}
struct Body {
    bytes: Bytes,
    panic: Option<Arc<AtomicBool>>,
}
impl Body {
    fn tracked(length: usize, probe: &Arc<Probe>) -> Self {
        Self {
            bytes: Bytes::from_owner(Owner {
                bytes: vec![b'x'; length],
                probe: Arc::clone(probe),
            }),
            panic: None,
        }
    }
}
impl Buf for Body {
    fn remaining(&self) -> usize {
        self.bytes.remaining()
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
fn setup(
    window: u32,
) -> (
    Streams<Body, client::Peer>,
    StreamRef<Body>,
    Arc<Probe>,
    Codec<Io, Prioritized<Body>>,
    Arc<Output>,
) {
    let mut streams = Streams::<Body, client::Peer>::new(Config {
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
    });
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
    let engine = Arc::downgrade(&streams.inner);
    let buffer = Arc::downgrade(&streams.send_buffer);
    let probe = Arc::new(Probe {
        drops: AtomicUsize::new(0),
        unlocked: AtomicUsize::new(0),
        available: Box::new(move || {
            engine.upgrade().is_some_and(|inner| available(&inner))
                && buffer.upgrade().is_some_and(|send| available(&send.inner))
        }),
    });
    let output = Arc::new(Output::default());
    let codec = Codec::new(Io(Arc::clone(&output)));
    (streams, send, probe, codec, output)
}
fn complete(
    streams: &mut Streams<Body, client::Peer>,
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
    let status = {
        let mut inner = streams.inner.lock().unwrap();
        inner
            .buffer_pending(&streams.send_buffer, &mut codec, &mut retired)
            .unwrap()
    };
    assert_eq!(status, BufferStatus::CodecFull);
    drop(retired);
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
