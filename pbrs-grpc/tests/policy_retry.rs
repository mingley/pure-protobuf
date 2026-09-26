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

use common::{name_of, req, serve};
use pbrs_grpc::hello::{Greeter, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Pushback, Request, Response, ServerConfig, Status, Streaming};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const SAY_HELLO: &str = "/helloworld.Greeter/SayHello";
const CALL_BUDGET: Duration = Duration::from_secs(15);

/// One scripted unary outcome. `Copy` so the script stays shareable.
#[derive(Clone, Copy)]
enum Step {
    Ok(&'static str),
    Fail(Code),
    FailPushback(Code, Pushback),
    SleepOk(Duration, &'static str),
    SleepFail(Duration, Code),
}

/// Counts unary calls and plays `steps[n]` for call `n` (1-based), repeating
/// the last step once the script runs out.
#[derive(Clone)]
struct Scripted {
    calls: Arc<AtomicUsize>,
    steps: Vec<Step>,
}

impl Scripted {
    fn new(steps: Vec<Step>) -> Self {
        assert!(!steps.is_empty());
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            steps,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
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
            Step::SleepFail(delay, code) => {
                tokio::time::sleep(delay).await;
                Err(Status::new(code, "scripted"))
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
        Err(Status::unimplemented("scripted"))
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
