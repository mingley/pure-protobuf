//! Service config through resolvers (A2/A21) and LB selection (A24).
//!
//! DNS TXT documents adopt onto resolver-managed channels: an invalid
//! initial document fails the channel, invalid updates keep the last
//! good document, and valid updates apply live (observed through
//! retry behavior). `loadBalancingConfig` selects the first
//! registered policy in preference order.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    unreachable_pub,
    reason = "integration tests"
)]

mod common;

use common::{ServerGuard, req};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::lb::{LbPolicyFactory, register_lb_policy_factory, select_lb_policy};
use pbrs_grpc::resolver::{
    DnsConfig, DnsLookup, ResolverConfig, TxtLookup, parse_target_uri, resolver_for,
};
use pbrs_grpc::{
    Channel, Code, LbPolicyConfig, MethodName, Request, Response, RetryPolicy, ServerConfig,
    ServiceConfig, Status,
};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

/// Retry three attempts on UNAVAILABLE, for every method.
const RETRY_DOC: &str = r#"{
    "methodConfig": [{
        "name": [],
        "retryPolicy": {
            "maxAttempts": 3,
            "initialBackoff": "0.001s",
            "maxBackoff": "0.005s",
            "backoffMultiplier": 1.0,
            "retryableStatusCodes": ["UNAVAILABLE"]
        }
    }]
}"#;

/// No retry anywhere.
const PLAIN_DOC: &str = r#"{"methodConfig": []}"#;

/// Always-UNAVAILABLE Greeter that counts handler executions.
struct Flaky {
    attempts: Arc<AtomicUsize>,
}

impl Greeter for Flaky {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err(Status::unavailable("flaky"))
    }
}

/// Scripted A answers repeating the last one.
struct ScriptA {
    steps: tokio::sync::Mutex<Vec<Vec<SocketAddr>>>,
}

impl DnsLookup for ScriptA {
    fn lookup(
        &self,
        _host: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, io::Error>> + Send + '_>> {
        Box::pin(async move {
            let mut steps = self.steps.lock().await;
            if steps.len() > 1 {
                Ok(steps.remove(0))
            } else {
                Ok(steps.first().cloned().unwrap_or_default())
            }
        })
    }
}

/// TXT cell the test rewrites between refreshes.
enum TxtOutcome {
    Doc(&'static str),
    Garbage,
    Fail,
}

struct CellTxt {
    current: tokio::sync::Mutex<TxtOutcome>,
}

impl TxtLookup for CellTxt {
    fn fetch_txt(
        &self,
        _name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, io::Error>> + Send + '_>> {
        Box::pin(async move {
            match *self.current.lock().await {
                TxtOutcome::Doc(doc) => Ok(vec![doc.to_owned()]),
                TxtOutcome::Garbage => Ok(vec!["{not json".to_owned()]),
                TxtOutcome::Fail => Err(io::Error::other("scripted txt failure")),
            }
        })
    }
}

fn bounds() -> DnsConfig {
    DnsConfig::new(
        Duration::from_millis(50),
        Duration::from_secs(1),
        Duration::from_millis(100),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Duration::from_secs(1),
    )
    .expect("bounds")
}

async fn bind() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    (addr, listener)
}

fn has_retry(doc: &ServiceConfig) -> bool {
    doc.method_config("helloworld.Greeter", "SayHello")
        .is_some_and(|m| m.retry_policy.is_some())
}

async fn wait_for_retry(channel: &Channel, want: bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let got = channel.service_config_doc().is_some_and(|d| has_retry(&d));
        if got == want {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "retry adoption stuck at {got}, want {want}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn snapshot_carries_txt_document() {
    let txt = Arc::new(CellTxt {
        current: tokio::sync::Mutex::new(TxtOutcome::Doc(RETRY_DOC)),
    });
    let a = Arc::new(ScriptA {
        steps: tokio::sync::Mutex::new(vec![vec!["10.9.9.9:443".parse().expect("addr")]]),
    });
    let config = ResolverConfig::with_dns_provider(bounds(), a).with_txt_provider(txt.clone());
    let target = parse_target_uri("dns:///cfg.invalid:443").expect("target");
    let built = resolver_for(&target, &config).await.expect("dns");
    assert_eq!(built.initial.service_config(), Some(RETRY_DOC));
}

#[tokio::test]
async fn invalid_initial_config_fails_the_channel() {
    let txt = Arc::new(CellTxt {
        current: tokio::sync::Mutex::new(TxtOutcome::Garbage),
    });
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Flaky {
            attempts: Arc::new(AtomicUsize::new(0)),
        })
        .serve_listener(listener)
        .await
        .ok();
    });
    let _guard = ServerGuard(handle);
    let a = Arc::new(ScriptA {
        steps: tokio::sync::Mutex::new(vec![vec![addr]]),
    });
    let config = ResolverConfig::with_dns_provider(bounds(), a).with_txt_provider(txt);
    let err = Channel::connect_uri("dns:///cfg.invalid:443", config)
        .await
        .expect_err("invalid initial config fails");
    assert_eq!(err.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn invalid_updates_keep_last_good_and_valid_updates_apply() {
    let txt = Arc::new(CellTxt {
        current: tokio::sync::Mutex::new(TxtOutcome::Doc(RETRY_DOC)),
    });
    let attempts = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&attempts);
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Flaky { attempts: worker })
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let a = Arc::new(ScriptA {
        steps: tokio::sync::Mutex::new(vec![vec![addr]]),
    });
    let config = ResolverConfig::with_dns_provider(bounds(), a).with_txt_provider(txt.clone());
    let channel = Channel::connect_uri("dns:///cfg.invalid:443", config)
        .await
        .expect("channel");
    let client = GreeterClient::new(channel.clone());

    // Phase 1: delivered retry policy drives 3 attempts.
    assert!(has_retry(
        &channel.service_config_doc().expect("initial adopted")
    ));
    let before = attempts.load(Ordering::SeqCst);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("flaky fails");
    assert_eq!(attempts.load(Ordering::SeqCst) - before, 3);

    // Phase 2: garbage TXT keeps the last good document.
    *txt.current.lock().await = TxtOutcome::Garbage;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(has_retry(&channel.service_config_doc().expect("kept good")));
    let before = attempts.load(Ordering::SeqCst);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("flaky fails");
    assert_eq!(attempts.load(Ordering::SeqCst) - before, 3);

    // A failed TXT fetch also keeps the last good document.
    *txt.current.lock().await = TxtOutcome::Fail;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(has_retry(&channel.service_config_doc().expect("kept good")));

    // Phase 3: a new valid document applies live: 1 attempt, no retry.
    *txt.current.lock().await = TxtOutcome::Doc(PLAIN_DOC);
    wait_for_retry(&channel, false).await;
    let before = attempts.load(Ordering::SeqCst);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("flaky fails");
    assert_eq!(attempts.load(Ordering::SeqCst) - before, 1);

    // Recovery: the retry document returns and retries resume.
    *txt.current.lock().await = TxtOutcome::Doc(RETRY_DOC);
    wait_for_retry(&channel, true).await;
    let before = attempts.load(Ordering::SeqCst);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("flaky fails");
    assert_eq!(attempts.load(Ordering::SeqCst) - before, 3);
}

struct Probe(&'static str);

impl LbPolicyFactory for Probe {
    fn name(&self) -> &str {
        self.0
    }
}

#[test]
fn lb_selection_prefers_first_registered() {
    register_lb_policy_factory(Arc::new(Probe("round_robin")));
    register_lb_policy_factory(Arc::new(Probe("test_custom")));
    let config = ServiceConfig::parse(
        r#"{"loadBalancingConfig": [
            {"no_such_policy": {}},
            {"test_custom": {"n": 1}},
            {"round_robin": {}}
        ]}"#,
    )
    .expect("parses");
    let selected = select_lb_policy(&config).expect("selected");
    assert_eq!(selected.name, "test_custom");
}

// --- FL-07: approved-contract §13 vectors ------------------------------------
// Numbered per docs/service-config.md §13. "Invalid" always means
// `Code::InvalidArgument` with a message naming the offending entry/field.

/// Assert `doc` fails with `InvalidArgument` naming every `fragments`.
fn assert_invalid(doc: &str, fragments: &[&str]) {
    let err = ServiceConfig::parse(doc).expect_err("document must be invalid");
    assert_eq!(err.code(), Code::InvalidArgument);
    for fragment in fragments {
        assert!(
            err.message().contains(fragment),
            "message {:?} names {fragment:?}",
            err.message()
        );
    }
}

/// `{"methodConfig": [{"name": [{}], "retryPolicy": <policy>}]}`.
fn retry_doc(policy: &str) -> String {
    format!(r#"{{"methodConfig": [{{"name": [{{}}], "retryPolicy": {policy}}}]}}"#)
}

/// Baseline valid policy `f0` from §13.2.
const F0: &str = r#"{"maxAttempts": 3, "initialBackoff": "0.1s", "maxBackoff": "1s", "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}"#;

/// Baseline valid policy with one field replaced by raw JSON `value`.
fn retry_policy_with(field: &str, value: &str) -> String {
    let mut policy = serde_json::from_str::<serde_json::Value>(F0).expect("f0");
    policy[field] = serde_json::from_str(value).expect("value");
    retry_doc(&policy.to_string())
}

/// Baseline valid policy with one field removed.
fn retry_policy_without(field: &str) -> String {
    let mut policy = serde_json::from_str::<serde_json::Value>(F0).expect("f0");
    policy
        .as_object_mut()
        .expect("object")
        .remove(field)
        .expect("field");
    retry_doc(&policy.to_string())
}

/// The global retry policy of a one-entry doc.
fn global_policy(config: &ServiceConfig) -> &RetryPolicy {
    config
        .method_config("", "")
        .expect("global entry")
        .retry_policy
        .as_ref()
        .expect("retry policy")
}

#[test]
fn fl07_vec01_rejects_garbage() {
    assert_invalid("not json", &["not valid JSON"]);
}

#[test]
fn fl07_vec02_requires_object() {
    assert_invalid("[]", &["must be a JSON object"]);
}

#[test]
fn fl07_vec03_method_config_must_be_array() {
    assert_invalid(
        r#"{"methodConfig": {}}"#,
        &["methodConfig", "must be an array"],
    );
}

#[test]
fn fl07_vec04_entry_must_be_object() {
    assert_invalid(
        r#"{"methodConfig": ["x"]}"#,
        &["methodConfig[0]", "must be an object"],
    );
}

#[test]
fn fl07_vec05_name_must_be_array() {
    assert_invalid(
        r#"{"methodConfig": [{"name": {}}]}"#,
        &["methodConfig[0].name", "must be an array"],
    );
}

#[test]
fn fl07_vec06_method_without_service() {
    assert_invalid(
        r#"{"methodConfig": [{"name": [{"method": "M"}]}]}"#,
        &["methodConfig[0].name[0]", "method without service"],
    );
}

#[test]
fn fl07_vec07_duplicates_rejected() {
    // Same exact pair in two entries.
    assert_invalid(
        r#"{"methodConfig": [
            {"name": [{"service": "s", "method": "m"}]},
            {"name": [{"service": "s", "method": "m"}]}
        ]}"#,
        &["duplicate"],
    );
    // Two global defaults.
    assert_invalid(
        r#"{"methodConfig": [{"name": [{}]}, {"name": [{}]}]}"#,
        &["duplicate"],
    );
    // Two missing names are two global defaults.
    assert_invalid(
        r#"{"methodConfig": [{}, {"timeout": "1s"}]}"#,
        &["duplicate"],
    );
    // Same pair twice inside one entry.
    assert_invalid(
        r#"{"methodConfig": [{"name": [{"service": "s"}, {"service": "s"}]}]}"#,
        &["duplicate"],
    );
}

#[test]
fn fl07_vec08_retry_and_hedging_exclusive() {
    let doc = format!(
        r#"{{"methodConfig": [{{"name": [{{}}], "retryPolicy": {F0}, "hedgingPolicy": {{"maxAttempts": 2}}}}]}}"#
    );
    assert_invalid(&doc, &["methodConfig[0]", "retryPolicy", "hedgingPolicy"]);
}

#[test]
fn fl07_vec09_unknown_fields_ignored() {
    let config = ServiceConfig::parse(
        r#"{"methodConfig": [{"name": [{}], "futureField": 1}], "topLevelFuture": {}}"#,
    )
    .expect("unknown fields are ignored");
    let method = config
        .method_config("any.Service", "AnyMethod")
        .expect("global default covers everything");
    assert_eq!(
        method.names,
        vec![MethodName {
            service: String::new(),
            method: String::new(),
        }]
    );
    assert_eq!(method.wait_for_ready, None);
    assert_eq!(method.timeout, None);
    assert_eq!(method.max_request_message_bytes, None);
    assert_eq!(method.max_response_message_bytes, None);
    assert!(method.retry_policy.is_none());
    assert!(method.hedging_policy.is_none());
}

#[test]
fn fl07_vec10_baseline_policy() {
    let config = ServiceConfig::parse(&retry_doc(F0)).expect("f0 is valid");
    let policy = global_policy(&config);
    assert_eq!(policy.max_attempts, 3);
    assert_eq!(policy.initial_backoff, Duration::from_millis(100));
    assert_eq!(policy.max_backoff, Duration::from_secs(1));
    assert_eq!(policy.backoff_multiplier, 2.0);
    assert_eq!(policy.per_attempt_recv_timeout, None);
    assert_eq!(policy.retryable_status_codes.len(), 1);
    assert!(policy.retryable_status_codes.contains(&Code::Unavailable));
}

#[test]
fn fl07_vec11_max_attempts_validated() {
    assert_invalid(
        &retry_policy_without("maxAttempts"),
        &["methodConfig[0].retryPolicy.maxAttempts", "is required"],
    );
    for raw in ["1", "0", "-2", "2.5", "\"3\""] {
        assert_invalid(
            &retry_policy_with("maxAttempts", raw),
            &["methodConfig[0].retryPolicy.maxAttempts"],
        );
    }
}

#[test]
fn fl07_vec12_max_attempts_clamped() {
    let config = ServiceConfig::parse(&retry_policy_with("maxAttempts", "7")).expect("clamps to 5");
    assert_eq!(global_policy(&config).max_attempts, 5);
}

/// Malformed durations from vectors 13/14/18: suffix-less, negative,
/// over-precise, non-numeric, non-string, empty.
const BAD_DURATIONS: [&str; 6] = [
    "\"100ms\"",
    "\"-1s\"",
    "\"1.1234567899s\"",
    "\"abc\"",
    "0.1",
    "\"\"",
];

#[test]
fn fl07_vec13_initial_backoff_validated() {
    assert_invalid(
        &retry_policy_without("initialBackoff"),
        &["methodConfig[0].retryPolicy.initialBackoff", "is required"],
    );
    for raw in BAD_DURATIONS {
        assert_invalid(
            &retry_policy_with("initialBackoff", raw),
            &["methodConfig[0].retryPolicy.initialBackoff"],
        );
    }
    // "0s" is valid (§5): zero backoff is the operator's choice.
    let config = ServiceConfig::parse(&retry_policy_with("initialBackoff", "\"0s\""))
        .expect("zero backoff is valid");
    assert_eq!(global_policy(&config).initial_backoff, Duration::ZERO);
}

#[test]
fn fl07_vec14_max_backoff_validated() {
    assert_invalid(
        &retry_policy_without("maxBackoff"),
        &["methodConfig[0].retryPolicy.maxBackoff", "is required"],
    );
    for raw in BAD_DURATIONS {
        assert_invalid(
            &retry_policy_with("maxBackoff", raw),
            &["methodConfig[0].retryPolicy.maxBackoff"],
        );
    }
}

#[test]
fn fl07_vec15_multiplier_validated() {
    assert_invalid(
        &retry_policy_without("backoffMultiplier"),
        &[
            "methodConfig[0].retryPolicy.backoffMultiplier",
            "is required",
        ],
    );
    for raw in ["0", "-1.5", "\"2\""] {
        assert_invalid(
            &retry_policy_with("backoffMultiplier", raw),
            &["methodConfig[0].retryPolicy.backoffMultiplier"],
        );
    }
    // JSON has no NaN/Infinity literals; they arrive via parse_value overlays
    // (serde_json cannot even represent them: they decode as null).
    for raw in [f64::NAN, f64::INFINITY] {
        let mut value = serde_json::from_str::<serde_json::Value>(&retry_doc(F0)).expect("doc");
        value["methodConfig"][0]["retryPolicy"]["backoffMultiplier"] = serde_json::Value::from(raw);
        let err = ServiceConfig::parse_value(&value).expect_err("non-finite rejected");
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(
            err.message()
                .contains("methodConfig[0].retryPolicy.backoffMultiplier"),
            "message {:?} names the field",
            err.message()
        );
    }
}

#[test]
fn fl07_vec16_codes_shape_validated() {
    assert_invalid(
        &retry_policy_without("retryableStatusCodes"),
        &[
            "methodConfig[0].retryPolicy.retryableStatusCodes",
            "is required",
        ],
    );
    assert_invalid(
        &retry_policy_with("retryableStatusCodes", "[]"),
        &["retryableStatusCodes", "must not be empty"],
    );
    assert_invalid(
        &retry_policy_with("retryableStatusCodes", "\"UNAVAILABLE\""),
        &["retryableStatusCodes", "must be an array"],
    );
}

#[test]
fn fl07_vec17_code_names_validated() {
    // Unknown names are validation errors, not skips.
    assert_invalid(
        &retry_policy_with("retryableStatusCodes", r#"["UNAVAILABLE", "BOGUS"]"#),
        &[
            "methodConfig[0].retryPolicy.retryableStatusCodes[1]",
            "BOGUS",
        ],
    );
    // Numeric spellings follow Code::from_str.
    let config = ServiceConfig::parse(&retry_policy_with("retryableStatusCodes", r#"["14"]"#))
        .expect("numeric UNAVAILABLE");
    let codes = &global_policy(&config).retryable_status_codes;
    assert_eq!(codes.len(), 1);
    assert!(codes.contains(&Code::Unavailable));
    for raw in [r#"["17"]"#, r#"["-1"]"#] {
        assert_invalid(
            &retry_policy_with("retryableStatusCodes", raw),
            &["retryableStatusCodes"],
        );
    }
}

#[test]
fn fl07_vec18_per_attempt_timeout() {
    let config = ServiceConfig::parse(&retry_policy_with("perAttemptRecvTimeout", "\"0.5s\""))
        .expect("valid per-attempt timeout");
    assert_eq!(
        global_policy(&config).per_attempt_recv_timeout,
        Some(Duration::from_millis(500))
    );
    for raw in ["\"500ms\"", "\"-2s\""] {
        assert_invalid(
            &retry_policy_with("perAttemptRecvTimeout", raw),
            &["methodConfig[0].retryPolicy.perAttemptRecvTimeout"],
        );
    }
    // Absent stays None.
    let config = ServiceConfig::parse(&retry_doc(F0)).expect("f0");
    assert_eq!(global_policy(&config).per_attempt_recv_timeout, None);
}

#[test]
fn fl07_vec19_throttling_valid() {
    let config =
        ServiceConfig::parse(r#"{"retryThrottling": {"maxTokens": 10, "tokenRatio": 0.5}}"#)
            .expect("valid throttling");
    let throttle = config.retry_throttling().expect("throttling");
    assert_eq!(throttle.max_tokens, 10.0);
    assert_eq!(throttle.token_ratio, 0.5);
}

#[test]
fn fl07_vec20_throttling_validated() {
    for doc in [
        r#"{"retryThrottling": []}"#,
        r#"{"retryThrottling": {"tokenRatio": 0.5}}"#,
        r#"{"retryThrottling": {"maxTokens": 10}}"#,
        r#"{"retryThrottling": {"maxTokens": 0, "tokenRatio": 0.5}}"#,
        r#"{"retryThrottling": {"maxTokens": 10, "tokenRatio": -1}}"#,
        r#"{"retryThrottling": {"maxTokens": "lots", "tokenRatio": 0.5}}"#,
    ] {
        assert_invalid(doc, &["retryThrottling"]);
    }
    // Non-finite via a parse_value overlay.
    let mut value = serde_json::from_str::<serde_json::Value>(
        r#"{"retryThrottling": {"maxTokens": 10, "tokenRatio": 0.5}}"#,
    )
    .expect("doc");
    value["retryThrottling"]["tokenRatio"] = serde_json::Value::from(f64::INFINITY);
    let err = ServiceConfig::parse_value(&value).expect_err("non-finite rejected");
    assert_eq!(err.code(), Code::InvalidArgument);
    assert!(
        err.message().contains("retryThrottling.tokenRatio"),
        "message {:?} names the field",
        err.message()
    );
}

#[test]
fn fl07_vec21_unknown_lb_policy_skips() {
    let doc = format!(
        r#"{{"loadBalancingConfig": [{{"no_such_policy": {{"n": 1}}}}], "methodConfig": [{{"name": [{{}}], "retryPolicy": {F0}}}]}}"#
    );
    let config = ServiceConfig::parse(&doc).expect("unknown LB never invalidates retry parse");
    assert_eq!(config.lb_policies().len(), 1);
    assert!(matches!(
        config.lb_policies()[0],
        LbPolicyConfig::Unknown(_)
    ));
    assert_eq!(global_policy(&config).max_attempts, 3);
}

#[test]
fn fl07_vec22_precedence_by_specificity() {
    let exact = r#"{"name": [{"service": "s", "method": "m"}], "timeout": "2s"}"#;
    let service = r#"{"name": [{"service": "s"}], "timeout": "1s"}"#;
    let global = r#"{"name": [{}], "timeout": "3s"}"#;
    // Specificity wins in every document order.
    for order in [
        [exact, service, global],
        [exact, global, service],
        [service, exact, global],
        [service, global, exact],
        [global, exact, service],
        [global, service, exact],
    ] {
        let doc = format!(
            r#"{{"methodConfig": [{}, {}, {}]}}"#,
            order[0], order[1], order[2]
        );
        let config = ServiceConfig::parse(&doc).expect("precedence doc parses");
        let timeout = |s: &str, m: &str| config.method_config(s, m).expect("covered").timeout;
        assert_eq!(timeout("s", "m"), Some(Duration::from_secs(2)));
        assert_eq!(timeout("s", "other"), Some(Duration::from_secs(1)));
        assert_eq!(timeout("t", "u"), Some(Duration::from_secs(3)));
    }
    // Absent global: uncovered calls resolve to None.
    let config = ServiceConfig::parse(&format!(r#"{{"methodConfig": [{exact}, {service}]}}"#))
        .expect("parses");
    assert!(config.method_config("t", "u").is_none());
}

#[test]
fn fl07_vec23_empty_doc_resolves_nothing() {
    for doc in ["{}", PLAIN_DOC] {
        let config = ServiceConfig::parse(doc).expect("empty doc parses");
        assert!(config.is_empty());
        assert!(config.lb_policies().is_empty());
        assert!(config.retry_throttling().is_none());
        assert!(config.method_config("s", "m").is_none());
    }
}

#[tokio::test]
async fn fl07_vec23_channel_without_config_has_no_policy() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&attempts);
    let (addr, _guard) = common::serve(Flaky { attempts: worker }, ServerConfig::new())
        .await
        .expect("serve");
    let channel = Channel::connect(addr).await.expect("connect");
    assert!(channel.service_config_doc().is_none());
    let client = GreeterClient::new(channel);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("flaky fails");
    // No policy, no policy retry: exactly one execution.
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn lb_selection_skips_unregistered_and_typed() {
    let config = ServiceConfig::parse(
        r#"{"loadBalancingConfig": [{"pick_first": {"shuffleAddressList": true}}]}"#,
    )
    .expect("parses");
    // pick_first is parsed (typed) but registers in CH-04: no selection yet.
    if pbrs_grpc::lb::is_policy_registered("pick_first") {
        let selected = select_lb_policy(&config).expect("selected");
        assert_eq!(selected.name, "pick_first");
    } else {
        assert!(select_lb_policy(&config).is_none());
    }
    let bare = ServiceConfig::parse(r#"{"methodConfig": []}"#).expect("parses");
    assert!(select_lb_policy(&bare).is_none());
}
