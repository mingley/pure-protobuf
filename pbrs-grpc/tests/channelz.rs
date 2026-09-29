//! Channelz (A14) end-to-end: counters round-trip through the
//! `grpc.channelz.v1.Channelz` service, dropped entities unregister,
//! pagination and error cases behave, and traces are present.
//!
//! Every test runs a greeter server under test plus a separate
//! channelz query server: the query traffic itself creates channels,
//! sockets, and server calls, so querying through the server under
//! test would pollute the exact counters under assertion. The global
//! registry is shared across tests in this binary, so each test
//! scopes its entities by its own listener port.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "integration test helpers assert helper behavior"
)]

mod common;

use common::{Echo, ServerGuard, name_of, req};
use pbrs_grpc::channelz::{
    ChannelzClient, ChannelzServer, ChannelzService, GetChannelRequest, GetServerRequest,
    GetServerSocketsRequest, GetServersRequest, GetSocketRequest, GetSubchannelRequest,
    GetTopChannelsRequest,
};
use pbrs_grpc::hello::{GreeterClient, GreeterServer};
use pbrs_grpc::{Code, Request, Router, Status};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

/// Serve Echo on a loopback listener. Channelz itself is queried
/// through a separate server so the query traffic never lands on
/// the counters under assertion.
async fn serve_under_test() -> (SocketAddr, ServerGuard) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

/// Serve Channelz alone: the query path for another test server.
async fn serve_query() -> (SocketAddr, ServerGuard) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(ChannelzServer::new(ChannelzService::shared_global()))
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

async fn greeter_client(addr: SocketAddr) -> GreeterClient {
    common::greeter_client(addr).await
}

async fn query_client(addr: SocketAddr) -> ChannelzClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match ChannelzClient::connect(addr).await {
            Ok(client) => return client,
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to channelz at {addr}: {last}");
}

fn top_channels() -> GetTopChannelsRequest {
    let mut q = GetTopChannelsRequest::new();
    q.set_start_channel_id(0);
    q.set_max_results(1000);
    q
}

fn servers() -> GetServersRequest {
    let mut q = GetServersRequest::new();
    q.set_start_server_id(0);
    q.set_max_results(1000);
    q
}

/// Channel ids whose target dials `port`.
async fn channels_for(client: &ChannelzClient, port: u16) -> Vec<i64> {
    let resp = client
        .get_top_channels(Request::new(top_channels()))
        .await
        .expect("get_top_channels");
    let needle = format!(":{port}");
    resp.get_ref()
        .channel()
        .into_iter()
        .filter(|c| c.data().target().to_str().unwrap_or("").contains(&needle))
        .map(|c| c.r#ref().channel_id())
        .collect()
}

/// Server ids with a socket touching `port` (local or remote TCP).
async fn servers_for(client: &ChannelzClient, port: u16) -> Vec<i64> {
    let resp = client
        .get_servers(Request::new(servers()))
        .await
        .expect("get_servers");
    let mut out = Vec::new();
    for server in resp.get_ref().server().into_iter() {
        let id = server.r#ref().server_id();
        let mut q = GetServerSocketsRequest::new();
        q.set_server_id(id);
        q.set_start_socket_id(0);
        q.set_max_results(1000);
        // Another test's server can drop mid-scan; skip the gone.
        let Ok(sockets) = client.get_server_sockets(Request::new(q)).await else {
            continue;
        };
        for socket in sockets.get_ref().socket_ref().into_iter() {
            let mut q = GetSocketRequest::new();
            q.set_socket_id(socket.socket_id());
            let Ok(sock) = client.get_socket(Request::new(q)).await else {
                continue;
            };
            let socket = sock.get_ref().socket();
            for addr in [socket.local(), socket.remote()] {
                if let Some(tcp) = addr.tcpip_address_opt() {
                    if tcp.port() == i32::from(port) {
                        out.push(id);
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

async fn socket_ids_of_server(client: &ChannelzClient, server_id: i64) -> Vec<i64> {
    let mut q = GetServerSocketsRequest::new();
    q.set_server_id(server_id);
    q.set_start_socket_id(0);
    q.set_max_results(1000);
    let sockets = client
        .get_server_sockets(Request::new(q))
        .await
        .expect("get_server_sockets");
    sockets
        .get_ref()
        .socket_ref()
        .into_iter()
        .map(|s| s.socket_id())
        .collect()
}

async fn socket_data(client: &ChannelzClient, socket_id: i64) -> pbrs_grpc::channelz::SocketData {
    let mut q = GetSocketRequest::new();
    q.set_socket_id(socket_id);
    client
        .get_socket(Request::new(q))
        .await
        .expect("get_socket")
        .into_inner()
        .socket()
        .data()
        .clone()
}

#[tokio::test]
async fn channelz_unary_counters_round_trip() {
    let (addr, _greeter) = serve_under_test().await;
    let (qaddr, _query) = serve_query().await;
    let greeter = greeter_client(addr).await;
    let channelz = query_client(qaddr).await;

    for name in ["ada", "bob", "cam"] {
        let resp = greeter
            .say_hello(Request::new(req(name)))
            .await
            .expect("unary");
        assert_eq!(name_of(&resp.into_inner()), name);
    }

    // Client channel: three calls started and succeeded.
    let channels = channels_for(&channelz, addr.port()).await;
    assert_eq!(channels.len(), 1, "one channel dials the test server");
    let mut q = GetChannelRequest::new();
    q.set_channel_id(channels[0]);
    let channel = channelz
        .get_channel(Request::new(q))
        .await
        .expect("get_channel");
    let data = channel.get_ref().channel().data();
    assert_eq!(data.calls_started(), 3);
    assert_eq!(data.calls_succeeded(), 3);
    assert_eq!(data.calls_failed(), 0);
    // Client socket: three streams, one message each way per call.
    let socket_refs: Vec<_> = channel
        .get_ref()
        .channel()
        .socket_ref()
        .into_iter()
        .collect();
    assert_eq!(socket_refs.len(), 1);
    let socket = socket_data(&channelz, socket_refs[0].socket_id()).await;
    assert_eq!(socket.streams_started(), 3);
    assert_eq!(socket.streams_succeeded(), 3);
    assert_eq!(socket.streams_failed(), 0);
    assert_eq!(socket.messages_sent(), 3);
    assert_eq!(socket.messages_received(), 3);

    // Server: three calls, and the accepted socket mirrors the client.
    let servers = servers_for(&channelz, addr.port()).await;
    assert_eq!(servers.len(), 1, "one server holds the test port");
    let mut q = GetServerRequest::new();
    q.set_server_id(servers[0]);
    let server = channelz
        .get_server(Request::new(q))
        .await
        .expect("get_server");
    let data = server.get_ref().server().data();
    assert_eq!(data.calls_started(), 3);
    assert_eq!(data.calls_succeeded(), 3);
    assert_eq!(data.calls_failed(), 0);
    assert_ne!(
        server
            .get_ref()
            .server()
            .listen_socket()
            .into_iter()
            .count(),
        0,
        "server lists its listen socket"
    );
    let mut accepted = None;
    for socket_id in socket_ids_of_server(&channelz, servers[0]).await {
        let data = socket_data(&channelz, socket_id).await;
        if data.streams_started() == 3 {
            accepted = Some(data);
        }
    }
    let accepted = accepted.expect("accepted socket carries the streams");
    assert_eq!(accepted.streams_succeeded(), 3);
    assert_eq!(accepted.streams_failed(), 0);
    assert_eq!(accepted.messages_received(), 3);
    assert_eq!(accepted.messages_sent(), 3);
}

#[tokio::test]
async fn channelz_streaming_counters_round_trip() {
    let (addr, _greeter) = serve_under_test().await;
    let (qaddr, _query) = serve_query().await;
    let greeter = greeter_client(addr).await;
    let channelz = query_client(qaddr).await;

    // Server-streaming: 1 request in, 2 replies out.
    let resp = greeter
        .server_hello(Request::new(req("ada,bob")))
        .await
        .expect("server-stream");
    let mut inbound = resp.into_inner();
    let mut replies = Vec::new();
    while let Some(msg) = inbound.message().await.expect("msg") {
        replies.push(name_of(&msg));
    }
    assert_eq!(replies, ["ada", "bob"]);
    drop(inbound);

    // Client-streaming: 2 requests in, 1 reply out.
    let (tx, call) = greeter.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.send(req("bob")).await.expect("send");
    tx.close();
    let resp = call.await.expect("client-stream");
    assert_eq!(name_of(&resp.into_inner()), "ada,bob");

    // Bidi: 2 requests in, 2 replies out.
    let (tx, call) = greeter.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.send(req("bob")).await.expect("send");
    tx.close();
    let resp = call.await.expect("bidi");
    let mut inbound = resp.into_inner();
    let mut replies = Vec::new();
    while let Some(msg) = inbound.message().await.expect("msg") {
        replies.push(name_of(&msg));
    }
    assert_eq!(replies, ["ada", "bob"]);
    drop(inbound);

    // Server socket: 3 streams, 5 messages each way.
    let servers = servers_for(&channelz, addr.port()).await;
    assert_eq!(servers.len(), 1);
    let mut q = GetServerRequest::new();
    q.set_server_id(servers[0]);
    let server = channelz
        .get_server(Request::new(q))
        .await
        .expect("get_server");
    assert_eq!(server.get_ref().server().data().calls_started(), 3);
    assert_eq!(server.get_ref().server().data().calls_succeeded(), 3);
    let mut accepted = None;
    for socket_id in socket_ids_of_server(&channelz, servers[0]).await {
        let data = socket_data(&channelz, socket_id).await;
        if data.streams_started() == 3 {
            accepted = Some(data);
        }
    }
    let accepted = accepted.expect("accepted socket carries the streams");
    assert_eq!(accepted.streams_succeeded(), 3);
    assert_eq!(accepted.messages_received(), 5);
    assert_eq!(accepted.messages_sent(), 5);

    // Client socket mirrors it.
    let channels = channels_for(&channelz, addr.port()).await;
    assert_eq!(channels.len(), 1);
    let mut q = GetChannelRequest::new();
    q.set_channel_id(channels[0]);
    let channel = channelz
        .get_channel(Request::new(q))
        .await
        .expect("get_channel");
    let socket_refs: Vec<_> = channel
        .get_ref()
        .channel()
        .socket_ref()
        .into_iter()
        .collect();
    assert_eq!(socket_refs.len(), 1);
    let socket = socket_data(&channelz, socket_refs[0].socket_id()).await;
    assert_eq!(socket.streams_started(), 3);
    assert_eq!(socket.streams_succeeded(), 3);
    assert_eq!(socket.messages_sent(), 5);
    assert_eq!(socket.messages_received(), 5);
}

#[tokio::test]
async fn channelz_churn_does_not_leak() {
    let (addr, _greeter) = serve_under_test().await;
    let (qaddr, _query) = serve_query().await;
    let channelz = query_client(qaddr).await;

    // Baseline: nothing dials this fresh port yet.
    assert!(channels_for(&channelz, addr.port()).await.is_empty());

    // Churn 25 channels; each makes one call so its socket registers.
    let mut held = Vec::new();
    for _ in 0..25 {
        let client = greeter_client(addr).await;
        client
            .say_hello(Request::new(req("ada")))
            .await
            .expect("unary");
        held.push(client);
    }
    let seen: Vec<i64> = channels_for(&channelz, addr.port()).await;
    assert_eq!(seen.len(), 25, "all churned channels register");

    // Dropping the clients unregisters every channel: the ids go
    // NOT_FOUND and a later channel never reuses an id. Poll until
    // the registry releases them (connection teardown is async).
    drop(held);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if channels_for(&channelz, addr.port()).await.is_empty() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "churned channels must unregister"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    for id in &seen {
        let mut q = GetChannelRequest::new();
        q.set_channel_id(*id);
        let err = channelz
            .get_channel(Request::new(q))
            .await
            .expect_err("dropped channel must be NOT_FOUND");
        assert_eq!(err.code(), Code::NotFound);
    }
    // Ids are never reused: the next channel sorts after every id seen.
    let max_seen = seen.into_iter().max().expect("saw channels");
    let _fresh = greeter_client(addr).await;
    let fresh = channels_for(&channelz, addr.port()).await;
    assert_eq!(fresh.len(), 1);
    assert!(
        fresh[0] > max_seen,
        "channel id {} must exceed every dropped id (max {max_seen})",
        fresh[0]
    );
}

#[tokio::test]
async fn channelz_pagination_and_errors() {
    let (addr, _greeter) = serve_under_test().await;
    let (qaddr, _query) = serve_query().await;
    let greeter = greeter_client(addr).await;
    let channelz = query_client(qaddr).await;
    greeter
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");

    // Negative max_results is INVALID_ARGUMENT on every paged RPC.
    let mut q = GetTopChannelsRequest::new();
    q.set_max_results(-1);
    let err = channelz
        .get_top_channels(Request::new(q))
        .await
        .expect_err("negative page must fail");
    assert_eq!(err.code(), Code::InvalidArgument);
    let mut q = GetServersRequest::new();
    q.set_max_results(-1);
    let err = channelz
        .get_servers(Request::new(q))
        .await
        .expect_err("negative page must fail");
    assert_eq!(err.code(), Code::InvalidArgument);

    // Unknown ids are NOT_FOUND on every lookup RPC.
    let mut q = GetChannelRequest::new();
    q.set_channel_id(i64::MAX);
    let err = channelz
        .get_channel(Request::new(q))
        .await
        .expect_err("unknown channel must be NOT_FOUND");
    assert_eq!(err.code(), Code::NotFound);
    let mut q = GetServerRequest::new();
    q.set_server_id(i64::MAX);
    let err = channelz
        .get_server(Request::new(q))
        .await
        .expect_err("unknown server must be NOT_FOUND");
    assert_eq!(err.code(), Code::NotFound);
    let mut q = GetSubchannelRequest::new();
    q.set_subchannel_id(i64::MAX);
    let err = channelz
        .get_subchannel(Request::new(q))
        .await
        .expect_err("unknown subchannel must be NOT_FOUND");
    assert_eq!(err.code(), Code::NotFound);
    let mut q = GetSocketRequest::new();
    q.set_socket_id(i64::MAX);
    let err = channelz
        .get_socket(Request::new(q))
        .await
        .expect_err("unknown socket must be NOT_FOUND");
    assert_eq!(err.code(), Code::NotFound);

    // Starting past the end returns empty with end set.
    let mut q = GetTopChannelsRequest::new();
    q.set_start_channel_id(i64::MAX);
    let resp = channelz
        .get_top_channels(Request::new(q))
        .await
        .expect("past-end page");
    assert!(resp.get_ref().channel().into_iter().count() == 0);
    assert!(resp.get_ref().end());

    // A page of one walks the channel list without overlap or gap.
    let mut seen = Vec::new();
    let mut start = 0i64;
    loop {
        let mut q = GetTopChannelsRequest::new();
        q.set_start_channel_id(start);
        q.set_max_results(1);
        let resp = channelz
            .get_top_channels(Request::new(q))
            .await
            .expect("page");
        let page: Vec<i64> = resp
            .get_ref()
            .channel()
            .into_iter()
            .map(|c| c.r#ref().channel_id())
            .collect();
        assert!(page.len() <= 1);
        let end = resp.get_ref().end();
        if let Some(id) = page.first() {
            assert!(!seen.contains(id), "paged channel {id} must not repeat");
            seen.push(*id);
            start = id + 1;
        }
        if end {
            break;
        }
    }
    assert!(!seen.is_empty(), "at least our channels page through");
}

#[tokio::test]
async fn channelz_traces_are_present() {
    let (addr, _greeter) = serve_under_test().await;
    let (qaddr, _query) = serve_query().await;
    let greeter = greeter_client(addr).await;
    let channelz = query_client(qaddr).await;
    greeter
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");

    let channels = channels_for(&channelz, addr.port()).await;
    assert_eq!(channels.len(), 1);
    let mut q = GetChannelRequest::new();
    q.set_channel_id(channels[0]);
    let channel = channelz
        .get_channel(Request::new(q))
        .await
        .expect("get_channel");
    let trace = channel.get_ref().channel().data().trace();
    assert!(
        trace.num_events_logged() >= 1,
        "channel trace logs its creation"
    );
    assert!(
        trace.creation_timestamp_opt().is_some(),
        "channel trace carries a creation timestamp"
    );

    let servers = servers_for(&channelz, addr.port()).await;
    assert_eq!(servers.len(), 1);
    let mut q = GetServerRequest::new();
    q.set_server_id(servers[0]);
    let server = channelz
        .get_server(Request::new(q))
        .await
        .expect("get_server");
    let trace = server.get_ref().server().data().trace();
    assert!(
        trace.num_events_logged() >= 1,
        "server trace logs its creation"
    );
}
