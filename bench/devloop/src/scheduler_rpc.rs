//! RX-10a: independent semantic fixture and library-spawn diagnostics.
//!
//! This command is outside the frozen performance cells. It uses one
//! current-thread runtime and one duplex connection, never compares wall time,
//! and labels application producers separately from library spawn sites.

use pbrs_grpc::{
    Channel, Code, HelloReply, HelloRequest, Request, Response, Rpc, Server, Service, Status,
    Streaming,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;

const UNARY: &str = "/scheduler.Echo/Unary";
const SERVER_STREAM: &str = "/scheduler.Echo/ServerStream";
const CLIENT_STREAM: &str = "/scheduler.Echo/ClientStream";
const BIDI: &str = "/scheduler.Echo/Bidi";
const BOUND: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LibrarySpawns {
    client_connection_driver: u64,
    client_bidi_upload: u64,
    client_server_stream_cancel: u64,
    server_rpc_dispatch: u64,
}

fn library_spawns() -> Option<LibrarySpawns> {
    let counts = pbrs_grpc::scheduler_counts();
    Some(LibrarySpawns {
        client_connection_driver: counts.client_connection_driver,
        client_bidi_upload: counts.client_bidi_upload,
        client_server_stream_cancel: counts.client_server_stream_cancel,
        server_rpc_dispatch: counts.server_rpc_dispatch,
    })
}
fn reset_library_spawns() {
    pbrs_grpc::reset_scheduler_counts();
}

fn spawn_json(counts: Option<LibrarySpawns>) -> Value {
    match counts {
        None => {
            json!({"status": "not_run", "reason": "library spawn counters absent at semantic baseline"})
        }
        Some(n) => json!({"status": "measured", "scope": "named library spawn expressions",
            "client_connection_driver": n.client_connection_driver,
            "client_bidi_upload": n.client_bidi_upload,
            "client_server_stream_cancel": n.client_server_stream_cancel,
            "server_rpc_dispatch": n.server_rpc_dispatch}),
    }
}

#[derive(Default)]
struct State {
    rpc_started: AtomicU64,
    rpc_completed: AtomicU64,
    producers_started: AtomicU64,
    producers_completed: AtomicU64,
    body_messages: AtomicU64,
    changed: Notify,
}

struct Completion {
    state: Arc<State>,
    producer: bool,
}

impl Drop for Completion {
    fn drop(&mut self) {
        let counter = if self.producer {
            &self.state.producers_completed
        } else {
            &self.state.rpc_completed
        };
        counter.fetch_add(1, Ordering::SeqCst);
        self.state.changed.notify_one();
    }
}

impl State {
    fn start(self: &Arc<Self>, producer: bool) -> Completion {
        let counter = if producer {
            &self.producers_started
        } else {
            &self.rpc_started
        };
        counter.fetch_add(1, Ordering::SeqCst);
        Completion {
            state: self.clone(),
            producer,
        }
    }

    fn body(&self, message: &HelloRequest, index: usize) -> String {
        let name = message.name().to_str().expect("UTF-8").to_owned();
        assert_eq!(
            name.as_bytes(),
            if index == 0 {
                "alpha".as_bytes()
            } else {
                "beta".as_bytes()
            }
        );
        self.body_messages.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_one();
        name
    }

    async fn body_reached(&self, count: u64) {
        loop {
            let changed = self.changed.notified();
            if self.body_messages.load(Ordering::SeqCst) >= count {
                return;
            }
            changed.await;
        }
    }

    async fn quiet(&self, tasks: usize) {
        tokio::time::timeout(BOUND, async {
            loop {
                if self.rpc_started.load(Ordering::SeqCst)
                    == self.rpc_completed.load(Ordering::SeqCst)
                    && self.producers_started.load(Ordering::SeqCst)
                        == self.producers_completed.load(Ordering::SeqCst)
                    && tokio::runtime::Handle::current()
                        .metrics()
                        .num_alive_tasks()
                        == tasks
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("RPC, producer and library tasks quiesce");
    }
}

fn request<T>(message: T, mode: &str) -> Request<T> {
    let mut request = Request::new(message);
    request
        .metadata_mut()
        .insert("x-fixture-mode", mode)
        .expect("mode metadata");
    request
}

fn message(name: &str) -> HelloRequest {
    let mut message = HelloRequest::new();
    message.set_name(name);
    message
}

fn reply(name: &str) -> HelloReply {
    let mut message = HelloReply::new();
    message.set_message(name);
    message
}

fn terminal() -> Status {
    let mut status = Status::failed_precondition("terminal");
    status
        .metadata_mut()
        .insert("x-terminal", "retained")
        .expect("terminal metadata");
    status.set_details(vec![0x08, 0x07]);
    status
}

fn assert_terminal(status: &Status) {
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(status.message(), "terminal");
    assert_eq!(status.metadata().get("x-terminal"), Some("retained"));
    assert_eq!(status.details(), &[0x08, 0x07]);
}

fn responses(state: Arc<State>, mode: String, names: Vec<String>) -> Streaming<HelloReply> {
    let (sender, replies) = Streaming::channel(2);
    let completed = state.start(true);
    drop(tokio::spawn(async move {
        let _completed = completed;
        for name in names {
            sender
                .send(reply(&name))
                .await
                .expect("response message accepted");
        }
        match mode.as_str() {
            "cancel" => sender.closed().await,
            "terminal" => sender.fail(terminal()).await,
            _ => sender.close(),
        }
    }));
    replies
}

struct Echo(Arc<State>);

impl Service for Echo {
    const NAME: &'static str = "scheduler.Echo";
    async fn call(&self, rpc: Rpc) {
        let _completed = self.0.start(false);
        let mode = rpc
            .metadata()
            .get("x-fixture-mode")
            .expect("mode")
            .to_owned();
        let state = self.0.clone();
        match rpc.method() {
            "Unary" => {
                rpc.unary(move |request: Request<HelloRequest>| async move {
                    let name = state.body(request.get_ref(), 0);
                    if mode == "cancel" {
                        request.cancelled().await;
                        return Err(Status::cancelled());
                    }
                    if mode == "terminal" {
                        return Err(terminal());
                    }
                    Ok(Response::new(reply(&name)))
                })
                .await
            }
            "ServerStream" => {
                rpc.server_streaming(move |request: Request<HelloRequest>| async move {
                    let name = state.body(request.get_ref(), 0);
                    let names = if mode == "echo" {
                        vec![name, "beta".to_owned()]
                    } else {
                        vec![name]
                    };
                    Ok::<_, Status>(Response::new(responses(state, mode, names)))
                })
                .await
            }
            "ClientStream" => {
                rpc.client_streaming(
                    move |request: Request<Streaming<HelloRequest>>| async move {
                        let cancelled = request.cancelled();
                        let mut inbound = request.into_inner();
                        let mut names = Vec::new();
                        while let Some(message) = inbound.message().await? {
                            names.push(state.body(&message, names.len()));
                            if mode == "early" || mode == "cancel" {
                                break;
                            }
                        }
                        if mode == "cancel" {
                            cancelled.await;
                            return Err(Status::cancelled());
                        }
                        if mode == "terminal" {
                            return Err(terminal());
                        }
                        Ok(Response::new(reply(&names.join(","))))
                    },
                )
                .await
            }
            "Bidi" => {
                rpc.bidi_streaming(
                    move |request: Request<Streaming<HelloRequest>>| async move {
                        let mut inbound = request.into_inner();
                        let mut names = Vec::new();
                        while let Some(message) = inbound.message().await? {
                            names.push(state.body(&message, names.len()));
                            if mode != "echo" {
                                break;
                            }
                        }
                        Ok::<_, Status>(Response::new(responses(state, mode, names)))
                    },
                )
                .await
            }
            _ => rpc.unimplemented(),
        }
    }
}

fn assert_reply(message: HelloReply, expected: &str) {
    assert_eq!(
        message.message().to_str().expect("reply UTF-8").as_bytes(),
        expected.as_bytes()
    );
}

async fn consume(mut replies: Streaming<HelloReply>, mode: &str) -> u64 {
    assert_reply(
        replies
            .message()
            .await
            .expect("first response")
            .expect("first item"),
        "alpha",
    );
    let count = if mode == "echo" {
        assert_reply(
            replies
                .message()
                .await
                .expect("second response")
                .expect("second item"),
            "beta",
        );
        2
    } else {
        1
    };
    if mode == "terminal" {
        assert_terminal(&replies.message().await.expect_err("terminal status"));
    } else {
        assert!(replies.message().await.expect("OK terminal").is_none());
        replies.trailers().await.expect("OK trailers");
    }
    count
}

async fn exercise(channel: &Channel, state: &State, shape: &str, mode: &str) -> u64 {
    match (shape, mode) {
        ("unary", "cancel") => {
            let call =
                channel.unary::<HelloRequest, HelloReply>(UNARY, request(message("alpha"), mode));
            let handle = call.handle();
            tokio::pin!(call);
            let target = state.body_messages.load(Ordering::SeqCst) + 1;
            tokio::select! { biased;
                () = state.body_reached(target) => {},
                result = &mut call => panic!("unary completed before cancel: {result:?}"),
            }
            handle.cancel();
            assert_eq!(
                call.await.expect_err("cancelled unary").code(),
                Code::Cancelled
            );
            0
        }
        ("unary", _) => {
            let response = channel
                .unary::<HelloRequest, HelloReply>(UNARY, request(message("alpha"), mode))
                .await;
            if mode == "terminal" {
                assert_terminal(&response.expect_err("terminal unary"));
                0
            } else {
                assert_reply(response.expect("unary").into_inner(), "alpha");
                1
            }
        }
        ("client_stream", _) => {
            let (sender, call) = channel
                .client_streaming::<HelloRequest, HelloReply>(CLIENT_STREAM, request((), mode));
            sender.send(message("alpha")).await.expect("upload alpha");
            if mode == "echo" || mode == "terminal" {
                sender.send(message("beta")).await.expect("upload beta");
                sender.close();
                let response = call.await;
                if mode == "terminal" {
                    assert_terminal(&response.expect_err("terminal upload"));
                    0
                } else {
                    assert_reply(response.expect("upload").into_inner(), "alpha,beta");
                    1
                }
            } else {
                let handle = call.handle();
                tokio::pin!(call);
                if mode == "cancel" {
                    let target = state.body_messages.load(Ordering::SeqCst) + 1;
                    tokio::select! { biased;
                        () = state.body_reached(target) => {},
                        result = &mut call => panic!("upload completed before cancel: {result:?}"),
                    }
                    handle.cancel();
                    assert_eq!(
                        (&mut call).await.expect_err("cancelled upload").code(),
                        Code::Cancelled
                    );
                } else {
                    assert_reply(
                        (&mut call)
                            .await
                            .expect("early upload response")
                            .into_inner(),
                        "alpha",
                    );
                }
                tokio::time::timeout(BOUND, sender.closed())
                    .await
                    .expect("input body released");
                assert_eq!(handle.is_cancelled(), mode == "cancel");
                u64::from(mode != "cancel")
            }
        }
        ("server_stream", "cancel") => {
            let call = channel.server_streaming::<HelloRequest, HelloReply>(
                SERVER_STREAM,
                request(message("alpha"), mode),
            );
            let handle = call.handle();
            let mut replies = call.await.expect("headers").into_inner();
            assert_reply(
                replies.message().await.expect("first").expect("item"),
                "alpha",
            );
            handle.cancel();
            assert_eq!(
                replies
                    .message()
                    .await
                    .expect_err("cancel after headers")
                    .code(),
                Code::Cancelled
            );
            1
        }
        ("server_stream", _) => {
            consume(
                channel
                    .server_streaming::<HelloRequest, HelloReply>(
                        SERVER_STREAM,
                        request(message("alpha"), mode),
                    )
                    .await
                    .expect("headers")
                    .into_inner(),
                mode,
            )
            .await
        }
        ("bidi", _) => {
            let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(BIDI, request((), mode));
            let handle = call.handle();
            sender.send(message("alpha")).await.expect("bidi alpha");
            if mode == "echo" {
                sender.send(message("beta")).await.expect("bidi beta");
                sender.close();
                return consume(call.await.expect("bidi headers").into_inner(), mode).await;
            }
            let mut replies = call.await.expect("bidi headers").into_inner();
            let count = if mode == "cancel" {
                assert_reply(
                    replies.message().await.expect("first").expect("item"),
                    "alpha",
                );
                handle.cancel();
                assert_eq!(
                    replies
                        .message()
                        .await
                        .expect_err("cancel after headers")
                        .code(),
                    Code::Cancelled
                );
                1
            } else {
                consume(replies, mode).await
            };
            tokio::time::timeout(BOUND, sender.closed())
                .await
                .expect("idle upload body released");
            assert_eq!(handle.is_cancelled(), mode == "cancel");
            count
        }
        _ => panic!("unknown shape {shape}"),
    }
}

async fn campaign() -> Value {
    let state = Arc::new(State::default());
    let (client, server) = tokio::io::duplex(16 * 1024);
    reset_library_spawns();
    let service_state = state.clone();
    let server_task = tokio::spawn(async move {
        Server::new(Echo(service_state))
            .serve_connection(server)
            .await
    });
    let channel = Channel::from_io(client, "localhost")
        .await
        .expect("duplex SETTINGS handshake");
    assert_eq!(
        library_spawns(),
        Some(LibrarySpawns {
            client_connection_driver: 1,
            ..LibrarySpawns::default()
        })
    );
    let setup = spawn_json(library_spawns());
    let shapes = ["unary", "server_stream", "client_stream", "bidi"];
    for shape in shapes {
        exercise(&channel, &state, shape, "echo").await;
        state.quiet(2).await;
    }
    let mut rows = Vec::new();
    for shape in shapes {
        for mode in ["echo", "terminal", "cancel", "early"] {
            if mode == "early" && (shape == "unary" || shape == "server_stream") {
                continue;
            }
            reset_library_spawns();
            let body_before = state.body_messages.load(Ordering::SeqCst);
            let producers_before = state.producers_started.load(Ordering::SeqCst);
            let rpc_before = state.rpc_completed.load(Ordering::SeqCst);
            let replies = tokio::time::timeout(BOUND, exercise(&channel, &state, shape, mode))
                .await
                .expect("semantic case bound");
            state.quiet(2).await;
            let counts = library_spawns();
            if let Some(counts) = counts {
                assert_eq!(
                    counts,
                    LibrarySpawns {
                        server_rpc_dispatch: 1,
                        client_server_stream_cancel: u64::from(shape == "server_stream"),
                        client_bidi_upload: u64::from(shape == "bidi"),
                        ..LibrarySpawns::default()
                    }
                );
            }
            let bodies = state.body_messages.load(Ordering::SeqCst) - body_before;
            let producers = state.producers_started.load(Ordering::SeqCst) - producers_before;
            assert_eq!(
                bodies,
                if (shape == "client_stream" || shape == "bidi")
                    && (mode == "echo" || (shape == "client_stream" && mode == "terminal"))
                {
                    2
                } else {
                    1
                }
            );
            assert_eq!(
                producers,
                u64::from(shape == "server_stream" || shape == "bidi")
            );
            assert_eq!(state.rpc_completed.load(Ordering::SeqCst) - rpc_before, 1);
            rows.push(json!({"shape": shape, "mode": mode, "body_messages": bodies, "reply_messages": replies,
                "terminal": match mode { "terminal" => "FAILED_PRECONDITION", "cancel" => "CANCELLED", _ => "OK" },
                "library_spawns": spawn_json(counts), "application_producer_spawns": producers,
                "rpc_and_producer_completion_acknowledged": true, "steady_alive_tasks": 2}));
        }
    }
    drop(channel);
    tokio::time::timeout(BOUND, server_task)
        .await
        .expect("server connection shutdown")
        .expect("server join")
        .expect("clean connection shutdown");
    state.quiet(0).await;
    json!({"schema": "scheduler-spawns/1", "scope": "semantic diagnostic, not a performance cell",
        "transport": "duplex", "runtime": "current_thread", "warmup_calls": 4,
        "setup": {"harness_connection_tasks": 1, "library_spawns": setup}, "rows": rows,
        "wakeups": {"status": "not_run", "reason": "spawn events do not measure waker invocations"},
        "channel_sends": {"status": "not_run", "reason": "shared sender API has no per-side attribution"},
        "cross_thread_handoffs": {"status": "not_run", "reason": "not instrumented; fixture uses one thread"},
        "context_switches": {"status": "not_run", "reason": "no per-side kernel scheduler collector"},
        "instruction_attribution": {"status": "not_run", "reason": "requires independent disjoint profiles"},
        "quiescent_alive_tasks": 0})
}

pub(crate) fn run() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("diagnostic runtime");
    println!(
        "{}",
        serde_json::to_string_pretty(&runtime.block_on(campaign())).expect("diagnostic JSON")
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn four_shapes_terminal_cancel_and_early_upload_quiesce() {
        super::run();
    }
}
