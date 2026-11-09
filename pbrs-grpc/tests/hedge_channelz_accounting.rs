//! Channelz stream accounting for aborted and unwinding hedged attempts.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "integration tests"
)]

mod common;

use bytes::Bytes;
use pbrs::Serialize;
use pbrs_grpc::channelz::{
    Channelz, ChannelzService, GetChannelRequest, GetSocketRequest, GetTopChannelsRequest,
};
use pbrs_grpc::hello::HelloReply;
use pbrs_grpc::{AttemptLabels, Channel, ChannelConfig, LifecycleObserver, Request, Status};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Notify, mpsc, oneshot};

const PATH: &str = "/helloworld.Greeter/SayHello";
const BUDGET: Duration = Duration::from_secs(10);
static DECODE_ENTERED: OnceLock<Notify> = OnceLock::new();

#[derive(Debug)]
struct PanicDecode;

impl pbrs_grpc::codec::CodecMessage for PanicDecode {
    fn encoded_len(&self) -> usize {
        0
    }

    fn encode_payload<W: pbrs::WireOut>(&self, _out: &mut W) -> Result<(), Status> {
        Ok(())
    }

    fn decode_payload(_payload: Bytes) -> Result<Self, Status> {
        DECODE_ENTERED.get_or_init(Notify::new).notify_one();
        panic!("channelz decoder panic");
    }

    fn empty() -> Self {
        Self
    }
}

struct CompletionObserver {
    ended: mpsc::UnboundedSender<u32>,
}

impl LifecycleObserver for CompletionObserver {
    fn on_attempt_end(&self, attempt: &AttemptLabels<'_>, _status: &Status, _elapsed: Duration) {
        let _ = self.ended.send(attempt.attempt());
    }
}

/// A peer accepts two streams and keeps both request bodies behind a small
/// receive window. The parent may request a response on the first stream,
/// or leave both pending for a dropped-call test.
struct Peer {
    channel: Channel,
    port: u16,
    hold: oneshot::Sender<()>,
    accepted: oneshot::Receiver<()>,
    command: oneshot::Sender<bool>,
    resets: oneshot::Receiver<Vec<h2::Reason>>,
    server: common::ServerGuard,
}

async fn peer() -> Peer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let (accepted_tx, accepted) = oneshot::channel();
    let (command, command_rx) = oneshot::channel();
    let (resets_tx, resets) = oneshot::channel();
    let (hold, hold_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("connection");
        let mut conn = h2::server::Builder::new()
            .initial_window_size(1_024)
            .handshake::<_, Bytes>(socket)
            .await
            .expect("server handshake");
        let (request_a, mut response_a) = conn
            .accept()
            .await
            .expect("first stream")
            .expect("first headers");
        let (request_b, mut response_b) = conn
            .accept()
            .await
            .expect("second stream")
            .expect("second headers");
        let mut body_a = request_a.into_body();
        let _body_b = request_b.into_body();
        let _driver = common::ServerGuard(tokio::spawn(async move {
            while let Some(next) = conn.accept().await {
                if next.is_err() {
                    break;
                }
            }
        }));
        accepted_tx.send(()).expect("report both streams");
        let respond = command_rx.await.expect("test command");
        let reasons = if respond {
            while let Some(chunk) = body_a.data().await {
                let chunk = chunk.expect("first request data");
                body_a
                    .flow_control()
                    .release_capacity(chunk.len())
                    .expect("return first stream capacity");
            }
            let headers = http::Response::builder()
                .status(http::StatusCode::OK)
                .header(http::header::CONTENT_TYPE, "application/grpc")
                .body(())
                .expect("response headers");
            let mut send = response_a.send_response(headers, false).expect("headers");
            let payload = common::reply("reply").serialize().expect("serialize reply");
            let frame = pbrs_grpc::codec::encode(&payload, false).expect("reply frame");
            send.send_data(frame, false).expect("response data");
            let mut trailers = http::HeaderMap::new();
            trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
            send.send_trailers(trailers).expect("response trailers");
            let reason = std::future::poll_fn(|cx| response_b.poll_reset(cx))
                .await
                .expect("sibling reset");
            vec![reason]
        } else {
            let first = std::future::poll_fn(|cx| response_a.poll_reset(cx));
            let second = std::future::poll_fn(|cx| response_b.poll_reset(cx));
            let (first, second) = tokio::join!(first, second);
            vec![first.expect("first reset"), second.expect("second reset")]
        };
        resets_tx.send(reasons).expect("report resets");
        let _ = hold_rx.await;
    });
    let channel = Channel::connect_with(
        addr,
        ChannelConfig::new()
            .connections(1)
            .max_send_buffer_size(1_024),
    )
    .await
    .expect("connect")
    .byte_budget(2 * 1024 * 1024)
    .service_config(
        r#"{"methodConfig":[{"name":[{}],"hedgingPolicy":{"maxAttempts":2,"hedgingDelay":"0s"}}]}"#,
    )
    .expect("hedging policy");
    Peer {
        channel,
        port: addr.port(),
        hold,
        accepted,
        command,
        resets,
        server: common::ServerGuard(server),
    }
}

async fn budget_released(channel: &Channel) {
    tokio::time::timeout(BUDGET, async {
        while channel.byte_budget_allocated() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("byte permits released");
}

async fn completed_attempts(ended: &mut mpsc::UnboundedReceiver<u32>, count: usize) {
    tokio::time::timeout(BUDGET, async {
        for _ in 0..count {
            ended.recv().await.expect("attempt completion");
        }
    })
    .await
    .expect("all attempt guards completed");
}

async fn socket_counts(port: u16) -> (i64, i64, i64, i64, i64) {
    let service = ChannelzService::shared_global();
    let mut query = GetTopChannelsRequest::new();
    query.set_start_channel_id(0);
    query.set_max_results(1000);
    let response = Channelz::get_top_channels(&service, Request::new(query))
        .await
        .expect("top channels");
    let needle = format!(":{port}");
    let channel_id = response
        .get_ref()
        .channel()
        .iter()
        .find(|channel| {
            channel
                .data()
                .target()
                .to_str()
                .unwrap_or("")
                .ends_with(&needle)
        })
        .expect("test channel")
        .r#ref()
        .channel_id();
    let mut query = GetChannelRequest::new();
    query.set_channel_id(channel_id);
    let response = Channelz::get_channel(&service, Request::new(query))
        .await
        .expect("channel");
    let sockets: Vec<i64> = response
        .get_ref()
        .channel()
        .socket_ref()
        .iter()
        .map(|socket| socket.socket_id())
        .collect();
    assert_eq!(sockets.len(), 1, "one test socket");
    let mut query = GetSocketRequest::new();
    query.set_socket_id(sockets[0]);
    let response = Channelz::get_socket(&service, Request::new(query))
        .await
        .expect("socket");
    let data = response.get_ref().socket().data();
    (
        data.streams_started(),
        data.streams_succeeded(),
        data.streams_failed(),
        data.messages_sent(),
        data.messages_received(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn success_and_cancelled_loser_balance_channelz() {
    let Peer {
        channel,
        port,
        hold,
        accepted,
        command,
        resets,
        server,
    } = peer().await;
    let (_hold, _server) = (hold, server);
    let (tx, mut ended) = mpsc::unbounded_channel();
    let channel = channel.observer(CompletionObserver { ended: tx });
    let call =
        channel.unary::<_, HelloReply>(PATH, Request::new(common::req(&"x".repeat(256 * 1024))));
    let task = tokio::spawn(call);
    tokio::time::timeout(BUDGET, accepted)
        .await
        .expect("streams")
        .expect("accept");
    assert!(channel.byte_budget_allocated() > 0);
    command.send(true).expect("respond");
    let response = tokio::time::timeout(BUDGET, task)
        .await
        .expect("call")
        .expect("join")
        .expect("success");
    assert_eq!(common::name_of(response.get_ref()), "reply");
    assert_eq!(
        tokio::time::timeout(BUDGET, resets)
            .await
            .expect("reset")
            .expect("witness"),
        vec![h2::Reason::CANCEL]
    );
    completed_attempts(&mut ended, 2).await;
    budget_released(&channel).await;
    assert_eq!(
        socket_counts(port).await,
        (2, 1, 1, 1, 1),
        "successful hedge must balance socket counters"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panic_and_cancelled_sibling_balance_channelz() {
    let Peer {
        channel,
        port,
        hold,
        accepted,
        command,
        resets,
        server,
    } = peer().await;
    let (_hold, _server) = (hold, server);
    let (tx, mut ended) = mpsc::unbounded_channel();
    let channel = channel.observer(CompletionObserver { ended: tx });
    let call =
        channel.unary::<_, PanicDecode>(PATH, Request::new(common::req(&"x".repeat(256 * 1024))));
    let task = tokio::spawn(call);
    tokio::time::timeout(BUDGET, accepted)
        .await
        .expect("streams")
        .expect("accept");
    assert!(channel.byte_budget_allocated() > 0);
    command.send(true).expect("respond");
    tokio::time::timeout(BUDGET, DECODE_ENTERED.get_or_init(Notify::new).notified())
        .await
        .expect("decode entered");
    completed_attempts(&mut ended, 1).await;
    task.abort();
    let error = task.await.expect_err("call task must not succeed");
    if error.is_panic() {
        let payload = error.into_panic();
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&'static str>().copied());
        assert_eq!(message, Some("channelz decoder panic"));
    } else {
        assert!(error.is_cancelled());
    }
    assert_eq!(
        tokio::time::timeout(BUDGET, resets)
            .await
            .expect("reset")
            .expect("witness"),
        vec![h2::Reason::CANCEL]
    );
    completed_attempts(&mut ended, 1).await;
    budget_released(&channel).await;
    assert_eq!(
        socket_counts(port).await,
        (2, 0, 2, 0, 0),
        "hedged panic must balance socket counters"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_call_balances_both_hedged_streams() {
    let Peer {
        channel,
        port,
        hold,
        accepted,
        command,
        resets,
        server,
    } = peer().await;
    let (_hold, _server) = (hold, server);
    let (tx, mut ended) = mpsc::unbounded_channel();
    let channel = channel.observer(CompletionObserver { ended: tx });
    let call =
        channel.unary::<_, HelloReply>(PATH, Request::new(common::req(&"x".repeat(256 * 1024))));
    let task = tokio::spawn(call);
    tokio::time::timeout(BUDGET, accepted)
        .await
        .expect("streams")
        .expect("accept");
    assert!(channel.byte_budget_allocated() > 0);
    command.send(false).expect("hold streams");
    task.abort();
    assert!(task.await.expect_err("call task aborted").is_cancelled());
    assert_eq!(
        tokio::time::timeout(BUDGET, resets)
            .await
            .expect("resets")
            .expect("witness"),
        vec![h2::Reason::CANCEL, h2::Reason::CANCEL]
    );
    completed_attempts(&mut ended, 2).await;
    budget_released(&channel).await;
    assert_eq!(
        socket_counts(port).await,
        (2, 0, 2, 0, 0),
        "dropped hedge must balance socket counters"
    );
}

async fn subchannel_counts(addr: std::net::SocketAddr) -> (i64, i64, i64) {
    use pbrs_grpc::channelz::GetSubchannelRequest;
    let service = ChannelzService::shared_global();
    let mut query = GetTopChannelsRequest::new();
    query.set_start_channel_id(0);
    query.set_max_results(1000);
    let response = Channelz::get_top_channels(&service, Request::new(query))
        .await
        .expect("top channels");
    let target = addr.to_string();
    let mut id = None;
    for channel in response.get_ref().channel().iter() {
        for subchannel in channel.subchannel_ref().iter() {
            if subchannel.name().to_str().unwrap_or("") == target {
                id = Some(subchannel.subchannel_id());
            }
        }
    }
    let mut query = GetSubchannelRequest::new();
    query.set_subchannel_id(id.expect("test subchannel"));
    let response = Channelz::get_subchannel(&service, Request::new(query))
        .await
        .expect("subchannel");
    let data = response.get_ref().subchannel().data();
    (
        data.calls_started(),
        data.calls_succeeded(),
        data.calls_failed(),
    )
}

struct FixedDns(std::net::SocketAddr);

impl pbrs_grpc::resolver::DnsLookup for FixedDns {
    fn lookup(
        &self,
        _host: &str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Vec<std::net::SocketAddr>, std::io::Error>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move { Ok(vec![self.0]) })
    }
}

struct FixedTxt;

impl pbrs_grpc::resolver::TxtLookup for FixedTxt {
    fn fetch_txt(
        &self,
        _name: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<String>, std::io::Error>> + Send + '_>,
    > {
        Box::pin(async {
            Ok(vec![r#"{"loadBalancingConfig":[{"round_robin":{}}],"methodConfig":[{"name":[{}],"hedgingPolicy":{"maxAttempts":2,"hedgingDelay":"30s"}}]}"#.to_owned()])
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transparent_redial_counts_each_stream_once() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let (hold, hold_rx) = oneshot::channel::<()>();
    let server = common::ServerGuard(tokio::spawn(async move {
        let (first, _) = listener.accept().await.expect("first connection");
        let mut conn = h2::server::Builder::new()
            .initial_window_size(1_024)
            .handshake::<_, Bytes>(first)
            .await
            .expect("first handshake");
        let (request, mut respond) = conn.accept().await.expect("first stream").expect("headers");
        respond.send_reset(h2::Reason::REFUSED_STREAM);
        drop(request);
        let _first_driver = common::ServerGuard(tokio::spawn(async move {
            while let Some(next) = conn.accept().await {
                if next.is_err() {
                    break;
                }
            }
        }));

        let (second, _) = listener.accept().await.expect("second connection");
        let mut conn = h2::server::handshake(second)
            .await
            .expect("second handshake");
        let (request, mut respond) = conn
            .accept()
            .await
            .expect("second stream")
            .expect("headers");
        let mut body = request.into_body();
        let mut hold_rx = hold_rx;
        while let Some(chunk) = tokio::select! {
            chunk = body.data() => chunk,
            next = conn.accept() => panic!("unexpected stream during request: {next:?}"),
        } {
            let chunk = chunk.expect("request body");
            body.flow_control()
                .release_capacity(chunk.len())
                .expect("capacity");
        }
        let headers = http::Response::builder()
            .status(http::StatusCode::OK)
            .header(http::header::CONTENT_TYPE, "application/grpc")
            .body(())
            .expect("response");
        let mut send = respond.send_response(headers, false).expect("headers");
        let payload = common::reply("redial").serialize().expect("serialize");
        send.send_data(
            pbrs_grpc::codec::encode(&payload, false).expect("frame"),
            false,
        )
        .expect("data");
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
        send.send_trailers(trailers).expect("trailers");
        tokio::select! {
            _ = &mut hold_rx => {},
            next = conn.accept() => {
                assert!(next.is_none(), "unexpected extra stream");
            }
        }
    }));
    let bounds = pbrs_grpc::resolver::DnsConfig::new(
        Duration::from_millis(50),
        Duration::from_secs(1),
        Duration::from_millis(100),
        Duration::from_millis(50),
        Duration::from_millis(200),
        Duration::from_secs(1),
    )
    .expect("DNS bounds");
    let resolver =
        pbrs_grpc::resolver::ResolverConfig::with_dns_provider(bounds, Arc::new(FixedDns(addr)))
            .with_txt_provider(Arc::new(FixedTxt));
    let channel = Channel::connect_uri("dns:///hedge-counter.invalid:443", resolver)
        .await
        .expect("round-robin channel");
    let call =
        channel.unary::<_, HelloReply>(PATH, Request::new(common::req(&"x".repeat(32 * 1024))));
    let response = tokio::time::timeout(BUDGET, call)
        .await
        .expect("call")
        .expect("redial succeeds");
    assert_eq!(common::name_of(response.get_ref()), "redial");
    assert_eq!(channel.retry_stats().transparent_retries, 1);
    assert_eq!(channel.retry_stats().hedged_sends, 0);
    assert_eq!(
        subchannel_counts(addr).await,
        (2, 1, 1),
        "redial must count both streams once"
    );
    drop(hold);
    drop(server);
}
