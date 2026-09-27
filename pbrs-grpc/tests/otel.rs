//! OpenTelemetry metrics bridge tests (GF-01): instrument names
//! match the gRFCs (A66/A94), counts and durations land with the
//! specified labels, and static custom attributes ride per-call
//! instruments (A108, channel level).

#![cfg(feature = "otel")]
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

use common::{ServerGuard, name_of, req};
use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{
    InMemoryMetricExporter, InMemoryMetricExporterBuilder, PeriodicReader, SdkMeterProvider,
};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::otel::{Metrics, instrument, label};
use pbrs_grpc::{Request, Response, Router, Status, Streaming};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

/// Echoes names, except "boom" which fails loudly (for status labels).
struct FailBoom;

fn reply(text: &str) -> HelloReply {
    let mut r = HelloReply::new();
    r.set_message(text);
    r
}

impl Greeter for FailBoom {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request.get_ref().name().to_str().unwrap_or("").to_owned();
        if name == "boom" {
            return Err(Status::invalid_argument("boom"));
        }
        Ok(Response::new(reply(&name)))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut inbound = request.into_inner();
        let mut last = String::new();
        while let Some(msg) = inbound.message().await? {
            last = msg.name().to_str().unwrap_or("").to_owned();
        }
        Ok(Response::new(reply(&last)))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = request.get_ref().name().to_str().unwrap_or("").to_owned();
        let (tx, stream) = Streaming::channel(8);
        drop(tokio::spawn(async move {
            for part in name.split(',') {
                if tx.send(reply(part)).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(8);
        drop(tokio::spawn(async move {
            while let Ok(Some(msg)) = inbound.message().await {
                let name = msg.name().to_str().unwrap_or("").to_owned();
                if tx.send(reply(&name)).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

fn provider() -> (SdkMeterProvider, InMemoryMetricExporter) {
    let exporter = InMemoryMetricExporterBuilder::new().build();
    let reader = PeriodicReader::builder(exporter.clone()).build();
    let provider = SdkMeterProvider::builder().with_reader(reader).build();
    (provider, exporter)
}

/// Collected metrics: name -> (sum | count, attribute sets).
#[derive(Default)]
struct Snapshot {
    sums: HashMap<String, f64>,
    counts: HashMap<String, u64>,
    attrs: HashMap<String, Vec<Vec<(String, String)>>>,
}

fn snapshot(provider: &SdkMeterProvider, exporter: &InMemoryMetricExporter) -> Snapshot {
    provider.force_flush().expect("flush");
    let mut out = Snapshot::default();
    for rm in exporter.get_finished_metrics().expect("metrics") {
        for scope in rm.scope_metrics() {
            for metric in scope.metrics() {
                let name = metric.name().to_owned();
                match metric.data() {
                    AggregatedMetrics::U64(MetricData::Sum(sum)) => {
                        let mut total = 0u64;
                        for point in sum.data_points() {
                            total += point.value();
                            out.attrs.entry(name.clone()).or_default().push(
                                point
                                    .attributes()
                                    .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                                    .collect(),
                            );
                        }
                        out.sums.insert(name.clone(), total as f64);
                        out.counts.insert(name, total);
                    }
                    AggregatedMetrics::F64(MetricData::Histogram(hist)) => {
                        let mut count = 0u64;
                        let mut sum = 0.0;
                        for point in hist.data_points() {
                            count += point.count();
                            sum += point.sum();
                            out.attrs.entry(name.clone()).or_default().push(
                                point
                                    .attributes()
                                    .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                                    .collect(),
                            );
                        }
                        out.sums.insert(name.clone(), sum);
                        out.counts.insert(name, count);
                    }
                    other => panic!("unexpected aggregation for {name}: {other:?}"),
                }
            }
        }
    }
    out
}

fn attr_sets<'a>(snap: &'a Snapshot, name: &str) -> &'a [Vec<(String, String)>] {
    snap.attrs.get(name).map(Vec::as_slice).unwrap_or(&[])
}

fn has_attr(set: &[(String, String)], key: &str, value: &str) -> bool {
    set.iter().any(|(k, v)| k == key && v == value)
}

async fn serve(metrics: Metrics) -> (SocketAddr, ServerGuard) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    serve_on(listener, metrics).await
}

async fn serve_on(listener: TcpListener, metrics: Metrics) -> (SocketAddr, ServerGuard) {
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(FailBoom))
            .observer(metrics)
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

#[tokio::test]
async fn otel_client_and_server_instruments() {
    let (provider, exporter) = provider();
    let meter = provider.meter("test");
    let metrics = Metrics::with_meter(meter);
    let (addr, _guard) = serve(metrics.clone()).await;

    let channel = pbrs_grpc::Channel::connect(addr).await.expect("connect");
    let client = GreeterClient::new(channel.observer(metrics.clone()));
    for name in ["ada", "bob"] {
        let resp = client
            .say_hello(Request::new(req(name)))
            .await
            .expect("unary");
        assert_eq!(name_of(&resp.into_inner()), name);
    }
    // A failing call exercises the status label.
    let err = client
        .say_hello(Request::new(req("boom")))
        .await
        .expect_err("boom fails");
    assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);

    let snap = snapshot(&provider, &exporter);

    // Client: 3 attempts started, 3 attempt + call durations.
    assert_eq!(
        snap.counts.get(instrument::CLIENT_ATTEMPT_STARTED),
        Some(&3),
        "attempt.started counts every attempt"
    );
    assert_eq!(
        snap.counts.get(instrument::CLIENT_ATTEMPT_DURATION),
        Some(&3)
    );
    assert_eq!(snap.counts.get(instrument::CLIENT_CALL_DURATION), Some(&3));
    // Labels: full method path, target, terminal status.
    for set in attr_sets(&snap, instrument::CLIENT_CALL_DURATION) {
        assert!(has_attr(set, label::METHOD, "helloworld.Greeter/SayHello"));
        assert!(has_attr(set, label::TARGET, &addr.to_string()));
    }
    let statuses: Vec<&str> = attr_sets(&snap, instrument::CLIENT_CALL_DURATION)
        .iter()
        .filter_map(|set| {
            set.iter()
                .find(|(k, _)| k == label::STATUS)
                .map(|(_, v)| v.as_str())
        })
        .collect();
    assert!(statuses.contains(&"OK"), "ok status recorded");
    assert!(
        statuses.contains(&"INVALID_ARGUMENT"),
        "error status recorded"
    );

    // Server: 3 calls started with durations; no target label (A66).
    assert_eq!(snap.counts.get(instrument::SERVER_CALL_STARTED), Some(&3));
    assert_eq!(snap.counts.get(instrument::SERVER_CALL_DURATION), Some(&3));
    for set in attr_sets(&snap, instrument::SERVER_CALL_DURATION) {
        assert!(has_attr(set, label::METHOD, "helloworld.Greeter/SayHello"));
        assert!(
            !set.iter().any(|(k, _)| k == label::TARGET),
            "server instruments carry no target"
        );
    }

    // Durations are positive.
    assert!(
        snap.sums
            .get(instrument::CLIENT_CALL_DURATION)
            .unwrap_or(&0.0)
            > &0.0
    );
    assert!(
        snap.sums
            .get(instrument::SERVER_CALL_DURATION)
            .unwrap_or(&0.0)
            > &0.0
    );
}

#[tokio::test]
async fn otel_subchannel_connection_counters() {
    let (provider, exporter) = provider();
    let metrics = Metrics::with_meter(provider.meter("test"));

    // Reserve a port, then bounce the server on it: reconnect outcomes
    // (retry_generation > 0) reach the observer via on_reconnect, while
    // initial dials are invisible — see the GF-01b note in otel/mod.rs.
    let probe = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = probe.local_addr().expect("local_addr");
    drop(probe);

    let listener = TcpListener::bind(addr).await.expect("bind addr");
    let (_, mut guard) = serve_on(listener, metrics.clone()).await;
    let channel = pbrs_grpc::Channel::connect_lazy(addr).expect("lazy");
    let client = GreeterClient::new(channel.observer(metrics.clone()));
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");

    // Kill the server and wait for teardown, so the next RPCs
    // redial a dead port instead of racing the old listener. The
    // first failures may ride the stale handle; keep calling until
    // the pool actually redials (or give up after ~4s).
    guard.0.abort();
    let _ = (&mut guard.0).await;
    let mut saw_failed = false;
    for i in 0..200 {
        let _ = client.say_hello(Request::new(req("ada"))).await;
        if i % 10 == 0 {
            let snap = snapshot(&provider, &exporter);
            if snap
                .counts
                .get(instrument::SUBCHANNEL_CONN_FAILED)
                .is_some_and(|n| *n >= 1)
            {
                saw_failed = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(saw_failed, "pool redialed the dead port");
    let _ = client.say_hello(Request::new(req("ada"))).await;

    // Revive the server on the same port: RPCs redial successfully.
    let listener = TcpListener::bind(addr).await.expect("rebind addr");
    let (_, _guard) = serve_on(listener, metrics.clone()).await;
    // The pool may need a redial cycle before the revived port answers.
    for _ in 0..20 {
        if client.say_hello(Request::new(req("ada"))).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary after revive");

    let snap = snapshot(&provider, &exporter);
    assert!(
        snap.counts
            .get(instrument::SUBCHANNEL_CONN_FAILED)
            .is_some_and(|n| *n >= 1),
        "failed redial counted, got {:?}",
        snap.counts.get(instrument::SUBCHANNEL_CONN_FAILED)
    );
    assert!(
        snap.counts
            .get(instrument::SUBCHANNEL_CONN_SUCCEEDED)
            .is_some_and(|n| *n >= 1),
        "successful redial counted, got {:?}",
        snap.counts.get(instrument::SUBCHANNEL_CONN_SUCCEEDED)
    );
    for set in attr_sets(&snap, instrument::SUBCHANNEL_CONN_SUCCEEDED) {
        assert!(has_attr(set, label::TARGET, &addr.to_string()));
    }
}

#[tokio::test]
async fn otel_static_custom_attributes() {
    let (provider, exporter) = provider();
    let metrics = Metrics::with_meter(provider.meter("test")).with_custom_attributes(vec![
        KeyValue::new("team", "grpc"),
        KeyValue::new("cell", "canary"),
    ]);
    let (addr, _guard) = serve(metrics.clone()).await;

    let channel = pbrs_grpc::Channel::connect(addr).await.expect("connect");
    let client = GreeterClient::new(channel.observer(metrics));
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");

    let snap = snapshot(&provider, &exporter);
    // Custom attributes ride per-call instruments (A108, static subset).
    for name in [
        instrument::CLIENT_ATTEMPT_STARTED,
        instrument::CLIENT_CALL_DURATION,
        instrument::SERVER_CALL_DURATION,
    ] {
        for set in attr_sets(&snap, name) {
            assert!(has_attr(set, "team", "grpc"), "{name} carries team");
            assert!(has_attr(set, "cell", "canary"), "{name} carries cell");
        }
    }
    // ... but not subchannel instruments (A108 scopes to per-call).
    for set in attr_sets(&snap, instrument::SUBCHANNEL_CONN_SUCCEEDED) {
        assert!(
            !set.iter().any(|(k, _)| k == "team"),
            "subchannel instruments carry no custom labels"
        );
    }
}
