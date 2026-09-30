//! A6 service-config retry, hedging, throttling, and pushback over unary calls.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests"
)]

mod common;

use common::{name_of, name_of_request, req, serve};
use pbrs_grpc::codec::CodecMessage;
use pbrs_grpc::hello::{Greeter, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Pushback, Request, Response, ServerConfig, Status, Streaming};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const SAY_HELLO: &str = "/helloworld.Greeter/SayHello";
const SERVER_HELLO: &str = "/helloworld.Greeter/ServerHello";
const CALL_BUDGET: Duration = Duration::from_secs(15);

struct AttemptObserver {
    ended: Arc<tokio::sync::Notify>,
    ready: AtomicUsize,
    cancel_on_retry_ready: Option<Arc<OnceLock<pbrs_grpc::CallHandle>>>,
}

impl pbrs_grpc::LifecycleObserver for AttemptObserver {
    fn on_attempt_end(&self, _: &pbrs_grpc::AttemptLabels<'_>, status: &Status, _: Duration) {
        if status.code() != Code::Ok {
            self.ended.notify_one();
        }
    }

    fn on_queue_wait(&self, _: &pbrs_grpc::CallLabels<'_>, _: Duration) {
        if let Some(handle) = &self.cancel_on_retry_ready {
            if self.ready.fetch_add(1, Ordering::SeqCst) == 1 {
                handle.get().expect("call handle registered").cancel();
            }
        }
    }
}

type SingleRequestResult = Result<Option<Streaming<HelloReply>>, Status>;

fn begin_single_request(
    channel: &Channel,
    request: Request<HelloRequest>,
    server_stream: bool,
) -> (
    pbrs_grpc::CallHandle,
    tokio::task::JoinHandle<SingleRequestResult>,
) {
    if server_stream {
        let call = channel.server_streaming::<_, HelloReply>(SERVER_HELLO, request);
        (
            call.handle(),
            tokio::spawn(async move { call.await.map(|response| Some(response.into_inner())) }),
        )
    } else {
        let call = channel.unary::<_, HelloReply>(SAY_HELLO, request);
        (
            call.handle(),
            tokio::spawn(async move { call.await.map(|_| None) }),
        )
    }
}

fn single_request_calls(service: &Scripted, server_stream: bool) -> usize {
    if server_stream {
        service.stream_calls()
    } else {
        service.calls()
    }
}

async fn follow_up_single_request(channel: &Channel, server_stream: bool) {
    if server_stream {
        assert_eq!(
            collect_stream(channel).await.expect("stream follow-up"),
            ["follow-up"]
        );
    } else {
        assert_eq!(
            name_of(say_hello(channel).await.expect("unary follow-up").get_ref()),
            "follow-up"
        );
    }
}

struct ObservableRequest {
    encoded: Arc<AtomicUsize>,
    message: HelloRequest,
}

impl CodecMessage for ObservableRequest {
    fn encoded_len(&self) -> usize {
        CodecMessage::encoded_len(&self.message)
    }

    fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
        self.encoded.fetch_add(1, Ordering::SeqCst);
        self.message.encode_payload(out)
    }

    fn decode_payload(_payload: bytes::Bytes) -> Result<Self, Status> {
        Err(Status::unimplemented("outbound-only test codec"))
    }

    fn empty() -> Self {
        Self {
            encoded: Arc::new(AtomicUsize::new(0)),
            message: HelloRequest::new(),
        }
    }
}

struct RecordingRetryPeer {
    requests: Arc<Mutex<Vec<(String, bool)>>>,
}

impl RecordingRetryPeer {
    async fn record(&self, request: &Request<HelloRequest>) -> usize {
        let mut requests = self.requests.lock().await;
        let compressed = request.compressed();
        assert_eq!(request.encoding(), compressed.then_some("gzip"));
        requests.push((name_of_request(request.get_ref()), compressed));
        requests.len()
    }
}

impl Greeter for RecordingRetryPeer {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        if self.record(&request).await < 3 {
            return Err(Status::unavailable("retry eligible rejection"));
        }
        Ok(Response::new(common::reply("recovered")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        if self.record(&request).await < 3 {
            return Err(Status::unavailable("retry eligible rejection"));
        }
        let (tx, stream) = Streaming::channel(1);
        tx.send(common::reply("recovered")).await.expect("message");
        drop(tx);
        Ok(Response::new(stream))
    }
}

#[tokio::test]
async fn policy_attempts_encode_single_request_once() {
    let mut encodings = Vec::new();
    for shape in ["unary", "server_stream"] {
        for compressed in [false, true] {
            let requests = Arc::new(Mutex::new(Vec::new()));
            let (addr, _guard) = serve(
                RecordingRetryPeer {
                    requests: requests.clone(),
                },
                ServerConfig::new().accept_compressed(true),
            )
            .await
            .expect("serve");
            let channel =
                Channel::connect_with(addr, pbrs_grpc::ChannelConfig::new().max_concurrent_rpcs(1))
                    .await
                    .expect("connect")
                    .byte_budget(1024)
                    .service_config(
                        r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
                "maxAttempts":3,"initialBackoff":"0s","maxBackoff":"0s",
                "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE"]}}]}"#,
                    )
                    .expect("policy");
            let encoded = Arc::new(AtomicUsize::new(0));
            let mut request = Request::new(ObservableRequest {
                encoded: encoded.clone(),
                message: req("same encoded request"),
            });
            request.set_compress(compressed);
            request.set_timeout(Duration::from_secs(3));
            if shape == "unary" {
                let response: Response<HelloReply> =
                    channel.unary(SAY_HELLO, request).await.expect("unary");
                assert_eq!(name_of(response.get_ref()), "recovered");
            } else {
                let mut stream = channel
                    .server_streaming::<_, HelloReply>(SERVER_HELLO, request)
                    .await
                    .expect("stream")
                    .into_inner();
                assert_eq!(
                    name_of(&stream.message().await.expect("receive").expect("message")),
                    "recovered"
                );
                assert!(stream.message().await.expect("EOF").is_none());
                drop(stream);
            }
            encodings.push((shape, compressed, encoded.load(Ordering::SeqCst)));
            assert_eq!(
                *requests.lock().await,
                vec![("same encoded request".to_owned(), compressed); 3]
            );
            assert_eq!(channel.retry_stats().policy_retries, 2);
            assert_eq!(channel.byte_budget_allocated(), 0);
            let follow_up = tokio::time::timeout(Duration::from_secs(3), say_hello(&channel))
                .await
                .expect("admission reclaimed")
                .expect("follow-up");
            assert_eq!(name_of(follow_up.get_ref()), "recovered");
            assert_eq!(channel.byte_budget_allocated(), 0);
        }
    }
    assert!(
        encodings.iter().all(|(_, _, count)| *count == 1),
        "{encodings:?}"
    );
}

#[tokio::test]
async fn transparent_refusal_replays_single_request_without_encoding_again() {
    let mut encodings = Vec::new();
    for shape in ["unary", "server_stream"] {
        for compressed in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("address");
            let service = Scripted::new(vec![Step::Ok("recovered")])
                .with_streams(vec![StreamStep::Messages(&["recovered"])]);
            let seen = service.clone();
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.expect("connection");
                let mut conn = h2::server::Builder::new()
                    .initial_window_size(1024)
                    .handshake::<_, bytes::Bytes>(socket)
                    .await
                    .expect("handshake");
                let (_request, mut respond) =
                    conn.accept().await.expect("request").expect("headers");
                respond.send_reset(h2::Reason::REFUSED_STREAM);
                let _driver = common::ServerGuard(tokio::spawn(async move {
                    while let Some(Ok(_)) = conn.accept().await {}
                }));
                let _native = common::serve_on(listener, service, ServerConfig::new());
                std::future::pending::<()>().await;
            });
            let _guard = common::ServerGuard(server);
            let channel = Channel::connect_with(
                addr,
                pbrs_grpc::ChannelConfig::new()
                    .connections(1)
                    .max_concurrent_rpcs(1)
                    .max_send_buffer_size(1024),
            )
            .await
            .expect("connect")
            .byte_budget(64 * 1024);
            let encoded = Arc::new(AtomicUsize::new(0));
            let mut request = Request::new(ObservableRequest {
                encoded: encoded.clone(),
                message: req(&"x".repeat(32 * 1024)),
            });
            request.set_compress(compressed);
            request.set_timeout(Duration::from_secs(3));
            if shape == "unary" {
                let response: Response<HelloReply> =
                    channel.unary(SAY_HELLO, request).await.expect("unary");
                assert_eq!(name_of(response.get_ref()), "recovered");
                assert_eq!(seen.calls(), 1);
            } else {
                let mut stream = channel
                    .server_streaming::<_, HelloReply>(SERVER_HELLO, request)
                    .await
                    .expect("stream")
                    .into_inner();
                assert_eq!(
                    name_of(&stream.message().await.expect("receive").expect("message")),
                    "recovered"
                );
                assert!(stream.message().await.expect("EOF").is_none());
                drop(stream);
                assert_eq!(seen.stream_calls(), 1);
            }
            encodings.push((shape, compressed, encoded.load(Ordering::SeqCst)));
            assert_eq!(channel.retry_stats().transparent_retries, 1);
            assert_eq!(channel.retry_stats().policy_retries, 0);
            assert_eq!(channel.byte_budget_allocated(), 0);
        }
    }
    assert!(
        encodings.iter().all(|(_, _, count)| *count == 1),
        "{encodings:?}"
    );
}

#[tokio::test]
async fn published_retry_throttler_remains_available_at_both_paths() {
    let config = pbrs_grpc::RetryThrottling {
        max_tokens: 4.0,
        token_ratio: 1.0,
    };
    let bucket: pbrs_grpc::service_config::RetryThrottler = pbrs_grpc::RetryThrottler::new(&config);
    assert_eq!(bucket.tokens().await, 4.0);
    assert_eq!(bucket.on_failure().await, 3.0);
    assert!(bucket.retry_allowed().await);
    assert_eq!(bucket.on_failure().await, 2.0);
    assert!(!bucket.retry_allowed().await);
    assert_eq!(bucket.on_success().await, 3.0);
    assert!(bucket.retry_allowed().await);
}

/// One scripted unary outcome. `Copy` so the script stays shareable.
#[derive(Clone, Copy)]
enum Step {
    Ok(&'static str),
    Fail(Code),
    FailPushback(Code, Pushback),
    SleepOk(Duration, &'static str),
}

/// One scripted server-streaming outcome.
#[derive(Clone, Copy)]
enum StreamStep {
    Messages(&'static [&'static str]),
    FailAfter(&'static [&'static str], Code),
    Fail(Code),
    FailPushback(Code, Pushback),
    SleepFail(Duration, Code),
}

/// Counts unary calls and plays `steps[n]` for call `n` (1-based), repeating
/// the last step once the script runs out. Streaming calls play
/// `stream_steps` under a separate counter.
#[derive(Clone)]
struct Scripted {
    calls: Arc<AtomicUsize>,
    steps: Vec<Step>,
    stream_calls: Arc<AtomicUsize>,
    stream_steps: Vec<StreamStep>,
}

impl Scripted {
    fn new(steps: Vec<Step>) -> Self {
        assert!(!steps.is_empty());
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            steps,
            stream_calls: Arc::new(AtomicUsize::new(0)),
            stream_steps: vec![StreamStep::Messages(&[])],
        }
    }

    fn with_streams(mut self, steps: Vec<StreamStep>) -> Self {
        assert!(!steps.is_empty());
        self.stream_steps = steps;
        self
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn stream_calls(&self) -> usize {
        self.stream_calls.load(Ordering::SeqCst)
    }

    async fn play(&self, n: usize) -> Result<Response<HelloReply>, Status> {
        let step = self.steps.get(n - 1).or(self.steps.last()).unwrap();
        match *step {
            Step::Ok(message) => {
                let mut reply = HelloReply::new();
                reply.set_message(message);
                Ok(Response::new(reply))
            }
            Step::Fail(code) => Err(Status::new(code, "scripted")),
            Step::FailPushback(code, pushback) => {
                Err(Status::new(code, "scripted").with_retry_pushback(pushback))
            }
            Step::SleepOk(delay, message) => {
                tokio::time::sleep(delay).await;
                let mut reply = HelloReply::new();
                reply.set_message(message);
                Ok(Response::new(reply))
            }
        }
    }
}

impl Greeter for Scripted {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.play(n).await
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("scripted"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let n = self.stream_calls.fetch_add(1, Ordering::SeqCst) + 1;
        let step = self
            .stream_steps
            .get(n - 1)
            .or(self.stream_steps.last())
            .unwrap();
        match *step {
            StreamStep::Fail(code) => Err(Status::new(code, "scripted")),
            StreamStep::FailPushback(code, pushback) => {
                Err(Status::new(code, "scripted").with_retry_pushback(pushback))
            }
            StreamStep::Messages(messages) => {
                let (tx, rx) = Streaming::channel(8);
                drop(tokio::spawn(async move {
                    for message in messages {
                        let mut reply = HelloReply::new();
                        reply.set_message(*message);
                        if tx.send(reply).await.is_err() {
                            break;
                        }
                    }
                }));
                Ok(Response::new(rx))
            }
            StreamStep::FailAfter(messages, code) => {
                let (tx, rx) = Streaming::channel(8);
                drop(tokio::spawn(async move {
                    for message in messages {
                        let mut reply = HelloReply::new();
                        reply.set_message(*message);
                        if tx.send(reply).await.is_err() {
                            return;
                        }
                    }
                    tx.fail(Status::new(code, "scripted")).await;
                }));
                Ok(Response::new(rx))
            }
            StreamStep::SleepFail(delay, code) => {
                tokio::time::sleep(delay).await;
                Err(Status::new(code, "scripted"))
            }
        }
    }

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("scripted"))
    }
}

async fn channel_with(service: Scripted, json: &str) -> (Scripted, Channel, common::ServerGuard) {
    let (addr, guard) = serve(service.clone(), ServerConfig::default())
        .await
        .unwrap();
    let channel = Channel::connect(addr)
        .await
        .unwrap()
        .service_config(json)
        .unwrap();
    (service, channel, guard)
}

async fn say_hello(channel: &Channel) -> Result<Response<HelloReply>, Status> {
    tokio::time::timeout(
        CALL_BUDGET,
        channel.unary(SAY_HELLO, Request::new(req("ada"))),
    )
    .await
    .expect("unary call hung")
}

async fn collect_stream(channel: &Channel) -> Result<Vec<String>, Status> {
    let mut stream = tokio::time::timeout(
        CALL_BUDGET,
        channel.server_streaming(SERVER_HELLO, Request::new(req("ada"))),
    )
    .await
    .expect("stream headers hung")?
    .into_inner();
    let mut out = Vec::new();
    loop {
        match tokio::time::timeout(CALL_BUDGET, stream.message())
            .await
            .expect("stream message hung")?
        {
            Some(reply) => out.push(name_of(&reply)),
            None => return Ok(out),
        }
    }
}

#[tokio::test]
async fn retry_recovers_after_transient_unavailable() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 3, "initialBackoff": "0.01s", "maxBackoff": "0.05s",
        "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![
            Step::Fail(Code::Unavailable),
            Step::Fail(Code::Unavailable),
            Step::Ok("recovered"),
        ]),
        json,
    )
    .await;
    let reply = say_hello(&channel).await.unwrap();
    assert_eq!(name_of(reply.get_ref()), "recovered");
    assert_eq!(service.calls(), 3);
}

#[tokio::test]
async fn cancellation_and_original_deadline_interrupt_policy_backoff() {
    for server_stream in [false, true] {
        for cancelled in [false, true] {
            let service = Scripted::new(vec![Step::Fail(Code::Unavailable), Step::Ok("follow-up")])
                .with_streams(vec![
                    StreamStep::Fail(Code::Unavailable),
                    StreamStep::Messages(&["follow-up"]),
                ]);
            let (addr, _guard) = serve(service.clone(), ServerConfig::new())
                .await
                .expect("serve");
            let ended = Arc::new(tokio::sync::Notify::new());
            let channel = Channel::connect_with(addr, pbrs_grpc::ChannelConfig::new()
                .connections(1).max_concurrent_rpcs(1)).await.expect("connect")
                .byte_budget(1024)
                .service_config(r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
                    "maxAttempts":3,"initialBackoff":"5s","maxBackoff":"5s",
                    "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE","CANCELLED","DEADLINE_EXCEEDED"]}}]}"#)
                .expect("policy")
                .observer(AttemptObserver { ended: ended.clone(), ready: AtomicUsize::new(0), cancel_on_retry_ready: None });
            let mut request = Request::new(req("first attempt"));
            request.set_timeout(if cancelled {
                Duration::from_secs(3)
            } else {
                Duration::from_millis(150)
            });
            let started = Instant::now();
            let (handle, task) = begin_single_request(&channel, request, server_stream);
            tokio::time::timeout(Duration::from_secs(1), ended.notified())
                .await
                .expect("first attempt ended");
            assert_eq!(single_request_calls(&service, server_stream), 1);
            assert_eq!(
                channel.retry_stats().policy_retries,
                0,
                "backoff is not a dispatched retry"
            );
            assert_eq!(
                channel.byte_budget_allocated(),
                0,
                "backoff holds no byte permit"
            );
            if cancelled {
                handle.cancel();
            }
            let status = tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .expect("backoff interrupted")
                .expect("call task")
                .expect_err("terminal signal");
            assert_eq!(
                status.code(),
                if cancelled {
                    Code::Cancelled
                } else {
                    Code::DeadlineExceeded
                }
            );
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "do not wait for five-second backoff"
            );
            assert_eq!(
                single_request_calls(&service, server_stream),
                1,
                "no execution after terminal signal"
            );
            assert_eq!(channel.retry_stats().policy_retries, 0);
            assert_eq!(channel.byte_budget_allocated(), 0);
            tokio::time::timeout(
                Duration::from_secs(2),
                follow_up_single_request(&channel, server_stream),
            )
            .await
            .expect("RPC admission reclaimed");
            assert_eq!(single_request_calls(&service, server_stream), 2);
            assert_eq!(channel.byte_budget_allocated(), 0);
        }
    }
}

#[tokio::test]
async fn retry_byte_admission_failure_records_no_dispatched_retry() {
    for server_stream in [false, true] {
        let service = Scripted::new(vec![Step::Fail(Code::Unavailable), Step::Ok("follow-up")])
            .with_streams(vec![
                StreamStep::Fail(Code::Unavailable),
                StreamStep::Messages(&["follow-up"]),
            ]);
        let (addr, _guard) = serve(service.clone(), ServerConfig::new())
            .await
            .expect("serve");
        let ended = Arc::new(tokio::sync::Notify::new());
        let channel = Channel::connect_with(
            addr,
            pbrs_grpc::ChannelConfig::new()
                .connections(1)
                .max_concurrent_rpcs(1),
        )
        .await
        .expect("connect")
        .byte_budget(1024)
        .service_config(
            r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
                "maxAttempts":3,"initialBackoff":"0.1s","maxBackoff":"0.1s",
                "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE"]}}]}"#,
        )
        .expect("policy")
        .observer(AttemptObserver {
            ended: ended.clone(),
            ready: AtomicUsize::new(0),
            cancel_on_retry_ready: None,
        });
        let mut request = Request::new(req("first attempt"));
        request.set_timeout(Duration::from_secs(2));
        let (_, task) = begin_single_request(&channel, request, server_stream);
        tokio::time::timeout(Duration::from_secs(1), ended.notified())
            .await
            .expect("first attempt ended");
        let held = channel
            .byte_budget_tracker()
            .acquire(1024)
            .expect("occupy admission during backoff");
        let status = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("admission refusal returned")
            .expect("call task")
            .expect_err("no send capacity");
        assert_eq!(status.code(), Code::ResourceExhausted);
        assert_eq!(single_request_calls(&service, server_stream), 1);
        assert_eq!(
            channel.retry_stats().policy_retries,
            0,
            "byte rejection is not a dispatch"
        );
        drop(held);
        assert_eq!(channel.byte_budget_allocated(), 0);
        tokio::time::timeout(
            Duration::from_secs(2),
            follow_up_single_request(&channel, server_stream),
        )
        .await
        .expect("RPC admission reclaimed");
        assert_eq!(single_request_calls(&service, server_stream), 2);
        assert_eq!(channel.byte_budget_allocated(), 0);
    }
}

#[tokio::test]
async fn retry_cancelled_after_connection_ready_records_no_dispatch() {
    for server_stream in [false, true] {
        let service = Scripted::new(vec![Step::Fail(Code::Unavailable), Step::Ok("follow-up")])
            .with_streams(vec![
                StreamStep::Fail(Code::Unavailable),
                StreamStep::Messages(&["follow-up"]),
            ]);
        let (addr, _guard) = serve(service.clone(), ServerConfig::new())
            .await
            .expect("serve");
        let handle = Arc::new(OnceLock::new());
        let channel = Channel::connect_with(
            addr,
            pbrs_grpc::ChannelConfig::new()
                .connections(1)
                .max_concurrent_rpcs(1),
        )
        .await
        .expect("connect")
        .byte_budget(1024)
        .service_config(
            r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
                "maxAttempts":3,"initialBackoff":"0s","maxBackoff":"0s",
                "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE"]}}]}"#,
        )
        .expect("policy")
        .observer(AttemptObserver {
            ended: Arc::new(tokio::sync::Notify::new()),
            ready: AtomicUsize::new(0),
            cancel_on_retry_ready: Some(handle.clone()),
        });
        let mut request = Request::new(req("first attempt"));
        request.set_timeout(Duration::from_secs(2));
        let (call_handle, task) = begin_single_request(&channel, request, server_stream);
        handle
            .set(call_handle)
            .expect("register handle before polling the call");
        let status = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("cancel returned")
            .expect("call task")
            .expect_err("HEADERS open cancelled");
        assert_eq!(status.code(), Code::Cancelled);
        assert_eq!(single_request_calls(&service, server_stream), 1);
        assert_eq!(
            channel.retry_stats().policy_retries,
            0,
            "cancellation before HEADERS is not a dispatch"
        );
        assert_eq!(channel.byte_budget_allocated(), 0);
        tokio::time::timeout(
            Duration::from_secs(2),
            follow_up_single_request(&channel, server_stream),
        )
        .await
        .expect("RPC admission reclaimed");
        assert_eq!(single_request_calls(&service, server_stream), 2);
        assert_eq!(channel.byte_budget_allocated(), 0);
    }
}

async fn interrupted_retry_cause_is_not_counted(per_attempt_timeout: bool) {
    for server_stream in [false, true] {
        let service = if per_attempt_timeout {
            Scripted::new(vec![
                Step::SleepOk(Duration::from_secs(5), "late"),
                Step::Ok("follow-up"),
            ])
            .with_streams(vec![
                StreamStep::SleepFail(Duration::from_secs(5), Code::Unavailable),
                StreamStep::Messages(&["follow-up"]),
            ])
        } else {
            Scripted::new(vec![
                Step::FailPushback(Code::Unavailable, Pushback::Delay(Duration::from_secs(5))),
                Step::Ok("follow-up"),
            ])
            .with_streams(vec![
                StreamStep::FailPushback(
                    Code::Unavailable,
                    Pushback::Delay(Duration::from_secs(5)),
                ),
                StreamStep::Messages(&["follow-up"]),
            ])
        };
        let (addr, _guard) = serve(service.clone(), ServerConfig::new())
            .await
            .expect("serve");
        let ended = Arc::new(tokio::sync::Notify::new());
        let attempt_limit = if per_attempt_timeout {
            r#", "perAttemptRecvTimeout":"0.05s""#
        } else {
            ""
        };
        let config = format!(
            r#"{{"methodConfig":[{{"name":[{{}}],"retryPolicy":{{
            "maxAttempts":3,"initialBackoff":"5s","maxBackoff":"5s",
            "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE"]{attempt_limit}}}}}]}}"#
        );
        let channel = Channel::connect_with(
            addr,
            pbrs_grpc::ChannelConfig::new()
                .connections(1)
                .max_concurrent_rpcs(1),
        )
        .await
        .expect("connect")
        .byte_budget(1024)
        .service_config(&config)
        .expect("policy")
        .observer(AttemptObserver {
            ended: ended.clone(),
            ready: AtomicUsize::new(0),
            cancel_on_retry_ready: None,
        });
        let mut request = Request::new(req("first attempt"));
        request.set_timeout(Duration::from_secs(2));
        let (handle, task) = begin_single_request(&channel, request, server_stream);
        tokio::time::timeout(Duration::from_secs(1), ended.notified())
            .await
            .expect("first attempt ended");
        let stats = channel.retry_stats();
        assert_eq!(
            (
                stats.policy_retries,
                stats.pushback_delays,
                stats.per_attempt_timeouts
            ),
            (0, 0, 0),
            "an interrupted retry cause is not a dispatched attempt"
        );
        handle.cancel();
        let status = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("backoff interrupted")
            .expect("call task")
            .expect_err("cancelled");
        assert_eq!(status.code(), Code::Cancelled);
        assert_eq!(single_request_calls(&service, server_stream), 1);
        assert_eq!(channel.byte_budget_allocated(), 0);
        tokio::time::timeout(
            Duration::from_secs(2),
            follow_up_single_request(&channel, server_stream),
        )
        .await
        .expect("RPC admission reclaimed");
        assert_eq!(single_request_calls(&service, server_stream), 2);
        let stats = channel.retry_stats();
        assert_eq!(
            (
                stats.policy_retries,
                stats.pushback_delays,
                stats.per_attempt_timeouts
            ),
            (0, 0, 0)
        );
        assert_eq!(channel.byte_budget_allocated(), 0);
    }
}

#[tokio::test]
async fn cancelled_pushback_records_no_dispatched_retry_cause() {
    interrupted_retry_cause_is_not_counted(false).await;
}

#[tokio::test]
async fn cancelled_attempt_timeout_backoff_records_no_retry_cause() {
    interrupted_retry_cause_is_not_counted(true).await;
}

#[tokio::test]
async fn transparent_replacement_does_not_count_the_policy_dispatch_twice() {
    use pbrs::Serialize;
    for server_stream in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("address");
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let third_entered = entered.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let third_release = release.clone();
        let server = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            while let Ok((socket, _)) = listener.accept().await {
                let seen = seen.clone();
                let entered = third_entered.clone();
                let release = third_release.clone();
                connections.spawn(async move {
                    let mut conn = h2::server::handshake(socket).await.expect("handshake");
                    while let Some(Ok((_request, mut respond))) = conn.accept().await {
                        let request = seen.fetch_add(1, Ordering::SeqCst);
                        if request == 1 {
                            respond.send_reset(h2::Reason::REFUSED_STREAM);
                            continue;
                        }
                        if request == 2 {
                            entered.notify_one();
                            release.notified().await;
                        }
                        let response = http::Response::builder()
                            .status(200)
                            .header("content-type", "application/grpc");
                        if request == 0 {
                            respond
                                .send_response(
                                    response
                                        .header("grpc-status", "14")
                                        .body(())
                                        .expect("rejection"),
                                    true,
                                )
                                .expect("trailers-only");
                        } else {
                            let mut send = respond
                                .send_response(response.body(()).expect("headers"), false)
                                .expect("response");
                            let payload =
                                common::reply("follow-up").serialize().expect("serialize");
                            send.send_data(
                                pbrs_grpc::codec::encode(&payload, false).expect("frame"),
                                false,
                            )
                            .expect("DATA");
                            let mut trailers = http::HeaderMap::new();
                            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                            send.send_trailers(trailers).expect("trailers");
                        }
                    }
                });
            }
        });
        let _guard = common::ServerGuard(server);
        let channel = Channel::connect_with(
            addr,
            pbrs_grpc::ChannelConfig::new()
                .connections(1)
                .max_concurrent_rpcs(1),
        )
        .await
        .expect("connect")
        .byte_budget(1024)
        .service_config(
            r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
                "maxAttempts":3,"initialBackoff":"0s","maxBackoff":"0s",
                "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE"]}}]}"#,
        )
        .expect("policy");
        let mut request = Request::new(req("side-effect"));
        request.set_timeout(Duration::from_secs(2));
        let (_, task) = begin_single_request(&channel, request, server_stream);
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .expect("transparent replacement entered");
        assert_eq!(requests.load(Ordering::SeqCst), 3);
        let stats = channel.retry_stats();
        assert_eq!(
            stats.policy_retries, 1,
            "policy dispatch counted once before terminal response"
        );
        assert_eq!(stats.transparent_retries, 1);
        release.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("response returned")
            .expect("call task")
            .expect("success");
        if let Some(mut stream) = result {
            assert_eq!(
                name_of(&stream.message().await.expect("receive").expect("message")),
                "follow-up"
            );
            assert!(stream.message().await.expect("EOF").is_none());
            drop(stream);
        }
        assert_eq!(channel.retry_stats().policy_retries, 1);
        assert_eq!(channel.byte_budget_allocated(), 0);
        tokio::time::timeout(
            Duration::from_secs(2),
            follow_up_single_request(&channel, server_stream),
        )
        .await
        .expect("RPC admission reclaimed");
        assert_eq!(channel.byte_budget_allocated(), 0);
    }
}

#[tokio::test]
async fn oversized_policy_request_never_dispatches_or_debits_throttling() {
    let service = Scripted::new(vec![Step::Fail(Code::Unavailable), Step::Ok("recovered")]);
    let (addr, _guard) = serve(service.clone(), ServerConfig::new())
        .await
        .expect("serve");
    let channel = Channel::connect_with(
        addr,
        pbrs_grpc::ChannelConfig::new()
            .connections(1)
            .max_concurrent_rpcs(1),
    )
    .await
    .expect("connect")
    .max_encoding_message_size(32)
    .byte_budget(1024)
    .service_config(
        r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
            "maxAttempts":3,"initialBackoff":"0s","maxBackoff":"0s",
            "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE","RESOURCE_EXHAUSTED"]}}],
            "retryThrottling":{"maxTokens":2,"tokenRatio":1}}"#,
    )
    .expect("policy");
    let status = channel
        .unary::<_, HelloReply>(SAY_HELLO, Request::new(req(&"x".repeat(128))))
        .await
        .expect_err("oversize request");
    assert_eq!(status.code(), Code::ResourceExhausted);
    assert_eq!(service.calls(), 0);
    assert_eq!(channel.retry_stats(), pbrs_grpc::RetryStats::default());
    assert_eq!(channel.byte_budget_allocated(), 0);
    // With maxTokens=2, a debit would leave the bucket at its refusal
    // threshold. This eligible failure can retry only if encode rejection
    // left the shared throttler intact.
    let response = tokio::time::timeout(Duration::from_secs(2), say_hello(&channel))
        .await
        .expect("RPC admission reclaimed")
        .expect("throttler was not debited");
    assert_eq!(name_of(response.get_ref()), "recovered");
    assert_eq!(service.calls(), 2);
    assert_eq!(channel.retry_stats().policy_retries, 1);
    assert_eq!(channel.byte_budget_allocated(), 0);
}

#[tokio::test]
async fn retry_exhausts_attempts() {
    let json = r#"{"methodConfig": [{"name": [{"service": "helloworld.Greeter", "method": "SayHello"}],
        "retryPolicy": {
        "maxAttempts": 3, "initialBackoff": "0.01s", "maxBackoff": "0.05s",
        "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) =
        channel_with(Scripted::new(vec![Step::Fail(Code::Unavailable)]), json).await;
    let err = say_hello(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::Unavailable);
    assert_eq!(service.calls(), 3);
}

#[tokio::test]
async fn non_retryable_code_fails_fast() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 4, "initialBackoff": "0.01s", "maxBackoff": "0.05s",
        "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) =
        channel_with(Scripted::new(vec![Step::Fail(Code::Internal)]), json).await;
    let err = say_hello(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert_eq!(service.calls(), 1);
}

#[tokio::test]
async fn pushback_do_not_retry_stops_after_one_attempt() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 4, "initialBackoff": "0.01s", "maxBackoff": "0.05s",
        "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![Step::FailPushback(
            Code::Unavailable,
            Pushback::DoNotRetry,
        )]),
        json,
    )
    .await;
    let err = say_hello(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::Unavailable);
    assert_eq!(err.retry_pushback(), Some(Pushback::DoNotRetry));
    assert_eq!(service.calls(), 1);
}

#[tokio::test]
async fn pushback_delay_still_retries() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 3, "initialBackoff": "1s", "maxBackoff": "1s",
        "backoffMultiplier": 1.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![
            Step::FailPushback(Code::Unavailable, Pushback::Delay(Duration::from_millis(5))),
            Step::Ok("pushed"),
        ]),
        json,
    )
    .await;
    // The 1s computed backoff would blow the call budget; the 5ms pushback
    // wins instead.
    let reply = say_hello(&channel).await.unwrap();
    assert_eq!(name_of(reply.get_ref()), "pushed");
    assert_eq!(service.calls(), 2);
}

#[tokio::test]
async fn throttling_disables_retries_when_drained() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 3, "initialBackoff": "0.005s", "maxBackoff": "0.01s",
        "backoffMultiplier": 1.0, "retryableStatusCodes": ["UNAVAILABLE"]}}],
        "retryThrottling": {"maxTokens": 4, "tokenRatio": 0.1}}"#;
    let (service, channel, _guard) =
        channel_with(Scripted::new(vec![Step::Fail(Code::Unavailable)]), json).await;
    // Two fully-failed calls drain 4 tokens to 2, which is not above half.
    for _ in 0..2 {
        let err = say_hello(&channel).await.unwrap_err();
        assert_eq!(err.code(), Code::Unavailable);
    }
    assert_eq!(service.calls(), 6);
    // The throttled call makes one attempt and fails without retrying.
    let err = say_hello(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::Unavailable);
    assert_eq!(service.calls(), 7);
}

#[tokio::test]
async fn throttling_refills_on_success() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 2, "initialBackoff": "0.005s", "maxBackoff": "0.01s",
        "backoffMultiplier": 1.0, "retryableStatusCodes": ["UNAVAILABLE"]}}],
        "retryThrottling": {"maxTokens": 4, "tokenRatio": 4.0}}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![
            Step::Fail(Code::Unavailable),
            Step::Fail(Code::Unavailable),
            Step::Ok("fine"),
            Step::Fail(Code::Unavailable),
            Step::Ok("fine"),
        ]),
        json,
    )
    .await;
    // 4 -> 3 after the failed call; the success refills to 4 (capped).
    say_hello(&channel).await.unwrap_err();
    assert_eq!(service.calls(), 2);
    let reply = say_hello(&channel).await.unwrap();
    assert_eq!(name_of(reply.get_ref()), "fine");
    // Still above half, so this call retries its transient failure.
    let reply = say_hello(&channel).await.unwrap();
    assert_eq!(name_of(reply.get_ref()), "fine");
    assert_eq!(service.calls(), 5);
}

#[tokio::test]
async fn hedging_first_ok_wins() {
    let json = r#"{"methodConfig": [{"name": [{}],
        "hedgingPolicy": {"maxAttempts": 3, "hedgingDelay": "0.05s",
        "nonFatalStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![
            Step::SleepOk(Duration::from_secs(1), "slow"),
            Step::Ok("hedged"),
        ]),
        json,
    )
    .await;
    let start = Instant::now();
    let reply = say_hello(&channel).await.unwrap();
    let elapsed = start.elapsed();
    assert_eq!(name_of(reply.get_ref()), "hedged");
    assert_eq!(service.calls(), 2);
    assert!(
        elapsed < Duration::from_millis(800),
        "hedged call took {elapsed:?}"
    );
}

#[tokio::test]
async fn hedging_fatal_status_commits() {
    let json = r#"{"methodConfig": [{"name": [{}],
        "hedgingPolicy": {"maxAttempts": 3, "hedgingDelay": "0.01s",
        "nonFatalStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) =
        channel_with(Scripted::new(vec![Step::Fail(Code::Internal)]), json).await;
    let err = say_hello(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert_eq!(service.calls(), 1);
}

#[tokio::test]
async fn hedging_returns_last_nonfatal_when_exhausted() {
    let json = r#"{"methodConfig": [{"name": [{}],
        "hedgingPolicy": {"maxAttempts": 2, "hedgingDelay": "0.01s",
        "nonFatalStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) =
        channel_with(Scripted::new(vec![Step::Fail(Code::Unavailable)]), json).await;
    let err = say_hello(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::Unavailable);
    assert_eq!(service.calls(), 2);
}

#[tokio::test]
async fn method_config_timeout_applies_without_request_overlay() {
    let json = r#"{"methodConfig": [{"name": [{}], "timeout": "0.05s"}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![Step::SleepOk(Duration::from_secs(5), "slow")]),
        json,
    )
    .await;
    let err = say_hello(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::DeadlineExceeded);
    assert_eq!(service.calls(), 1);
}

#[tokio::test]
async fn per_attempt_timeout_retries_without_listing_deadline() {
    let json = r#"{"methodConfig": [{"name": [{}], "timeout": "10s",
        "retryPolicy": {"maxAttempts": 3,
        "initialBackoff": "0.005s", "maxBackoff": "0.01s",
        "backoffMultiplier": 1.0, "perAttemptRecvTimeout": "0.05s",
        "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![
            Step::SleepOk(Duration::from_secs(5), "slow"),
            Step::Ok("second"),
        ]),
        json,
    )
    .await;
    let reply = say_hello(&channel).await.unwrap();
    assert_eq!(name_of(reply.get_ref()), "second");
    assert_eq!(service.calls(), 2);
}

#[tokio::test]
async fn streaming_retry_recovers_before_headers() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 3, "initialBackoff": "0.01s", "maxBackoff": "0.05s",
        "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![Step::Ok("unused")]).with_streams(vec![
            StreamStep::Fail(Code::Unavailable),
            StreamStep::Fail(Code::Unavailable),
            StreamStep::Messages(&["a", "b"]),
        ]),
        json,
    )
    .await;
    let messages = collect_stream(&channel).await.unwrap();
    assert_eq!(messages, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(service.stream_calls(), 3);
}

#[tokio::test]
async fn streaming_retry_exhausts_attempts() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 3, "initialBackoff": "0.01s", "maxBackoff": "0.05s",
        "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![Step::Ok("unused")])
            .with_streams(vec![StreamStep::Fail(Code::Unavailable)]),
        json,
    )
    .await;
    let err = collect_stream(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::Unavailable);
    assert_eq!(service.stream_calls(), 3);
}

#[tokio::test]
async fn streaming_never_retries_after_first_message() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 3, "initialBackoff": "0.01s", "maxBackoff": "0.05s",
        "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![Step::Ok("unused")])
            .with_streams(vec![StreamStep::FailAfter(&["first"], Code::Unavailable)]),
        json,
    )
    .await;
    let mut stream = tokio::time::timeout(
        CALL_BUDGET,
        channel.server_streaming(SERVER_HELLO, Request::new(req("ada"))),
    )
    .await
    .expect("stream headers hung")
    .unwrap()
    .into_inner();
    let first = tokio::time::timeout(CALL_BUDGET, stream.message())
        .await
        .expect("first message hung")
        .unwrap()
        .unwrap();
    assert_eq!(name_of(&first), "first");
    let err = tokio::time::timeout(CALL_BUDGET, stream.message())
        .await
        .expect("terminal status hung")
        .unwrap_err();
    assert_eq!(err.code(), Code::Unavailable);
    assert_eq!(service.stream_calls(), 1);
}

#[tokio::test]
async fn streaming_non_retryable_code_fails_fast() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 4, "initialBackoff": "0.01s", "maxBackoff": "0.05s",
        "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![Step::Ok("unused")])
            .with_streams(vec![StreamStep::Fail(Code::Internal)]),
        json,
    )
    .await;
    let err = collect_stream(&channel).await.unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert_eq!(service.stream_calls(), 1);
}

#[tokio::test]
async fn streaming_per_attempt_timeout_retries_slow_headers() {
    let json = r#"{"methodConfig": [{"name": [{}], "timeout": "10s",
        "retryPolicy": {"maxAttempts": 3,
        "initialBackoff": "0.005s", "maxBackoff": "0.01s",
        "backoffMultiplier": 1.0, "perAttemptRecvTimeout": "0.05s",
        "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![Step::Ok("unused")]).with_streams(vec![
            StreamStep::SleepFail(Duration::from_secs(5), Code::Unavailable),
            StreamStep::Messages(&["second"]),
        ]),
        json,
    )
    .await;
    let messages = collect_stream(&channel).await.unwrap();
    assert_eq!(messages, vec!["second".to_string()]);
    assert_eq!(service.stream_calls(), 2);
}

#[tokio::test]
async fn retry_stats_count_dispatched_policy_attempts() {
    let json = r#"{"methodConfig": [{"name": [{}], "retryPolicy": {
        "maxAttempts": 3, "initialBackoff": "0.005s", "maxBackoff": "0.01s",
        "backoffMultiplier": 1.0, "retryableStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (service, channel, _guard) = channel_with(
        Scripted::new(vec![
            Step::Fail(Code::Unavailable),
            Step::Fail(Code::Unavailable),
            Step::Ok("recovered"),
            Step::Fail(Code::Unavailable),
            Step::Fail(Code::Unavailable),
            Step::Fail(Code::Unavailable),
        ]),
        json,
    )
    .await;
    say_hello(&channel).await.unwrap();
    say_hello(&channel).await.unwrap_err();
    assert_eq!(service.calls(), 6);
    let stats = channel.retry_stats();
    assert_eq!(stats.calls, 2);
    assert_eq!(stats.policy_retries, 4);
    assert_eq!(stats.exhausted, 1);
    assert_eq!(stats.committed_ok, 1);
    assert_eq!(stats.committed_err, 1);
    assert_eq!(stats.transparent_retries, 0);
    assert_eq!(stats.throttled, 0);
}

#[tokio::test]
async fn retry_stats_count_hedged_sends() {
    let json = r#"{"methodConfig": [{"name": [{}],
        "hedgingPolicy": {"maxAttempts": 3, "hedgingDelay": "0.05s",
        "nonFatalStatusCodes": ["UNAVAILABLE"]}}]}"#;
    let (_service, channel, _guard) = channel_with(
        Scripted::new(vec![
            Step::SleepOk(Duration::from_secs(1), "slow"),
            Step::Ok("hedged"),
        ]),
        json,
    )
    .await;
    say_hello(&channel).await.unwrap();
    let stats = channel.retry_stats();
    assert_eq!(stats.calls, 1);
    assert_eq!(stats.hedged_sends, 1);
    assert_eq!(stats.committed_ok, 1);
    assert_eq!(stats.committed_err, 0);
}

#[tokio::test]
async fn invalid_service_config_leaves_channel_unchanged() {
    let (addr, _guard) = serve(
        Scripted::new(vec![Step::Ok("fine")]),
        ServerConfig::default(),
    )
    .await
    .unwrap();
    let channel = Channel::connect(addr).await.unwrap();
    let err = channel
        .clone()
        .service_config(r#"{"methodConfig": [{"name": [{"method": "X"}]}]}"#)
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    let err = channel.clone().service_config("not json").unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(channel.service_config_doc().is_none());
}
