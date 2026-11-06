//! URI identity and connection-driver placement through real HTTP/2 peers.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "bounded integration assertions and intentionally discarding executors"
)]

use bytes::Bytes;
use pbrs_grpc::{
    Channel, ChannelConfig, Code, Endpoint, Executor, ExecutorFuture, GreeterClient, HelloReply,
    HelloRequest, Request,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};

const BOUND: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Counts {
    scheduled: AtomicUsize,
    polled: AtomicUsize,
    finished: AtomicUsize,
}

struct Finished(Arc<Counts>);
impl Drop for Finished {
    fn drop(&mut self) {
        self.0.finished.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Default)]
struct Recording(Arc<Counts>);
impl Executor for Recording {
    fn execute(&self, future: ExecutorFuture) {
        self.0.scheduled.fetch_add(1, Ordering::SeqCst);
        let counts = Arc::clone(&self.0);
        drop(tokio::spawn(async move {
            let _finished = Finished(Arc::clone(&counts));
            counts.polled.fetch_add(1, Ordering::SeqCst);
            future.await;
        }));
    }
}

#[derive(Clone, Default)]
struct Discard(Arc<AtomicUsize>);
impl Executor for Discard {
    fn execute(&self, future: ExecutorFuture) {
        self.0.fetch_add(1, Ordering::SeqCst);
        drop(future);
    }
}

async fn count(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(BOUND, async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

struct Peer {
    addr: SocketAddr,
    accepted: Arc<AtomicUsize>,
    health: Arc<AtomicUsize>,
    fail_next_health: Arc<AtomicBool>,
    require_health_authority: Arc<AtomicBool>,
    refuse_next_rpc: Arc<AtomicBool>,
    fail_next_rpc: Arc<AtomicBool>,
    hold_next_rpc: Arc<AtomicBool>,
    authorities: Arc<tokio::sync::Mutex<Vec<String>>>,
    close: watch::Sender<usize>,
    release: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Peer {
    async fn start(name: &'static str, hold_server_streams: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let health = Arc::new(AtomicUsize::new(0));
        let fail_next_health = Arc::new(AtomicBool::new(false));
        let require_health_authority = Arc::new(AtomicBool::new(false));
        let refuse_next_rpc = Arc::new(AtomicBool::new(false));
        let fail_next_rpc = Arc::new(AtomicBool::new(false));
        let hold_next_rpc = Arc::new(AtomicBool::new(false));
        let authorities = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let (close, closed) = watch::channel(0);
        let (release, released) = watch::channel(false);
        let connections = Arc::clone(&accepted);
        let health_calls = Arc::clone(&health);
        let observed = Arc::clone(&authorities);
        let failed_health = Arc::clone(&fail_next_health);
        let required_health = Arc::clone(&require_health_authority);
        let refused_rpc = Arc::clone(&refuse_next_rpc);
        let failed_rpc = Arc::clone(&fail_next_rpc);
        let held_rpc = Arc::clone(&hold_next_rpc);
        let expected_authority = addr.to_string();
        let task = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (io, _) = accepted.unwrap();
                        let number = connections.fetch_add(1, Ordering::SeqCst) + 1;
                        let tag = format!("{name}:{number}");
                        let mut closed = closed.clone();
                        closed.borrow_and_update();
                        let released = released.clone();
                        let health = Arc::clone(&health_calls);
                        let observed = Arc::clone(&observed);
                        let failed_health = Arc::clone(&failed_health);
                        let required_health = Arc::clone(&required_health);
                        let refused_rpc = Arc::clone(&refused_rpc);
                        let failed_rpc = Arc::clone(&failed_rpc);
                        let held_rpc = Arc::clone(&held_rpc);
                        let expected_authority = expected_authority.clone();
                        tasks.spawn(async move {
                            let mut connection = match h2::server::handshake(io).await {
                                Ok(connection) => connection,
                                Err(_) => return,
                            };
                            let mut handlers = JoinSet::new();
                            loop {
                                tokio::select! {
                                    _ = closed.changed() => return,
                                    incoming = connection.accept() => {
                                        let Some(Ok((request, mut respond))) = incoming else { return; };
                                        let tag = tag.clone();
                                        let mut released = released.clone();
                                        let health = Arc::clone(&health);
                                        let observed = Arc::clone(&observed);
                                        let failed_health = Arc::clone(&failed_health);
                                        let required_health = Arc::clone(&required_health);
                                        let refused_rpc = Arc::clone(&refused_rpc);
                                        let failed_rpc = Arc::clone(&failed_rpc);
                                        let held_rpc = Arc::clone(&held_rpc);
                                        let expected_authority = expected_authority.clone();
                                        handlers.spawn(async move {
                                            let path = request.uri().path().to_owned();
                                            assert_eq!(request.uri().scheme_str(), Some("http"));
                                            let authority = request.uri().authority().unwrap().as_str().to_owned();
                                            observed.lock().await.push(authority.clone());
                                            assert!(!path.contains('?'), "origin path/query is not a method path");
                                            let mut body = request.into_body();
                                            while let Some(data) = body.data().await {
                                                let Ok(data) = data else { return; };
                                                body.flow_control().release_capacity(data.len()).unwrap();
                                            }
                                            let is_health = path == "/grpc.health.v1.Health/Watch";
                                            if !is_health && refused_rpc.swap(false, Ordering::SeqCst) {
                                                respond.send_reset(h2::Reason::REFUSED_STREAM);
                                                return;
                                            }
                                            if (is_health && required_health.load(Ordering::SeqCst) && authority != expected_authority)
                                                || (!is_health && failed_rpc.swap(false, Ordering::SeqCst))
                                            {
                                                let response = http::Response::builder().status(200)
                                                    .header("content-type", "application/grpc")
                                                    .header("grpc-status", "14")
                                                    .header("grpc-retry-pushback-ms", "0")
                                                    .body(()).unwrap();
                                                respond.send_response(response, true).ok();
                                                return;
                                            }
                                            if !is_health && held_rpc.swap(false, Ordering::SeqCst) {
                                                while !*released.borrow() {
                                                    if released.changed().await.is_err() { return; }
                                                }
                                            }
                                            let response = http::Response::builder().status(200)
                                                .header("content-type", "application/grpc").body(()).unwrap();
                                            let Ok(mut output) = respond.send_response(response, false) else { return; };
                                            let payload = if is_health {
                                                health.fetch_add(1, Ordering::SeqCst);
                                                if failed_health.swap(false, Ordering::SeqCst) {
                                                    let mut trailers = http::HeaderMap::new();
                                                    trailers.insert("grpc-status", http::HeaderValue::from_static("14"));
                                                    output.send_trailers(trailers).ok();
                                                    return;
                                                }
                                                vec![8, 1]
                                            } else {
                                                let mut payload = vec![10, u8::try_from(tag.len()).unwrap()];
                                                payload.extend_from_slice(tag.as_bytes());
                                                payload
                                            };
                                            let mut frame = vec![0];
                                            frame.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_be_bytes());
                                            frame.extend_from_slice(&payload);
                                            if output.send_data(Bytes::from(frame), false).is_err() { return; }
                                            if is_health {
                                                // Health Watch remains open until its connection is stopped.
                                                std::future::pending::<()>().await;
                                            }
                                            if hold_server_streams && path.ends_with("/ServerHello") {
                                                while !*released.borrow() {
                                                    if released.changed().await.is_err() { return; }
                                                }
                                            }
                                            let mut trailers = http::HeaderMap::new();
                                            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                                            output.send_trailers(trailers).ok();
                                        });
                                    }
                                    finished = handlers.join_next(), if !handlers.is_empty() => {
                                        finished.unwrap().unwrap();
                                    }
                                }
                            }
                        });
                    }
                    finished = tasks.join_next(), if !tasks.is_empty() => {
                        finished.unwrap().unwrap();
                    }
                }
            }
        });
        Self {
            addr,
            accepted,
            health,
            fail_next_health,
            require_health_authority,
            refuse_next_rpc,
            fail_next_rpc,
            hold_next_rpc,
            authorities,
            close,
            release,
            task,
        }
    }

    fn endpoint(&self, alias: &str) -> Endpoint {
        Endpoint::from_shared(format!("http://{}/{alias}", self.addr)).unwrap()
    }

    fn disconnect(&self) {
        self.close.send_modify(|epoch| *epoch += 1);
    }

    async fn stop(self) {
        self.task.abort();
        if let Err(error) = self.task.await {
            assert!(error.is_cancelled(), "peer failed: {error}");
        }
    }
}

fn message() -> HelloRequest {
    HelloRequest::new()
}
fn tag(reply: &HelloReply) -> String {
    reply.message().to_str().unwrap().to_owned()
}

async fn unary(client: &GreeterClient) -> Result<String, pbrs_grpc::Status> {
    tokio::time::timeout(BOUND, client.say_hello(Request::new(message())))
        .await
        .unwrap()
        .map(|reply| tag(reply.get_ref()))
}

async fn all_shapes_twice(channel: Channel) -> Vec<String> {
    tokio::time::timeout(BOUND, async move {
        let client = GreeterClient::new(channel);
        let mut tags = Vec::new();
        for _ in 0..2 {
            tags.push(unary(&client).await.unwrap());
        }
        for _ in 0..2 {
            let mut stream = client
                .server_hello(Request::new(message()))
                .await
                .unwrap()
                .into_inner();
            tags.push(tag(&stream.message().await.unwrap().unwrap()));
            assert!(stream.message().await.unwrap().is_none());
        }
        for _ in 0..2 {
            let (sender, response) = client.client_hello(Request::new(()));
            sender.send(message()).await.unwrap();
            sender.close();
            tags.push(tag(response.await.unwrap().get_ref()));
        }
        for _ in 0..2 {
            let (sender, response) = client.stream_hello(Request::new(()));
            sender.send(message()).await.unwrap();
            sender.close();
            let mut stream = response.await.unwrap().into_inner();
            tags.push(tag(&stream.message().await.unwrap().unwrap()));
            assert!(stream.message().await.unwrap().is_none());
        }
        tags
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn different_peers_keep_each_executor_for_all_shapes_and_redials() {
    let a = Peer::start("A", false).await;
    let b = Peer::start("B", false).await;
    let ea = Recording::default();
    let eb = Recording::default();
    let channel = Endpoint::balance_list(
        [
            a.endpoint("a").executor(ea.clone()),
            b.endpoint("b").executor(eb.clone()),
        ],
        ChannelConfig::new(),
    )
    .await
    .unwrap();
    assert_eq!(ea.0.scheduled.load(Ordering::SeqCst), 0);
    assert_eq!(eb.0.scheduled.load(Ordering::SeqCst), 0);
    assert_eq!(
        all_shapes_twice(channel.clone()).await,
        ["A:1", "B:1"].repeat(4)
    );
    count(&ea.0.polled, 1).await;
    count(&eb.0.polled, 1).await;
    assert_eq!(ea.0.scheduled.load(Ordering::SeqCst), 1);
    assert_eq!(eb.0.scheduled.load(Ordering::SeqCst), 1);
    a.disconnect();
    b.disconnect();
    count(&ea.0.finished, 1).await;
    count(&eb.0.finished, 1).await;
    assert_eq!(
        all_shapes_twice(channel.clone()).await,
        ["A:2", "B:2"].repeat(4)
    );
    assert_eq!(ea.0.scheduled.load(Ordering::SeqCst), 2);
    assert_eq!(eb.0.scheduled.load(Ordering::SeqCst), 2);
    drop(channel);
    count(&ea.0.finished, 2).await;
    count(&eb.0.finished, 2).await;
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn same_socket_uri_aliases_keep_separate_default_and_custom_connections() {
    let peer = Peer::start("ALIAS", false).await;
    let executor = Recording::default();
    let channel = Endpoint::balance_list(
        [
            peer.endpoint("one").executor(executor.clone()),
            peer.endpoint("two"),
        ],
        ChannelConfig::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        all_shapes_twice(channel.clone()).await,
        ["ALIAS:1", "ALIAS:2"].repeat(4)
    );
    assert_eq!(peer.accepted.load(Ordering::SeqCst), 2);
    assert_eq!(executor.0.scheduled.load(Ordering::SeqCst), 1);
    assert!(
        peer.authorities
            .lock()
            .await
            .iter()
            .all(|authority| authority == &peer.addr.to_string())
    );
    drop(channel);
    count(&executor.0.finished, 1).await;
    peer.stop().await;
}

#[tokio::test]
async fn a_discarding_alias_cannot_be_routed_through_the_other_executor() {
    let peer = Peer::start("POISON", false).await;
    let discard = Discard::default();
    let executor = Recording::default();
    let channel = Endpoint::balance_list(
        [
            peer.endpoint("discard").executor(discard.clone()),
            peer.endpoint("run").executor(executor.clone()),
        ],
        ChannelConfig::new(),
    )
    .await
    .unwrap();
    let client = GreeterClient::new(channel.clone());
    // Replay may already move the first call from the discarded connection.
    let first = unary(&client).await;
    if let Err(error) = first {
        assert_eq!(error.code(), Code::Unavailable);
    }
    let tags = all_shapes_twice(channel.clone()).await;
    assert!(tags.iter().all(|tag| tag == "POISON:2"));
    assert!(discard.0.load(Ordering::SeqCst) >= 1);
    assert_eq!(executor.0.scheduled.load(Ordering::SeqCst), 1);
    drop(client);
    drop(channel);
    count(&executor.0.finished, 1).await;
    peer.stop().await;
}

#[tokio::test]
async fn duplicate_full_uri_uses_the_last_endpoint_metadata() {
    let peer = Peer::start("LAST", false).await;
    let discard = Discard::default();
    let executor = Recording::default();
    let channel = Endpoint::balance_list(
        [
            peer.endpoint("same").executor(discard.clone()),
            peer.endpoint("same").executor(executor.clone()),
        ],
        ChannelConfig::new(),
    )
    .await
    .unwrap();
    assert!(
        all_shapes_twice(channel.clone())
            .await
            .iter()
            .all(|tag| tag == "LAST:1")
    );
    assert_eq!(discard.0.load(Ordering::SeqCst), 0);
    assert_eq!(executor.0.scheduled.load(Ordering::SeqCst), 1);
    drop(channel);
    count(&executor.0.finished, 1).await;
    peer.stop().await;
}

#[tokio::test]
async fn same_uri_executor_update_drains_old_stream_without_migrating_it() {
    let peer = Peer::start("SWAP", true).await;
    let a = Recording::default();
    let b = Recording::default();
    let endpoint = peer.endpoint("same");
    let (publish, endpoints) = watch::channel(vec![endpoint.clone().executor(a.clone())]);
    let channel = Endpoint::balance_channel(
        endpoints,
        ChannelConfig::new().max_connection_age_grace(Duration::from_secs(3)),
    )
    .await
    .unwrap();
    let client = GreeterClient::new(channel.clone());
    let mut old = tokio::time::timeout(BOUND, client.server_hello(Request::new(message())))
        .await
        .unwrap()
        .unwrap()
        .into_inner();
    assert_eq!(tag(&old.message().await.unwrap().unwrap()), "SWAP:1");
    publish.send(vec![endpoint.executor(b.clone())]).unwrap();
    tokio::time::timeout(BOUND, async {
        loop {
            if unary(&client).await.unwrap() == "SWAP:2" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        a.0.finished.load(Ordering::SeqCst),
        0,
        "old stream still owns its driver"
    );
    assert_eq!(b.0.scheduled.load(Ordering::SeqCst), 1);
    for _ in 0..4 {
        assert_eq!(unary(&client).await.unwrap(), "SWAP:2");
    }
    peer.release.send(true).unwrap();
    assert!(
        tokio::time::timeout(BOUND, old.message())
            .await
            .unwrap()
            .unwrap()
            .is_none()
    );
    drop(old);
    assert!(
        peer.authorities
            .lock()
            .await
            .iter()
            .all(|authority| authority == &peer.addr.to_string())
    );
    count(&a.0.finished, 1).await;
    drop(publish);
    drop(client);
    drop(channel);
    count(&b.0.finished, 1).await;
    peer.stop().await;
}

#[tokio::test]
async fn cloned_snapshot_and_reorder_retain_each_uri_connection() {
    let peer = Peer::start("KEEP", false).await;
    let a = Recording::default();
    let b = Recording::default();
    let one = peer.endpoint("one").executor(a.clone());
    let two = peer.endpoint("two").executor(b.clone());
    let (publish, endpoints) = watch::channel(vec![one.clone(), two.clone()]);
    let channel = Endpoint::balance_channel(endpoints, ChannelConfig::new())
        .await
        .unwrap();
    assert_eq!(
        all_shapes_twice(channel.clone()).await,
        ["KEEP:1", "KEEP:2"].repeat(4)
    );
    publish.send(vec![one.clone(), two.clone()]).unwrap();
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    publish.send(vec![two, one]).unwrap();
    let client = GreeterClient::new(channel.clone());
    let mut seen = std::collections::HashSet::new();
    for _ in 0..8 {
        seen.insert(unary(&client).await.unwrap());
    }
    assert_eq!(
        seen,
        std::collections::HashSet::from(["KEEP:1".to_owned(), "KEEP:2".to_owned()])
    );
    assert_eq!(a.0.scheduled.load(Ordering::SeqCst), 1);
    assert_eq!(b.0.scheduled.load(Ordering::SeqCst), 1);
    assert_eq!(a.0.finished.load(Ordering::SeqCst), 0);
    assert_eq!(b.0.finished.load(Ordering::SeqCst), 0);
    drop(publish);
    drop(client);
    drop(channel);
    count(&a.0.finished, 1).await;
    count(&b.0.finished, 1).await;
    peer.stop().await;
}

#[tokio::test]
async fn health_watches_follow_uri_identity_for_aliases_of_one_socket() {
    let peer = Peer::start("HEALTH", false).await;
    let a = Recording::default();
    let b = Recording::default();
    let channel = Endpoint::balance_list(
        [
            peer.endpoint("one").executor(a.clone()),
            peer.endpoint("two").executor(b.clone()),
        ],
        ChannelConfig::new().health_checking(true),
    )
    .await
    .unwrap()
    .service_config(r#"{"healthCheckConfig":{"serviceName":"alias"}}"#)
    .unwrap();
    assert_eq!(
        all_shapes_twice(channel.clone()).await,
        ["HEALTH:1", "HEALTH:2"].repeat(4)
    );
    assert_eq!(peer.health.load(Ordering::SeqCst), 2);
    assert_eq!(a.0.scheduled.load(Ordering::SeqCst), 1);
    assert_eq!(b.0.scheduled.load(Ordering::SeqCst), 1);
    drop(channel);
    count(&a.0.finished, 1).await;
    count(&b.0.finished, 1).await;
    peer.stop().await;
}

struct OrdinaryDns(SocketAddr);
impl pbrs_grpc::resolver::DnsLookup for OrdinaryDns {
    fn lookup(
        &self,
        _: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<SocketAddr>, std::io::Error>> + Send + '_>,
    > {
        let address = self.0;
        Box::pin(async move { Ok(vec![address]) })
    }
}
impl pbrs_grpc::resolver::TxtLookup for OrdinaryDns {
    fn fetch_txt(
        &self,
        _: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<String>, std::io::Error>> + Send + '_>,
    > {
        Box::pin(async {
            Ok(vec![r#"{"loadBalancingConfig":[{"round_robin":{}}],"healthCheckConfig":{"serviceName":"ordinary"}}"#.to_owned()])
        })
    }
}

async fn ordinary_dns_channel(address: SocketAddr) -> Channel {
    use pbrs_grpc::resolver::{DnsConfig, ResolverConfig};
    let resolver = ResolverConfig::with_dns_provider(
        DnsConfig::new(
            Duration::from_secs(60),
            Duration::from_secs(60),
            Duration::from_secs(60),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::ZERO,
        )
        .unwrap(),
        Arc::new(OrdinaryDns(address)),
    )
    .with_txt_provider(Arc::new(OrdinaryDns(address)));
    Channel::connect_uri_with(
        "dns:///ordinary-watch.invalid:443",
        ChannelConfig::new().health_checking(true),
        resolver,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn ordinary_dns_round_robin_does_not_restart_watch_after_first_report() {
    let peer = Peer::start("DNS", false).await;
    let channel = ordinary_dns_channel(peer.addr).await;
    assert!(
        all_shapes_twice(channel.clone())
            .await
            .iter()
            .all(|tag| tag == "DNS:1")
    );
    assert_eq!(peer.health.load(Ordering::SeqCst), 1);
    drop(channel);
    peer.stop().await;
}

#[tokio::test]
async fn ordinary_dns_health_recovers_after_its_serving_connection_closes() {
    let peer = Peer::start("RECOVER", false).await;
    let channel = ordinary_dns_channel(peer.addr).await;
    let client = GreeterClient::new(channel.clone());
    assert_eq!(unary(&client).await.unwrap(), "RECOVER:1");
    peer.disconnect();
    // Let the Watch observe EOF before another RPC can win the redial race.
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        unary(&client)
            .await
            .expect("a dead Watch must permit a fresh connection"),
        "RECOVER:2"
    );
    assert_eq!(peer.health.load(Ordering::SeqCst), 2);
    drop(client);
    drop(channel);
    peer.stop().await;
}

#[tokio::test]
async fn retryable_application_health_status_keeps_the_same_live_connection() {
    let peer = Peer::start("APPHEALTH", false).await;
    peer.fail_next_health.store(true, Ordering::SeqCst);
    let executor = Recording::default();
    let channel = Endpoint::balance_list(
        [peer.endpoint("same").executor(executor.clone())],
        ChannelConfig::new()
            .health_checking(true)
            .wait_for_ready(true),
    )
    .await
    .unwrap()
    .service_config(r#"{"healthCheckConfig":{"serviceName":"application"}}"#)
    .unwrap();
    assert!(
        all_shapes_twice(channel.clone())
            .await
            .iter()
            .all(|tag| tag == "APPHEALTH:1")
    );
    assert_eq!(peer.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(peer.health.load(Ordering::SeqCst), 2);
    assert_eq!(executor.0.scheduled.load(Ordering::SeqCst), 1);
    drop(channel);
    count(&executor.0.finished, 1).await;
    peer.stop().await;
}

#[tokio::test]
async fn stream_keeps_its_executor_owner_after_last_balanced_channel_drops() {
    let peer = Peer::start("OWNER", true).await;
    let executor = Arc::new(Recording::default());
    let owner = Arc::downgrade(&executor);
    let counts = Arc::clone(&executor.0);
    let channel = Endpoint::balance_list(
        [peer.endpoint("same").executor(Arc::clone(&executor))],
        ChannelConfig::new(),
    )
    .await
    .unwrap();
    drop(executor);
    let client = GreeterClient::new(channel);
    let mut stream = tokio::time::timeout(BOUND, client.server_hello(Request::new(message())))
        .await
        .unwrap()
        .unwrap()
        .into_inner();
    assert_eq!(tag(&stream.message().await.unwrap().unwrap()), "OWNER:1");
    drop(client);
    assert!(owner.upgrade().is_some());
    assert_eq!(counts.finished.load(Ordering::SeqCst), 0);
    drop(stream);
    tokio::time::timeout(BOUND, async {
        while owner.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    count(&counts.finished, 1).await;
    peer.stop().await;
}

async fn channel_id_for_peer(peer: SocketAddr) -> i64 {
    use pbrs_grpc::channelz::{Channelz, ChannelzService, GetTopChannelsRequest};
    let response = Channelz::get_top_channels(
        &ChannelzService::shared_global(),
        Request::new(GetTopChannelsRequest::new()),
    )
    .await
    .unwrap();
    response
        .get_ref()
        .channel()
        .iter()
        .find(|channel| {
            channel
                .subchannel_ref()
                .iter()
                .any(|reference| reference.name().to_str().unwrap() == peer.to_string())
        })
        .expect("peer subchannel identifies this test's channel")
        .r#ref()
        .channel_id()
}

async fn trace_contains(channel: i64, needle: &str) -> bool {
    use pbrs_grpc::channelz::{Channelz, ChannelzService, GetChannelRequest};
    let mut request = GetChannelRequest::new();
    request.set_channel_id(channel);
    let response = Channelz::get_channel(&ChannelzService::shared_global(), Request::new(request))
        .await
        .unwrap();
    response
        .get_ref()
        .channel()
        .data()
        .trace()
        .events()
        .iter()
        .any(|event| event.description().to_str().unwrap().contains(needle))
}

#[tokio::test]
async fn invalid_full_update_is_atomic_and_empty_removal_can_repopulate() {
    let peer = Peer::start("UPDATE", false).await;
    let a = Recording::default();
    let b = Recording::default();
    let endpoint = peer.endpoint("same");
    let (publish, endpoints) = watch::channel(vec![endpoint.clone().executor(a.clone())]);
    let channel = Endpoint::balance_channel(endpoints, ChannelConfig::new())
        .await
        .unwrap();
    let client = GreeterClient::new(channel.clone());
    assert_eq!(unary(&client).await.unwrap(), "UPDATE:1");
    let id = channel_id_for_peer(peer.addr).await;
    publish
        .send(vec![
            endpoint.clone().executor(b.clone()),
            peer.endpoint("invalid")
                .origin("http://virtual.invalid".parse().unwrap()),
        ])
        .unwrap();
    tokio::time::timeout(BOUND, async {
        while !trace_contains(id, "Endpoint list update rejected").await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(unary(&client).await.unwrap(), "UPDATE:1");
    assert_eq!(b.0.scheduled.load(Ordering::SeqCst), 0);
    publish.send(Vec::new()).unwrap();
    tokio::time::timeout(BOUND, async {
        while !trace_contains(id, "Endpoint list updated: 0 URI entries").await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(unary(&client).await.unwrap_err().code(), Code::Unavailable);
    count(&a.0.finished, 1).await;
    publish.send(vec![endpoint.executor(b.clone())]).unwrap();
    tokio::time::timeout(BOUND, async {
        loop {
            if let Ok(tag) = unary(&client).await {
                assert_eq!(tag, "UPDATE:2");
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(publish);
    drop(client);
    drop(channel);
    count(&b.0.finished, 1).await;
    peer.stop().await;
}

#[derive(Clone, Default)]
struct Queued {
    tasks: Arc<tokio::sync::Mutex<Vec<ExecutorFuture>>>,
    scheduled: Arc<AtomicUsize>,
}
impl Executor for Queued {
    fn execute(&self, future: ExecutorFuture) {
        self.tasks.try_lock().unwrap().push(future);
        self.scheduled.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn a_late_failure_from_replaced_uri_cannot_discard_its_replacement() {
    let peer = Peer::start("LATE", false).await;
    let queued = Queued::default();
    let executor = Recording::default();
    let endpoint = peer.endpoint("same");
    let (publish, endpoints) = watch::channel(vec![endpoint.clone().executor(queued.clone())]);
    let channel = Endpoint::balance_channel(endpoints, ChannelConfig::new())
        .await
        .unwrap();
    let old_client = GreeterClient::new(channel.clone());
    let pending = tokio::spawn(async move { unary(&old_client).await });
    count(&queued.scheduled, 1).await;
    let id = channel_id_for_peer(peer.addr).await;
    publish
        .send(vec![endpoint.executor(executor.clone())])
        .unwrap();
    tokio::time::timeout(BOUND, async {
        while !trace_contains(id, "Endpoint list updated: 1 URI entries").await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let client = GreeterClient::new(channel.clone());
    tokio::time::timeout(BOUND, async {
        loop {
            if let Ok(tag) = unary(&client).await {
                assert_eq!(tag, "LATE:2");
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(queued.tasks.lock().await.pop().unwrap());
    let old = tokio::time::timeout(BOUND, pending).await.unwrap().unwrap();
    if let Ok(tag) = old {
        assert_eq!(tag, "LATE:2");
    }
    for _ in 0..4 {
        assert_eq!(unary(&client).await.unwrap(), "LATE:2");
    }
    assert_eq!(executor.0.scheduled.load(Ordering::SeqCst), 1);
    drop(publish);
    drop(client);
    drop(channel);
    count(&executor.0.finished, 1).await;
    peer.stop().await;
}

#[cfg(feature = "tonic")]
mod reference {
    use super::*;
    use bytes::Buf;
    use pbrs_grpc::codec::CodecMessage;
    use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

    impl<F> hyper::rt::Executor<F> for Recording
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        fn execute(&self, future: F) {
            pbrs_grpc::Executor::execute(self, Box::pin(future));
        }
    }
    impl<F> hyper::rt::Executor<F> for Discard
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        fn execute(&self, future: F) {
            pbrs_grpc::Executor::execute(self, Box::pin(future));
        }
    }

    struct EchoCodec;
    struct EchoEncoder;
    struct EchoDecoder;
    impl Codec for EchoCodec {
        type Encode = HelloRequest;
        type Decode = HelloReply;
        type Encoder = EchoEncoder;
        type Decoder = EchoDecoder;
        fn encoder(&mut self) -> EchoEncoder {
            EchoEncoder
        }
        fn decoder(&mut self) -> EchoDecoder {
            EchoDecoder
        }
    }
    impl Encoder for EchoEncoder {
        type Item = HelloRequest;
        type Error = tonic::Status;
        fn encode(
            &mut self,
            item: HelloRequest,
            output: &mut EncodeBuf<'_>,
        ) -> Result<(), Self::Error> {
            item.encode_payload_bufmut(output)
                .map_err(|error| tonic::Status::internal(error.to_string()))
        }
    }
    impl Decoder for EchoDecoder {
        type Item = HelloReply;
        type Error = tonic::Status;
        fn decode(&mut self, input: &mut DecodeBuf<'_>) -> Result<Option<HelloReply>, Self::Error> {
            let data = input.copy_to_bytes(input.remaining());
            HelloReply::decode_payload(data)
                .map(Some)
                .map_err(|error| tonic::Status::internal(error.to_string()))
        }
    }

    async fn unary<T>(client: &mut tonic::client::Grpc<T>) -> String
    where
        T: tonic::client::GrpcService<tonic::body::Body, ResponseBody = tonic::body::Body>,
        T::Error: Into<tonic::codegen::StdError> + std::fmt::Debug,
    {
        client.ready().await.unwrap();
        tag(client
            .unary(
                tonic::Request::new(message()),
                http::uri::PathAndQuery::from_static("/balance.Reference/Unary"),
                EchoCodec,
            )
            .await
            .unwrap()
            .get_ref())
    }

    async fn shapes<T>(client: &mut tonic::client::Grpc<T>) -> Vec<String>
    where
        T: tonic::client::GrpcService<tonic::body::Body, ResponseBody = tonic::body::Body>,
        T::Error: Into<tonic::codegen::StdError> + std::fmt::Debug,
    {
        tokio::time::timeout(BOUND, async {
            let mut tags = Vec::new();
            for _ in 0..2 {
                tags.push(unary(client).await);
            }
            for _ in 0..2 {
                client.ready().await.unwrap();
                let mut stream = client
                    .server_streaming(
                        tonic::Request::new(message()),
                        http::uri::PathAndQuery::from_static("/balance.Reference/ServerHello"),
                        EchoCodec,
                    )
                    .await
                    .unwrap()
                    .into_inner();
                tags.push(tag(&stream.message().await.unwrap().unwrap()));
                assert!(stream.message().await.unwrap().is_none());
            }
            for _ in 0..2 {
                client.ready().await.unwrap();
                let response = client
                    .client_streaming(
                        tonic::Request::new(tokio_stream::iter([message()])),
                        http::uri::PathAndQuery::from_static("/balance.Reference/ClientHello"),
                        EchoCodec,
                    )
                    .await
                    .unwrap();
                tags.push(tag(response.get_ref()));
            }
            for _ in 0..2 {
                client.ready().await.unwrap();
                let mut stream = client
                    .streaming(
                        tonic::Request::new(tokio_stream::iter([message()])),
                        http::uri::PathAndQuery::from_static("/balance.Reference/StreamHello"),
                        EchoCodec,
                    )
                    .await
                    .unwrap()
                    .into_inner();
                tags.push(tag(&stream.message().await.unwrap().unwrap()));
                assert!(stream.message().await.unwrap().is_none());
            }
            tags
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn tonic_reference_preserves_same_socket_uri_alias_executors() {
        let peer = Peer::start("TONIC", false).await;
        let a = Recording::default();
        let b = Recording::default();
        let channel = tonic::transport::Channel::balance_list(
            [
                tonic::transport::Endpoint::from_shared(format!("http://{}/one", peer.addr))
                    .unwrap()
                    .executor(a.clone()),
                tonic::transport::Endpoint::from_shared(format!("http://{}/two", peer.addr))
                    .unwrap()
                    .executor(b.clone()),
            ]
            .into_iter(),
        );
        let mut client = tonic::client::Grpc::new(channel);
        let tags = shapes(&mut client).await;
        assert!(tags.iter().all(|tag| tag == "TONIC:1" || tag == "TONIC:2"));
        count(&a.0.polled, 1).await;
        count(&b.0.polled, 1).await;
        // Hyper sends multiple stream/background task kinds to this executor.
        // The independent peer's accept count is the connection-count oracle.
        assert!(a.0.scheduled.load(Ordering::SeqCst) >= 1);
        assert!(b.0.scheduled.load(Ordering::SeqCst) >= 1);
        assert_eq!(peer.accepted.load(Ordering::SeqCst), 2);
        drop(client);
        peer.stop().await;
    }

    #[tokio::test]
    async fn tonic_reference_same_uri_insert_replaces_executor_and_keeps_old_stream() {
        let peer = Peer::start("TONICSWAP", true).await;
        let a = Recording::default();
        let b = Recording::default();
        let uri = format!("http://{}/same", peer.addr);
        let (channel, publish) = tonic::transport::Channel::balance_channel::<usize>(4);
        publish
            .send(tonic::transport::channel::Change::Insert(
                1,
                tonic::transport::Endpoint::from_shared(uri.clone())
                    .unwrap()
                    .executor(a.clone()),
            ))
            .await
            .unwrap();
        let mut client = tonic::client::Grpc::new(channel);
        client.ready().await.unwrap();
        let mut old = tokio::time::timeout(
            BOUND,
            client.server_streaming(
                tonic::Request::new(message()),
                http::uri::PathAndQuery::from_static("/balance.Reference/ServerHello"),
                EchoCodec,
            ),
        )
        .await
        .unwrap()
        .unwrap()
        .into_inner();
        assert_eq!(tag(&old.message().await.unwrap().unwrap()), "TONICSWAP:1");
        publish
            .send(tonic::transport::channel::Change::Insert(
                1,
                tonic::transport::Endpoint::from_shared(uri)
                    .unwrap()
                    .executor(b.clone()),
            ))
            .await
            .unwrap();
        tokio::time::timeout(BOUND, async {
            while unary(&mut client).await != "TONICSWAP:2" {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(b.0.scheduled.load(Ordering::SeqCst) >= 1);
        peer.release.send(true).unwrap();
        assert!(
            tokio::time::timeout(BOUND, old.message())
                .await
                .unwrap()
                .unwrap()
                .is_none()
        );
        drop(old);
        drop(client);
        drop(publish);
        peer.stop().await;
    }

    #[tokio::test]
    async fn opaque_tonic_client_keeps_native_balanced_identity_for_all_shapes() {
        let peer = Peer::start("OPAQUE", false).await;
        let a = Recording::default();
        let b = Recording::default();
        let channel = Endpoint::balance_list(
            [
                Endpoint::from_shared(format!("http://127.0.0.1:{}/one", peer.addr.port()))
                    .unwrap()
                    .executor(a.clone()),
                Endpoint::from_shared(format!("http://127.0.0.1:0{}/two", peer.addr.port()))
                    .unwrap()
                    .executor(b.clone()),
            ],
            ChannelConfig::new(),
        )
        .await
        .unwrap();
        let mut client = tonic::client::Grpc::new(channel);
        assert_eq!(
            shapes(&mut client).await,
            ["OPAQUE:1", "OPAQUE:2"].repeat(4)
        );
        assert_eq!(
            *peer.authorities.lock().await,
            [
                format!("127.0.0.1:{}", peer.addr.port()),
                format!("127.0.0.1:0{}", peer.addr.port())
            ]
            .iter()
            .cycle()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
        );
        assert_eq!(a.0.scheduled.load(Ordering::SeqCst), 1);
        assert_eq!(b.0.scheduled.load(Ordering::SeqCst), 1);
        drop(client);
        count(&a.0.finished, 1).await;
        count(&b.0.finished, 1).await;
        peer.stop().await;
    }

    #[tokio::test]
    async fn selected_uri_authorities_match_tonic_and_preserve_raw_port_spelling() {
        let peer = Peer::start("AUTHORITY", false).await;
        let first = format!("http://127.0.0.1:{}/one", peer.addr.port());
        let second = format!("http://127.0.0.1:0{}/two", peer.addr.port());
        let a = Endpoint::from_shared(first.clone()).unwrap();
        let b = Endpoint::from_shared(second.clone()).unwrap();
        assert_eq!(a.uri().port_u16(), b.uri().port_u16());
        assert_ne!(a.uri().authority(), b.uri().authority());
        let channel = Endpoint::balance_list([a, b], ChannelConfig::new())
            .await
            .unwrap();
        assert_eq!(
            all_shapes_twice(channel.clone()).await,
            ["AUTHORITY:1", "AUTHORITY:2"].repeat(4)
        );
        let expected = [first.clone(), second.clone()].map(|uri| {
            uri.parse::<http::Uri>()
                .unwrap()
                .authority()
                .unwrap()
                .as_str()
                .to_owned()
        });
        assert_eq!(
            *peer.authorities.lock().await,
            expected.iter().cycle().take(8).cloned().collect::<Vec<_>>()
        );
        drop(channel);
        peer.authorities.lock().await.clear();
        for uri in [first, second] {
            let expected = uri
                .parse::<http::Uri>()
                .unwrap()
                .authority()
                .unwrap()
                .as_str()
                .to_owned();
            let endpoint = tonic::transport::Endpoint::from_shared(uri).unwrap();
            let channel = tonic::transport::Channel::balance_list([endpoint].into_iter());
            let mut client = tonic::client::Grpc::new(channel);
            shapes(&mut client).await;
            let received = std::mem::take(&mut *peer.authorities.lock().await);
            assert_eq!(received.len(), 8);
            assert!(received.iter().all(|authority| authority == &expected));
            drop(client);
        }
        peer.stop().await;
    }
}

fn logged(channel: Channel) -> (Channel, Arc<pbrs_grpc::binlog::VecSink>) {
    use pbrs_grpc::binlog::{BinaryLogFilter, BinaryLogger, VecSink};
    let sink = Arc::new(VecSink::new());
    let logger = BinaryLogger::new(BinaryLogFilter::parse("*{h}").unwrap(), sink.clone());
    (channel.binary_logger(logger), sink)
}

fn logged_authorities(sink: &pbrs_grpc::binlog::VecSink) -> Vec<String> {
    sink.records()
        .into_iter()
        .filter_map(|record| match record.entry.payload {
            Some(pbrs_grpc::binlog::Payload::ClientHeader(header)) => Some(header.authority),
            _ => None,
        })
        .collect()
}

async fn assert_authorities(peer: &Peer, expected: &str, count: usize) {
    let actual = std::mem::take(&mut *peer.authorities.lock().await);
    assert_eq!(actual, vec![expected; count]);
}

#[tokio::test]
async fn selected_endpoint_authorities_and_binary_logs_follow_all_four_shapes() {
    let a = Peer::start("A", false).await;
    let b = Peer::start("B", false).await;
    let channel = Endpoint::balance_list(
        [a.endpoint("ignored-a"), b.endpoint("ignored-b")],
        ChannelConfig::new(),
    )
    .await
    .unwrap();
    let (channel, sink) = logged(channel);
    assert_eq!(
        all_shapes_twice(channel.clone()).await,
        ["A:1", "B:1"].repeat(4)
    );
    assert_authorities(&a, &a.addr.to_string(), 4).await;
    assert_authorities(&b, &b.addr.to_string(), 4).await;
    assert_eq!(
        logged_authorities(&sink),
        [a.addr.to_string(), b.addr.to_string()]
            .iter()
            .cycle()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
    );
    drop(channel);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn clone_authority_overrides_include_the_diagnostic_sentinel() {
    let a = Peer::start("A", false).await;
    let b = Peer::start("B", false).await;
    let base = Endpoint::balance_list([a.endpoint("a"), b.endpoint("b")], ChannelConfig::new())
        .await
        .unwrap();
    let diagnostic = base.authority().to_owned();
    assert_eq!(diagnostic, "pbrs-endpoint-list.invalid:443");
    for authority in ["virtual.example:7443", diagnostic.as_str()] {
        let (overridden, sink) = logged(base.clone().origin(authority).unwrap());
        assert_eq!(overridden.authority(), authority);
        assert_eq!(
            all_shapes_twice(overridden.clone()).await,
            ["A:1", "B:1"].repeat(4)
        );
        assert_authorities(&a, authority, 4).await;
        assert_authorities(&b, authority, 4).await;
        assert_eq!(logged_authorities(&sink), vec![authority; 8]);
        drop(overridden);
        assert_eq!(
            all_shapes_twice(base.clone()).await,
            ["A:1", "B:1"].repeat(4)
        );
        assert_authorities(&a, &a.addr.to_string(), 4).await;
        assert_authorities(&b, &b.addr.to_string(), 4).await;
    }
    assert_eq!(base.authority(), diagnostic);
    drop(base);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn direct_channel_authority_keeps_the_original_target() {
    let peer = Peer::start("DIRECT", false).await;
    let base = Channel::connect(peer.addr).await.unwrap();
    assert_eq!(base.authority(), peer.addr.to_string());
    for channel in [
        base.clone(),
        base.clone().origin("direct.override:81").unwrap(),
        base.clone(),
    ] {
        let expected = channel.authority().to_owned();
        let (channel, sink) = logged(channel);
        assert_eq!(all_shapes_twice(channel).await, vec!["DIRECT:1"; 8]);
        assert_authorities(&peer, &expected, 8).await;
        assert_eq!(logged_authorities(&sink), vec![expected; 8]);
    }
    drop(base);
    peer.stop().await;
}

#[tokio::test]
async fn balanced_health_uses_selected_target_without_clone_authority_overlay() {
    let a = Peer::start("A", false).await;
    let b = Peer::start("B", false).await;
    a.require_health_authority.store(true, Ordering::SeqCst);
    b.require_health_authority.store(true, Ordering::SeqCst);
    let channel = Endpoint::balance_list(
        [a.endpoint("a"), b.endpoint("b")],
        ChannelConfig::new().health_checking(true),
    )
    .await
    .unwrap()
    .service_config(r#"{"healthCheckConfig":{"serviceName":"virtual-host"}}"#)
    .unwrap()
    .origin("user-rpc.override:7443")
    .unwrap();
    assert_eq!(
        all_shapes_twice(channel.clone()).await,
        ["A:1", "B:1"].repeat(4)
    );
    for peer in [&a, &b] {
        let observed = peer.authorities.lock().await;
        assert_eq!(
            observed.first().unwrap(),
            &peer.addr.to_string(),
            "health uses the selected transport target"
        );
        assert_eq!(
            observed
                .iter()
                .filter(|a| *a == "user-rpc.override:7443")
                .count(),
            4
        );
        assert_eq!(observed.len(), 5);
        assert_eq!(peer.health.load(Ordering::SeqCst), 1);
    }
    drop(channel);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn retries_and_transparent_redials_use_each_new_selected_authority() {
    for server_stream in [false, true] {
        for transparent in [false, true] {
            let a = Peer::start("FIRST", false).await;
            let b = Peer::start("SECOND", false).await;
            if transparent {
                a.refuse_next_rpc.store(true, Ordering::SeqCst);
            } else {
                a.fail_next_rpc.store(true, Ordering::SeqCst);
            }
            let mut channel =
                Endpoint::balance_list([a.endpoint("a"), b.endpoint("b")], ChannelConfig::new())
                    .await
                    .unwrap();
            if !transparent {
                channel = channel.service_config(r#"{"methodConfig":[{"name":[{}],"retryPolicy":{"maxAttempts":2,"initialBackoff":"0.001s","maxBackoff":"0.001s","backoffMultiplier":1.0,"retryableStatusCodes":["UNAVAILABLE"]}}]}"#).unwrap();
            }
            let (channel, sink) = logged(channel);
            let client = GreeterClient::new(channel.clone());
            if server_stream {
                tokio::time::timeout(BOUND, async {
                    let mut response = client
                        .server_hello(Request::new(message()))
                        .await
                        .unwrap()
                        .into_inner();
                    assert_eq!(tag(&response.message().await.unwrap().unwrap()), "SECOND:1");
                    assert!(response.message().await.unwrap().is_none());
                })
                .await
                .unwrap();
            } else {
                assert_eq!(unary(&client).await.unwrap(), "SECOND:1");
            }
            assert_authorities(&a, &a.addr.to_string(), 1).await;
            assert_authorities(&b, &b.addr.to_string(), 1).await;
            assert_eq!(
                logged_authorities(&sink),
                [a.addr.to_string(), b.addr.to_string()]
            );
            drop(client);
            drop(channel);
            a.stop().await;
            b.stop().await;
        }
    }
}

#[tokio::test]
async fn hedged_attempts_keep_independent_selected_authorities() {
    let a = Peer::start("HELD", false).await;
    let b = Peer::start("WINNER", false).await;
    a.hold_next_rpc.store(true, Ordering::SeqCst);
    let channel = Endpoint::balance_list([a.endpoint("a"), b.endpoint("b")], ChannelConfig::new()).await.unwrap()
        .service_config(r#"{"methodConfig":[{"name":[{}],"hedgingPolicy":{"maxAttempts":2,"hedgingDelay":"0.001s","nonFatalStatusCodes":["UNAVAILABLE"]}}]}"#).unwrap();
    let (channel, sink) = logged(channel);
    let client = GreeterClient::new(channel.clone());
    assert_eq!(unary(&client).await.unwrap(), "WINNER:1");
    assert_authorities(&a, &a.addr.to_string(), 1).await;
    assert_authorities(&b, &b.addr.to_string(), 1).await;
    assert_eq!(
        logged_authorities(&sink),
        [a.addr.to_string(), b.addr.to_string()]
    );
    a.release.send(true).unwrap();
    drop(client);
    drop(channel);
    a.stop().await;
    b.stop().await;
}
