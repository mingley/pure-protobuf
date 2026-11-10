//! Binary-log ordering when supervised hedge tasks overlap in their sinks.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::let_underscore_must_use,
    clippy::panic,
    reason = "bounded integration-test synchronization and assertions"
)]

mod common;

use pbrs_grpc::binlog::{BinaryLogFilter, BinaryLogger, EventType, LogRecord, Sink, VecSink};
use pbrs_grpc::hello::{Greeter, HelloReply, HelloRequest};
use pbrs_grpc::{AttemptLabels, Channel, Code, LifecycleObserver, Request, Response, Status};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, mpsc};

const PATH: &str = "/helloworld.Greeter/SayHello";
const BUDGET: Duration = Duration::from_secs(10);

struct Blocker;

impl Greeter for Blocker {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        std::future::pending().await
    }
}

#[derive(Debug, Default)]
struct Gate {
    open: Mutex<bool>,
    ready: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.open.lock().expect("gate lock") = true;
        self.ready.notify_all();
    }

    fn wait(&self) {
        let open = self.open.lock().expect("gate lock");
        let (open, elapsed) = self
            .ready
            .wait_timeout_while(open, BUDGET, |open| !*open)
            .expect("gate wait");
        assert!(
            *open && !elapsed.timed_out(),
            "header gate was not released"
        );
    }
}

// A failing assertion must not leave synchronous callbacks blocked in the runtime.
struct ReleaseOnDrop([Arc<Gate>; 2]);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        for gate in &self.0 {
            gate.release();
        }
    }
}

#[derive(Debug)]
struct BlockingHeaders {
    records: VecSink,
    gates: [Arc<Gate>; 2],
    block_both: bool,
    next: AtomicUsize,
    entered: mpsc::UnboundedSender<usize>,
    client_message: Notify,
    cancelled: Notify,
}

impl Sink for BlockingHeaders {
    fn emit(&self, record: &LogRecord) {
        self.records.emit(record);
        match record.entry.event {
            EventType::ClientHeader => {
                let index = self.next.fetch_add(1, Ordering::SeqCst);
                assert!(index < 2, "unexpected extra header callback");
                self.entered.send(index).expect("test receiver");
                if index == 0 || self.block_both {
                    self.gates[index].wait();
                }
            }
            EventType::ClientMessage => self.client_message.notify_one(),
            EventType::Cancel => self.cancelled.notify_one(),
            _ => {}
        }
    }
}

struct AttemptEnds(mpsc::UnboundedSender<()>);

impl LifecycleObserver for AttemptEnds {
    fn on_attempt_end(&self, _attempt: &AttemptLabels<'_>, _status: &Status, _latency: Duration) {
        let _ = self.0.send(());
    }
}

async fn exercise_hedge_headers(block_both: bool) {
    let (addr, _server) = common::serve(Blocker, Default::default())
        .await
        .expect("spawn blocker");
    let gates = [Arc::new(Gate::default()), Arc::new(Gate::default())];
    let _release = ReleaseOnDrop(gates.clone());
    let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
    let (ended_tx, mut ended_rx) = mpsc::unbounded_channel();
    let sink = Arc::new(BlockingHeaders {
        records: VecSink::new(),
        gates: gates.clone(),
        block_both,
        next: AtomicUsize::new(0),
        entered: entered_tx,
        client_message: Notify::new(),
        cancelled: Notify::new(),
    });
    let logger = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(AttemptEnds(ended_tx))
        .binary_logger(logger)
        .service_config(
            r#"{"methodConfig":[{"name":[{}],"hedgingPolicy":{"maxAttempts":2,"hedgingDelay":"0s"}}]}"#,
        )
        .expect("hedging policy");
    let call = channel.unary::<_, HelloReply>(PATH, Request::new(common::req("hedge")));
    let handle = call.handle();
    let task = tokio::spawn(call);
    let first = tokio::time::timeout(BUDGET, entered_rx.recv())
        .await
        .expect("first header entered")
        .expect("first header signal");
    let second = tokio::time::timeout(BUDGET, entered_rx.recv())
        .await
        .expect("second header entered")
        .expect("second header signal");
    let mut headers = [first, second];
    headers.sort_unstable();
    assert_eq!(headers, [0, 1]);

    if !block_both {
        // A message can be logged only after the second header callback and
        // its header guard returned. The first callback is still blocked.
        tokio::time::timeout(BUDGET, sink.client_message.notified())
            .await
            .expect("unblocked hedge sent its message");
    }

    handle.cancel();
    let status = tokio::time::timeout(BUDGET, task)
        .await
        .expect("coordinator completed while a callback was blocked")
        .expect("call task joined")
        .expect_err("call was cancelled");
    assert_eq!(status.code(), Code::Cancelled);
    let before_release = sink
        .records
        .records()
        .iter()
        .filter(|record| record.entry.event == EventType::Cancel)
        .count();
    assert_eq!(
        before_release,
        usize::from(!block_both),
        "cancel waits for the first completed header callback"
    );

    // A detached handle after the coordinator finishes cannot duplicate Cancel.
    handle.cancel();
    gates[0].release();
    tokio::time::timeout(BUDGET, sink.cancelled.notified())
        .await
        .expect("cancel emitted after a header callback");
    gates[1].release();

    for _ in 0..2 {
        tokio::time::timeout(BUDGET, ended_rx.recv())
            .await
            .expect("hedged task finished")
            .expect("attempt end");
    }
    handle.cancel();
    let events: Vec<_> = sink
        .records
        .records()
        .into_iter()
        .map(|record| record.entry.event)
        .collect();
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == EventType::ClientHeader)
            .count(),
        2,
        "two overlapping headers: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == EventType::Cancel)
            .count(),
        1,
        "exactly one cancellation: {events:?}"
    );
    let cancel = events
        .iter()
        .position(|event| *event == EventType::Cancel)
        .expect("cancel");
    assert!(
        events
            .iter()
            .take(cancel)
            .any(|event| *event == EventType::ClientHeader),
        "a completed header callback precedes cancel: {events:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hedged_cancel_survives_coordinator_drop_while_both_headers_blocked() {
    exercise_hedge_headers(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hedged_cancel_after_one_header_finishes_allows_other_to_finish_later() {
    exercise_hedge_headers(false).await;
}
