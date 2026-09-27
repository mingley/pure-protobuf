//! ORCA load reports and weighted_round_robin end to end (A51/A58/A114, CH-06).
//!
//! Backends record utilization into ORCA recorders; per-call trailers and
//! OOB streams carry reports to a `weighted_round_robin` channel whose
//! picks converge to the reported weight ratio. A golden vector from the
//! pinned grpc-go peer (`tests/interop/go/orcagolden`) proves wire
//! compatibility of the report codec in the decode direction; the reverse
//! direction is checked by hand in CH-06 evidence.

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

use common::{ServerGuard, reply, req};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::orca::{OrcaRecorder, OrcaResponseHook, OrcaService};
use pbrs_grpc::resolver::{DnsConfig, DnsLookup, ResolverConfig, TxtLookup};
use pbrs_grpc::{Channel, Request, Response, Router, Status};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

/// Golden bytes marshaled by the pinned grpc-go peer
/// (`go run ./orcagolden -mode=encode` in tests/interop/go):
/// cpu=0.5, mem=0.25, request_cost{bytes:3487}, utilization{gpu:0.3},
/// rps=100.5, eps=2, named{queue:0.9}, app=0.8.
const GO_GOLDEN_HEX: &str = "09000000000000e03f11000000000000d03f22100a0562797465731100000000003eab402a0e0a0367707511333333333333d33f31000000000020594039000000000000004042100a05717565756511cdccccccccccec3f499a9999999999e93f";

fn hex_decode(hex: &str) -> Vec<u8> {
    let digits: Vec<u8> = hex.bytes().collect();
    digits
        .chunks(2)
        .map(|pair| {
            let hi = u8::try_from((pair[0] as char).to_digit(16).expect("hex")).expect("hex");
            let lo = u8::try_from((pair[1] as char).to_digit(16).expect("hex")).expect("hex");
            hi << 4 | lo
        })
        .collect()
}

#[test]
fn pinned_go_report_decodes() {
    let bytes = hex_decode(GO_GOLDEN_HEX);
    let report = pbrs_grpc::orca::decode_trailer(&bytes).expect("go golden decodes");
    assert_eq!(report.cpu_utilization(), 0.5);
    assert_eq!(report.mem_utilization(), 0.25);
    assert_eq!(report.rps_fractional(), 100.5);
    assert_eq!(report.eps(), 2.0);
    assert_eq!(report.application_utilization(), 0.8);
    assert_eq!(report.request_cost().get("bytes"), Some(3487.0));
    assert_eq!(report.utilization().get("gpu"), Some(0.3));
    assert_eq!(report.named_metrics().get("queue"), Some(0.9));
    // Our encoder emits byte-identical output for the same values
    // (single-entry maps encode deterministically on both sides),
    // and the pinned peer decodes our bytes back to these fields
    // (`go run ./orcagolden -mode=decode`, CH-06 evidence).
    let ours = pbrs_grpc::orca::encode_trailer(&report).expect("encode");
    assert_eq!(ours, bytes);
}

struct Tagged {
    tag: &'static str,
    unaries: Arc<AtomicUsize>,
}

impl Greeter for Tagged {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.unaries.fetch_add(1, Ordering::SeqCst);
        Ok(Response::new(reply(format!(
            "{}:{}",
            self.tag,
            request.get_ref().name()
        ))))
    }
}

/// A backend serving Greeter with per-call ORCA trailers from `recorder`.
async fn serve_trailer(
    tag: &'static str,
    recorder: OrcaRecorder,
) -> (SocketAddr, Arc<AtomicUsize>, ServerGuard) {
    let unaries = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Tagged {
                tag,
                unaries: worker,
            }))
            .on_response(OrcaResponseHook::new(recorder))
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, unaries, ServerGuard(handle))
}

/// A backend serving Greeter plus the OOB service, without per-call trailers.
async fn serve_oob(
    tag: &'static str,
    recorder: OrcaRecorder,
    min_interval: Duration,
) -> (SocketAddr, Arc<AtomicUsize>, ServerGuard) {
    let unaries = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let orca = pbrs_grpc::orca::OpenRcaServiceServer::new(OrcaService::with_min_interval(
        recorder,
        min_interval,
    ));
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Tagged {
                tag,
                unaries: worker,
            }))
            .add_service(orca)
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, unaries, ServerGuard(handle))
}

/// A backend serving Greeter with no ORCA at all.
async fn serve_plain(tag: &'static str) -> (SocketAddr, Arc<AtomicUsize>, ServerGuard) {
    let unaries = Arc::new(AtomicUsize::new(0));
    let worker = Arc::clone(&unaries);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Tagged {
                tag,
                unaries: worker,
            }))
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, unaries, ServerGuard(handle))
}

/// One fixed DNS answer, repeated on every refresh.
struct FixedAddrs(Vec<SocketAddr>);

impl DnsLookup for FixedAddrs {
    fn lookup(
        &self,
        _host: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, io::Error>> + Send + '_>> {
        let addrs = self.0.clone();
        Box::pin(async move { Ok(addrs) })
    }
}

/// One TXT document, served on every fetch.
struct FixedTxt(&'static str);

impl TxtLookup for FixedTxt {
    fn fetch_txt(
        &self,
        _name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, io::Error>> + Send + '_>> {
        let doc = self.0.to_owned();
        Box::pin(async move { Ok(vec![doc]) })
    }
}

fn fast_bounds() -> DnsConfig {
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

async fn wrr_channel(addrs: Vec<SocketAddr>, doc: &'static str, host: &str) -> Channel {
    let dns = Arc::new(FixedAddrs(addrs));
    let txt = Arc::new(FixedTxt(doc));
    let config = ResolverConfig::with_dns_provider(fast_bounds(), dns).with_txt_provider(txt);
    Channel::connect_uri(&format!("dns:///{host}:443"), config)
        .await
        .expect("channel")
}

const WRR_PER_CALL_DOC: &str = r#"{"loadBalancingConfig": [{"weighted_round_robin": {
    "blackoutPeriod": "0s", "weightUpdatePeriod": "0.1s"}}]}"#;
const WRR_OOB_DOC: &str = r#"{"loadBalancingConfig": [{"weighted_round_robin": {
    "blackoutPeriod": "0s", "weightUpdatePeriod": "0.1s",
    "enableOobLoadReport": true, "oobReportingPeriod": "0.1s"}}]}"#;

async fn unary_tag(client: &GreeterClient) -> String {
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary")
        .into_inner()
        .message()
        .to_string()
}

fn recorder(app_utilization: f64, qps: f64) -> OrcaRecorder {
    let r = OrcaRecorder::new();
    r.set_application_utilization(app_utilization);
    r.set_rps_fractional(qps);
    r
}

#[tokio::test]
async fn per_call_reports_converge_to_weight_ratio() {
    // Equal QPS, 9x utilization gap: B weighs 9x A.
    let (addr_a, _ua, _ga) = serve_trailer("A", recorder(0.9, 100.0)).await;
    let (addr_b, _ub, _gb) = serve_trailer("B", recorder(0.1, 100.0)).await;
    let channel = wrr_channel(vec![addr_a, addr_b], WRR_PER_CALL_DOC, "wrr.invalid").await;
    let client = GreeterClient::new(channel);
    let mut hits_b = 0u32;
    let total = 200u32;
    for _ in 0..total {
        if unary_tag(&client).await.starts_with("B:") {
            hits_b += 1;
        }
    }
    // Expect ~90% to B; the first two RPCs run unweighted (one per
    // backend to collect reports) and EDF start jitter adds noise.
    assert!(
        (140..=195).contains(&hits_b),
        "B share out of band: {hits_b}/{total}"
    );
}

#[tokio::test]
async fn oob_stream_drives_weights_without_trailers() {
    let (addr_a, _ua, _ga) = serve_oob("A", recorder(0.9, 100.0), Duration::from_millis(50)).await;
    let (addr_b, _ub, _gb) = serve_oob("B", recorder(0.1, 100.0), Duration::from_millis(50)).await;
    let channel = wrr_channel(vec![addr_a, addr_b], WRR_OOB_DOC, "oob.invalid").await;
    let client = GreeterClient::new(channel);
    // The pump's immediate snapshot weights both backends within a few
    // RPCs; poll windows until B dominates or time out.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "OOB weights never converged"
        );
        let mut hits_b = 0u32;
        for _ in 0..40 {
            if unary_tag(&client).await.starts_with("B:") {
                hits_b += 1;
            }
        }
        if hits_b >= 28 {
            return;
        }
    }
}

#[tokio::test]
async fn oob_unimplemented_backends_stay_even() {
    let (addr_a, _ua, _ga) = serve_plain("A").await;
    let (addr_b, _ub, _gb) = serve_plain("B").await;
    let channel = wrr_channel(vec![addr_a, addr_b], WRR_OOB_DOC, "plain.invalid").await;
    let client = GreeterClient::new(channel);
    // No OOB service: UNIMPLEMENTED stops the pumps silently and picks
    // stay round-robin; every RPC still succeeds.
    let mut hits = [0u32; 2];
    for _ in 0..20 {
        let tag = unary_tag(&client).await;
        if tag.starts_with("A:") {
            hits[0] += 1;
        } else if tag.starts_with("B:") {
            hits[1] += 1;
        } else {
            panic!("unexpected tag {tag}");
        }
    }
    assert_eq!(hits, [10, 10]);
}

#[tokio::test]
async fn trailer_carries_decodable_report() {
    let rec = OrcaRecorder::new();
    rec.set_cpu_utilization(0.5);
    rec.set_named_metric("queue", 0.9);
    let (addr, _u, _g) = serve_trailer("A", rec).await;
    let channel = wrr_channel(vec![addr], WRR_PER_CALL_DOC, "one.invalid").await;
    let client = GreeterClient::new(channel);
    let response = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    let raw = response
        .trailers()
        .get_bin(pbrs_grpc::orca::TRAILER)
        .expect("orca trailer");
    let report = pbrs_grpc::orca::decode_trailer(&raw).expect("decode");
    assert_eq!(report.cpu_utilization(), 0.5);
    assert_eq!(report.named_metrics().get("queue"), Some(0.9));
}
