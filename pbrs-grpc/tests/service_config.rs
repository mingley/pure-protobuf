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
use pbrs_grpc::{Channel, Code, Request, Response, ServiceConfig, Status};
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
