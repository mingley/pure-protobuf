// Embedded adaptation of the frozen additive-only eighteen-test harness.
// Original standalone results do not qualify this embedded adaptation.
//
// Compose as a child module of the existing admission_tests.rs, for example
// with a #[path = "../admission-review/admission_additional_tests.rs"] module
// declaration in a NEW test harness copy. Do not rewrite archived harnesses.
// It reuses only the existing request(), pending_request(), Frame, DropGuard,
// WakeGuard, Opened, and imports. All fixtures are current-thread/paused-clock.
//
// Targets the additive-only design: no old Sender::poll_ready invocation and
// no per-Sender pending rejection in the NEW inner poll_send_request. Original
// send_request/poll_ready/pending fields and reset semantics remain intact.
// This draft is not evidence of passing tests or FIFO pre-admission fairness.

use super::*;
use std::collections::VecDeque;
use tokio::sync::{mpsc, oneshot};

const BOUND: Duration = Duration::from_secs(2);

// Acknowledged pause fences polling the entire Connection, not just IO writes.
// Blocking an AsyncWrite alone would allow pending->active promotion before the
// fence and would not establish the intended after-admission/before-writer state.
struct DriverCommand {
    paused: bool,
    acknowledged: oneshot::Sender<()>,
}

struct Driver {
    commands: mpsc::UnboundedSender<DriverCommand>,
    task: tokio::task::JoinHandle<()>,
}

impl Driver {
    async fn pause(&self, paused: bool) {
        let (acknowledged, ack) = oneshot::channel();
        self.commands
            .send(DriverCommand {
                paused,
                acknowledged,
            })
            .expect("driver exited before fence");
        tokio::time::timeout(BOUND, ack)
            .await
            .expect("driver fence stalled")
            .expect("driver exited before acknowledging fence");
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// Preserve all frames preceding SETTINGS ACK, rather than silently discarding
// HEADERS/DATA/RST. The buffered parser also preserves partial frame bytes when
// quiet() cancels a pending read; AsyncReadExt::read is cancellation-safe.
struct Peer {
    io: DuplexStream,
    buffered: Vec<u8>,
    observed: VecDeque<Frame>,
}

impl Peer {
    fn take_buffered_frame(&mut self) -> Option<Frame> {
        if self.buffered.len() < 9 {
            return None;
        }
        let length = usize::from(self.buffered[0]) * 65536
            + usize::from(self.buffered[1]) * 256
            + usize::from(self.buffered[2]);
        assert!(length <= 16384, "unexpected oversized fixture frame");
        if self.buffered.len() < 9 + length {
            return None;
        }
        let next = Frame {
            kind: self.buffered[3],
            flags: self.buffered[4],
            id: u32::from_be_bytes(self.buffered[5..9].try_into().unwrap()) & 0x7fff_ffff,
            payload: self.buffered[9..9 + length].to_vec(),
        };
        self.buffered.drain(..9 + length);
        Some(next)
    }

    async fn read_frame(&mut self) -> Frame {
        loop {
            if let Some(next) = self.take_buffered_frame() {
                return next;
            }
            let mut chunk = [0; 4096];
            let read = self.io.read(&mut chunk).await.unwrap();
            assert!(read != 0, "peer EOF while waiting for frame");
            self.buffered.extend_from_slice(&chunk[..read]);
        }
    }

    async fn write_frame(&mut self, kind: u8, flags: u8, id: u32, payload: &[u8]) {
        assert!(payload.len() <= 16384);
        assert_eq!(id & 0x8000_0000, 0);
        let length = payload.len() as u32;
        let mut encoded = Vec::with_capacity(9 + payload.len());
        encoded.extend_from_slice(&length.to_be_bytes()[1..]);
        encoded.extend_from_slice(&[kind, flags]);
        encoded.extend_from_slice(&id.to_be_bytes());
        encoded.extend_from_slice(payload);
        self.io.write_all(&encoded).await.unwrap();
        self.io.flush().await.unwrap();
    }

    async fn settings(&mut self, values: &[(u16, u32)]) {
        let mut payload = Vec::with_capacity(6 * values.len());
        for (key, value) in values {
            payload.extend_from_slice(&key.to_be_bytes());
            payload.extend_from_slice(&value.to_be_bytes());
        }
        self.write_frame(4, 0, 0, &payload).await;
        tokio::time::timeout(BOUND, async {
            loop {
                let next = self.read_frame().await;
                if next.kind == 4 && next.flags == 1 {
                    assert_eq!(next.id, 0);
                    assert!(next.payload.is_empty());
                    return;
                }
                self.observed.push_back(next);
            }
        })
        .await
        .expect("SETTINGS ACK stalled");
    }

    async fn stream_frame(&mut self) -> Frame {
        tokio::time::timeout(BOUND, async {
            loop {
                let next = match self.observed.pop_front() {
                    Some(next) => next,
                    None => self.read_frame().await,
                };
                if next.id != 0 {
                    return next;
                }
                assert_ne!(next.kind, 7, "unexpected connection GOAWAY: {next:?}");
            }
        })
        .await
        .expect("stream frame stalled")
    }

    async fn expect_headers(&mut self, id: u32) {
        let next = self.stream_frame().await;
        assert_eq!((next.kind, next.id), (1, id), "{next:?}");
        assert_ne!(next.flags & 4, 0, "fixture HEADERS must be complete");
    }

    async fn quiet(&mut self) {
        while let Some(next) = self.observed.pop_front() {
            assert_eq!(next.id, 0, "unexpected queued stream frame: {next:?}");
            assert_ne!(next.kind, 7, "unexpected connection GOAWAY: {next:?}");
        }
        while let Ok(next) =
            tokio::time::timeout(Duration::from_millis(30), self.read_frame()).await
        {
            assert_eq!(next.id, 0, "unexpected stream frame: {next:?}");
            assert_ne!(next.kind, 7, "unexpected connection GOAWAY: {next:?}");
        }
    }

    async fn no_stream_until_eof_or_quiet(&mut self) {
        // GOAWAY may end the Connection task and close IO before the silence
        // timer. EOF is allowed here, but a partial frame or any stream frame
        // remains a failure. This helper is used only after terminal GOAWAY.
        loop {
            while let Some(next) = self
                .observed
                .pop_front()
                .or_else(|| self.take_buffered_frame())
            {
                assert_eq!(next.id, 0, "stream escaped terminal GOAWAY: {next:?}");
            }
            let mut chunk = [0; 4096];
            match tokio::time::timeout(Duration::from_millis(30), self.io.read(&mut chunk)).await {
                Err(_) => return,
                Ok(Ok(0)) => {
                    assert!(self.buffered.is_empty(), "partial frame before EOF");
                    return;
                }
                Ok(Ok(read)) => self.buffered.extend_from_slice(&chunk[..read]),
                Ok(Err(error)) => panic!("unexpected fixture IO error: {error}"),
            }
        }
    }

    async fn reset(&mut self, id: u32) {
        self.write_frame(3, 0, id, &8u32.to_be_bytes()).await;
    }

    async fn response_headers(&mut self, id: u32, end_stream: bool) {
        // HPACK static-table index 8 is :status=200. No dynamic table state.
        self.write_frame(1, 4 | u8::from(end_stream), id, &[0x88])
            .await;
    }

    async fn data(&mut self, id: u32, payload: &[u8], end_stream: bool) {
        self.write_frame(0, u8::from(end_stream), id, payload).await;
    }

    async fn window_update(&mut self, id: u32, increment: u32) {
        assert!((1..=0x7fff_ffff).contains(&increment));
        self.write_frame(8, 0, id, &increment.to_be_bytes()).await;
    }

    async fn goaway(&mut self, last_stream_id: u32, reason: u32) {
        let mut payload = last_stream_id.to_be_bytes().to_vec();
        payload.extend_from_slice(&reason.to_be_bytes());
        self.write_frame(7, 0, 0, &payload).await;
    }
}

async fn component_connection(
    initial: Option<usize>,
    remote_settings: Option<&[(u16, u32)]>,
) -> (SendRequest<Bytes>, Driver, Peer) {
    let (io, mut peer_io) = tokio::io::duplex(65536);
    let mut builder = h2::client::Builder::new();
    if let Some(initial) = initial {
        builder.initial_max_send_streams(initial);
    }
    let (send, connection) = builder.handshake(io).await.unwrap();
    let (commands, mut changes) = mpsc::unbounded_channel::<DriverCommand>();
    let task = tokio::spawn(async move {
        let mut connection = Box::pin(connection);
        let mut paused = false;
        loop {
            let command = if paused {
                changes.recv().await
            } else {
                tokio::select! {
                    biased;
                    command = changes.recv() => command,
                    _ = connection.as_mut() => return,
                }
            };
            let Some(command) = command else { return };
            paused = command.paused;
            let _ = command.acknowledged.send(());
        }
    });
    let driver = Driver { commands, task };
    let mut preface = [0; 24];
    peer_io.read_exact(&mut preface).await.unwrap();
    assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
    let mut peer = Peer {
        io: peer_io,
        buffered: Vec::new(),
        observed: VecDeque::new(),
    };
    let initial_settings = peer.read_frame().await;
    assert_eq!(
        (
            initial_settings.kind,
            initial_settings.id,
            initial_settings.flags
        ),
        (4, 0, 0)
    );
    if let Some(values) = remote_settings {
        peer.settings(values).await;
    }
    (send, driver, peer)
}

async fn opened(task: tokio::task::JoinHandle<Result<Opened, h2::Error>>) -> Opened {
    tokio::time::timeout(BOUND, task)
        .await
        .expect("admission stalled")
        .expect("admission task panicked")
        .expect("unexpected admission error")
}

#[tokio::test(start_paused = true)]
async fn mixed_legacy_pending_cancel_releases_only_new_task_and_request() {
    let (mut send, _driver, mut peer) = component_connection(Some(0), Some(&[(3, 0)])).await;
    // Original API deliberately queues while cap0, assigning id1 and setting
    // this exact Sender.pending. A fresh clone would miss the regression.
    let legacy = send.send_request(request(), true).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut req = request();
    req.extensions_mut()
        .insert(Arc::new(DropGuard(dropped.clone())));
    let wakes = Arc::new(AtomicUsize::new(0));
    let task_waker = Arc::new(WakeGuard(wakes.clone()));
    let weak = Arc::downgrade(&task_waker);
    let waker = Waker::from(task_waker);
    waker.wake_by_ref();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    let mut pending = Box::pin(send.send_request_when_ready(req, true));
    {
        let mut cx = Context::from_waker(&waker);
        assert!(pending.as_mut().poll(&mut cx).is_pending());
    }
    drop(waker);
    drop(pending);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(
        weak.upgrade().is_none(),
        "new task retained on legacy stream"
    );
    // The legacy request remains intact; only its stream ID was allocated.
    peer.settings(&[(3, 1)]).await;
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    peer.expect_headers(1).await;
    let next = pending_request(&send).await;
    peer.quiet().await;
    assert!(
        !next.is_finished(),
        "legacy active stream still occupies cap1"
    );
    peer.reset(1).await;
    let next = opened(next).await;
    peer.expect_headers(3).await;
    drop((legacy, next));
}

#[tokio::test(start_paused = true)]
async fn legacy_pending_queue_reservations_bound_additive_admission() {
    let (mut first_sender, driver, mut peer) = component_connection(Some(0), Some(&[(3, 1)])).await;
    driver.pause(true).await;
    let first = first_sender.send_request(request(), true).unwrap();
    let mut second_sender = first_sender.clone();
    let second = second_sender.send_request(request(), true).unwrap();
    // Active count is still zero, but the original API queued two reservations.
    let third = pending_request(&first_sender).await;
    peer.quiet().await;
    assert!(!third.is_finished());
    driver.pause(false).await;
    peer.expect_headers(1).await;
    peer.quiet().await;
    assert!(!third.is_finished());
    peer.reset(1).await;
    peer.expect_headers(3).await;
    peer.quiet().await;
    assert!(
        !third.is_finished(),
        "older original reservation must consume slot"
    );
    peer.reset(3).await;
    let third = opened(third).await;
    peer.expect_headers(5).await;
    drop((first, second, third));
}

#[tokio::test(start_paused = true)]
async fn first_settings_without_max_releases_native_initial_zero() {
    let (send, _driver, mut peer) = component_connection(Some(0), None).await;
    let pending = pending_request(&send).await;
    peer.quiet().await;
    assert_eq!(send.current_max_send_streams(), 0);
    peer.settings(&[]).await;
    let next = opened(pending).await;
    assert_eq!(send.current_max_send_streams(), usize::MAX);
    peer.expect_headers(1).await;
    drop(next);
}

#[tokio::test(start_paused = true)]
async fn later_settings_without_max_preserves_existing_cap() {
    let (mut send, _driver, mut peer) = component_connection(Some(0), Some(&[(3, 1)])).await;
    let first = send.send_request_when_ready(request(), true).await.unwrap();
    peer.expect_headers(1).await;
    let pending = pending_request(&send).await;
    peer.settings(&[]).await;
    assert_eq!(send.current_max_send_streams(), 1);
    peer.quiet().await;
    assert!(!pending.is_finished());
    peer.reset(1).await;
    let next = opened(pending).await;
    peer.expect_headers(3).await;
    drop((first, next));
}

#[tokio::test(start_paused = true)]
async fn upstream_default_can_admit_before_first_peer_settings() {
    let (mut send, _driver, mut peer) = component_connection(None, None).await;
    assert_eq!(send.current_max_send_streams(), usize::MAX);
    let next = tokio::time::timeout(BOUND, send.send_request_when_ready(request(), true))
        .await
        .expect("upstream initial unlimited policy changed")
        .unwrap();
    // No peer SETTINGS has been written. This is not the native initial0 policy.
    peer.expect_headers(1).await;
    drop(next);
}

async fn after_admission_cancel_preserves_wire_open(implicit: bool) {
    let (mut send, driver, mut peer) = component_connection(Some(0), Some(&[(3, 1)])).await;
    driver.pause(true).await;
    let mut first = send
        .send_request_when_ready(request(), false)
        .await
        .unwrap();
    peer.quiet().await; // admitted, but the entire writer remains fenced
    let retained = if implicit {
        drop(first);
        None
    } else {
        first.1.send_reset(h2::Reason::CANCEL);
        Some(first)
    };
    let next = pending_request(&send).await;
    assert!(
        !next.is_finished(),
        "cancel cannot release unflushed reservation"
    );
    driver.pause(false).await;
    peer.expect_headers(1).await;
    let reset = peer.stream_frame().await;
    assert_eq!(
        (reset.kind, reset.id),
        (3, 1),
        "HEADERS must precede RST: {reset:?}"
    );
    assert_eq!(reset.payload, 8u32.to_be_bytes());
    let next = opened(next).await;
    peer.expect_headers(3).await;
    drop((retained, next));
}

#[tokio::test(start_paused = true)]
async fn explicit_cancel_after_admission_sends_headers_then_reset() {
    after_admission_cancel_preserves_wire_open(false).await;
}

#[tokio::test(start_paused = true)]
async fn last_handle_drop_after_admission_sends_headers_then_reset() {
    after_admission_cancel_preserves_wire_open(true).await;
}

#[tokio::test(start_paused = true)]
async fn response_eof_does_not_release_slot_with_flow_blocked_request_data() {
    let (mut send, _driver, mut peer) =
        component_connection(Some(0), Some(&[(3, 1), (4, 1)])).await;
    let (response, mut upload) = send
        .send_request_when_ready(request(), false)
        .await
        .unwrap();
    upload.send_data(Bytes::from_static(b"abc"), true).unwrap();
    peer.expect_headers(1).await;
    let prefix = peer.stream_frame().await;
    assert_eq!((prefix.kind, prefix.id, prefix.flags & 1), (0, 1, 0));
    assert_eq!(prefix.payload, b"a");
    peer.response_headers(1, true).await;
    let response = tokio::time::timeout(BOUND, response)
        .await
        .unwrap()
        .unwrap();
    let body = response.into_body(); // keep response handle alive after remote EOS
    let pending = pending_request(&send).await;
    peer.quiet().await;
    assert!(
        !pending.is_finished(),
        "two buffered request bytes still consume quota"
    );
    peer.window_update(1, 2).await;
    let suffix = peer.stream_frame().await;
    assert_eq!((suffix.kind, suffix.id, suffix.flags & 1), (0, 1, 1));
    assert_eq!(suffix.payload, b"bc");
    let next = opened(pending).await;
    peer.expect_headers(3).await;
    drop((upload, body, next));
}

#[tokio::test(start_paused = true)]
async fn open_response_body_occupies_slot_but_terminal_handles_do_not() {
    let (mut send, _driver, mut peer) = component_connection(Some(0), Some(&[(3, 1)])).await;
    let (response, upload) = send.send_request_when_ready(request(), true).await.unwrap();
    peer.expect_headers(1).await;
    peer.response_headers(1, false).await;
    peer.data(1, b"abc", false).await;
    let response = tokio::time::timeout(BOUND, response)
        .await
        .unwrap()
        .unwrap();
    let mut body = response.into_body(); // response headers alone do not close it
    let pending = pending_request(&send).await;
    peer.quiet().await;
    assert!(
        !pending.is_finished(),
        "response has not reached protocol EOF"
    );
    peer.data(1, &[], true).await;
    let next = opened(pending).await;
    peer.expect_headers(3).await;
    // Slot release follows real protocol closure, not dropping all user handles
    // or consuming buffered response DATA. Delivery remains exact afterwards.
    let mut delivered = Vec::new();
    while let Some(data) = tokio::time::timeout(BOUND, body.data()).await.unwrap() {
        let data = data.unwrap();
        delivered.extend_from_slice(&data);
        body.flow_control().release_capacity(data.len()).unwrap();
    }
    assert_eq!(delivered, b"abc");
    drop((upload, body, next));
}

#[tokio::test(start_paused = true)]
async fn goaway_wakes_mixed_legacy_pending_without_opening_new_headers() {
    let (mut send, _driver, mut peer) = component_connection(Some(0), Some(&[(3, 0)])).await;
    let legacy = send.send_request(request(), true).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut req = request();
    req.extensions_mut()
        .insert(Arc::new(DropGuard(dropped.clone())));
    let mut pending = Box::pin(send.send_request_when_ready(req, true));
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    peer.goaway(0, 0).await;
    let error = tokio::time::timeout(BOUND, pending)
        .await
        .expect("GOAWAY did not wake pending admission")
        .expect_err("connection error must precede capacity admission");
    assert!(error.is_go_away());
    assert_eq!(error.reason(), Some(h2::Reason::NO_ERROR));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    // cap0 prohibited both the old queued id1 and the untouched new request.
    peer.no_stream_until_eof_or_quiet().await;
    drop(legacy);
}

#[tokio::test(start_paused = true)]
async fn eof_wakes_mixed_legacy_pending_with_connection_error() {
    let (mut send, _driver, peer) = component_connection(Some(0), Some(&[(3, 0)])).await;
    let legacy = send.send_request(request(), true).unwrap();
    let mut pending = Box::pin(send.send_request_when_ready(request(), true));
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(peer);
    let error = tokio::time::timeout(BOUND, pending)
        .await
        .expect("EOF did not wake pending admission")
        .expect_err("closed connection must fail, not remain behind capacity wait");
    assert!(error.is_io());
    drop(legacy);
}
