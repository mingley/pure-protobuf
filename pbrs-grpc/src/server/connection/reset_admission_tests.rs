//! Already-reset requests must not take bounded admission slots.
#![allow(
    clippy::expect_used,
    clippy::disallowed_types,
    reason = "test event capture"
)]

use super::{ConnectionInfo, serve_io_in};
use crate::config::ServerConfig;
use crate::rt::{Interval, Runtime, TimedOut, TokioRuntime};
use crate::server::dispatch::Dispatch;
use crate::server::rpc::Rpc;
use crate::status::{Code, Status};
use crate::telemetry::{CallLabels, LifecycleObserver};
use std::cell::RefCell;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Notify, Semaphore, watch};

struct Gate {
    spawns: AtomicUsize,
    release: Semaphore,
}

thread_local! {
    static GATE: RefCell<Option<Arc<Gate>>> = const { RefCell::new(None) };
}

struct DelayedTasks;

impl Runtime for DelayedTasks {
    fn now() -> tokio::time::Instant {
        TokioRuntime::now()
    }

    fn sleep_until(at: tokio::time::Instant) -> impl Future<Output = ()> + Send {
        TokioRuntime::sleep_until(at)
    }

    fn timeout<F>(
        duration: Duration,
        fut: F,
    ) -> impl Future<Output = Result<F::Output, TimedOut>> + Send
    where
        F: Future + Send,
    {
        TokioRuntime::timeout(duration, fut)
    }

    fn interval(period: Duration) -> impl Interval {
        TokioRuntime::interval(period)
    }

    fn spawn<F>(fut: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let gate = GATE.with(|slot| slot.borrow().as_ref().expect("test gate installed").clone());
        gate.spawns.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            let permit = gate.release.acquire().await.expect("release dispatch");
            drop(permit);
            fut.await
        })
    }
}

#[derive(Default)]
struct Events {
    ends: Mutex<Vec<Code>>,
    changed: Notify,
}

impl LifecycleObserver for Events {
    fn on_server_call_end(&self, _: &CallLabels<'_>, status: &Status, _: Duration) {
        self.ends.lock().expect("events").push(status.code());
        self.changed.notify_one();
    }
}

struct Discard {
    observer: Arc<dyn LifecycleObserver>,
}

impl Dispatch for Discard {
    async fn dispatch(&self, rpc: Rpc) {
        rpc.unimplemented();
    }

    fn observer(&self) -> Option<&Arc<dyn LifecycleObserver>> {
        Some(&self.observer)
    }
}

fn frame(output: &mut Vec<u8>, kind: u8, flags: u8, stream: u8, payload: &[u8]) {
    let size = u32::try_from(payload.len())
        .expect("frame size")
        .to_be_bytes();
    output.extend_from_slice(&size[1..]);
    output.extend_from_slice(&[kind, flags, 0, 0, 0, stream]);
    output.extend_from_slice(payload);
}

#[tokio::test]
async fn queued_resets_do_not_spawn_dispatch_or_exhaust_rpc_slots() {
    let gate = Arc::new(Gate {
        spawns: AtomicUsize::new(0),
        release: Semaphore::new(0),
    });
    GATE.with(|slot| *slot.borrow_mut() = Some(gate.clone()));
    let events = Arc::new(Events::default());
    let dispatch = Arc::new(Discard {
        observer: events.clone(),
    });
    let slots = Arc::new(Semaphore::new(1));
    let (mut peer, io) = tokio::io::duplex(4096);
    // Feed HEADERS and resets together before polling the server. Its h2
    // accept queue contains already-reset streams. RPC tasks deliberately
    // cannot run yet, making admission independent of scheduler timing.
    let mut bytes = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
    frame(&mut bytes, 4, 0, 0, &[]);
    let mut headers = vec![0x83, 0x86, 0x04, 18];
    headers.extend_from_slice(b"/test.Service/Call");
    headers.extend_from_slice(&[0x01, 4]);
    headers.extend_from_slice(b"test");
    headers.extend_from_slice(&[0x0f, 16, 16]);
    headers.extend_from_slice(b"application/grpc");
    headers.extend_from_slice(&[0x00, 2, b't', b'e', 8]);
    headers.extend_from_slice(b"trailers");
    for stream in [1, 3, 5] {
        frame(&mut bytes, 1, 4, stream, &headers);
        frame(&mut bytes, 3, 0, stream, &[0, 0, 0, 8]);
    }
    peer.write_all(&bytes).await.expect("queued frames");
    let (drain, drained) = watch::channel(false);
    let server = tokio::spawn(serve_io_in::<DelayedTasks, _, _>(
        dispatch,
        io,
        ConnectionInfo::new(),
        ServerConfig::default(),
        drained,
        Some(slots.clone()),
    ));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if events.ends.lock().expect("events").len() >= 2 {
                break;
            }
            events.changed.notified().await;
        }
    })
    .await
    .expect("server processes the queued resets");
    tokio::task::yield_now().await;
    assert_eq!(
        *events.ends.lock().expect("events"),
        vec![Code::Cancelled; 3]
    );
    assert_eq!(gate.spawns.load(Ordering::SeqCst), 0);
    assert_eq!(slots.available_permits(), 1);
    server.abort();
    drop(drain);
    GATE.with(|slot| *slot.borrow_mut() = None);
}
