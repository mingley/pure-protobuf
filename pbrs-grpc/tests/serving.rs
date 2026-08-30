//! Serving surface exercised the way a third party would: a hand-written
//! [`Service`], a [`Router`] hosting two services, and graceful drain.
//!
//! This file deliberately avoids the generated `GreeterServer` wrapper for the
//! hand-written cases, because the point is that the public API is enough.

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

use common::{name_of, name_of_request, req, serve_at, spawn_greeter, until_ok, Echo};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{
    Call, Channel, ChannelConfig, ClientTls, Code, ConnectionInfo, Empty, Identity, Incoming,
    InteropTestService, MessageLimits, Outgoing, Payload, PeerCred, PeerIdentity, Request,
    Response, ResponseParameters, Router, Rpc, Server, ServerConfig, ServerTls, Service,
    ServiceExt, SimpleRequest, SimpleResponse, Status, StreamingInputCallRequest,
    StreamingInputCallResponse, StreamingOutputCallRequest, StreamingOutputCallResponse,
    TestService, TestServiceClient, TestServiceServer,
};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

const CA: &str = include_str!("tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("tls_data/server.crt");
const SERVER_KEY: &str = include_str!("tls_data/server.key");
const CLIENT_CERT: &str = include_str!("tls_data/client.crt");
const CLIENT_KEY: &str = include_str!("tls_data/client.key");

fn server_identity() -> Identity {
    Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity")
}

fn client_identity() -> Identity {
    Identity::from_pem(CLIENT_CERT, CLIENT_KEY).expect("client identity")
}

/// A service written without any generated code, mounted on the public API.
struct Reverser {
    seen: Arc<AtomicUsize>,
    /// When `Some`, every call must expose this leaf on `Rpc` and `Request`.
    want_leaf: Option<Arc<[u8]>>,
}

impl Reverser {
    fn new(seen: Arc<AtomicUsize>) -> Self {
        Self {
            seen,
            want_leaf: None,
        }
    }

    fn mtls(seen: Arc<AtomicUsize>, leaf: impl Into<Arc<[u8]>>) -> Self {
        Self {
            seen,
            want_leaf: Some(leaf.into()),
        }
    }
}

fn reversed_hello(name: &str) -> HelloReply {
    let mut reply = HelloReply::new();
    reply.set_message(name.chars().rev().collect::<String>());
    reply
}

fn reversed_stream(name: String) -> Response<pbrs_grpc::Streaming<HelloReply>> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        tx.send(reversed_hello(&name)).await.ok();
    }));
    Response::new(stream)
}

fn check_peer<T>(
    request: &Request<T>,
    peer: Option<SocketAddr>,
    local: Option<SocketAddr>,
    tls_id: Option<&PeerIdentity>,
    want_leaf: Option<&[u8]>,
) -> Result<(), Status> {
    let rpc_leaf = tls_id.and_then(PeerIdentity::leaf);
    let req_leaf = request.peer_identity().and_then(PeerIdentity::leaf);
    match (want_leaf, rpc_leaf, req_leaf) {
        (None, None, None) => {}
        (Some(want), Some(rpc_leaf), Some(req_leaf)) if rpc_leaf == want && req_leaf == want => {}
        _ => {
            return Err(Status::internal(
                "peer_identity did not match the transport",
            ));
        }
    }
    if let Some(cred) = request.peer_cred() {
        if peer.is_some()
            || local.is_some()
            || request.local_addr().is_some()
            || request.remote_addr().is_some()
        {
            return Err(Status::internal("unix has no std::net::SocketAddr"));
        }
        if cred.pid() != Some(std::process::id()) {
            return Err(Status::internal(format!(
                "unix pid {:?} want {}",
                cred.pid(),
                std::process::id()
            )));
        }
        return Ok(());
    }
    if peer.is_none() && local.is_none() {
        if request.local_addr().is_some() || request.remote_addr().is_some() {
            return Err(Status::internal("from_io must not invent TCP addrs"));
        }
        return Ok(());
    }
    if peer.is_none() {
        return Err(Status::internal("expected a peer address"));
    }
    if local.is_none() || request.local_addr() != local {
        return Err(Status::internal("expected a local address"));
    }
    Ok(())
}

impl Service for Reverser {
    const NAME: &'static str = "demo.Reverser";

    async fn call(&self, rpc: Rpc) {
        let seen = Arc::clone(&self.seen);
        let want_leaf = self.want_leaf.clone();
        let peer = rpc.remote_addr();
        let local = rpc.local_addr();
        let tls_id = rpc.peer_identity().cloned();
        match rpc.method() {
            "Reverse" => {
                rpc.unary(move |request: Request<HelloRequest>| async move {
                    seen.fetch_add(1, Ordering::Relaxed);
                    check_peer(&request, peer, local, tls_id.as_ref(), want_leaf.as_deref())?;
                    Ok(Response::new(reversed_hello(&name_of_request(
                        request.get_ref(),
                    ))))
                })
                .await;
            }
            "Server" => {
                rpc.server_streaming(move |request: Request<HelloRequest>| async move {
                    seen.fetch_add(1, Ordering::Relaxed);
                    check_peer(&request, peer, local, tls_id.as_ref(), want_leaf.as_deref())?;
                    Ok(reversed_stream(name_of_request(request.get_ref())))
                })
                .await;
            }
            "Client" => {
                rpc.client_streaming(
                    move |request: Request<pbrs_grpc::Streaming<HelloRequest>>| async move {
                        seen.fetch_add(1, Ordering::Relaxed);
                        check_peer(&request, peer, local, tls_id.as_ref(), want_leaf.as_deref())?;
                        let mut inbound = request.into_inner();
                        let msg = inbound
                            .message()
                            .await?
                            .ok_or_else(|| Status::internal("empty stream"))?;
                        Ok(Response::new(reversed_hello(&name_of_request(&msg))))
                    },
                )
                .await;
            }
            "Bidi" => {
                rpc.bidi_streaming(
                    move |request: Request<pbrs_grpc::Streaming<HelloRequest>>| async move {
                        seen.fetch_add(1, Ordering::Relaxed);
                        check_peer(&request, peer, local, tls_id.as_ref(), want_leaf.as_deref())?;
                        let mut inbound = request.into_inner();
                        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
                        drop(tokio::spawn(async move {
                            loop {
                                match inbound.message().await {
                                    Ok(Some(msg)) => {
                                        if tx
                                            .send(reversed_hello(&name_of_request(&msg)))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    Ok(None) => break,
                                    Err(status) => {
                                        tx.fail(status).await;
                                        break;
                                    }
                                }
                            }
                        }));
                        Ok(Response::new(stream))
                    },
                )
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

async fn bind() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    (addr, listener)
}

async fn channel(addr: SocketAddr) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}");
}

async fn channel_cfg(addr: SocketAddr, cfg: ChannelConfig) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_with(addr, cfg).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}");
}

async fn tls_channel_with(addr: SocketAddr, tls: ClientTls) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_tls(addr, tls.clone()).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}");
}

async fn tls_channel_cfg(addr: SocketAddr, tls: ClientTls, cfg: ChannelConfig) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_tls_with(addr, cfg, tls.clone()).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}");
}

async fn tls_channel(addr: SocketAddr) -> Channel {
    tls_channel_with(addr, ClientTls::ca("localhost", CA).expect("client tls")).await
}

async fn serve_tls_at(
    addr: SocketAddr,
    tls: ServerTls,
) -> Result<tokio::task::JoinHandle<()>, Status> {
    let mut last = Status::unavailable("bind");
    for _ in 0..100 {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let handle = tokio::spawn(async move {
                    GreeterServer::new(Echo)
                        .serve_tls_with_shutdown(listener, std::future::pending(), tls)
                        .await
                        .ok();
                });
                return Ok(handle);
            }
            Err(e) => {
                last = Status::unavailable(e.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(last)
}

async fn serve_test_at(addr: SocketAddr) -> Result<tokio::task::JoinHandle<()>, Status> {
    let mut last = Status::unavailable("bind");
    for _ in 0..100 {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let handle = tokio::spawn(async move {
                    TestServiceServer::new(InteropTestService)
                        .serve_listener(listener)
                        .await
                        .ok();
                });
                return Ok(handle);
            }
            Err(e) => {
                last = Status::unavailable(e.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(last)
}

async fn serve_test_tls_at(
    addr: SocketAddr,
    tls: ServerTls,
) -> Result<tokio::task::JoinHandle<()>, Status> {
    let mut last = Status::unavailable("bind");
    for _ in 0..100 {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let handle = tokio::spawn(async move {
                    TestServiceServer::new(InteropTestService)
                        .serve_tls_with_shutdown(listener, std::future::pending(), tls)
                        .await
                        .ok();
                });
                return Ok(handle);
            }
            Err(e) => {
                last = Status::unavailable(e.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(last)
}

async fn serve_reverser_at(addr: SocketAddr) -> Result<tokio::task::JoinHandle<()>, Status> {
    let mut last = Status::unavailable("bind");
    for _ in 0..100 {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let service = Reverser::new(Arc::new(AtomicUsize::new(0)));
                let handle = tokio::spawn(async move {
                    Server::new(service).serve_listener(listener).await.ok();
                });
                return Ok(handle);
            }
            Err(e) => {
                last = Status::unavailable(e.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(last)
}

async fn serve_reverser_tls_at(
    addr: SocketAddr,
    tls: ServerTls,
) -> Result<tokio::task::JoinHandle<()>, Status> {
    let mut last = Status::unavailable("bind");
    for _ in 0..100 {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let service = Reverser::new(Arc::new(AtomicUsize::new(0)));
                let handle = tokio::spawn(async move {
                    Server::new(service)
                        .serve_tls_with_shutdown(listener, std::future::pending(), tls)
                        .await
                        .ok();
                });
                return Ok(handle);
            }
            Err(e) => {
                last = Status::unavailable(e.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(last)
}

async fn serve_reverser_mtls_at(
    addr: SocketAddr,
    tls: ServerTls,
) -> Result<tokio::task::JoinHandle<()>, Status> {
    let mut last = Status::unavailable("bind");
    for _ in 0..100 {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let identity = client_identity();
                let leaf = identity.certificates().next().expect("leaf");
                let service = Reverser::mtls(Arc::new(AtomicUsize::new(0)), leaf);
                let handle = tokio::spawn(async move {
                    Server::new(service)
                        .serve_tls_with_shutdown(listener, std::future::pending(), tls)
                        .await
                        .ok();
                });
                return Ok(handle);
            }
            Err(e) => {
                last = Status::unavailable(e.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(last)
}

async fn assert_hand_written_serves(channel: Channel, seen: &AtomicUsize) {
    echo_reverser_every_shape(&channel).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    assert_unimplemented_path(&channel, "/demo.Reverser/Nope").await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
}

#[tokio::test]
async fn a_hand_written_service_serves_without_generated_code() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });
    assert_hand_written_serves(channel(addr).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_hand_written_service_serves_without_generated_code() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_hand_written_serves(tls_channel(addr).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_hand_written_service_serves_without_generated_code() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    );
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_hand_written_serves(tls_channel_with(addr, client_tls).await, &seen).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_hand_written_service_serves_without_generated_code() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service).serve_unix(sock).await.ok();
    });
    assert_hand_written_serves(unix_channel(&path).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_hand_written_service_serves_without_generated_code() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let server = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    assert_hand_written_serves(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
        &seen,
    )
    .await;
    server.abort();
}

#[test]
fn channel_call_apis_document_hand_written_services() {
    let src = include_str!("../src/client.rs");
    let needle = "A hand-written [`crate::Service`] is first-class on this path;";
    assert_eq!(
        src.matches(needle).count(),
        4,
        "Channel unary / server_streaming / client_streaming / bidi must name hand-written Service as first-class"
    );
    let caps = "[`Self::max_encoding_message_size`] / [`Self::max_decoding_message_size`]\n    /// fail this path as [`Code::ResourceExhausted`], including over TLS, mTLS,\n    /// Unix, and [`Self::from_io`]. Distinct from generated client wrappers.";
    assert_eq!(
        src.matches(caps).count(),
        4,
        "Channel unary / server_streaming / client_streaming / bidi must name message caps on every transport"
    );
    assert!(
        src.contains("inbound cap. Applies to every call shape."),
        "Channel::connect must name every call shape"
    );
    assert!(
        src.contains("Build a channel that dials on the first RPC instead of now.\n    /// Applies to every call shape."),
        "Channel::connect_lazy must name every call shape"
    );
    assert!(
        src.contains("Dial `target` over TLS with `config`. Applies to every call shape."),
        "Channel::connect_tls_with must name every call shape"
    );
    assert!(
        src.contains(
            "[`Self::connect_lazy`] with `config`. Each slot dials when an RPC first\n    /// lands on it, not all at once. Applies to every call shape."
        ),
        "Channel::connect_lazy_with must name every call shape"
    );
    assert!(
        src.contains("[`Self::connect_lazy`] over TLS. Applies to every call shape."),
        "Channel::connect_tls_lazy must name every call shape"
    );
    assert!(
        src.contains("[`Self::connect_lazy_with`] over TLS. Applies to every call shape."),
        "Channel::connect_tls_lazy_with must name every call shape"
    );
    assert!(
        src.contains("Dial `target` with `config`. Applies to every call shape."),
        "Channel::connect_with must name every call shape"
    );
    assert!(
        src.contains("not a `unix://` URI.\n    /// Applies to every call shape."),
        "Channel::connect_unix must name every call shape"
    );
    assert!(
        src.contains("[`Self::connect_unix`] with `config`. Applies to every call shape."),
        "Channel::connect_unix_with must name every call shape"
    );
    assert!(
        src.contains(
            "[`Self::connect_unix`] that dials on the first RPC instead of now.\n    /// Applies to every call shape."
        ),
        "Channel::connect_unix_lazy must name every call shape"
    );
    assert!(
        src.contains("[`Self::connect_unix_lazy`] with `config`. Applies to every call shape."),
        "Channel::connect_unix_lazy_with must name every call shape"
    );
    assert!(
        src.contains("call [`Self::https_scheme`].\n    /// Applies to every call shape."),
        "Channel::from_io must name every call shape"
    );
    assert!(
        src.contains(
            "next RPC on that slot dials again, including over TLS, mTLS, and Unix.\n/// [`Self::from_io`] cannot redial."
        ),
        "Channel rustdoc must name redial on TLS, mTLS, and Unix"
    );
    assert!(
        src.contains(
            "do not keep it. The next RPC of every call shape redials, including over\n/// TLS, mTLS, and Unix. [`Self::from_io`] cannot redial and fails with"
        ),
        "Channel rustdoc must name client idle redial on TLS, mTLS, and Unix"
    );
    assert!(
        src.contains(
            "dropping the last `Channel`\n/// clone after headers still lets you read the stream to the end, including\n/// over TLS, mTLS, Unix, and [`Self::from_io`]."
        ),
        "Channel rustdoc must name drop-Channel live stream on every transport"
    );
    assert!(
        src.contains(
            "Cap inbound messages at `limit` bytes. Default 4 MiB.\n    /// Applies to every call shape, including over TLS, mTLS, Unix, and\n    /// [`Self::from_io`]."
        ),
        "Channel::max_decoding_message_size must name every transport"
    );
    assert!(
        src.contains(
            "Cap outbound messages at `limit` bytes. Default unlimited.\n    /// Applies to every call shape, including over TLS, mTLS, Unix, and\n    /// [`Self::from_io`]."
        ),
        "Channel::max_encoding_message_size must name every transport"
    );
    assert!(
        src.contains(
            "Distinct from [`Self::max_encoding_message_size`] /\n    /// [`Self::max_decoding_message_size`]. Oversize is\n    /// [`Code::ResourceExhausted`], including over TLS, mTLS, Unix, and\n    /// [`Self::from_io`]."
        ),
        "Channel::message_limits must name the combined setter on every transport"
    );
    let stream = include_str!("../src/stream.rs");
    assert!(
        stream.contains(
            "dropping the client after headers still lets you read to the end,\n/// including over TLS, mTLS, Unix, and [`crate::Channel::from_io`]."
        ),
        "Streaming rustdoc must name drop-Channel live stream on every transport"
    );
    assert!(
        stream.contains(
            "returns [`crate::Code::DeadlineExceeded`], not `Ok(None)`, including\n/// over TLS, mTLS, Unix, and [`crate::Channel::from_io`]."
        ),
        "Streaming rustdoc must name expired deadline is not a clean end on every transport"
    );
    assert!(
        stream.contains(
            "trailer must not appear as a header, including over TLS, mTLS, Unix,\n    /// and [`crate::Channel::from_io`]."
        ),
        "Streaming::trailers rustdoc must name -bin trailers on every transport"
    );
    assert!(
        src.contains(
            "OK-path custom trailers land on [`crate::Response::trailers`]; a `-bin`\n    /// trailer must not appear as a header, including over TLS, mTLS, Unix,\n    /// and [`Self::from_io`]."
        ),
        "Channel unary and client-streaming rustdoc must name OK-path -bin trailers on every transport"
    );
    assert!(
        src.contains(
            "[`crate::Streaming::trailers`] waits for end-of-stream, including when\n    /// called before draining messages. A non-OK trailing `grpc-status` is\n    /// `Err`. A `-bin` trailer must not appear as a header, including over\n    /// TLS, mTLS, Unix, and [`Self::from_io`]."
        ),
        "Channel server-streaming and bidi rustdoc must name Streaming::trailers on every transport"
    );
    assert!(
        src.contains(
            "RPC as [`Code::Unavailable`] (including over TLS, mTLS, and Unix), or\n    /// waits until the deadline if that RPC"
        ),
        "Channel::connect_lazy must name fail-fast on TLS, mTLS, and Unix"
    );
    assert!(
        src.contains("[`Self::from_io`] with `config`. Applies to every call shape."),
        "Channel::from_io_with must name every call shape"
    );
    assert!(
        src.contains(
            "[`ChannelConfig::connections`] is forced to 1: one duplex is one\n    /// HTTP/2 connection."
        ),
        "Channel::from_io_with must name that pooling is forced to one duplex"
    );
    assert!(
        src.contains(
            "[`ChannelConfig::connections`] opens that many TLS sockets (including\n    /// mTLS); all of them must succeed. [`Self::from_io`] cannot pool."
        ),
        "Channel::connect_tls_with must name TLS pooling"
    );
    assert!(
        src.contains(
            "[`ChannelConfig::connections`] opens that many Unix sockets; all of\n    /// them must succeed. [`Self::from_io`] cannot pool."
        ),
        "Channel::connect_unix_with must name Unix pooling"
    );
    assert!(
        src.contains(
            "TLS (including mTLS) pooling is [`Self::connect_tls_with`] plus\n    /// [`ChannelConfig::connections`]; Unix is [`Self::connect_unix_with`].\n    /// [`Self::from_io`] cannot pool."
        ),
        "Channel::connect_pool must name TLS and Unix pooling"
    );
    assert!(
        src.contains("The configuration in effect. Applies to every call shape."),
        "Channel::config must name every call shape"
    );
    assert!(
        src.contains(
            "Run `interceptor` on every outbound RPC before the stream opens.\n    /// Applies to every call shape."
        ),
        "Channel::intercept must name every call shape"
    );
    assert!(
        src.contains(
            "visible even after `clear_*` opts out of\n    /// the already-applied default."
        ),
        "Channel::intercept must name overlays after clear_*"
    );
    assert!(
        src.contains(
            "[`crate::Outgoing::clear_compress`] then\n    /// [`crate::Outgoing::set_compress`] from [`Self::compresses_outbound`]\n    /// reapplies channel gzip on every call shape."
        ),
        "Channel::intercept must name gzip reapply after clear"
    );
    assert!(
        src.contains(
            "stamps [`crate::StreamSender::compress`] on client-streaming and bidi\n    /// request streams."
        ),
        "Channel::intercept must name StreamSender gzip stamp on request streams"
    );
    assert!(
        src.contains(
            "[`crate::Outgoing::clear_timeout`] opts out of the\n    /// channel timeout on every call shape."
        ),
        "Channel::intercept must name clear_timeout opt-out"
    );
    assert!(
        src.contains(
            "Interceptors run after this fill and can still set or\n    /// [`crate::Outgoing::clear_timeout`]."
        ),
        "Channel::timeout must name interceptor clear_timeout"
    );
    assert!(
        src.contains(
            "Default per-RPC deadline when the request omits one. Applies to every\n    /// call shape, including over TLS, mTLS, Unix, and [`Self::from_io`]."
        ),
        "Channel::timeout must name every transport"
    );
    assert!(
        src.contains(
            "Taken from the [`Target`] used to dial. A [`SocketAddr`] is that\n    /// address"
        ),
        "Channel::authority must name Target, not TLS SNI"
    );
    let intercept = include_str!("../src/interceptor.rs");
    assert!(
        intercept.contains(
            "[`crate::Outgoing::clear_timeout`] opts out of the channel timeout\n/// on every call shape."
        ),
        "ClientInterceptor rustdoc must name clear_timeout opt-out"
    );
    assert!(
        intercept.contains(
            "stamps [`crate::StreamSender::compress`] on client-streaming and bidi\n/// request streams."
        ),
        "ClientInterceptor rustdoc must name StreamSender gzip stamp"
    );
    assert!(
        intercept.contains(
            "overwrite a hop with [`crate::Metadata::set`] / [`crate::Metadata::set_bin`];\n/// those mutations reach the\n/// handler on h2c, TLS including mTLS, Unix, and [`crate::Channel::from_io`]"
        ),
        "Interceptor rustdoc must name set/remove/retain on every transport"
    );
    assert!(
        intercept.contains(
            "[`crate::Request::extensions`] / [`crate::Parts::extensions`] (including\n/// over TLS, mTLS, Unix, and [`crate::Channel::from_io`])"
        ),
        "Interceptor rustdoc must name typed extensions on Request/Parts every transport"
    );
    assert!(
        intercept.contains(
            "A single interceptor\n/// still rejects before the handler on every call shape, including over TLS,\n/// mTLS, Unix, and [`crate::Channel::from_io`]."
        ),
        "Interceptor rustdoc must name a single ServiceExt intercept reject on every transport"
    );
    assert!(
        intercept.contains(
            "A single interceptor\n    /// still rejects before the handler on every call shape, including over\n    /// TLS, mTLS, Unix, and [`crate::Channel::from_io`]."
        ),
        "ServiceExt::intercept rustdoc must name a single intercept reject on every transport"
    );
    assert!(
        src.contains(
            "Applies to client-streaming and bidi request streams opened from this\n    /// clone."
        ),
        "Channel::stream_buffer must name the streaming shapes it queues"
    );
    assert!(
        src.contains(
            "Applies to every call shape, including over TLS, mTLS, Unix, and\n    /// [`Self::from_io`]. Inserting `user-agent` into request metadata cannot\n    /// replace this value on those transports."
        ),
        "Channel::user_agent must name every transport and that metadata cannot override"
    );
    assert!(
        src.contains(
            "Applies to every call shape, including over TLS, mTLS,\n    /// Unix, and [`Self::from_io`]."
        ),
        "Channel::send_compressed must name every transport"
    );
    assert!(
        src.contains("Interceptors run after this fill and can still set\n    /// or clear it."),
        "Channel::wait_for_ready must name interceptor set/clear"
    );
    assert!(
        src.contains(
            "(`cancel_after_begin`) is [`crate::Code::Cancelled`], not OK from a\n    /// half-close: hold the [`StreamSender`] until the [`Call`] settles,\n    /// including over TLS, mTLS, Unix, and [`Self::from_io`]."
        ),
        "Channel::client_streaming must name cancel_after_begin on every transport"
    );
    let status = include_str!("../src/status.rs");
    assert!(
        status.contains(
            "Raw bytes still round-trip on every call shape, including over TLS,\n    /// mTLS, Unix, and [`crate::Channel::from_io`]. They do not appear as a\n    /// `grpc-status-details-bin` metadata key."
        ),
        "Status::details must name raw bytes on every transport"
    );
    assert!(
        status.contains(
            "A non-empty blob is `grpc-status-details-bin` on the wire for every\n    /// call shape, including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].\n    /// [`Self::details`] returns those bytes; they do not appear as a metadata\n    /// key."
        ),
        "Status::set_details must name the wire trailer on every transport"
    );
}

#[test]
fn official_interop_rustdoc_names_every_transport() {
    let testing = include_str!("../src/testing.rs");
    assert!(
        testing.contains(
            "Official uncompressed `_TEST_CASES` and the four gzip cases pass against
/// this server over TLS, mTLS, Unix, and [`crate::Server::serve_connection`]."
        ),
        "InteropTestService rustdoc must name official cases on every transport"
    );
    assert!(
        testing.contains(
            "A [`TestServiceClient`]\n//! `message_limits` is `RESOURCE_EXHAUSTED` on UnaryCall /\n//! StreamingOutputCall / StreamingInputCall / FullDuplexCall, including over\n//! TLS, mTLS, Unix, and [`crate::Channel::from_io`]. Distinct from wrapping\n//! `max_encoding_message_size` / `max_decoding_message_size`."
        ),
        "testing crate rustdoc must name TestServiceClient message_limits on every transport"
    );
    assert!(
        testing.contains(
            "[`TestServiceClient::connect_tls_with`] /\n//! [`TestServiceClient::connect_unix_with`] /\n//! [`TestServiceClient::from_io_with`] with\n//! [`crate::ChannelConfig::message_limits`] refuse the same oversize, distinct\n//! from wrapping a live client."
        ),
        "testing crate rustdoc must name dial-time ChannelConfig message_limits on every transport"
    );
    assert!(
        testing.contains(
            "[`TestServiceServer::max_header_list_size`]\n//! refuses oversize metadata on EmptyCall / StreamingOutputCall /\n//! StreamingInputCall / FullDuplexCall, including over TLS, mTLS, Unix, and\n//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter\n//! server."
        ),
        "testing crate rustdoc must name header-list flood on every TestService shape"
    );
    assert!(
        testing.contains(
            "[`TestServiceServer::max_frame_size`] still serves EmptyCall /
//! StreamingOutputCall / StreamingInputCall / FullDuplexCall at the HTTP/2
//! 16 KiB SETTINGS minimum, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter
//! server."
        ),
        "testing crate rustdoc must name max_frame_size still-serves on every TestService shape"
    );
    assert!(
        testing.contains(
            "A [`TestServiceClient`] pool larger than
//! [`TestServiceServer::max_concurrent_connections`] fails the whole dial as
//! `UNAVAILABLE` on TLS, mTLS, and Unix. [`TestServiceClient::from_io_with`]
//! cannot pool."
        ),
        "testing crate rustdoc must name pool-vs-cap UNAVAILABLE on TLS, mTLS, and Unix"
    );
    let cases = include_str!("../src/interop_cases.rs");
    assert!(
        cases.contains(
            "Applies to the call shapes that case uses, including over TLS, mTLS,
/// Unix, and [`crate::Channel::from_io`]."
        ),
        "run_case rustdoc must name every transport"
    );
}

#[test]
fn channel_config_connect_timeout_documents_every_call_shape() {
    let src = include_str!("../src/config.rs");
    assert!(
        src.contains(
            "This is a dial bound, not an RPC overlay. Every call shape uses the\n    /// same bound when the channel actually dials (eager `connect`, a lazy\n    /// first RPC, or a reconnect). Applies to every call shape once that\n    /// dial happens."
        ),
        "ChannelConfig::connect_timeout must name every call shape as a dial bound"
    );
    assert!(
        src.contains(
            "never speaks HTTP/2\n    /// (or never finishes TLS, including mTLS) fails with\n    /// [`crate::Code::Unavailable`] instead of hanging [`crate::Channel::connect`]\n    /// / [`crate::Channel::connect_tls`] / [`crate::Channel::connect_unix`] forever."
        ),
        "ChannelConfig::connect_timeout must name TLS, mTLS, and Unix hang"
    );
    assert!(
        src.contains(
            "Connection refused still fails immediately on those dialers; this bound is\n    /// for the hang, not the bounce."
        ),
        "ChannelConfig::connect_timeout must name refused-connect fail-fast on TLS and Unix"
    );
    assert!(
        src.contains(
            "shape redials, including over TLS, mTLS, and Unix, except on\n    /// [`crate::Channel::from_io`], which cannot redial and fails with"
        ),
        "ChannelConfig::max_connection_idle must name client idle redial on TLS, mTLS, and Unix"
    );
    assert!(
        src.contains(
            "Open `n` independent HTTP/2 connections and spread RPCs round-robin.\n    /// Applies to every call shape, including over TLS, mTLS, and Unix.\n    /// [`crate::Channel::from_io`] cannot pool: [`crate::Channel::from_io_with`]\n    /// forces `connections` to 1."
        ),
        "ChannelConfig::connections must name every call shape"
    );
    assert!(
        src.contains(
            "All of them must succeed: a pool larger than the server's\n    /// [`crate::Server::max_concurrent_connections`] fails the dial as\n    /// [`crate::Code::Unavailable`]."
        ),
        "ChannelConfig::connections must name pool-vs-cap UNAVAILABLE on every transport"
    );
    assert!(
        src.contains(
            "Applies to every call shape, including when set on\n    /// [`crate::Channel::connect_tls_with`] / [`crate::Channel::connect_unix_with`]\n    /// / [`crate::Channel::from_io_with`]. Distinct from wrapping a live\n    /// [`crate::Channel`] with [`crate::Channel::max_decoding_message_size`]."
        ),
        "ChannelConfig::max_decoding_message_size must name dial-time overlay on every transport"
    );
    assert!(
        src.contains(
            "Applies to every call shape, including when set on\n    /// [`crate::Channel::connect_tls_with`] / [`crate::Channel::connect_unix_with`]\n    /// / [`crate::Channel::from_io_with`]. Distinct from wrapping a live\n    /// [`crate::Channel`] with [`crate::Channel::max_encoding_message_size`]."
        ),
        "ChannelConfig::max_encoding_message_size must name dial-time overlay on every transport"
    );
    assert!(
        src.contains(
            "Dial-time overlay on [`crate::Channel::connect_tls_with`] /\n    /// [`crate::Channel::connect_unix_with`] / [`crate::Channel::from_io_with`].\n    /// Distinct from [`Self::max_encoding_message_size`] /\n    /// [`Self::max_decoding_message_size`]."
        ),
        "ChannelConfig::message_limits must name dial-time combined setter on every transport"
    );
    assert!(
        src.contains(
            "Oversize response headers or trailers are refused, including over TLS,\n    /// mTLS, Unix, and [`crate::Channel::from_io`]. Distinct from\n    /// [`ServerConfig::max_header_list_size`], which caps inbound request\n    /// metadata."
        ),
        "ChannelConfig::max_header_list_size must name oversize response metadata on every transport"
    );
    assert!(
        src.contains(
            "Messages queued between a client-streaming caller and the wire.\n    /// Default 16. Applies to client-streaming and bidi request streams."
        ),
        "ChannelConfig::stream_buffer must name the streaming shapes it queues"
    );
    assert!(
        src.contains("Configured message caps. Applies to every call shape."),
        "ServerConfig::limits and ChannelConfig::limits must name every call shape"
    );
    assert!(
        src.contains("Configured per-connection send buffer. Applies to every call shape."),
        "send_buffer_size getters must name every call shape"
    );
    assert!(
        src.contains(
            "HTTP/2 per-stream receive window. See [`Self::initial_stream_window_size`].\n    /// Applies to every call shape."
        ),
        "stream_window getters must name every call shape"
    );
    assert!(
        src.contains(
            "HTTP/2 per-connection receive window. See [`Self::initial_connection_window_size`].\n    /// Applies to every call shape."
        ),
        "connection_window getters must name every call shape"
    );
    assert!(
        src.contains(
            "HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. See [`Self::max_frame_size`].\n    /// Applies to every call shape."
        ),
        "frame_size getters must name every call shape"
    );
    assert!(
        src.contains(
            "Configured outbound streaming queue depth. Applies to client-streaming\n    /// and bidi request streams."
        ),
        "ChannelConfig::stream_buffer_size must not claim every call shape"
    );
    assert!(
        src.contains(
            "Configured max connection age, if any. The next RPC of every call\n    /// shape redials. See [`Self::max_connection_age`]."
        ),
        "ServerConfig::connection_age must name redial, not in-flight retry on every shape"
    );
    assert!(
        src.contains(
            "Dial bound: TCP/Unix connect, optional TLS, peer SETTINGS.\n    /// See [`Self::connect_timeout`]. Applies to every call shape once that\n    /// dial happens."
        ),
        "ChannelConfig::dial_timeout must name the dial bound"
    );
    assert!(
        src.contains("a later interceptor\n    /// can still set or clear it."),
        "ChannelConfig::wait_for_ready must name interceptor set/clear"
    );
    assert!(
        src.contains(
            "This is not TCP keepalive. PINGs run on Unix sockets and TLS\n    /// (including mTLS);"
        ),
        "ServerConfig::keep_alive_interval must name Unix and mTLS"
    );
    assert!(
        src.contains(
            "PINGs run on Unix sockets, TLS (including\n    /// mTLS), and [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::keep_alive_interval must name Unix, mTLS, and from_io"
    );
    assert!(
        src.contains(
            "Applies to every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::send_compressed must name every transport"
    );
    assert!(
        src.contains(
            "Default per-RPC deadline when the request omits `grpc-timeout`.\n    /// Applies to every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::timeout must name every transport"
    );
    assert!(
        src.contains(
            "Cap every RPC to this duration even when the client omits `grpc-timeout`.\n    /// Applies to every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]."
        ),
        "ServerConfig::timeout must name every transport"
    );
    assert!(
        src.contains(
            "the other end redials the next RPC of every call shape, including over\n    /// TLS, mTLS, and Unix."
        ),
        "ServerConfig::max_connection_age must name redial on TLS, mTLS, and Unix"
    );
    assert!(
        src.contains(
            "dropping the socket. Default 10 s. Values below 1 ms are raised to 1 ms.\n    /// Applies to every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]."
        ),
        "ServerConfig::max_connection_age_grace must name in-flight finish on every transport"
    );
    assert!(
        src.contains("of every call shape redials, including over TLS, mTLS, and Unix."),
        "ServerConfig::max_connection_idle must name redial on TLS, mTLS, and Unix"
    );
    assert!(
        src.contains(
            "not look idle, including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]."
        ),
        "ServerConfig::max_connection_idle must name in-flight not-idle on every transport"
    );
    assert!(
        src.contains(
            "Cap how many TCP/Unix connections the accept loop will serve at once,\n    /// including TLS and mTLS listeners. Applies to every call shape."
        ),
        "ServerConfig::max_concurrent_connections must name TLS and mTLS"
    );
    assert!(
        src.contains(
            "How long TLS accept (if any) and the HTTP/2 preface may each take.\n    /// Default 20 s. Values below 1 ms are raised to 1 ms.\n    /// Applies to every call shape, including over TLS, mTLS, and Unix."
        ),
        "ServerConfig::handshake_timeout must name TLS, mTLS, and Unix"
    );
    assert!(
        src.contains(
            "Cap how many RPCs the process will run at once, across every\n    /// connection. Applies to every call shape, including over TLS, mTLS,\n    /// Unix, and [`crate::Server::serve_connection`]."
        ),
        "ServerConfig::max_concurrent_rpcs must name every transport"
    );
    assert!(
        src.contains(
            "HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`. Distinct from\n    /// [`Self::max_concurrent_rpcs`], which refuses extras as\n    /// [`crate::Code::ResourceExhausted`]. A well-behaved client waits; both\n    /// RPCs still complete, including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]."
        ),
        "ServerConfig::max_concurrent_streams must name serialize vs RESOURCE_EXHAUSTED on every transport"
    );
    assert_eq!(
        src.matches(
            "A well-behaved client waits; both\n    /// RPCs still complete, including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]."
        )
        .count(),
        1,
        "ChannelConfig::max_concurrent_streams must not copy the server serialize Distinct"
    );
    assert!(
        src.contains(
            "HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS` the client advertises. Distinct\n    /// from [`ServerConfig::max_concurrent_streams`], which serializes extra\n    /// RPCs on the server. Push is disabled, including over TLS, mTLS, Unix,\n    /// and [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::max_concurrent_streams must name client SETTINGS Distinct from server serialize"
    );
    assert!(
        src.contains(
            "A well-behaved client splits DATA; every call shape still completes,\n    /// including over TLS, mTLS, Unix, and [`crate::Server::serve_connection`].\n    /// Distinct from [`Self::max_header_list_size`], which refuses oversize\n    /// metadata, and from [`Self::max_concurrent_streams`], which serializes\n    /// extra RPCs."
        ),
        "ServerConfig::max_frame_size must name still-serves Distinct from header-list and stream cap"
    );
    assert_eq!(
        src.matches("A well-behaved client splits DATA; every call shape still completes,")
            .count(),
        1,
        "ChannelConfig::max_frame_size must not copy the server still-serves Distinct"
    );
    assert!(
        src.contains(
            "HTTP/2 `SETTINGS_MAX_FRAME_SIZE` the client advertises. Distinct\n    /// from [`ServerConfig::max_frame_size`], which still serves every call\n    /// shape when the server advertises a small cap. A well-behaved server\n    /// splits DATA, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::max_frame_size must name client SETTINGS Distinct from server still-serves"
    );
    assert!(
        src.contains(
            "A well-behaved client still completes every call shape, including over\n    /// TLS, mTLS, Unix, and [`crate::Server::serve_connection`]. Distinct from\n    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS\n    /// minimum, and from [`Self::max_concurrent_streams`], which serializes\n    /// extra RPCs."
        ),
        "ServerConfig window setters must name still-serves Distinct from frame size and stream cap"
    );
    assert_eq!(
        src.matches("A well-behaved client still completes every call shape, including over")
            .count(),
        2,
        "ChannelConfig window setters must not copy the server still-serves Distinct"
    );
    assert!(
        src.contains(
            "HTTP/2 stream receive window the client advertises. Distinct from\n    /// [`ServerConfig::initial_stream_window_size`], which still serves when\n    /// the server advertises a small window. A well-behaved server still\n    /// completes every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::initial_stream_window_size must name client advertised Distinct from server still-serves"
    );
    assert!(
        src.contains(
            "HTTP/2 connection receive window the client advertises. Distinct from\n    /// [`ServerConfig::initial_connection_window_size`], which still serves when\n    /// the server advertises a small window. A well-behaved server still\n    /// completes every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::initial_connection_window_size must name client advertised Distinct from server still-serves"
    );
    assert!(
        src.contains(
            "Write backpressure still completes every call shape, including over\n    /// TLS, mTLS, Unix, and [`crate::Server::serve_connection`]. Distinct from\n    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS\n    /// minimum, and from [`Self::initial_stream_window_size`], which still\n    /// serves at a small receive window."
        ),
        "ServerConfig::max_send_buffer_size must name still-serves Distinct from frame size and windows"
    );
    assert_eq!(
        src.matches("Write backpressure still completes every call shape, including over")
            .count(),
        1,
        "ChannelConfig::max_send_buffer_size must not copy the server still-serves Distinct"
    );
    assert!(
        src.contains(
            "HTTP/2 send buffer the client applies on outbound frames. Distinct from\n    /// [`ServerConfig::max_send_buffer_size`], which still serves when the\n    /// server advertises a small buffer. A well-behaved server still completes\n    /// every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::max_send_buffer_size must name client buffer Distinct from server still-serves"
    );
    assert!(
        src.contains(
            "A well-behaved client never fills that queue; every call shape still\n    /// completes, including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]. Distinct from a raw HTTP/2 peer."
        ),
        "ServerConfig::max_pending_accept_reset_streams must name still-serves Distinct from a raw RST flood"
    );
    assert_eq!(
        src.matches("A well-behaved client never fills that queue; every call shape still")
            .count(),
        1,
        "ChannelConfig::max_pending_accept_reset_streams must not copy the server still-serves Distinct"
    );
    assert!(
        src.contains(
            "Distinct from [`ServerConfig::max_pending_accept_reset_streams`], which\n    /// still serves when the server caps that queue. A well-behaved server\n    /// never fills this client queue; every call shape still completes,\n    /// including over TLS, mTLS, Unix, and [`crate::Channel::from_io`]."
        ),
        "ChannelConfig::max_pending_accept_reset_streams must name client queue Distinct from server still-serves"
    );
    assert!(
        src.contains(
            "Distinct from [`Self::max_decoding_message_size`] /\n    /// [`Self::max_encoding_message_size`]. Oversize inbound or outbound is\n    /// [`crate::Code::ResourceExhausted`], including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]."
        ),
        "ServerConfig::message_limits must name combined-setter oversize on every transport"
    );
    assert!(
        src.contains(
            "Oversize metadata is refused, including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]. Distinct from a raw HTTP/2 peer."
        ),
        "ServerConfig::max_header_list_size must name oversize metadata on every transport"
    );
    let crate_src = include_str!("../src/lib.rs");
    assert!(
        crate_src.contains(
            "HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`; extras wait, they are not `RESOURCE_EXHAUSTED`"
        ),
        "crate docs must name stream-cap serialize vs RESOURCE_EXHAUSTED"
    );
}

#[test]
fn server_and_router_config_document_every_call_shape() {
    let src = include_str!("../src/server.rs");
    assert_eq!(
        src.matches("The configuration in effect. Applies to every call shape.")
            .count(),
        2,
        "Server::server_config and Router::server_config must name every call shape"
    );
    assert_eq!(
        src.matches(
            "Replace the transport and limit configuration. Applies to every call\n    /// shape."
        )
        .count(),
        2,
        "Server::config and Router::config must name every call shape"
    );
    assert_eq!(
        src.matches("Cap every RPC even when the client omits `grpc-timeout`.\n    /// Applies to every call shape.")
            .count(),
        2,
        "Server::rpc_timeout and Router::rpc_timeout must name every call shape"
    );
    assert_eq!(
        src.matches(
            "Whether responses are gzipped when the client accepts gzip.\n    /// Applies to every call shape."
        )
        .count(),
        2,
        "Server::compresses_outbound and Router::compresses_outbound must name every call shape"
    );
    assert!(
        src.contains(
            "Serve until `shutdown` resolves, then drain. Applies to every call\n    /// shape. In-flight RPCs finish; new connections are refused. TLS and"
        ),
        "Server::serve_with_shutdown must name TLS and Unix drain"
    );
    assert_eq!(
        src.matches("HTTP/2 PING keepalive. Applies to every call shape.")
            .count(),
        2,
        "Server::keep_alive_interval and Router::keep_alive_interval must name every call shape"
    );
    assert_eq!(
        src.matches(
            "How long to wait for a PING acknowledgement. Applies to every call\n    /// shape."
        )
        .count(),
        2,
        "Server::keep_alive_timeout and Router::keep_alive_timeout must name every call shape"
    );
    assert_eq!(
        src.matches("TCP `SO_KEEPALIVE`. Applies to every call shape.")
            .count(),
        2,
        "Server::tcp_keepalive and Router::tcp_keepalive must name every call shape"
    );
    assert_eq!(
        src.matches(
            "Send GOAWAY this long after accept. The next RPC of every call shape\n    /// redials, including over TLS, mTLS, and Unix; transparent retry of the\n    /// same in-flight RPC is unary and server-streaming only."
        )
        .count(),
        2,
        "Server::max_connection_age and Router::max_connection_age must name redial on TLS, mTLS, and Unix"
    );
    assert_eq!(
        src.matches(
            "Send GOAWAY after this long with no outstanding RPCs. The next RPC of\n    /// every call shape redials, including over TLS, mTLS, and Unix."
        )
        .count(),
        2,
        "Server::max_connection_idle and Router::max_connection_idle must name redial on TLS, mTLS, and Unix"
    );
    assert_eq!(
        src.matches(
            "including over TLS, mTLS, Unix, and [`Self::serve_connection`].\n    /// Applies to every call shape. See [`ServerConfig::max_connection_age_grace`]."
        )
        .count(),
        2,
        "Server::max_connection_age_grace and Router::max_connection_age_grace must name every transport"
    );
    assert_eq!(
        src.matches(
            "Cap how many TCP/Unix connections the accept loop will serve at once,\n    /// including TLS and mTLS listeners. Applies to every call shape."
        )
        .count(),
        2,
        "Server::max_concurrent_connections and Router::max_concurrent_connections must name TLS and mTLS"
    );
    assert_eq!(
        src.matches(
            "Drop a client that never finishes TLS or the HTTP/2 preface.\n    /// Applies to every call shape, including over TLS, mTLS, and Unix."
        )
        .count(),
        2,
        "Server::handshake_timeout and Router::handshake_timeout must name TLS, mTLS, and Unix"
    );
    assert_eq!(
        src.matches("fails. Applies to every call shape.").count(),
        2,
        "Server::serve_unix and Router::serve_unix must name every call shape"
    );
    assert_eq!(
        src.matches(
            "Serve over TLS until `shutdown` resolves, then drain.\n    /// Applies to every call shape, including mTLS. In-flight RPCs finish;\n    /// new connections are refused."
        )
        .count(),
        2,
        "Server::serve_tls_with_shutdown and Router::serve_tls_with_shutdown must name mTLS drain"
    );
    assert_eq!(
        src.matches(
            "Serve h2c on a Unix listener until `shutdown` resolves, then drain.\n    /// Applies to every call shape. In-flight RPCs finish; new connections\n    /// are refused."
        )
        .count(),
        2,
        "Server::serve_unix_with_shutdown and Router::serve_unix_with_shutdown must name drain"
    );
    assert_eq!(
        src.matches("Replace both message caps at once. Applies to every call shape.")
            .count(),
        2,
        "Server::message_limits and Router::message_limits must name every call shape"
    );
    assert_eq!(
        src.matches(
            "Distinct from [`Self::max_decoding_message_size`] /\n    /// [`Self::max_encoding_message_size`]. Oversize inbound or outbound\n    /// is [`Code::ResourceExhausted`], including over TLS, mTLS, Unix, and\n    /// [`Self::serve_connection`]."
        )
        .count(),
        2,
        "Server::message_limits and Router::message_limits must name combined-setter oversize on every transport"
    );
    assert_eq!(
        src.matches(
            "Cap how many RPCs the process will run at once.\n    /// Applies to every call shape, including over TLS, mTLS, Unix, and\n    /// [`Self::serve_connection`]."
        )
        .count(),
        2,
        "Server::max_concurrent_rpcs and Router::max_concurrent_rpcs must name every transport"
    );
    assert_eq!(
        src.matches("Concurrent RPCs allowed per HTTP/2 connection. Applies to every call\n    /// shape.")
            .count(),
        2,
        "Server::max_concurrent_streams and Router::max_concurrent_streams must name every call shape"
    );
    assert_eq!(
        src.matches(
            "HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`. Distinct from\n    /// [`Self::max_concurrent_rpcs`], which refuses extras as\n    /// [`Code::ResourceExhausted`]. A well-behaved client waits; both RPCs\n    /// still complete, including over TLS, mTLS, Unix, and\n    /// [`Self::serve_connection`]."
        )
        .count(),
        2,
        "Server::max_concurrent_streams and Router::max_concurrent_streams must name serialize vs RESOURCE_EXHAUSTED on every transport"
    );
    assert_eq!(
        src.matches("HTTP/2 per-stream receive window. Applies to every call shape.")
            .count(),
        2,
        "Server::initial_stream_window_size and Router::initial_stream_window_size must name every call shape"
    );
    assert_eq!(
        src.matches("HTTP/2 per-connection receive window. Applies to every call shape.")
            .count(),
        2,
        "Server::initial_connection_window_size and Router::initial_connection_window_size must name every call shape"
    );
    assert_eq!(
        src.matches("HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. Applies to every call shape.")
            .count(),
        2,
        "Server::max_frame_size and Router::max_frame_size must name every call shape"
    );
    assert_eq!(
        src.matches(
            "A well-behaved client splits DATA; every call shape still completes,\n    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct\n    /// from [`Self::max_header_list_size`], which refuses oversize metadata,\n    /// and from [`Self::max_concurrent_streams`], which serializes extra RPCs."
        )
        .count(),
        2,
        "Server::max_frame_size and Router::max_frame_size must name still-serves Distinct from header-list and stream cap"
    );
    assert_eq!(
        src.matches(
            "A well-behaved client still completes every call shape, including over\n    /// TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct from\n    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS\n    /// minimum, and from [`Self::max_concurrent_streams`], which serializes\n    /// extra RPCs."
        )
        .count(),
        4,
        "Server and Router window setters must name still-serves Distinct from frame size and stream cap"
    );
    assert_eq!(
        src.matches("HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`. Applies to every call shape.")
            .count(),
        2,
        "Server::max_header_list_size and Router::max_header_list_size must name every call shape"
    );
    assert_eq!(
        src.matches(
            "Oversize metadata is refused, including over TLS, mTLS, Unix, and\n    /// [`Self::serve_connection`]. Distinct from a raw HTTP/2 peer."
        )
        .count(),
        2,
        "Server::max_header_list_size and Router::max_header_list_size must name oversize metadata on every transport"
    );
    assert_eq!(
        src.matches("Per-connection HTTP/2 send buffer. Applies to every call shape.")
            .count(),
        2,
        "Server::max_send_buffer_size and Router::max_send_buffer_size must name every call shape"
    );
    assert_eq!(
        src.matches(
            "Write backpressure still completes every call shape, including over\n    /// TLS, mTLS, Unix, and [`Self::serve_connection`]. Distinct from\n    /// [`Self::max_frame_size`], which still serves at the 16 KiB SETTINGS\n    /// minimum, and from [`Self::initial_stream_window_size`], which still\n    /// serves at a small receive window."
        )
        .count(),
        2,
        "Server::max_send_buffer_size and Router::max_send_buffer_size must name still-serves Distinct from frame size and windows"
    );
    assert_eq!(
        src.matches(
            "Cap remotely-reset HTTP/2 streams waiting in the accept queue.\n    /// Applies to every call shape."
        )
        .count(),
        2,
        "Server::max_pending_accept_reset_streams and Router::max_pending_accept_reset_streams must name every call shape"
    );
    assert_eq!(
        src.matches(
            "A well-behaved client never fills that queue; every call shape still\n    /// completes, including over TLS, mTLS, Unix, and [`Self::serve_connection`].\n    /// Distinct from a raw HTTP/2 peer."
        )
        .count(),
        2,
        "Server and Router max_pending_accept_reset_streams must name still-serves Distinct from a raw RST flood"
    );
    assert_eq!(
        src.matches(
            "Serve a single already-accepted byte stream until it closes.\n    /// Applies to every call shape."
        )
        .count(),
        2,
        "Server::serve_connection and Router::serve_connection must name every call shape"
    );
    assert!(
        src.contains(
            "and [`Rpc::peer_cred`] are `None`. Generated handlers see the same\n    /// empty facts on [`Request`] and [`crate::Parts`]."
        ),
        "Server::serve_connection must name empty peer facts on Request/Parts"
    );
    assert!(
        src.contains("listener-side work fails.\n    /// Applies to every call shape."),
        "Server::serve_with_incoming must name every call shape"
    );
    assert_eq!(
        src.matches(
            "Override [`Incoming::peer`] to fill [`Rpc::local_addr`],\n    /// [`Rpc::peer_identity`], [`Rpc::peer_cred`], or a transport\n    /// [`Rpc::scheme`] without changing [`IncomingAccept`]."
        )
        .count(),
        2,
        "Server::serve_with_incoming and Router::serve_with_incoming must name Incoming::peer"
    );
    assert!(
        src.contains(
            "you accepted yourself). Applies to every call shape on that\n    /// connection."
        ),
        "Incoming::peer must name every call shape on that connection"
    );
    assert!(
        src.contains(
            "[`Incoming::peer`] is how a custom acceptor supplies a local address,\n/// mTLS identity, Unix credentials, or a transport `:scheme`. The default\n/// keeps the `SocketAddr` from [`IncomingAccept`] and does not override\n/// `:scheme`. [`Server::serve_connection`] leaves every field unset.\n/// Applies to every call shape on that connection."
        ),
        "ConnectionInfo rustdoc must name Incoming::peer on every call shape"
    );
    assert!(
        src.contains(
            "Serve connections from `incoming` until it is exhausted.\n    /// Applies to every call shape. See [`Server::serve_with_incoming`]."
        ),
        "Router::serve_with_incoming must name every call shape"
    );
    assert_eq!(
        src.matches(
            "[`Self::serve_with_incoming`] until `shutdown` resolves, then drain.\n    /// Applies to every call shape."
        )
        .count(),
        2,
        "Server::serve_with_incoming_shutdown and Router::serve_with_incoming_shutdown must name every call shape"
    );
    assert!(
        src.contains("TLS uses the client's [`crate::Target`], not SNI."),
        "Rpc::authority must name Target, not TLS SNI"
    );
    assert!(
        src.contains(
            "[`Self::max_decoding_message_size`] and\n    /// [`Self::max_encoding_message_size`] stay in effect on every mounted\n    /// service, on every call shape of those mounts, including over TLS, mTLS,\n    /// Unix, and [`Self::serve_connection`]."
        ),
        "Server::add_service must name decode and encode caps on every mount and transport"
    );
    assert!(
        src.contains(
            "A path whose service is not mounted, or a method a mounted service does\n/// not have, is [`crate::Code::Unimplemented`] on every call shape, including\n/// over TLS, mTLS, Unix, and [`Server::serve_connection`]."
        ),
        "Router rustdoc must name UNIMPLEMENTED on every mount miss and transport"
    );
    assert!(
        src.contains(
            "The last mount is the one that serves, on every call shape, including\n    /// over TLS, mTLS, Unix, and [`Self::serve_connection`]."
        ),
        "Router::add_service must name last-wins remount on every transport"
    );
    assert!(
        src.contains(
            "A hand-written [`Service`] is first-class. Unknown methods are\n/// [`crate::Code::Unimplemented`] on every call shape, including over TLS,\n/// mTLS, Unix, and [`Server::serve_connection`]."
        ),
        "Server rustdoc must name hand-written unknown-method UNIMPLEMENTED on every transport"
    );
    assert!(
        src.contains(
            "A single\n    /// interceptor still rejects before the handler on every call shape,\n    /// including over TLS, mTLS, Unix, and [`Self::serve_connection`]."
        ),
        "Server::intercept rustdoc must name a single intercept reject on every transport"
    );
    assert_eq!(
        src.matches(
            "gzip responses when the client advertises gzip. Applies to every call\n    /// shape, including over TLS, mTLS, Unix, and [`Self::serve_connection`]."
        )
        .count(),
        2,
        "Server::send_compressed and Router::send_compressed must name every transport"
    );
    assert_eq!(
        src.matches(
            "Cap every RPC even when the client omits `grpc-timeout`. Applies to\n    /// every call shape, including over TLS, mTLS, Unix, and\n    /// [`Self::serve_connection`]."
        )
        .count(),
        2,
        "Server::timeout and Router::timeout must name every transport"
    );
}

#[test]
fn request_deadline_documents_every_transport() {
    let src = include_str!("../src/request.rs");
    assert!(
        src.contains(
            "onto a downstream call preserves the remaining budget. Stamped on\n    /// every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]. The Instant elapses while the handler\n    /// runs; [`Self::timeout`] stays the duration stamped at dispatch."
        ),
        "Request::deadline must name every transport and that the Instant elapses"
    );
    assert!(
        src.contains(
            "Passing `false` opts out of a later [`crate::Server::send_compressed`]\n    /// overlay on every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]."
        ),
        "Response::set_compress must name handler opt-out of send_compressed on every transport"
    );
    assert!(
        src.contains(
            "Passing `false` opts out of a later [`crate::Channel::send_compressed`]\n    /// overlay on every call shape, including over TLS, mTLS, Unix, and\n    /// [`crate::Channel::from_io`]."
        ),
        "Request::set_compress must name call-site opt-out of send_compressed on every transport"
    );
    assert!(
        src.contains(
            "A default server (no [`crate::Server::send_compressed`])\n    /// leaves this `None` on every call shape, including over TLS, mTLS, Unix,\n    /// and [`crate::Channel::from_io`]."
        ),
        "Response::encoding must name default identity on every transport"
    );
    assert!(
        src.contains(
            "A `-bin` trailer must not appear as a header, including over TLS,\n    /// mTLS, Unix, and [`crate::Channel::from_io`]."
        ),
        "Response::trailers rustdoc must name -bin trailers on every transport"
    );
    assert!(
        src.contains(
            "stays live until that drain, including over TLS, mTLS, Unix, and\n    /// [`crate::Server::serve_connection`]."
        ),
        "Request::cancelled must name spawned producer drain on every transport"
    );
    assert!(
        src.contains("settles, including over TLS, mTLS, Unix, and [`crate::Channel::from_io`]."),
        "CallHandle rustdoc must name cancel_after_begin on every transport"
    );
}

fn greeter_and_test_router() -> Router {
    Router::new()
        .add_service(GreeterServer::new(Echo))
        .add_service(TestServiceServer::new(InteropTestService))
}

async fn assert_unimplemented_path(channel: &Channel, path: &'static str) {
    let err = channel
        .unary::<HelloRequest, HelloReply>(path, Request::new(req("x")))
        .await
        .expect_err("unary unimplemented");
    assert_eq!(err.code(), Code::Unimplemented, "{path} unary {err}");
    match channel
        .server_streaming::<HelloRequest, HelloReply>(path, Request::new(req("x")))
        .await
    {
        Err(err) => assert_eq!(
            err.code(),
            Code::Unimplemented,
            "{path} server-stream {err}"
        ),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => {
                assert_eq!(
                    err.code(),
                    Code::Unimplemented,
                    "{path} server-stream {err}"
                )
            }
            Ok(_) => panic!("{path} server-stream must be unimplemented"),
        },
    }
    let (tx, call) = channel.client_streaming::<HelloRequest, HelloReply>(path, Request::new(()));
    let err = call.await.expect_err("client-stream unimplemented");
    assert_eq!(
        err.code(),
        Code::Unimplemented,
        "{path} client-stream {err}"
    );
    drop(tx);
    let (tx, call) = channel.bidi::<HelloRequest, HelloReply>(path, Request::new(()));
    let err = call.await.expect_err("bidi unimplemented");
    assert_eq!(err.code(), Code::Unimplemented, "{path} bidi {err}");
    drop(tx);
}

async fn assert_router_dispatches(channel: Channel) {
    echo_every_shape(&GreeterClient::new(channel.clone()), None).await;
    echo_test_every_shape(&TestServiceClient::new(channel.clone())).await;
    assert_unimplemented_path(&channel, "/nope.Absent/Method").await;
    assert_unimplemented_path(&channel, "/helloworld.Greeter/Nope").await;
}

#[tokio::test]
async fn a_router_dispatches_between_two_services() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_and_test_router()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_router_dispatches(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_router_dispatches_between_two_services() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_and_test_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_router_dispatches(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_router_dispatches_between_two_services() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_and_test_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_router_dispatches(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_router_dispatches_between_two_services() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        greeter_and_test_router().serve_unix(sock).await.ok();
    });
    assert_router_dispatches(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_router_dispatches_between_two_services() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        greeter_and_test_router()
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_router_dispatches(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn router_reports_its_mounted_services() {
    let router = Router::new()
        .add_service(GreeterServer::new(Echo))
        .add_service(TestServiceServer::new(InteropTestService));
    let mut names: Vec<&str> = router.service_names().collect();
    names.sort_unstable();
    assert_eq!(names, ["grpc.testing.TestService", "helloworld.Greeter"]);
}

#[tokio::test]
async fn mounting_the_same_service_twice_keeps_the_last() {
    let router = Router::new()
        .add_service(GreeterServer::new(Echo))
        .add_service(GreeterServer::new(Echo));
    assert_eq!(router.service_names().count(), 1);
}

fn last_wins_router() -> Router {
    Router::new()
        .add_service(GreeterServer::new(FailGreeter))
        .add_service(GreeterServer::new(Echo))
}

async fn assert_last_mount_wins(channel: Channel) {
    echo_every_shape(&GreeterClient::new(channel), None).await;
}

#[tokio::test]
async fn a_router_serves_the_last_mount_of_a_service() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        last_wins_router().serve_listener(listener).await.ok();
    });
    assert_last_mount_wins(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_router_serves_the_last_mount_of_a_service() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        last_wins_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_last_mount_wins(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_router_serves_the_last_mount_of_a_service() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        last_wins_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_last_mount_wins(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_router_serves_the_last_mount_of_a_service() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        last_wins_router().serve_unix(sock).await.ok();
    });
    assert_last_mount_wins(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_router_serves_the_last_mount_of_a_service() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        last_wins_router().serve_connection(server_io).await.ok();
    });
    assert_last_mount_wins(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

/// A handler slow enough that the drain has to wait for it.
struct Slow;

impl Slow {
    async fn hang(&self) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

impl pbrs_grpc::Greeter for Slow {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.hang().await;
        let mut reply = HelloReply::new();
        reply.set_message(request.get_ref().name());
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        self.hang().await;
        Ok(Response::new(common::reply("ok")))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        self.hang().await;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        self.hang().await;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }
}

async fn assert_in_flight_finishes(
    client: GreeterClient,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    served: tokio::task::JoinHandle<()>,
) {
    let mut call = client.say_hello(Request::new(req("ada")));
    // Drive the call far enough that Slow's 200 ms handler is running, then
    // signal drain. Creating a Call does not start the RPC; first poll does.
    tokio::select! {
        biased;
        result = &mut call => panic!("Slow returned before shutdown: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    shutdown_tx.send(()).expect("signal");
    let reply = call.await.expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");
}

async fn assert_drain_closes_listener(
    client: GreeterClient,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    served: tokio::task::JoinHandle<()>,
    refused: impl std::future::Future<Output = bool>,
) {
    echo_every_shape(&client, None).await;
    drop(client);
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");
    assert!(refused.await, "the listener must be closed after drain");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_finishes_in_flight_rpcs() {
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_with_shutdown(listener, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    assert_in_flight_finishes(GreeterClient::new(channel(addr).await), shutdown_tx, served).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_graceful_shutdown_finishes_in_flight_rpcs() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(
                listener,
                async {
                    shutdown_rx.await.ok();
                },
                tls,
            )
            .await
            .ok();
    });
    assert_in_flight_finishes(
        GreeterClient::new(tls_channel(addr).await),
        shutdown_tx,
        served,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_graceful_shutdown_finishes_in_flight_rpcs() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(
                listener,
                async {
                    shutdown_rx.await.ok();
                },
                tls,
            )
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_in_flight_finishes(
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
        shutdown_tx,
        served,
    )
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_graceful_shutdown_finishes_in_flight_rpcs() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_unix_until_shutdown(sock, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    assert_in_flight_finishes(
        GreeterClient::new(unix_channel(&path).await),
        shutdown_tx,
        served,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_stops_accepting_new_connections() {
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_shutdown(listener, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    assert_drain_closes_listener(
        GreeterClient::new(channel(addr).await),
        shutdown_tx,
        served,
        async {
            match Channel::connect(addr).await {
                Err(_) => true,
                Ok(channel) => GreeterClient::new(channel)
                    .say_hello(Request::new(req("late")))
                    .await
                    .is_err(),
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_graceful_shutdown_stops_accepting_new_connections() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(
                listener,
                async {
                    shutdown_rx.await.ok();
                },
                tls,
            )
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_drain_closes_listener(
        GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        shutdown_tx,
        served,
        async {
            match Channel::connect_tls(addr, client_tls).await {
                Err(_) => true,
                Ok(channel) => GreeterClient::new(channel)
                    .say_hello(Request::new(req("late")))
                    .await
                    .is_err(),
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_graceful_shutdown_stops_accepting_new_connections() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(
                listener,
                async {
                    shutdown_rx.await.ok();
                },
                tls,
            )
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_drain_closes_listener(
        GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        shutdown_tx,
        served,
        async {
            match Channel::connect_tls(addr, client_tls).await {
                Err(_) => true,
                Ok(channel) => GreeterClient::new(channel)
                    .say_hello(Request::new(req("late")))
                    .await
                    .is_err(),
            }
        },
    )
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_graceful_shutdown_stops_accepting_new_connections() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_until_shutdown(sock, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    assert_drain_closes_listener(
        GreeterClient::new(unix_channel(&path).await),
        shutdown_tx,
        served,
        async {
            match Channel::connect_unix(&path).await {
                Err(_) => true,
                Ok(channel) => GreeterClient::new(channel)
                    .say_hello(Request::new(req("late")))
                    .await
                    .is_err(),
            }
        },
    )
    .await;
}

/// A server-streaming handler whose producer stops as soon as reading the
/// request stream fails, which is what almost every real handler does. When the
/// read fails because the deadline expired, the RPC must not look like a clean
/// end of stream.
struct QuietUntilDeadline;

impl pbrs_grpc::Greeter for QuietUntilDeadline {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("quiet"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("quiet"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("quiet"))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = pbrs_grpc::Streaming::channel(4);
        drop(tokio::spawn(async move {
            // Swallows the error and stops, exactly like the reference
            // interop service and most hand-written handlers.
            while let Ok(Some(_)) = inbound.message().await {}
            drop(tx);
        }));
        Ok(Response::new(stream))
    }
}

async fn assert_expired_deadline_is_never_a_clean_end_of_stream(client: &GreeterClient) {
    // Repeat, because the original bug was a coin flip between the deadline
    // firing and the producer stopping.
    for _ in 0..12 {
        let mut request = Request::new(());
        request.set_timeout(Duration::from_millis(5));
        let (tx, call) = client.stream_hello(request);
        tx.send(req("ada")).await.ok();
        let outcome = match call.await {
            Err(status) => Err(status),
            Ok(response) => response.into_inner().message().await,
        };
        match outcome {
            Err(status) => assert_eq!(status.code(), Code::DeadlineExceeded),
            Ok(None) => panic!("an expired deadline must not read as a clean end"),
            Ok(Some(_)) => panic!("the handler sends nothing"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_deadline_is_never_a_clean_end_of_stream() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(QuietUntilDeadline)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_expired_deadline_is_never_a_clean_end_of_stream(&GreeterClient::new(
        channel(addr).await,
    ))
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_an_expired_deadline_is_never_a_clean_end_of_stream() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(QuietUntilDeadline)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_expired_deadline_is_never_a_clean_end_of_stream(&GreeterClient::new(
        tls_channel(addr).await,
    ))
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_an_expired_deadline_is_never_a_clean_end_of_stream() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(QuietUntilDeadline)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_expired_deadline_is_never_a_clean_end_of_stream(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_an_expired_deadline_is_never_a_clean_end_of_stream() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(QuietUntilDeadline)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_expired_deadline_is_never_a_clean_end_of_stream(&GreeterClient::new(
        unix_channel(&path).await,
    ))
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_an_expired_deadline_is_never_a_clean_end_of_stream() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(QuietUntilDeadline)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_expired_deadline_is_never_a_clean_end_of_stream(&GreeterClient::new(channel)).await;
    server.abort();
}

/// The wrapping-service pattern from `docs/grpc.md`: authenticate, then
/// delegate. `NAME` is inherited, so the wrapper mounts where the wrapped
/// service would.
struct RequireAuth<S> {
    inner: Arc<S>,
    token: String,
}

impl<S: Service> Service for RequireAuth<S> {
    const NAME: &'static str = S::NAME;

    async fn call(&self, rpc: Rpc) {
        if rpc.metadata().get("authorization") != Some(self.token.as_str()) {
            return rpc.reject(Status::unauthenticated("bad or missing token"));
        }
        self.inner.call(rpc).await;
    }
}

#[tokio::test]
async fn a_wrapping_service_can_reject_before_the_body_is_read() {
    let (addr, listener) = bind().await;
    let guard = RequireAuth {
        inner: Arc::new(GreeterServer::new(Echo)),
        token: "Bearer letmein".to_owned(),
    };
    let task = tokio::spawn(async move {
        Server::new(guard).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(channel(addr).await);

    assert_err_on_every_shape(&client, Code::Unauthenticated).await;

    let allowed = GreeterClient::new(channel(addr).await).intercept(inject_bearer);
    echo_every_shape(&allowed, None).await;

    task.abort();
}

#[tokio::test]
async fn a_tls_wrapping_service_can_reject_before_the_body_is_read() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let guard = RequireAuth {
        inner: Arc::new(GreeterServer::new(Echo)),
        token: "Bearer letmein".to_owned(),
    };
    let task = tokio::spawn(async move {
        Server::new(guard)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_err_on_every_shape(
        &GreeterClient::new(tls_channel(addr).await),
        Code::Unauthenticated,
    )
    .await;
    let allowed = GreeterClient::new(tls_channel(addr).await).intercept(inject_bearer);
    echo_every_shape(&allowed, None).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_wrapping_service_can_reject_before_the_body_is_read() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let guard = RequireAuth {
        inner: Arc::new(GreeterServer::new(Echo)),
        token: "Bearer letmein".to_owned(),
    };
    let task = tokio::spawn(async move {
        Server::new(guard)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_err_on_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        Code::Unauthenticated,
    )
    .await;
    let allowed =
        GreeterClient::new(tls_channel_with(addr, client_tls).await).intercept(inject_bearer);
    echo_every_shape(&allowed, None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_wrapping_service_can_reject_before_the_body_is_read() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        Server::new(RequireAuth {
            inner: Arc::new(GreeterServer::new(Echo)),
            token: "Bearer letmein".to_owned(),
        })
        .serve_unix(sock)
        .await
        .ok();
    });
    assert_err_on_every_shape(
        &GreeterClient::new(unix_channel(&path).await),
        Code::Unauthenticated,
    )
    .await;
    let allowed = GreeterClient::new(unix_channel(&path).await).intercept(inject_bearer);
    echo_every_shape(&allowed, None).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_wrapping_service_can_reject_before_the_body_is_read() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Server::new(RequireAuth {
            inner: Arc::new(GreeterServer::new(Echo)),
            token: "Bearer letmein".to_owned(),
        })
        .serve_connection(server_io)
        .await
        .ok();
    });
    let ch = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_err_on_every_shape(&GreeterClient::new(ch.clone()), Code::Unauthenticated).await;
    echo_every_shape(&GreeterClient::new(ch).intercept(inject_bearer), None).await;
    server.abort();
}

fn require_bearer(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.metadata().get("authorization") != Some("Bearer letmein") {
        return Err(Status::unauthenticated("bad or missing token"));
    }
    Ok(())
}

fn inject_bearer(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.metadata_mut()
        .insert("authorization", "Bearer letmein")?;
    Ok(())
}

fn interceptor_require_trace(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.metadata().get("x-trace").is_none() {
        return Err(Status::invalid_argument("missing x-trace"));
    }
    Ok(())
}

fn interceptor_inject_trace_and_bearer(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.metadata_mut().insert("x-trace", "1")?;
    call.metadata_mut()
        .insert("authorization", "Bearer letmein")?;
    Ok(())
}

fn only_auth<T>(mut request: Request<T>) -> Request<T> {
    request
        .metadata_mut()
        .insert("authorization", "Bearer letmein")
        .expect("metadata");
    request
}

async fn assert_stack_rejects_auth_without_trace(client: &GreeterClient) {
    assert_err_on_every_shape(client, Code::InvalidArgument).await;
    let err = client
        .say_hello(only_auth(Request::new(req("ada"))))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), Code::InvalidArgument, "{err}");
    let err = client
        .server_hello(only_auth(Request::new(req("ada"))))
        .await
        .expect_err("server-stream");
    assert_eq!(err.code(), Code::InvalidArgument, "{err}");
    let (tx, call) = client.client_hello(only_auth(Request::new(())));
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), Code::InvalidArgument, "{err}");
    drop(tx);
    let (tx, call) = client.stream_hello(only_auth(Request::new(())));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), Code::InvalidArgument, "{err}");
    drop(tx);
}

async fn assert_add_service_bearer(
    denied_g: &GreeterClient,
    denied_t: &TestServiceClient,
    allowed_g: &GreeterClient,
    allowed_t: &TestServiceClient,
) {
    assert_err_on_every_shape(denied_g, Code::Unauthenticated).await;
    assert_err_on_test_every_shape(denied_t, Code::Unauthenticated).await;
    echo_every_shape(allowed_g, None).await;
    echo_test_every_shape(allowed_t).await;
}

#[tokio::test]
async fn h2c_requests_use_the_http_scheme() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let flag = Arc::clone(&seen);
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(move |rpc: &mut Rpc| {
                let n = match rpc.scheme() {
                    Some("http") => 1,
                    Some("https") => 2,
                    _ => 3,
                };
                flag.store(n, Ordering::SeqCst);
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    echo_every_shape(&client, None).await;
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn tcp_rpcs_expose_local_and_remote_addr() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let flag = Arc::clone(&seen);
    let listen = addr;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(move |rpc: &mut Rpc| {
                let n = match (
                    rpc.local_addr(),
                    rpc.remote_addr(),
                    rpc.peer_identity(),
                    rpc.peer_cred(),
                ) {
                    (Some(local), Some(remote), None, None)
                        if local == listen && remote.ip().is_loopback() =>
                    {
                        1
                    }
                    _ => 2,
                };
                flag.store(n, Ordering::SeqCst);
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    echo_every_shape(&client, None).await;
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn a_generated_handler_sees_authority_scheme_and_parts() {
    struct SeesHttp;

    impl Greeter for SeesHttp {
        async fn say_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            let msg = sees_http(request)?;
            Ok(Response::new(common::reply(common::name_of_request(&msg))))
        }

        async fn client_hello(
            &self,
            request: Request<pbrs_grpc::Streaming<HelloRequest>>,
        ) -> Result<Response<HelloReply>, Status> {
            let _ = sees_http(request)?;
            Ok(Response::new(common::reply("ada")))
        }

        async fn server_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
            let msg = sees_http(request)?;
            Ok(echo_named_stream(common::name_of_request(&msg)))
        }

        async fn stream_hello(
            &self,
            request: Request<pbrs_grpc::Streaming<HelloRequest>>,
        ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
            let _ = sees_http(request)?;
            Ok(echo_named_stream("ada".into()))
        }
    }

    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesHttp)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

struct SeesDeadline;

impl Greeter for SeesDeadline {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        sees_deadline(&request).await?;
        Ok(Response::new(common::reply(common::name_of_request(
            request.get_ref(),
        ))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        sees_deadline(&request).await?;
        Ok(Response::new(common::reply("ada")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        sees_deadline(&request).await?;
        Ok(echo_named_stream(common::name_of_request(
            request.get_ref(),
        )))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        sees_deadline(&request).await?;
        Ok(echo_named_stream("ada".into()))
    }
}

async fn assert_handler_deadline_elapses(client: &GreeterClient) {
    echo_every_shape(client, Some(Duration::from_millis(200))).await;
}

#[tokio::test]
async fn a_handler_deadline_is_an_instant_that_elapses() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesDeadline)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_handler_deadline_elapses(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_handler_deadline_is_an_instant_that_elapses() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesDeadline)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_handler_deadline_elapses(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_handler_deadline_is_an_instant_that_elapses() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesDeadline)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_handler_deadline_elapses(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_handler_deadline_is_an_instant_that_elapses() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesDeadline).serve_unix(sock).await.ok();
    });
    assert_handler_deadline_elapses(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_handler_deadline_is_an_instant_that_elapses() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesDeadline)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_handler_deadline_elapses(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

async fn assert_generated_intercept(ch: Channel) {
    assert_err_on_every_shape(&GreeterClient::new(ch.clone()), Code::Unauthenticated).await;
    echo_every_shape(&GreeterClient::new(ch).intercept(inject_bearer), None).await;
}

#[tokio::test]
async fn a_generated_server_interceptor_rejects_before_the_handler() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_generated_intercept(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_generated_server_interceptor_rejects_before_the_handler() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_generated_intercept(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_generated_server_interceptor_rejects_before_the_handler() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_generated_intercept(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_generated_server_interceptor_rejects_before_the_handler() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_generated_intercept(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_generated_server_interceptor_rejects_before_the_handler() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_generated_intercept(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn generated_server_interceptors_stack_in_declaration_order() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(channel(addr).await)).await;
    echo_every_shape(
        &GreeterClient::new(channel(addr).await).intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_generated_server_interceptors_stack_in_declaration_order() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(tls_channel(addr).await)).await;
    echo_every_shape(
        &GreeterClient::new(tls_channel(addr).await).intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_generated_server_interceptors_stack_in_declaration_order() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(
        tls_channel_with(addr, client_tls.clone()).await,
    ))
    .await;
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await)
            .intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_generated_server_interceptors_stack_in_declaration_order() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(unix_channel(&path).await)).await;
    echo_every_shape(
        &GreeterClient::new(unix_channel(&path).await)
            .intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_generated_server_interceptors_stack_in_declaration_order() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let ch = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(ch.clone())).await;
    echo_every_shape(
        &GreeterClient::new(ch).intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn intercept_on_a_generated_server_survives_add_service() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_add_service_bearer(
        &GreeterClient::new(channel(addr).await),
        &TestServiceClient::new(channel(addr).await),
        &GreeterClient::new(channel(addr).await).intercept(inject_bearer),
        &TestServiceClient::new(channel(addr).await).intercept(inject_bearer),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_intercept_on_a_generated_server_survives_add_service() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_add_service_bearer(
        &GreeterClient::new(tls_channel(addr).await),
        &TestServiceClient::new(tls_channel(addr).await),
        &GreeterClient::new(tls_channel(addr).await).intercept(inject_bearer),
        &TestServiceClient::new(tls_channel(addr).await).intercept(inject_bearer),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_intercept_on_a_generated_server_survives_add_service() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_add_service_bearer(
        &GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        &TestServiceClient::new(tls_channel_with(addr, client_tls.clone()).await),
        &GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await)
            .intercept(inject_bearer),
        &TestServiceClient::new(tls_channel_with(addr, client_tls).await).intercept(inject_bearer),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_intercept_on_a_generated_server_survives_add_service() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_add_service_bearer(
        &GreeterClient::new(unix_channel(&path).await),
        &TestServiceClient::new(unix_channel(&path).await),
        &GreeterClient::new(unix_channel(&path).await).intercept(inject_bearer),
        &TestServiceClient::new(unix_channel(&path).await).intercept(inject_bearer),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_intercept_on_a_generated_server_survives_add_service() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_bearer)
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let ch = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_add_service_bearer(
        &GreeterClient::new(ch.clone()),
        &TestServiceClient::new(ch.clone()),
        &GreeterClient::new(ch.clone()).intercept(inject_bearer),
        &TestServiceClient::new(ch).intercept(inject_bearer),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn test_service_interceptor_rejects_with_typed_status() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_test_blocked_every_shape(&TestServiceClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_client_interceptor_rejects_with_typed_status() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = TestServiceClient::new(channel(addr).await)
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_test_blocked_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_client_interceptor_sees_every_shape_context() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_context)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = TestServiceClient::new(channel(addr).await).intercept(stamp_outgoing_context);
    echo_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_send_compressed_gzips_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = TestServiceClient::new(channel(addr).await.send_compressed());
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_send_compressed_gzips_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = TestServiceClient::new(tls_channel(addr).await.send_compressed());
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_send_compressed_gzips_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::new(tls_channel_with(addr, client_tls).await.send_compressed());
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_interceptor_rejects_with_typed_status() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_test_blocked_every_shape(&TestServiceClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_client_interceptor_rejects_with_typed_status() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = TestServiceClient::new(tls_channel(addr).await)
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_test_blocked_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_client_interceptor_sees_every_shape_context() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_context)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = TestServiceClient::new(tls_channel(addr).await).intercept(stamp_outgoing_context);
    echo_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_interceptor_rejects_with_typed_status() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_test_blocked_every_shape(&TestServiceClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_client_interceptor_rejects_with_typed_status() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_test_blocked_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_client_interceptor_sees_every_shape_context() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_context)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(stamp_outgoing_context);
    echo_test_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_send_compressed_gzips_every_shape() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::connect_unix(&path)
            .await
            .expect("connect")
            .send_compressed(),
    );
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_interceptor_rejects_with_typed_status() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()))
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    assert_test_blocked_every_shape(&TestServiceClient::new(
        Channel::connect_unix(&path).await.expect("connect"),
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_client_interceptor_rejects_with_typed_status() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let client = TestServiceClient::new(Channel::connect_unix(&path).await.expect("connect"))
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_test_blocked_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_client_interceptor_sees_every_shape_context() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_context)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let client = TestServiceClient::new(Channel::connect_unix(&path).await.expect("connect"))
        .intercept(stamp_outgoing_context);
    echo_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_send_compressed_gzips_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .send_compressed(),
    );
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_interceptor_rejects_with_typed_status() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_test_blocked_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_client_interceptor_rejects_with_typed_status() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_test_blocked_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_client_interceptor_sees_every_shape_context() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_context)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(stamp_outgoing_context);
    echo_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_handlers_return_typed_status_on_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(FailTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_test_blocked_every_shape(&TestServiceClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_handlers_return_typed_status_on_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(FailTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_test_blocked_every_shape(&TestServiceClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_handlers_return_typed_status_on_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(FailTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_test_blocked_every_shape(&TestServiceClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_handlers_return_typed_status_on_every_shape() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        TestServiceServer::new(FailTestService)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    assert_test_blocked_every_shape(&TestServiceClient::new(
        Channel::connect_unix(&path).await.expect("connect"),
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_handlers_return_typed_status_on_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        TestServiceServer::new(FailTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_test_blocked_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn test_service_typed_google_rpc_status_after_a_streamed_message() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(TypedAfterHeadersTest)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_test_typed_status_after_streamed_message(&TestServiceClient::new(channel(addr).await))
        .await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_typed_google_rpc_status_after_a_streamed_message() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(TypedAfterHeadersTest)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_test_typed_status_after_streamed_message(&TestServiceClient::new(
        tls_channel(addr).await,
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_typed_google_rpc_status_after_a_streamed_message() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(TypedAfterHeadersTest)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_test_typed_status_after_streamed_message(&TestServiceClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_typed_google_rpc_status_after_a_streamed_message() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(TypedAfterHeadersTest)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_test_typed_status_after_streamed_message(&TestServiceClient::new(
        unix_channel(&path).await,
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_typed_google_rpc_status_after_a_streamed_message() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        TestServiceServer::new(TypedAfterHeadersTest)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_test_typed_status_after_streamed_message(&client).await;
    task.abort();
}

async fn assert_hand_written_intercept(ch: Channel, seen: &AtomicUsize) {
    assert_reverser_err_every_shape(&ch, Code::Unauthenticated).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    echo_reverser_every_shape(&ch.clone().intercept(inject_bearer)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
}

#[tokio::test]
async fn service_ext_intercept_wraps_a_hand_written_service() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });
    assert_hand_written_intercept(channel(addr).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn tls_service_ext_intercept_wraps_a_hand_written_service() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_hand_written_intercept(tls_channel(addr).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn mtls_service_ext_intercept_wraps_a_hand_written_service() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    )
    .intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_hand_written_intercept(tls_channel_with(addr, client_tls).await, &seen).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_service_ext_intercept_wraps_a_hand_written_service() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service).serve_unix(sock).await.ok();
    });
    assert_hand_written_intercept(unix_channel(&path).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn from_io_service_ext_intercept_wraps_a_hand_written_service() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_bearer);
    let server = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    assert_hand_written_intercept(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
        &seen,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn service_ext_intercept_rejects_with_typed_status() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service =
        Reverser::new(Arc::clone(&seen)).intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()));
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });

    assert_reverser_blocked_every_shape(&channel(addr).await).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);

    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_rejects_reverser_with_typed_status() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });

    let ch = channel(addr)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_reverser_blocked_every_shape(&ch).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);

    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_reverser_context() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_stamped_context);
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });

    echo_reverser_every_shape(&channel(addr).await.intercept(stamp_outgoing_context)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);

    task.abort();
}

#[tokio::test]
async fn reverser_send_compressed_gzips_every_shape() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    gzip_reverser_every_shape(&channel(addr).await.send_compressed()).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_tls_send_compressed_gzips_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    gzip_reverser_every_shape(&tls_channel(addr).await.send_compressed()).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_mtls_send_compressed_gzips_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    );
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    gzip_reverser_every_shape(&tls_channel_with(addr, client_tls).await.send_compressed()).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_tls_interceptor_rejects_with_typed_status() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service =
        Reverser::new(Arc::clone(&seen)).intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()));
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_reverser_blocked_every_shape(&tls_channel(addr).await).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    task.abort();
}

#[tokio::test]
async fn reverser_tls_client_interceptor_rejects_with_typed_status() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let ch = tls_channel(addr)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_reverser_blocked_every_shape(&ch).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    task.abort();
}

#[tokio::test]
async fn reverser_tls_client_interceptor_sees_every_shape_context() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_stamped_context);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_reverser_every_shape(&tls_channel(addr).await.intercept(stamp_outgoing_context)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_mtls_interceptor_rejects_with_typed_status() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service =
        Reverser::new(Arc::clone(&seen)).intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()));
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reverser_blocked_every_shape(&tls_channel_with(addr, client_tls).await).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    task.abort();
}

#[tokio::test]
async fn reverser_mtls_client_interceptor_rejects_with_typed_status() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let ch = tls_channel_with(addr, client_tls)
        .await
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_reverser_blocked_every_shape(&ch).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    task.abort();
}

#[tokio::test]
async fn reverser_mtls_client_interceptor_sees_every_shape_context() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    )
    .intercept(require_stamped_context);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reverser_every_shape(
        &tls_channel_with(addr, client_tls)
            .await
            .intercept(stamp_outgoing_context),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn reverser_unix_send_compressed_gzips_every_shape() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    gzip_reverser_every_shape(
        &Channel::connect_unix(&path)
            .await
            .expect("connect")
            .send_compressed(),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn reverser_unix_interceptor_rejects_with_typed_status() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let seen = Arc::new(AtomicUsize::new(0));
    let service =
        Reverser::new(Arc::clone(&seen)).intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()));
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    assert_reverser_blocked_every_shape(&Channel::connect_unix(&path).await.expect("connect"))
        .await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn reverser_unix_client_interceptor_rejects_with_typed_status() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let ch = Channel::connect_unix(&path)
        .await
        .expect("connect")
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_reverser_blocked_every_shape(&ch).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn reverser_unix_client_interceptor_sees_every_shape_context() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_stamped_context);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    echo_reverser_every_shape(
        &Channel::connect_unix(&path)
            .await
            .expect("connect")
            .intercept(stamp_outgoing_context),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_from_io_send_compressed_gzips_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_connection(server_io)
            .await
            .ok();
    });
    gzip_reverser_every_shape(
        &Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .send_compressed(),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_from_io_interceptor_rejects_with_typed_status() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service =
        Reverser::new(Arc::clone(&seen)).intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()));
    let task = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    assert_reverser_blocked_every_shape(
        &Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    task.abort();
}

#[tokio::test]
async fn reverser_from_io_client_interceptor_rejects_with_typed_status() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    let ch = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_reverser_blocked_every_shape(&ch).await;
    assert_eq!(seen.load(Ordering::Relaxed), 0);
    task.abort();
}

#[tokio::test]
async fn reverser_from_io_client_interceptor_sees_every_shape_context() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_stamped_context);
    let task = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    echo_reverser_every_shape(
        &Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .intercept(stamp_outgoing_context),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_handlers_return_typed_status_on_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(FailReverser)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_reverser_blocked_every_shape(&channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn reverser_tls_handlers_return_typed_status_on_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(FailReverser)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_reverser_blocked_every_shape(&tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn reverser_mtls_handlers_return_typed_status_on_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(FailReverser)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reverser_blocked_every_shape(&tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn reverser_unix_handlers_return_typed_status_on_every_shape() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        Server::new(FailReverser)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    assert_reverser_blocked_every_shape(&Channel::connect_unix(&path).await.expect("connect"))
        .await;
    task.abort();
}

#[tokio::test]
async fn reverser_from_io_handlers_return_typed_status_on_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        Server::new(FailReverser)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_reverser_blocked_every_shape(
        &Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn reverser_typed_google_rpc_status_after_a_streamed_message() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(TypedAfterHeadersReverser)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_reverser_typed_status_after_streamed_message(&channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn reverser_tls_typed_google_rpc_status_after_a_streamed_message() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(TypedAfterHeadersReverser)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_reverser_typed_status_after_streamed_message(&tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn reverser_mtls_typed_google_rpc_status_after_a_streamed_message() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(TypedAfterHeadersReverser)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reverser_typed_status_after_streamed_message(&tls_channel_with(addr, client_tls).await)
        .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn reverser_unix_typed_google_rpc_status_after_a_streamed_message() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        Server::new(TypedAfterHeadersReverser)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    assert_reverser_typed_status_after_streamed_message(
        &Channel::connect_unix(&path).await.expect("connect"),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn reverser_from_io_typed_google_rpc_status_after_a_streamed_message() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        Server::new(TypedAfterHeadersReverser)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_reverser_typed_status_after_streamed_message(
        &Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn service_ext_interceptors_stack_in_declaration_order() {
    // Same contract as generated_server_interceptors_stack_in_declaration_order,
    // but wrapping the Service itself. Intercepted::intercept is inherent, so
    // svc.intercept(trace).intercept(require_bearer) runs trace first. Onion
    // wrapping would run require_bearer first and return Unauthenticated.
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen))
        .intercept(interceptor_require_trace)
        .intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });
    assert_service_ext_stack(channel(addr).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn tls_service_ext_interceptors_stack_in_declaration_order() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen))
        .intercept(interceptor_require_trace)
        .intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_service_ext_stack(tls_channel(addr).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn mtls_service_ext_interceptors_stack_in_declaration_order() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    )
    .intercept(interceptor_require_trace)
    .intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_service_ext_stack(tls_channel_with(addr, client_tls).await, &seen).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_service_ext_interceptors_stack_in_declaration_order() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen))
        .intercept(interceptor_require_trace)
        .intercept(require_bearer);
    let task = tokio::spawn(async move {
        Server::new(service).serve_unix(sock).await.ok();
    });
    assert_service_ext_stack(unix_channel(&path).await, &seen).await;
    task.abort();
}

#[tokio::test]
async fn from_io_service_ext_interceptors_stack_in_declaration_order() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen))
        .intercept(interceptor_require_trace)
        .intercept(require_bearer);
    let server = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    assert_service_ext_stack(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
        &seen,
    )
    .await;
    server.abort();
}

async fn assert_service_ext_stack(ch: Channel, seen: &AtomicUsize) {
    assert_reverser_err_every_shape(&ch, Code::InvalidArgument).await;
    assert_reverser_err_every_shape(&ch.clone().intercept(inject_bearer), Code::InvalidArgument)
        .await;
    echo_reverser_every_shape(&ch.intercept(interceptor_inject_trace_and_bearer)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
}

struct TenantEcho;

fn tenant_of<T>(request: Request<T>) -> Result<String, Status> {
    let Some(tenant) = request.extensions().get::<String>().cloned() else {
        return Err(Status::internal("missing tenant extension"));
    };
    let (_msg, parts) = request.into_message_and_parts();
    match parts.extensions().get::<String>() {
        Some(same) if same == &tenant => Ok(tenant),
        _ => Err(Status::internal("parts dropped tenant extension")),
    }
}

fn tenant_reply(tenant: String) -> Response<HelloReply> {
    Response::new(common::reply(&tenant))
}

fn tenant_stream(tenant: String) -> Response<pbrs_grpc::Streaming<HelloReply>> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        tx.send(common::reply(&tenant)).await.ok();
    }));
    Response::new(stream)
}

impl Service for TenantEcho {
    const NAME: &'static str = "demo.TenantEcho";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Unary" => {
                rpc.unary(|request: Request<HelloRequest>| async move {
                    Ok(tenant_reply(tenant_of(request)?))
                })
                .await;
            }
            "Server" => {
                rpc.server_streaming(|request: Request<HelloRequest>| async move {
                    Ok(tenant_stream(tenant_of(request)?))
                })
                .await;
            }
            "Client" => {
                rpc.client_streaming(
                    |request: Request<pbrs_grpc::Streaming<HelloRequest>>| async move {
                        Ok(tenant_reply(tenant_of(request)?))
                    },
                )
                .await;
            }
            "Bidi" => {
                rpc.bidi_streaming(
                    |request: Request<pbrs_grpc::Streaming<HelloRequest>>| async move {
                        Ok(tenant_stream(tenant_of(request)?))
                    },
                )
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

fn interceptor_attach_tenant(rpc: &mut Rpc) -> Result<(), Status> {
    let Some(tenant) = rpc.metadata().get("x-tenant").map(str::to_owned) else {
        return Err(Status::unauthenticated("missing x-tenant"));
    };
    rpc.extensions_mut().insert(tenant);
    Ok(())
}

fn stamp_tenant<T>(mut request: Request<T>) -> Request<T> {
    request
        .metadata_mut()
        .insert("x-tenant", "acme")
        .expect("metadata");
    request
}

async fn assert_tenant_echo(ch: &Channel) {
    let denied = ch
        .unary::<HelloRequest, HelloReply>("/demo.TenantEcho/Unary", Request::new(req("ignored")))
        .await
        .expect_err("unary");
    assert_eq!(denied.code(), Code::Unauthenticated);
    let denied = ch
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.TenantEcho/Server",
            Request::new(req("ignored")),
        )
        .await
        .expect_err("server-stream");
    assert_eq!(denied.code(), Code::Unauthenticated);
    let (tx, call) = ch
        .client_streaming::<HelloRequest, HelloReply>("/demo.TenantEcho/Client", Request::new(()));
    let denied = call.await.expect_err("client-stream");
    assert_eq!(denied.code(), Code::Unauthenticated);
    drop(tx);
    let (tx, call) = ch.bidi::<HelloRequest, HelloReply>("/demo.TenantEcho/Bidi", Request::new(()));
    let denied = call.await.expect_err("bidi");
    assert_eq!(denied.code(), Code::Unauthenticated);
    drop(tx);

    let reply = ch
        .unary::<HelloRequest, HelloReply>(
            "/demo.TenantEcho/Unary",
            stamp_tenant(Request::new(req("ignored"))),
        )
        .await
        .expect("unary")
        .into_inner();
    assert_eq!(name_of(&reply), "acme");

    let mut stream = ch
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.TenantEcho/Server",
            stamp_tenant(Request::new(req("ignored"))),
        )
        .await
        .expect("server-stream")
        .into_inner();
    let first = stream.message().await.expect("item").expect("first");
    assert_eq!(name_of(&first), "acme");
    assert!(stream.message().await.expect("end").is_none());

    let (tx, call) = ch.client_streaming::<HelloRequest, HelloReply>(
        "/demo.TenantEcho/Client",
        stamp_tenant(Request::new(())),
    );
    tx.close();
    let reply = call.await.expect("client-stream").into_inner();
    assert_eq!(name_of(&reply), "acme");

    let (tx, call) = ch
        .bidi::<HelloRequest, HelloReply>("/demo.TenantEcho/Bidi", stamp_tenant(Request::new(())));
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    let first = inbound.message().await.expect("item").expect("first");
    assert_eq!(name_of(&first), "acme");
    assert!(inbound.message().await.expect("end").is_none());
}

#[tokio::test]
async fn an_interceptor_can_attach_typed_state_the_handler_reads() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(TenantEcho.intercept(interceptor_attach_tenant))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_tenant_echo(&channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_interceptor_can_attach_typed_state_the_handler_reads() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(TenantEcho.intercept(interceptor_attach_tenant))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_tenant_echo(&tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_interceptor_can_attach_typed_state_the_handler_reads() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(TenantEcho.intercept(interceptor_attach_tenant))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_tenant_echo(&tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_interceptor_can_attach_typed_state_the_handler_reads() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        Server::new(TenantEcho.intercept(interceptor_attach_tenant))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_tenant_echo(&unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_interceptor_can_attach_typed_state_the_handler_reads() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Server::new(TenantEcho.intercept(interceptor_attach_tenant))
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_tenant_echo(
        &Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn router_interceptors_stack_in_declaration_order() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_listener(listener)
            .await
            .ok();
    });

    assert_stack_rejects_auth_without_trace(&GreeterClient::new(channel(addr).await)).await;
    echo_every_shape(
        &GreeterClient::new(channel(addr).await).intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_router_interceptors_stack_in_declaration_order() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(tls_channel(addr).await)).await;
    echo_every_shape(
        &GreeterClient::new(tls_channel(addr).await).intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_router_interceptors_stack_in_declaration_order() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(
        tls_channel_with(addr, client_tls.clone()).await,
    ))
    .await;
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await)
            .intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_router_interceptors_stack_in_declaration_order() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(unix_channel(&path).await)).await;
    echo_every_shape(
        &GreeterClient::new(unix_channel(&path).await)
            .intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_router_interceptors_stack_in_declaration_order() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .intercept(interceptor_require_trace)
            .intercept(require_bearer)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let ch = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_stack_rejects_auth_without_trace(&GreeterClient::new(ch.clone())).await;
    echo_every_shape(
        &GreeterClient::new(ch).intercept(interceptor_inject_trace_and_bearer),
        None,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_the_authority() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let channel = channel(addr).await;
    let want = channel.authority().to_owned();
    let client = GreeterClient::new(channel).intercept(move |call: &mut Outgoing<'_>| {
        if call.authority() != want.as_str() {
            return Err(Status::internal(format!(
                "authority {} want {want}",
                call.authority()
            )));
        }
        Ok(())
    });
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn a_server_interceptor_sees_the_authority() {
    let (addr, listener) = bind().await;
    let want = addr.to_string();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(move |rpc: &mut Rpc| {
                if rpc.authority() != Some(want.as_str()) {
                    return Err(Status::internal(format!(
                        "authority {:?} want {want}",
                        rpc.authority()
                    )));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

fn interceptor_require_https_authority(want: String) -> impl Fn(&mut Rpc) -> Result<(), Status> {
    move |rpc: &mut Rpc| {
        if rpc.authority() != Some(want.as_str()) {
            return Err(Status::internal(format!(
                "authority {:?} want {want}",
                rpc.authority()
            )));
        }
        if rpc.scheme() != Some("https") {
            return Err(Status::internal(format!("scheme {:?}", rpc.scheme())));
        }
        Ok(())
    }
}

fn interceptor_require_client_https_authority(
    want: String,
) -> impl Fn(&mut Outgoing<'_>) -> Result<(), Status> {
    move |call: &mut Outgoing<'_>| {
        if call.authority() != want.as_str() {
            return Err(Status::internal(format!(
                "authority {} want {want}",
                call.authority()
            )));
        }
        if call.scheme() != "https" {
            return Err(Status::internal(format!("scheme {}", call.scheme())));
        }
        Ok(())
    }
}

async fn assert_tls_socket_authority_not_sni(channel: Channel, want: &str) {
    assert_eq!(channel.authority(), want);
    assert_eq!(channel.scheme(), "https");
    assert_ne!(
        channel.authority(),
        "localhost",
        "TLS :authority follows Target, not SNI"
    );
    let client = GreeterClient::new(channel);
    assert_eq!(client.authority(), want);
    assert_eq!(client.scheme(), "https");
    let client = client.intercept(interceptor_require_client_https_authority(want.to_owned()));
    echo_every_shape(&client, None).await;
}

#[tokio::test]
async fn tls_interceptors_see_socket_authority_not_sni() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let want = addr.to_string();
    let server_want = want.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_https_authority(server_want))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_tls_socket_authority_not_sni(tls_channel(addr).await, &want).await;
    task.abort();
}

#[tokio::test]
async fn mtls_interceptors_see_socket_authority_not_sni() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let want = addr.to_string();
    let server_want = want.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_https_authority(server_want))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_tls_socket_authority_not_sni(tls_channel_with(addr, client_tls).await, &want).await;
    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_cannot_insert_reserved_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(interceptor_reserved_metadata);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    task.abort();
}

fn interceptor_reserved_metadata(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.metadata_mut()
        .insert("grpc-previous-rpc-attempts", "1")?;
    Ok(())
}

fn reserved_test(channel: Channel) -> TestServiceClient {
    TestServiceClient::new(channel).intercept(interceptor_reserved_metadata)
}

fn reserved_channel(channel: Channel) -> Channel {
    channel.intercept(interceptor_reserved_metadata)
}

#[tokio::test]
async fn a_tls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client =
        GreeterClient::new(tls_channel(addr).await).intercept(interceptor_reserved_metadata);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(interceptor_reserved_metadata);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_cannot_insert_reserved_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    let client =
        GreeterClient::new(unix_channel(&path).await).intercept(interceptor_reserved_metadata);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_cannot_insert_reserved_metadata() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(interceptor_reserved_metadata);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    server.abort();
}

#[tokio::test]
async fn a_test_client_interceptor_cannot_insert_reserved_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(&reserved_test(channel(addr).await), Code::InvalidArgument)
        .await;
    task.abort();
}

#[tokio::test]
async fn a_test_tls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(
        &reserved_test(tls_channel(addr).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_err_on_test_every_shape(
        &reserved_test(tls_channel_with(addr, client_tls).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_cannot_insert_reserved_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(
        &reserved_test(unix_channel(&path).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_cannot_insert_reserved_metadata() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(
        &reserved_test(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::InvalidArgument,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_reverser_client_interceptor_cannot_insert_reserved_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &reserved_channel(channel(addr).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &reserved_channel(tls_channel(addr).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_cannot_insert_reserved_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reverser_err_every_shape(
        &reserved_channel(tls_channel_with(addr, client_tls).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_cannot_insert_reserved_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &reserved_channel(unix_channel(&path).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_cannot_insert_reserved_metadata() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &reserved_channel(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::InvalidArgument,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(interceptor_hop_by_hop);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    task.abort();
}

fn interceptor_hop_by_hop(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.metadata_mut().insert("connection", "close")?;
    Ok(())
}

fn hop_test(channel: Channel) -> TestServiceClient {
    TestServiceClient::new(channel).intercept(interceptor_hop_by_hop)
}

fn hop_channel(channel: Channel) -> Channel {
    channel.intercept(interceptor_hop_by_hop)
}

#[tokio::test]
async fn a_tls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await).intercept(interceptor_hop_by_hop);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(interceptor_hop_by_hop);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await).intercept(interceptor_hop_by_hop);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(interceptor_hop_by_hop);
    assert_err_on_every_shape(&client, Code::InvalidArgument).await;
    server.abort();
}

#[tokio::test]
async fn a_test_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(&hop_test(channel(addr).await), Code::InvalidArgument).await;
    task.abort();
}

#[tokio::test]
async fn a_test_tls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(&hop_test(tls_channel(addr).await), Code::InvalidArgument).await;
    task.abort();
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_err_on_test_every_shape(
        &hop_test(tls_channel_with(addr, client_tls).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(&hop_test(unix_channel(&path).await), Code::InvalidArgument)
        .await;
    task.abort();
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(
        &hop_test(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::InvalidArgument,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_reverser_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(&hop_channel(channel(addr).await), Code::InvalidArgument).await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(&hop_channel(tls_channel(addr).await), Code::InvalidArgument)
        .await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reverser_err_every_shape(
        &hop_channel(tls_channel_with(addr, client_tls).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &hop_channel(unix_channel(&path).await),
        Code::InvalidArgument,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_cannot_insert_hop_by_hop_headers() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &hop_channel(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::InvalidArgument,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(interceptor_fail_before_open);
    assert_err_on_every_shape(&client, Code::FailedPrecondition).await;
    task.abort();
}

fn interceptor_fail_before_open(_: &mut Outgoing<'_>) -> Result<(), Status> {
    Err(Status::failed_precondition("blocked locally"))
}

fn fail_open_test(channel: Channel) -> TestServiceClient {
    TestServiceClient::new(channel).intercept(interceptor_fail_before_open)
}

fn fail_open_channel(channel: Channel) -> Channel {
    channel.intercept(interceptor_fail_before_open)
}

#[tokio::test]
async fn a_tls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client =
        GreeterClient::new(tls_channel(addr).await).intercept(interceptor_fail_before_open);
    assert_err_on_every_shape(&client, Code::FailedPrecondition).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(interceptor_fail_before_open);
    assert_err_on_every_shape(&client, Code::FailedPrecondition).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    let client =
        GreeterClient::new(unix_channel(&path).await).intercept(interceptor_fail_before_open);
    assert_err_on_every_shape(&client, Code::FailedPrecondition).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(interceptor_fail_before_open);
    assert_err_on_every_shape(&client, Code::FailedPrecondition).await;
    server.abort();
}

#[tokio::test]
async fn a_test_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(
        &fail_open_test(channel(addr).await),
        Code::FailedPrecondition,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_test_tls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(
        &fail_open_test(tls_channel(addr).await),
        Code::FailedPrecondition,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_err_on_test_every_shape(
        &fail_open_test(tls_channel_with(addr, client_tls).await),
        Code::FailedPrecondition,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(
        &fail_open_test(unix_channel(&path).await),
        Code::FailedPrecondition,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_err_on_test_every_shape(
        &fail_open_test(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::FailedPrecondition,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_reverser_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &fail_open_channel(channel(addr).await),
        Code::FailedPrecondition,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &fail_open_channel(tls_channel(addr).await),
        Code::FailedPrecondition,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reverser_err_every_shape(
        &fail_open_channel(tls_channel_with(addr, client_tls).await),
        Code::FailedPrecondition,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &fail_open_channel(unix_channel(&path).await),
        Code::FailedPrecondition,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_can_fail_the_rpc_before_the_stream_opens() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_reverser_err_every_shape(
        &fail_open_channel(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        Code::FailedPrecondition,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn client_interceptors_run_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create(Channel::connect_lazy(addr).expect("lazy"), &ran);
    assert_interceptors_run_on_create(&client, &ran);
}

fn intercept_counts_create(channel: Channel, ran: &Arc<AtomicUsize>) -> GreeterClient {
    let flag = Arc::clone(ran);
    GreeterClient::new(channel).intercept(move |_: &mut Outgoing<'_>| {
        flag.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
}

fn assert_interceptors_run_on_create(client: &GreeterClient, ran: &Arc<AtomicUsize>) {
    let unary = client.say_hello(Request::new(req("ada")));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "unary interceptor must run when the method returns"
    );
    drop(unary);

    let streaming = client.server_hello(Request::new(req("ada")));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "server-streaming interceptor must run when the method returns"
    );
    drop(streaming);

    let (tx, call) = client.client_hello(Request::new(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        3,
        "client-streaming interceptor must run when the method returns"
    );
    drop(call);
    drop(tx);

    let (tx, call) = client.stream_hello(Request::new(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        4,
        "bidi interceptor must run when the method returns"
    );
    drop(call);
    drop(tx);
}

fn intercept_counts_create_test(channel: Channel, ran: &Arc<AtomicUsize>) -> TestServiceClient {
    let flag = Arc::clone(ran);
    TestServiceClient::new(channel).intercept(move |_: &mut Outgoing<'_>| {
        flag.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
}

fn assert_interceptors_run_on_create_test(client: &TestServiceClient, ran: &Arc<AtomicUsize>) {
    let unary = client.empty_call(Request::new(Empty::new()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "unary interceptor must run when the method returns"
    );
    drop(unary);

    let streaming = client.streaming_output_call(Request::new(StreamingOutputCallRequest::new()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "server-streaming interceptor must run when the method returns"
    );
    drop(streaming);

    let (tx, call) = client.streaming_input_call(Request::new(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        3,
        "client-streaming interceptor must run when the method returns"
    );
    drop(call);
    drop(tx);

    let (tx, call) = client.full_duplex_call(Request::new(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        4,
        "bidi interceptor must run when the method returns"
    );
    drop(call);
    drop(tx);
}

fn intercept_counts_create_channel(channel: Channel, ran: &Arc<AtomicUsize>) -> Channel {
    let flag = Arc::clone(ran);
    channel.intercept(move |_: &mut Outgoing<'_>| {
        flag.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
}

fn assert_interceptors_run_on_create_channel(channel: &Channel, ran: &Arc<AtomicUsize>) {
    let unary = channel
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(req("stressed")));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "unary interceptor must run when the method returns"
    );
    drop(unary);

    let streaming = channel.server_streaming::<HelloRequest, HelloReply>(
        "/demo.Reverser/Server",
        Request::new(req("stressed")),
    );
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "server-streaming interceptor must run when the method returns"
    );
    drop(streaming);

    let (tx, call) = channel
        .client_streaming::<HelloRequest, HelloReply>("/demo.Reverser/Client", Request::new(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        3,
        "client-streaming interceptor must run when the method returns"
    );
    drop(call);
    drop(tx);

    let (tx, call) =
        channel.bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", Request::new(()));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        4,
        "bidi interceptor must run when the method returns"
    );
    drop(call);
    drop(tx);
}

#[tokio::test]
async fn a_tls_client_interceptor_runs_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = intercept_counts_create(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create(&client, &ran);
}

#[tokio::test]
async fn an_mtls_client_interceptor_runs_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = intercept_counts_create(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create(&client, &ran);
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_runs_when_the_call_is_created() {
    let (path, _guard) = unix_test_path();
    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create(Channel::connect_unix_lazy(&path).expect("lazy"), &ran);
    assert_interceptors_run_on_create(&client, &ran);
}

#[tokio::test]
async fn a_from_io_client_interceptor_runs_when_the_call_is_created() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
        &ran,
    );
    assert_interceptors_run_on_create(&client, &ran);
    server.abort();
}

#[tokio::test]
async fn test_client_interceptors_run_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create_test(Channel::connect_lazy(addr).expect("lazy"), &ran);
    assert_interceptors_run_on_create_test(&client, &ran);
}

#[tokio::test]
async fn a_test_tls_client_interceptor_runs_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = intercept_counts_create_test(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_test(&client, &ran);
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_runs_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = intercept_counts_create_test(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_test(&client, &ran);
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_runs_when_the_call_is_created() {
    let (path, _guard) = unix_test_path();
    let ran = Arc::new(AtomicUsize::new(0));
    let client =
        intercept_counts_create_test(Channel::connect_unix_lazy(&path).expect("lazy"), &ran);
    assert_interceptors_run_on_create_test(&client, &ran);
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_runs_when_the_call_is_created() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let ran = Arc::new(AtomicUsize::new(0));
    let client = intercept_counts_create_test(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
        &ran,
    );
    assert_interceptors_run_on_create_test(&client, &ran);
    server.abort();
}

#[tokio::test]
async fn reverser_client_interceptors_run_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let channel = intercept_counts_create_channel(Channel::connect_lazy(addr).expect("lazy"), &ran);
    assert_interceptors_run_on_create_channel(&channel, &ran);
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_runs_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = intercept_counts_create_channel(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_channel(&channel, &ran);
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_runs_when_the_call_is_created() {
    let (addr, listener) = bind().await;
    drop(listener);

    let ran = Arc::new(AtomicUsize::new(0));
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = intercept_counts_create_channel(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
        &ran,
    );
    assert_interceptors_run_on_create_channel(&channel, &ran);
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_runs_when_the_call_is_created() {
    let (path, _guard) = unix_test_path();
    let ran = Arc::new(AtomicUsize::new(0));
    let channel =
        intercept_counts_create_channel(Channel::connect_unix_lazy(&path).expect("lazy"), &ran);
    assert_interceptors_run_on_create_channel(&channel, &ran);
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_runs_when_the_call_is_created() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let ran = Arc::new(AtomicUsize::new(0));
    let channel = intercept_counts_create_channel(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
        &ran,
    );
    assert_interceptors_run_on_create_channel(&channel, &ran);
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_the_method_path() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.metadata().get("x-path") != Some(rpc.path()) {
                    return Err(Status::invalid_argument(format!(
                        "x-path {:?} path {}",
                        rpc.metadata().get("x-path"),
                        rpc.path()
                    )));
                }
                if rpc.metadata().get("x-service") != Some(rpc.service()) {
                    return Err(Status::invalid_argument(format!(
                        "x-service {:?} service {}",
                        rpc.metadata().get("x-service"),
                        rpc.service()
                    )));
                }
                if rpc.metadata().get("x-method") != Some(rpc.method()) {
                    return Err(Status::invalid_argument(format!(
                        "x-method {:?} method {}",
                        rpc.metadata().get("x-method"),
                        rpc.method()
                    )));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        let path = call.path();
        call.metadata_mut().insert("x-path", path)?;
        let service = call.service();
        call.metadata_mut().set("x-service", service)?;
        let method = call.method();
        call.metadata_mut().set("x-method", method)?;
        Ok(())
    });
    echo_every_shape(&client, None).await;

    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_every_shape_context() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_context)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(stamp_outgoing_context);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_interceptor_can_set_a_deadline() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.set_timeout(Duration::from_millis(40));
        Ok(())
    });
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;

    task.abort();
}

fn interceptor_set_timeout(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.set_timeout(Duration::from_millis(40));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tls_client_interceptor_can_set_a_deadline() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await).intercept(interceptor_set_timeout);
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_mtls_client_interceptor_can_set_a_deadline() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(interceptor_set_timeout);
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_unix_client_interceptor_can_set_a_deadline() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await).intercept(interceptor_set_timeout);
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_from_io_client_interceptor_can_set_a_deadline() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(interceptor_set_timeout);
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_a_deadline_instant() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });

    let timeout = Duration::from_secs(5);
    let client = GreeterClient::new(channel(addr).await)
        .timeout(timeout)
        .intercept(require_deadline_instant(timeout));
    echo_every_shape(&client, None).await;

    task.abort();
}

#[tokio::test]
async fn a_tls_client_interceptor_sees_a_deadline_instant() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let timeout = Duration::from_secs(5);
    let client = GreeterClient::new(tls_channel(addr).await)
        .timeout(timeout)
        .intercept(require_deadline_instant(timeout));
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_sees_a_deadline_instant() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let timeout = Duration::from_secs(5);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .timeout(timeout)
        .intercept(require_deadline_instant(timeout));
    echo_every_shape(&client, None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_sees_a_deadline_instant() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    let timeout = Duration::from_secs(5);
    let client = GreeterClient::new(unix_channel(&path).await)
        .timeout(timeout)
        .intercept(require_deadline_instant(timeout));
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_sees_a_deadline_instant() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let timeout = Duration::from_secs(5);
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .timeout(timeout)
    .intercept(require_deadline_instant(timeout));
    echo_every_shape(&client, None).await;
    server.abort();
}

fn channel_timeout_cfg() -> ChannelConfig {
    ChannelConfig::new().timeout(Duration::from_millis(40))
}

async fn assert_channel_timeout_expires(ch: Channel) {
    assert_deadline_quickly_on_every_shape(
        &GreeterClient::new(ch).timeout(Duration::from_millis(40)),
        None,
        Duration::from_millis(150),
    )
    .await;
}

async fn assert_channel_config_timeout(ch: Channel) {
    assert_deadline_quickly_on_every_shape(
        &GreeterClient::new(ch),
        None,
        Duration::from_millis(150),
    )
    .await;
}

async fn assert_request_timeout_wins(ch: Channel) {
    slow_every_shape(
        &GreeterClient::new(ch).timeout(Duration::from_millis(40)),
        Some(Duration::from_secs(5)),
    )
    .await;
}

#[tokio::test]
async fn a_channel_timeout_expires_when_the_request_omits_one() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    assert_channel_timeout_expires(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_channel_timeout_expires_when_the_request_omits_one() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_channel_timeout_expires(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_channel_timeout_expires_when_the_request_omits_one() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_channel_timeout_expires(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_channel_timeout_expires_when_the_request_omits_one() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_unix(sock).await.ok();
    });
    assert_channel_timeout_expires(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_channel_timeout_expires_when_the_request_omits_one() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_channel_timeout_expires(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_channel_config_timeout_is_the_default_rpc_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    assert_channel_config_timeout(channel_cfg(addr, channel_timeout_cfg()).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_channel_config_timeout_is_the_default_rpc_deadline() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_channel_config_timeout(tls_channel_cfg(addr, client_tls, channel_timeout_cfg()).await)
        .await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_channel_config_timeout_is_the_default_rpc_deadline() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_channel_config_timeout(tls_channel_cfg(addr, client_tls, channel_timeout_cfg()).await)
        .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_channel_config_timeout_is_the_default_rpc_deadline() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_unix(sock).await.ok();
    });
    assert_channel_config_timeout(unix_channel_with(&path, channel_timeout_cfg()).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_channel_config_timeout_is_the_default_rpc_deadline() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_channel_config_timeout(
        Channel::from_io_with(client_io, "localhost", channel_timeout_cfg())
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_request_timeout_wins_over_the_channel_default() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    assert_request_timeout_wins(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_request_timeout_wins_over_the_channel_default() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_request_timeout_wins(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_request_timeout_wins_over_the_channel_default() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_request_timeout_wins(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_request_timeout_wins_over_the_channel_default() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_unix(sock).await.ok();
    });
    assert_request_timeout_wins(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_request_timeout_wins_over_the_channel_default() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_request_timeout_wins(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_clear_the_channel_timeout() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    let client = clear_timeout_client(channel(addr).await);
    slow_every_shape(&client, None).await;
    task.abort();
}

fn interceptor_clear_timeout(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.clear_timeout();
    Ok(())
}

fn clear_timeout_client(channel: Channel) -> GreeterClient {
    GreeterClient::new(channel)
        .timeout(Duration::from_millis(40))
        .intercept(interceptor_clear_timeout)
}

#[tokio::test]
async fn a_tls_client_interceptor_can_clear_the_channel_timeout() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = clear_timeout_client(tls_channel(addr).await);
    slow_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_can_clear_the_channel_timeout() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = clear_timeout_client(tls_channel_with(addr, client_tls).await);
    slow_every_shape(&client, None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_can_clear_the_channel_timeout() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_unix(sock).await.ok();
    });
    let client = clear_timeout_client(unix_channel(&path).await);
    slow_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_can_clear_the_channel_timeout() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = clear_timeout_client(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    slow_every_shape(&client, None).await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = overlay_after_clear_client(Channel::connect_lazy(addr).expect("lazy"));
    assert_cleared_wait_fails_fast(&client).await;
}

fn overlays_survive_clear(call: &mut Outgoing<'_>) -> Result<(), Status> {
    if call.rpc_timeout() != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!(
            "rpc_timeout {:?}",
            call.rpc_timeout()
        )));
    }
    if !call.waits_for_ready() {
        return Err(Status::internal("waits_for_ready overlay"));
    }
    if !call.compresses_outbound() {
        return Err(Status::internal("compresses_outbound overlay"));
    }
    if call.timeout() != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!("timeout {:?}", call.timeout())));
    }
    if !call.wait_for_ready_is_set() || !call.wait_for_ready() {
        return Err(Status::internal("wait-for-ready not filled"));
    }
    if !call.compress_is_set() || !call.compress() {
        return Err(Status::internal("compress not filled"));
    }
    call.clear_timeout();
    call.clear_wait_for_ready();
    call.clear_compress();
    if call.rpc_timeout() != Some(Duration::from_secs(5))
        || !call.waits_for_ready()
        || !call.compresses_outbound()
    {
        return Err(Status::internal("overlays vanished after clear"));
    }
    Ok(())
}

fn overlay_after_clear_channel(channel: Channel) -> Channel {
    channel
        .timeout(Duration::from_secs(5))
        .wait_for_ready()
        .send_compressed()
        .intercept(overlays_survive_clear)
}

fn overlay_after_clear_client(channel: Channel) -> GreeterClient {
    GreeterClient::new(overlay_after_clear_channel(channel))
}

async fn assert_cleared_wait_fails_fast(client: &GreeterClient) {
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_every_shape(client, Code::Unavailable),
    )
    .await
    .expect("cleared wait-for-ready hung");
}

async fn assert_cleared_wait_fails_fast_test(client: &TestServiceClient) {
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_test_every_shape(client, Code::Unavailable),
    )
    .await
    .expect("cleared wait-for-ready hung");
}

async fn assert_cleared_wait_fails_fast_reverser(channel: &Channel) {
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reverser_err_every_shape(channel, Code::Unavailable),
    )
    .await
    .expect("cleared wait-for-ready hung");
}

#[tokio::test]
async fn a_tls_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client =
        overlay_after_clear_client(Channel::connect_tls_lazy(addr, client_tls).expect("lazy"));
    assert_cleared_wait_fails_fast(&client).await;
}

#[tokio::test]
async fn an_mtls_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client =
        overlay_after_clear_client(Channel::connect_tls_lazy(addr, client_tls).expect("lazy"));
    assert_cleared_wait_fails_fast(&client).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_sees_channel_overlays_after_clear() {
    let (path, _guard) = unix_test_path();
    let client = overlay_after_clear_client(Channel::connect_unix_lazy(&path).expect("lazy"));
    assert_cleared_wait_fails_fast(&client).await;
}

#[tokio::test]
async fn a_from_io_client_interceptor_sees_channel_overlays_after_clear() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = overlay_after_clear_client(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    echo_every_shape(&client, None).await;
    server.abort();
}

#[tokio::test]
async fn a_test_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = TestServiceClient::new(overlay_after_clear_channel(
        Channel::connect_lazy(addr).expect("lazy"),
    ));
    assert_cleared_wait_fails_fast_test(&client).await;
}

#[tokio::test]
async fn a_test_tls_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = TestServiceClient::new(overlay_after_clear_channel(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
    ));
    assert_cleared_wait_fails_fast_test(&client).await;
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::new(overlay_after_clear_channel(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
    ));
    assert_cleared_wait_fails_fast_test(&client).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_sees_channel_overlays_after_clear() {
    let (path, _guard) = unix_test_path();
    let client = TestServiceClient::new(overlay_after_clear_channel(
        Channel::connect_unix_lazy(&path).expect("lazy"),
    ));
    assert_cleared_wait_fails_fast_test(&client).await;
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_sees_channel_overlays_after_clear() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(overlay_after_clear_channel(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ));
    echo_test_every_shape(&client).await;
    server.abort();
}

#[tokio::test]
async fn a_reverser_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = overlay_after_clear_channel(Channel::connect_lazy(addr).expect("lazy"));
    assert_cleared_wait_fails_fast_reverser(&channel).await;
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel =
        overlay_after_clear_channel(Channel::connect_tls_lazy(addr, client_tls).expect("lazy"));
    assert_cleared_wait_fails_fast_reverser(&channel).await;
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_sees_channel_overlays_after_clear() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel =
        overlay_after_clear_channel(Channel::connect_tls_lazy(addr, client_tls).expect("lazy"));
    assert_cleared_wait_fails_fast_reverser(&channel).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_sees_channel_overlays_after_clear() {
    let (path, _guard) = unix_test_path();
    let channel = overlay_after_clear_channel(Channel::connect_unix_lazy(&path).expect("lazy"));
    assert_cleared_wait_fails_fast_reverser(&channel).await;
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_sees_channel_overlays_after_clear() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let server = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    let channel = overlay_after_clear_channel(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    echo_reverser_every_shape(&channel).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client =
        GreeterClient::new(channel(addr).await.send_compressed()).intercept(reapply_channel_gzip);
    gzip_every_shape(&client).await;
    task.abort();
}

fn reapply_channel_gzip(call: &mut Outgoing<'_>) -> Result<(), Status> {
    if !call.compresses_outbound() {
        return Err(Status::internal("compresses_outbound overlay"));
    }
    call.clear_compress();
    call.set_compress(call.compresses_outbound());
    Ok(())
}

#[tokio::test]
async fn a_tls_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await.send_compressed())
        .intercept(reapply_channel_gzip);
    gzip_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await.send_compressed())
        .intercept(reapply_channel_gzip);
    gzip_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await.send_compressed())
        .intercept(reapply_channel_gzip);
    gzip_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .send_compressed(),
    )
    .intercept(reapply_channel_gzip);
    gzip_every_shape(&client).await;
    server.abort();
}

#[tokio::test]
async fn a_test_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = TestServiceClient::new(channel(addr).await.send_compressed())
        .intercept(reapply_channel_gzip);
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_test_tls_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = TestServiceClient::new(tls_channel(addr).await.send_compressed())
        .intercept(reapply_channel_gzip);
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::new(tls_channel_with(addr, client_tls).await.send_compressed())
        .intercept(reapply_channel_gzip);
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = TestServiceClient::new(unix_channel(&path).await.send_compressed())
        .intercept(reapply_channel_gzip);
    gzip_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .send_compressed()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .send_compressed(),
    )
    .intercept(reapply_channel_gzip);
    gzip_test_every_shape(&client).await;
    server.abort();
}

#[tokio::test]
async fn a_reverser_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    gzip_reverser_every_shape(
        &channel(addr)
            .await
            .send_compressed()
            .intercept(reapply_channel_gzip),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    gzip_reverser_every_shape(
        &tls_channel(addr)
            .await
            .send_compressed()
            .intercept(reapply_channel_gzip),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    );
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    gzip_reverser_every_shape(
        &tls_channel_with(addr, client_tls)
            .await
            .send_compressed()
            .intercept(reapply_channel_gzip),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_unix(sock)
            .await
            .ok();
    });
    gzip_reverser_every_shape(
        &unix_channel(&path)
            .await
            .send_compressed()
            .intercept(reapply_channel_gzip),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_can_reapply_channel_gzip_after_clear() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let server = tokio::spawn(async move {
        Server::new(service)
            .send_compressed()
            .serve_connection(server_io)
            .await
            .ok();
    });
    gzip_reverser_every_shape(
        &Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .send_compressed()
            .intercept(reapply_channel_gzip),
    )
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    server.abort();
}

#[derive(Clone)]
struct Tenant(String);

fn interceptor_stamp_tenant(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let Some(tenant) = call.extensions().get::<Tenant>().cloned() else {
        return Err(Status::internal("missing Tenant"));
    };
    call.metadata_mut().insert("x-tenant", tenant.0)?;
    Ok(())
}

fn require_tenant(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.metadata().get("x-tenant") != Some("acme") {
        return Err(Status::unauthenticated("missing tenant"));
    }
    Ok(())
}

fn with_tenant<T>(mut request: Request<T>) -> Request<T> {
    request.extensions_mut().insert(Tenant("acme".into()));
    request
}

async fn echo_tenant_every_shape(client: &GreeterClient) {
    let reply = client
        .say_hello(with_tenant(Request::new(req("ada"))))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let mut stream = client
        .server_hello(with_tenant(Request::new(req("ada"))))
        .await
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    let (tx, call) = client.client_hello(with_tenant(Request::new(())));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let (tx, call) = client.stream_hello(with_tenant(Request::new(())));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert_err_on_every_shape(client, Code::Internal).await;
}

async fn echo_tenant_test_every_shape(client: &TestServiceClient) {
    client
        .empty_call(with_tenant(Request::new(Empty::new())))
        .await
        .expect("unary");

    let mut stream = client
        .streaming_output_call(with_tenant(Request::new(StreamingOutputCallRequest::new())))
        .await
        .expect("server-stream")
        .into_inner();
    assert!(
        stream.message().await.expect("end").is_none(),
        "empty StreamingOutputCall plan must end"
    );

    let (tx, call) = client.streaming_input_call(with_tenant(Request::new(())));
    tx.close();
    call.await.expect("client-stream");

    let (tx, call) = client.full_duplex_call(with_tenant(Request::new(())));
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    assert!(
        inbound.message().await.expect("end").is_none(),
        "empty FullDuplexCall must end"
    );
    assert_err_on_test_every_shape(client, Code::Internal).await;
}

async fn echo_tenant_reverser_every_shape(channel: &Channel) {
    let reply: HelloReply = channel
        .unary(
            "/demo.Reverser/Reverse",
            with_tenant(Request::new(req("stressed"))),
        )
        .await
        .expect("unary")
        .into_inner();
    assert_eq!(name_of(&reply), "desserts");

    let mut stream = channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            with_tenant(Request::new(req("stressed"))),
        )
        .await
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "desserts");
    assert!(stream.message().await.expect("end").is_none());

    let (tx, call) = channel.client_streaming::<HelloRequest, HelloReply>(
        "/demo.Reverser/Client",
        with_tenant(Request::new(())),
    );
    tx.send(req("stressed")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "desserts");

    let (tx, call) = channel
        .bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", with_tenant(Request::new(())));
    tx.send(req("stressed")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "desserts");
    assert!(inbound.message().await.expect("end").is_none());
    assert_reverser_err_every_shape(channel, Code::Internal).await;
}

#[tokio::test]
async fn a_client_interceptor_reads_caller_extensions() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_tenant)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(interceptor_stamp_tenant);
    echo_tenant_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_tenant)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await).intercept(interceptor_stamp_tenant);
    echo_tenant_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_tenant)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(interceptor_stamp_tenant);
    echo_tenant_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_reads_caller_extensions() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_tenant)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await).intercept(interceptor_stamp_tenant);
    echo_tenant_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_reads_caller_extensions() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_tenant)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(interceptor_stamp_tenant);
    echo_tenant_every_shape(&client).await;
    server.abort();
}

#[tokio::test]
async fn a_test_client_interceptor_reads_caller_extensions() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_tenant)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = TestServiceClient::new(channel(addr).await).intercept(interceptor_stamp_tenant);
    echo_tenant_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_test_tls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_tenant)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client =
        TestServiceClient::new(tls_channel(addr).await).intercept(interceptor_stamp_tenant);
    echo_tenant_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_tenant)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(interceptor_stamp_tenant);
    echo_tenant_test_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_reads_caller_extensions() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_tenant)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client =
        TestServiceClient::new(unix_channel(&path).await).intercept(interceptor_stamp_tenant);
    echo_tenant_test_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_reads_caller_extensions() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_tenant)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(interceptor_stamp_tenant);
    echo_tenant_test_every_shape(&client).await;
    server.abort();
}

#[tokio::test]
async fn a_reverser_client_interceptor_reads_caller_extensions() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_tenant);
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });
    let channel = channel(addr).await.intercept(interceptor_stamp_tenant);
    echo_tenant_reverser_every_shape(&channel).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_tenant);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let channel = tls_channel(addr).await.intercept(interceptor_stamp_tenant);
    echo_tenant_reverser_every_shape(&channel).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_reads_caller_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    )
    .intercept(require_tenant);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = tls_channel_with(addr, client_tls)
        .await
        .intercept(interceptor_stamp_tenant);
    echo_tenant_reverser_every_shape(&channel).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_reads_caller_extensions() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_tenant);
    let task = tokio::spawn(async move {
        Server::new(service).serve_unix(sock).await.ok();
    });
    let channel = unix_channel(&path)
        .await
        .intercept(interceptor_stamp_tenant);
    echo_tenant_reverser_every_shape(&channel).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_reads_caller_extensions() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_tenant);
    let server = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .intercept(interceptor_stamp_tenant);
    echo_tenant_reverser_every_shape(&channel).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_the_h2c_scheme() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.metadata().get("x-scheme") != Some("http") {
                    return Err(Status::internal(format!(
                        "x-scheme {:?}",
                        rpc.metadata().get("x-scheme")
                    )));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        if call.scheme() != "http" {
            return Err(Status::internal(format!("scheme {}", call.scheme())));
        }
        let scheme = call.scheme();
        call.metadata_mut().set("x-scheme", scheme)?;
        Ok(())
    });
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_the_user_agent() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = user_agent_client(channel(addr).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

fn interceptor_stamp_user_agent(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let ua = call.user_agent();
    if !ua.starts_with("inventory/2.1 ") || !ua.contains("pbrs-grpc/") {
        return Err(Status::internal(format!("user-agent {ua}")));
    }
    call.metadata_mut().set("x-ua", ua)?;
    Ok(())
}

fn require_stamped_user_agent(rpc: &mut Rpc) -> Result<(), Status> {
    let ua = rpc.metadata().get("user-agent").unwrap_or("");
    let stamped = rpc.metadata().get("x-ua").unwrap_or("");
    if stamped != ua || !ua.starts_with("inventory/2.1 ") || !ua.contains("pbrs-grpc/") {
        return Err(Status::internal(format!("ua {ua:?} x-ua {stamped:?}")));
    }
    Ok(())
}

fn user_agent_client(channel: Channel) -> GreeterClient {
    GreeterClient::new(channel.user_agent("inventory/2.1").expect("user-agent"))
        .intercept(interceptor_stamp_user_agent)
}

fn user_agent_test(channel: Channel) -> TestServiceClient {
    TestServiceClient::new(channel.user_agent("inventory/2.1").expect("user-agent"))
        .intercept(interceptor_stamp_user_agent)
}

fn user_agent_channel(channel: Channel) -> Channel {
    channel
        .user_agent("inventory/2.1")
        .expect("user-agent")
        .intercept(interceptor_stamp_user_agent)
}

#[tokio::test]
async fn a_tls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = user_agent_client(tls_channel(addr).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = user_agent_client(tls_channel_with(addr, client_tls).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_sees_the_user_agent() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = user_agent_client(unix_channel(&path).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_sees_the_user_agent() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = user_agent_client(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    echo_every_shape(&client, None).await;
    server.abort();
}

#[tokio::test]
async fn a_test_client_interceptor_sees_the_user_agent() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    echo_test_every_shape(&user_agent_test(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_test_tls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_test_every_shape(&user_agent_test(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_test_every_shape(&user_agent_test(tls_channel_with(addr, client_tls).await)).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_sees_the_user_agent() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    echo_test_every_shape(&user_agent_test(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_sees_the_user_agent() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_stamped_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_test_every_shape(&user_agent_test(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn a_reverser_client_interceptor_sees_the_user_agent() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_stamped_user_agent);
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });
    echo_reverser_every_shape(&user_agent_channel(channel(addr).await)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_stamped_user_agent);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_reverser_every_shape(&user_agent_channel(tls_channel(addr).await)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_sees_the_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    )
    .intercept(require_stamped_user_agent);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reverser_every_shape(&user_agent_channel(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_sees_the_user_agent() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_stamped_user_agent);
    let task = tokio::spawn(async move {
        Server::new(service).serve_unix(sock).await.ok();
    });
    echo_reverser_every_shape(&user_agent_channel(unix_channel(&path).await)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_sees_the_user_agent() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_stamped_user_agent);
    let server = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    echo_reverser_every_shape(&user_agent_channel(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_sees_message_limits() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let client = limits_client(channel_with(addr, limits_config()).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

fn test_message_limits() -> MessageLimits {
    MessageLimits::new()
        .with_max_decoding(16)
        .with_max_encoding(32)
}

fn limits_config() -> ChannelConfig {
    ChannelConfig::new().message_limits(test_message_limits())
}

fn interceptor_require_limits(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let want = test_message_limits();
    if call.limits() != want {
        return Err(Status::internal(format!("limits {:?}", call.limits())));
    }
    Ok(())
}

fn limits_client(channel: Channel) -> GreeterClient {
    GreeterClient::new(channel).intercept(interceptor_require_limits)
}

fn limits_test(channel: Channel) -> TestServiceClient {
    TestServiceClient::new(channel).intercept(interceptor_require_limits)
}

fn limits_channel(channel: Channel) -> Channel {
    channel.intercept(interceptor_require_limits)
}

#[tokio::test]
async fn a_tls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = limits_client(tls_channel_cfg(addr, client_tls, limits_config()).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = limits_client(tls_channel_cfg(addr, client_tls, limits_config()).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_sees_message_limits() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    let client = limits_client(unix_channel_with(&path, limits_config()).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_sees_message_limits() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = limits_client(
        Channel::from_io_with(client_io, "localhost", limits_config())
            .await
            .expect("from_io"),
    );
    echo_every_shape(&client, None).await;
    server.abort();
}

#[tokio::test]
async fn a_test_client_interceptor_sees_message_limits() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    echo_test_every_shape(&limits_test(channel_with(addr, limits_config()).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_test_tls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    echo_test_every_shape(&limits_test(
        tls_channel_cfg(addr, client_tls, limits_config()).await,
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn a_test_mtls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_test_every_shape(&limits_test(
        tls_channel_cfg(addr, client_tls, limits_config()).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_test_unix_client_interceptor_sees_message_limits() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix(sock)
            .await
            .ok();
    });
    echo_test_every_shape(&limits_test(
        unix_channel_with(&path, limits_config()).await,
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn a_test_from_io_client_interceptor_sees_message_limits() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_test_every_shape(&limits_test(
        Channel::from_io_with(client_io, "localhost", limits_config())
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn a_reverser_client_interceptor_sees_message_limits() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });
    echo_reverser_every_shape(&limits_channel(channel_with(addr, limits_config()).await)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_tls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    echo_reverser_every_shape(&limits_channel(
        tls_channel_cfg(addr, client_tls, limits_config()).await,
    ))
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_mtls_client_interceptor_sees_message_limits() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    );
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reverser_every_shape(&limits_channel(
        tls_channel_cfg(addr, client_tls, limits_config()).await,
    ))
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_reverser_unix_client_interceptor_sees_message_limits() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        Server::new(service).serve_unix(sock).await.ok();
    });
    echo_reverser_every_shape(&limits_channel(
        unix_channel_with(&path, limits_config()).await,
    ))
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn a_reverser_from_io_client_interceptor_sees_message_limits() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen));
    let server = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    echo_reverser_every_shape(&limits_channel(
        Channel::from_io_with(client_io, "localhost", limits_config())
            .await
            .expect("from_io"),
    ))
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    server.abort();
}

#[derive(Clone, Copy)]
struct Trace(&'static str);

fn interceptor_insert_trace(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.extensions_mut().insert(Trace("abc"));
    Ok(())
}

fn interceptor_stamp_trace(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let Some(trace) = call.extensions().get::<Trace>().copied() else {
        return Err(Status::internal("first interceptor did not run"));
    };
    call.metadata_mut().insert("x-trace", trace.0)?;
    Ok(())
}

fn require_trace(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.metadata().get("x-trace") != Some("abc") {
        return Err(Status::invalid_argument("missing trace"));
    }
    Ok(())
}

fn stacked_trace_client(channel: Channel) -> GreeterClient {
    GreeterClient::new(channel)
        .intercept(interceptor_insert_trace)
        .intercept(interceptor_stamp_trace)
}

#[tokio::test]
async fn client_interceptors_stack_and_share_extensions() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_trace)
            .serve_listener(listener)
            .await
            .ok();
    });
    echo_every_shape(&stacked_trace_client(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_trace)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&stacked_trace_client(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_trace)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &stacked_trace_client(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_client_interceptors_stack_and_share_extensions() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_trace)
            .serve_unix(sock)
            .await
            .ok();
    });
    echo_every_shape(&stacked_trace_client(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_interceptors_stack_and_share_extensions() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_trace)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_every_shape(
        &stacked_trace_client(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn stacked_trace_test(channel: Channel) -> TestServiceClient {
    TestServiceClient::new(channel)
        .intercept(interceptor_insert_trace)
        .intercept(interceptor_stamp_trace)
}

fn stacked_trace_channel(channel: Channel) -> Channel {
    channel
        .intercept(interceptor_insert_trace)
        .intercept(interceptor_stamp_trace)
}

#[tokio::test]
async fn test_client_interceptors_stack_and_share_extensions() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_trace)
            .serve_listener(listener)
            .await
            .ok();
    });
    echo_test_every_shape(&stacked_trace_test(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_tls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_trace)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_test_every_shape(&stacked_trace_test(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_mtls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_trace)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_test_every_shape(&stacked_trace_test(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_client_interceptors_stack_and_share_extensions() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_trace)
            .serve_unix(sock)
            .await
            .ok();
    });
    echo_test_every_shape(&stacked_trace_test(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_from_io_client_interceptors_stack_and_share_extensions() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .intercept(require_trace)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_test_every_shape(&stacked_trace_test(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn reverser_client_interceptors_stack_and_share_extensions() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_trace);
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });
    echo_reverser_every_shape(&stacked_trace_channel(channel(addr).await)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_tls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_trace);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_reverser_every_shape(&stacked_trace_channel(tls_channel(addr).await)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_mtls_client_interceptors_stack_and_share_extensions() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::mtls(
        Arc::clone(&seen),
        client_identity().certificates().next().expect("leaf"),
    )
    .intercept(require_trace);
    let task = tokio::spawn(async move {
        Server::new(service)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reverser_every_shape(&stacked_trace_channel(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn reverser_unix_client_interceptors_stack_and_share_extensions() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_trace);
    let task = tokio::spawn(async move {
        Server::new(service).serve_unix(sock).await.ok();
    });
    echo_reverser_every_shape(&stacked_trace_channel(unix_channel(&path).await)).await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    task.abort();
}

#[tokio::test]
async fn reverser_from_io_client_interceptors_stack_and_share_extensions() {
    let (client_io, server_io) = duplex_pair();
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser::new(Arc::clone(&seen)).intercept(require_trace);
    let server = tokio::spawn(async move {
        Server::new(service).serve_connection(server_io).await.ok();
    });
    echo_reverser_every_shape(&stacked_trace_channel(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    let client = GreeterClient::new(channel).intercept(|call: &mut Outgoing<'_>| {
        call.set_wait_for_ready(true);
        Ok(())
    });
    wait_then_complete_every_shape(&client, false, async {
        serve_at(addr, Echo, ServerConfig::default())
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tls_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    let client = GreeterClient::new(channel).intercept(|call: &mut Outgoing<'_>| {
        call.set_wait_for_ready(true);
        Ok(())
    });
    wait_then_complete_every_shape(&client, false, async {
        serve_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_mtls_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    let client = GreeterClient::new(channel).intercept(|call: &mut Outgoing<'_>| {
        call.set_wait_for_ready(true);
        Ok(())
    });
    wait_then_complete_every_shape(&client, false, async {
        serve_tls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_unix_client_interceptor_can_set_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path).expect("lazy");
    let client = GreeterClient::new(channel).intercept(|call: &mut Outgoing<'_>| {
        call.set_wait_for_ready(true);
        Ok(())
    });
    wait_then_complete_every_shape(&client, false, async {
        let sock = path.clone();
        tokio::spawn(async move {
            GreeterServer::new(Echo).serve_unix(sock).await.ok();
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = GreeterClient::new(Channel::connect_lazy(addr).expect("lazy").wait_for_ready())
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = GreeterClient::new(
        Channel::connect_tls_lazy(addr, client_tls)
            .expect("lazy")
            .wait_for_ready(),
    )
    .intercept(|call: &mut Outgoing<'_>| {
        call.set_wait_for_ready(false);
        Ok(())
    });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_mtls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(
        Channel::connect_tls_lazy(addr, client_tls)
            .expect("lazy")
            .wait_for_ready(),
    )
    .intercept(|call: &mut Outgoing<'_>| {
        call.set_wait_for_ready(false);
        Ok(())
    });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_unix_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let client = GreeterClient::new(
        Channel::connect_unix_lazy(&path)
            .expect("lazy")
            .wait_for_ready(),
    )
    .intercept(|call: &mut Outgoing<'_>| {
        call.set_wait_for_ready(false);
        Ok(())
    });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

fn interceptor_blocked() -> Status {
    let mut info = pbrs_grpc::pb::ErrorInfo::new();
    info.set_reason("BLOCKED");
    info.set_domain("example.com");
    Status::with_error_details(
        Code::FailedPrecondition,
        "blocked locally",
        [pbrs_grpc::pb::Any::pack(&info).expect("pack")],
    )
    .expect("details")
}

fn assert_interceptor_blocked(err: &Status) {
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    assert_eq!(err.message(), "blocked locally");
    let info = err
        .rpc()
        .expect("google.rpc.Status")
        .details()
        .get(0)
        .expect("one Any")
        .unpack::<pbrs_grpc::pb::ErrorInfo>()
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "BLOCKED");
    assert_eq!(info.domain().to_str().unwrap_or(""), "example.com");
    let unpacked = err
        .error_details()
        .expect("ErrorDetails")
        .error_info
        .expect("ErrorInfo");
    assert_eq!(unpacked.reason().to_str().unwrap_or(""), "BLOCKED");
    assert_eq!(unpacked.domain().to_str().unwrap_or(""), "example.com");
}

async fn assert_greeter_blocked_every_shape(client: &GreeterClient) {
    assert_interceptor_blocked(
        &client
            .say_hello(Request::new(req("ada")))
            .await
            .expect_err("unary"),
    );
    match client.server_hello(Request::new(req("ada"))).await {
        Err(err) => assert_interceptor_blocked(&err),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_interceptor_blocked(&err),
            Ok(_) => panic!("server-stream interceptor reject must fail"),
        },
    }
    let (tx, call) = client.client_hello(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("client-stream"));
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

struct FailGreeter;

impl Greeter for FailGreeter {
    async fn say_hello(&self, _: Request<HelloRequest>) -> Result<Response<HelloReply>, Status> {
        Err(interceptor_blocked())
    }

    async fn client_hello(
        &self,
        _: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(interceptor_blocked())
    }

    async fn server_hello(
        &self,
        _: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(interceptor_blocked())
    }

    async fn stream_hello(
        &self,
        _: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(interceptor_blocked())
    }
}

fn rich_fail() -> Status {
    let mut status = Status::failed_precondition("quota");
    status.set_details(vec![0x08, 0x09]);
    status
        .metadata_mut()
        .insert("x-retry-after", "30")
        .expect("md");
    status
}

fn assert_rich_fail(err: &Status) {
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    assert_eq!(err.message(), "quota");
    assert_eq!(err.details(), &[0x08, 0x09]);
    assert_eq!(err.metadata().get("x-retry-after"), Some("30"));
    assert!(err.metadata().get_bin("grpc-status-details-bin").is_none());
}

async fn assert_raw_status_details_every_shape(client: &GreeterClient) {
    assert_rich_fail(
        &client
            .say_hello(Request::new(req("ada")))
            .await
            .expect_err("unary"),
    );
    match client.server_hello(Request::new(req("ada"))).await {
        Err(err) => assert_rich_fail(&err),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_rich_fail(&err),
            Ok(_) => panic!("server-stream raw details must fail"),
        },
    }
    let (tx, call) = client.client_hello(Request::new(()));
    assert_rich_fail(&call.await.expect_err("client-stream"));
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_rich_fail(&call.await.expect_err("bidi"));
    drop(tx);
}

struct RichFail;

impl Greeter for RichFail {
    async fn say_hello(&self, _: Request<HelloRequest>) -> Result<Response<HelloReply>, Status> {
        Err(rich_fail())
    }

    async fn client_hello(
        &self,
        _: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(rich_fail())
    }

    async fn server_hello(
        &self,
        _: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(rich_fail())
    }

    async fn stream_hello(
        &self,
        _: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(rich_fail())
    }
}

fn typed_after_headers_status() -> Status {
    let mut info = pbrs_grpc::pb::ErrorInfo::new();
    info.set_reason("API_DISABLED");
    info.set_domain("example.com");
    let mut status = Status::with_error_details(
        Code::FailedPrecondition,
        "api disabled",
        [pbrs_grpc::pb::Any::pack(&info).expect("pack")],
    )
    .expect("encode");
    status
        .metadata_mut()
        .insert("x-retry-after", "30")
        .expect("md");
    status
}

fn assert_typed_after_headers(err: &Status) {
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    assert_eq!(err.message(), "api disabled");
    let info = err
        .rpc()
        .expect("google.rpc.Status")
        .details()
        .get(0)
        .expect("one Any")
        .unpack::<pbrs_grpc::pb::ErrorInfo>()
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
    assert_eq!(info.domain().to_str().unwrap_or(""), "example.com");
    let unpacked = err
        .error_details()
        .expect("ErrorDetails")
        .error_info
        .expect("ErrorInfo");
    assert_eq!(unpacked.reason().to_str().unwrap_or(""), "API_DISABLED");
    assert_eq!(unpacked.domain().to_str().unwrap_or(""), "example.com");
    assert_eq!(err.metadata().get("x-retry-after"), Some("30"));
    assert!(err.metadata().get_bin("grpc-status-details-bin").is_none());
}

fn fail_after_one() -> pbrs_grpc::Streaming<HelloReply> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        let mut reply = HelloReply::new();
        reply.set_message("ada");
        tx.send(reply).await.ok();
        tx.fail(typed_after_headers_status()).await;
    }));
    stream
}

/// Server-streaming and bidi only: unary and client-streaming have no
/// response DATA then trailers.
struct TypedAfterHeaders;

impl Greeter for TypedAfterHeaders {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Ok(Response::new(fail_after_one()))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Ok(Response::new(fail_after_one()))
    }
}

async fn assert_typed_status_after_streamed_message(client: &GreeterClient) {
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_after_headers(&stream.message().await.expect_err("status"));

    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_after_headers(&stream.trailers().await.expect_err("trailers"));

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_after_headers(&stream.message().await.expect_err("status"));

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_after_headers(&stream.trailers().await.expect_err("trailers"));
}

struct FailTestService;

impl TestService for FailTestService {
    async fn empty_call(&self, _: Request<Empty>) -> Result<Response<Empty>, Status> {
        Err(interceptor_blocked())
    }

    async fn unary_call(
        &self,
        _: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        Err(interceptor_blocked())
    }

    async fn cacheable_unary_call(
        &self,
        _: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        Err(interceptor_blocked())
    }

    async fn streaming_output_call(
        &self,
        _: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<StreamingOutputCallResponse>>, Status> {
        Err(interceptor_blocked())
    }

    async fn streaming_input_call(
        &self,
        _: Request<pbrs_grpc::Streaming<StreamingInputCallRequest>>,
    ) -> Result<Response<StreamingInputCallResponse>, Status> {
        Err(interceptor_blocked())
    }

    async fn full_duplex_call(
        &self,
        _: Request<pbrs_grpc::Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<StreamingOutputCallResponse>>, Status> {
        Err(interceptor_blocked())
    }

    async fn half_duplex_call(
        &self,
        _: Request<pbrs_grpc::Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<StreamingOutputCallResponse>>, Status> {
        Err(interceptor_blocked())
    }

    async fn unimplemented_call(&self, _: Request<Empty>) -> Result<Response<Empty>, Status> {
        Err(interceptor_blocked())
    }
}

fn fail_test_after_one() -> pbrs_grpc::Streaming<StreamingOutputCallResponse> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        let mut reply = StreamingOutputCallResponse::new();
        let mut payload = Payload::new();
        payload.set_body(b"ada".to_vec());
        reply.set_payload(payload);
        tx.send(reply).await.ok();
        tx.fail(typed_after_headers_status()).await;
    }));
    stream
}

/// Server-streaming and bidi only: EmptyCall / StreamingInputCall have no
/// response DATA then trailers.
struct TypedAfterHeadersTest;

impl TestService for TypedAfterHeadersTest {
    async fn empty_call(&self, _: Request<Empty>) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn unary_call(
        &self,
        _: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn cacheable_unary_call(
        &self,
        _: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn streaming_output_call(
        &self,
        _: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<StreamingOutputCallResponse>>, Status> {
        Ok(Response::new(fail_test_after_one()))
    }

    async fn streaming_input_call(
        &self,
        _: Request<pbrs_grpc::Streaming<StreamingInputCallRequest>>,
    ) -> Result<Response<StreamingInputCallResponse>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn full_duplex_call(
        &self,
        _: Request<pbrs_grpc::Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<StreamingOutputCallResponse>>, Status> {
        Ok(Response::new(fail_test_after_one()))
    }

    async fn half_duplex_call(
        &self,
        _: Request<pbrs_grpc::Streaming<StreamingOutputCallRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<StreamingOutputCallResponse>>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }

    async fn unimplemented_call(&self, _: Request<Empty>) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented("typed-after-headers"))
    }
}

async fn assert_test_typed_status_after_streamed_message(client: &TestServiceClient) {
    let mut stream = client
        .streaming_output_call(Request::new(StreamingOutputCallRequest::new()))
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(first.payload().body().as_ref(), b"ada");
    assert_typed_after_headers(&stream.message().await.expect_err("status"));

    let mut stream = client
        .streaming_output_call(Request::new(StreamingOutputCallRequest::new()))
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(first.payload().body().as_ref(), b"ada");
    assert_typed_after_headers(&stream.trailers().await.expect_err("trailers"));

    let (tx, call) = client.full_duplex_call(Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(first.payload().body().as_ref(), b"ada");
    assert_typed_after_headers(&stream.message().await.expect_err("status"));

    let (tx, call) = client.full_duplex_call(Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(first.payload().body().as_ref(), b"ada");
    assert_typed_after_headers(&stream.trailers().await.expect_err("trailers"));
}

struct FailReverser;

impl Service for FailReverser {
    const NAME: &'static str = "demo.Reverser";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Reverse" => {
                rpc.unary(|_: Request<HelloRequest>| async {
                    Err::<Response<HelloReply>, _>(interceptor_blocked())
                })
                .await;
            }
            "Server" => {
                rpc.server_streaming(|_: Request<HelloRequest>| async {
                    Err::<Response<pbrs_grpc::Streaming<HelloReply>>, _>(interceptor_blocked())
                })
                .await;
            }
            "Client" => {
                rpc.client_streaming(|_: Request<pbrs_grpc::Streaming<HelloRequest>>| async {
                    Err::<Response<HelloReply>, _>(interceptor_blocked())
                })
                .await;
            }
            "Bidi" => {
                rpc.bidi_streaming(|_: Request<pbrs_grpc::Streaming<HelloRequest>>| async {
                    Err::<Response<pbrs_grpc::Streaming<HelloReply>>, _>(interceptor_blocked())
                })
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

/// Server-streaming and bidi only: unary and client-streaming have no
/// response DATA then trailers.
struct TypedAfterHeadersReverser;

impl Service for TypedAfterHeadersReverser {
    const NAME: &'static str = "demo.Reverser";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Reverse" => {
                rpc.unary(|_: Request<HelloRequest>| async {
                    Err::<Response<HelloReply>, _>(Status::unimplemented("typed-after-headers"))
                })
                .await;
            }
            "Server" => {
                rpc.server_streaming(|_: Request<HelloRequest>| async {
                    Ok::<Response<pbrs_grpc::Streaming<HelloReply>>, Status>(Response::new(
                        fail_after_one(),
                    ))
                })
                .await;
            }
            "Client" => {
                rpc.client_streaming(|_: Request<pbrs_grpc::Streaming<HelloRequest>>| async {
                    Err::<Response<HelloReply>, _>(Status::unimplemented("typed-after-headers"))
                })
                .await;
            }
            "Bidi" => {
                rpc.bidi_streaming(|_: Request<pbrs_grpc::Streaming<HelloRequest>>| async {
                    Ok::<Response<pbrs_grpc::Streaming<HelloReply>>, Status>(Response::new(
                        fail_after_one(),
                    ))
                })
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

async fn assert_reverser_typed_status_after_streamed_message(channel: &Channel) {
    let mut stream = channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            Request::new(req("ada")),
        )
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_after_headers(&stream.message().await.expect_err("status"));

    let mut stream = channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            Request::new(req("ada")),
        )
        .await
        .expect("headers")
        .into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_after_headers(&stream.trailers().await.expect_err("trailers"));

    let (tx, call) =
        channel.bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_after_headers(&stream.message().await.expect_err("status"));

    let (tx, call) =
        channel.bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", Request::new(()));
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&first), "ada");
    assert_typed_after_headers(&stream.trailers().await.expect_err("trailers"));
}

fn stamp_outgoing_context(call: &mut Outgoing<'_>) -> Result<(), Status> {
    let path = call.path();
    call.metadata_mut().insert("x-path", path)?;
    let service = call.service();
    call.metadata_mut().set("x-service", service)?;
    let method = call.method();
    call.metadata_mut().set("x-method", method)?;
    let authority = call.authority();
    call.metadata_mut().insert("x-authority", authority)?;
    let scheme = call.scheme();
    call.metadata_mut().set("x-scheme", scheme)?;
    Ok(())
}

fn require_deadline_instant(timeout: Duration) -> impl Fn(&mut Outgoing<'_>) -> Result<(), Status> {
    move |call: &mut Outgoing<'_>| {
        if call.timeout() != Some(timeout) {
            return Err(Status::internal("channel overlay should fill timeout"));
        }
        let Some(deadline) = call.deadline() else {
            return Err(Status::internal("missing deadline Instant"));
        };
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left > timeout {
            return Err(Status::internal("deadline Instant later than timeout"));
        }
        if call.wait_for_ready_is_set() {
            return Err(Status::internal("wait-for-ready should be unset"));
        }
        Ok(())
    }
}

fn require_stamped_context(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.metadata().get("x-path") != Some(rpc.path()) {
        return Err(Status::invalid_argument(format!(
            "x-path {:?} path {}",
            rpc.metadata().get("x-path"),
            rpc.path()
        )));
    }
    if rpc.metadata().get("x-service") != Some(rpc.service()) {
        return Err(Status::invalid_argument(format!(
            "x-service {:?} service {}",
            rpc.metadata().get("x-service"),
            rpc.service()
        )));
    }
    if rpc.metadata().get("x-method") != Some(rpc.method()) {
        return Err(Status::invalid_argument(format!(
            "x-method {:?} method {}",
            rpc.metadata().get("x-method"),
            rpc.method()
        )));
    }
    if rpc.metadata().get("x-authority") != rpc.authority() {
        return Err(Status::invalid_argument(format!(
            "x-authority {:?} authority {:?}",
            rpc.metadata().get("x-authority"),
            rpc.authority()
        )));
    }
    if rpc.metadata().get("x-scheme") != rpc.scheme() {
        return Err(Status::invalid_argument(format!(
            "x-scheme {:?} scheme {:?}",
            rpc.metadata().get("x-scheme"),
            rpc.scheme()
        )));
    }
    Ok(())
}

#[tokio::test]
async fn a_client_interceptor_can_reject_with_typed_status_details() {
    let (addr, listener) = bind().await;
    drop(listener);
    let client = GreeterClient::new(Channel::connect_lazy(addr).expect("lazy"))
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));

    assert_interceptor_blocked(
        &client
            .say_hello(Request::new(req("ada")))
            .await
            .expect_err("unary"),
    );
    assert_interceptor_blocked(
        &client
            .server_hello(Request::new(req("ada")))
            .await
            .expect_err("server-stream"),
    );
    let (tx, call) = client.client_hello(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("client-stream"));
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

struct ActorEcho;

fn actor_is_kernel<T>(request: Request<T>) -> Result<T, Status> {
    let actors: Vec<_> = request.metadata().get_all("x-actor").collect();
    if actors != ["kernel"] {
        return Err(Status::internal(format!("x-actor {actors:?}")));
    }
    if request.metadata().get_bin("x-actor-bin").as_deref() != Some(&[9u8][..]) {
        return Err(Status::internal(format!(
            "x-actor-bin {:?}",
            request.metadata().get_bin("x-actor-bin")
        )));
    }
    let (msg, parts) = request.into_message_and_parts();
    let actors: Vec<_> = parts.metadata().get_all("x-actor").collect();
    if actors != ["kernel"] {
        return Err(Status::internal(format!("parts x-actor {actors:?}")));
    }
    if parts.metadata().get_bin("x-actor-bin").as_deref() != Some(&[9u8][..]) {
        return Err(Status::internal(format!(
            "parts x-actor-bin {:?}",
            parts.metadata().get_bin("x-actor-bin")
        )));
    }
    Ok(msg)
}

fn kernel_stream() -> Response<pbrs_grpc::Streaming<HelloReply>> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        tx.send(common::reply("kernel")).await.ok();
    }));
    Response::new(stream)
}

impl pbrs_grpc::Greeter for ActorEcho {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let _msg = actor_is_kernel(request)?;
        Ok(Response::new(common::reply("kernel")))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let _stream = actor_is_kernel(request)?;
        Ok(Response::new(common::reply("kernel")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _msg = actor_is_kernel(request)?;
        Ok(kernel_stream())
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _stream = actor_is_kernel(request)?;
        Ok(kernel_stream())
    }
}

fn interceptor_inject_actor(rpc: &mut Rpc) -> Result<(), Status> {
    rpc.metadata_mut().set("x-actor", "kernel")?;
    rpc.metadata_mut().set_bin("x-actor-bin", [9u8])?;
    Ok(())
}

fn smuggled_actor<T>(mut request: Request<T>) -> Request<T> {
    request
        .metadata_mut()
        .insert("x-actor", "smuggled")
        .expect("metadata");
    request
        .metadata_mut()
        .insert_bin("x-actor-bin", [1u8])
        .expect("bin");
    request
}

async fn assert_injected_actor(client: &GreeterClient) {
    let reply = client
        .say_hello(smuggled_actor(Request::new(req("ignored"))))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "kernel");
    let mut stream = client
        .server_hello(smuggled_actor(Request::new(req("ignored"))))
        .await
        .expect("server-stream")
        .into_inner();
    assert_eq!(
        name_of(&stream.message().await.expect("item").expect("first")),
        "kernel"
    );
    let (tx, call) = client.client_hello(smuggled_actor(Request::new(())));
    tx.send(req("ignored")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "kernel");
    let (tx, call) = client.stream_hello(smuggled_actor(Request::new(())));
    tx.send(req("ignored")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    assert_eq!(
        name_of(&inbound.message().await.expect("item").expect("first")),
        "kernel"
    );
}

struct SeesAuth;

fn auth_stripped<T>(request: Request<T>) -> Result<T, Status> {
    if request.metadata().get("authorization").is_some() {
        return Err(Status::internal("authorization leaked to handler"));
    }
    let (msg, parts) = request.into_message_and_parts();
    if parts.metadata().get("authorization").is_some() {
        return Err(Status::internal("authorization leaked to parts"));
    }
    Ok(msg)
}

impl pbrs_grpc::Greeter for SeesAuth {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let msg = auth_stripped(request)?;
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let _stream = auth_stripped(request)?;
        Ok(Response::new(common::reply("ada")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let msg = auth_stripped(request)?;
        let name = common::name_of_request(&msg);
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(common::reply(name)).await.ok();
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _stream = auth_stripped(request)?;
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(common::reply("ada")).await.ok();
        }));
        Ok(Response::new(stream))
    }
}

fn interceptor_strip_authorization(rpc: &mut Rpc) -> Result<(), Status> {
    rpc.metadata_mut().remove("authorization");
    Ok(())
}

fn bearer_secret<T>(mut request: Request<T>) -> Request<T> {
    request
        .metadata_mut()
        .insert("authorization", "Bearer secret")
        .expect("metadata");
    request
}

async fn assert_stripped_authorization(client: &GreeterClient) {
    let reply = client
        .say_hello(bearer_secret(Request::new(req("ada"))))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let mut stream = client
        .server_hello(bearer_secret(Request::new(req("ada"))))
        .await
        .expect("server-stream")
        .into_inner();
    assert_eq!(
        name_of(&stream.message().await.expect("item").expect("first")),
        "ada"
    );
    let (tx, call) = client.client_hello(bearer_secret(Request::new(())));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let (tx, call) = client.stream_hello(bearer_secret(Request::new(())));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    assert_eq!(
        name_of(&inbound.message().await.expect("item").expect("first")),
        "ada"
    );
}

struct SeesHops;

fn hops_ok<T>(request: Request<T>) -> Result<T, Status> {
    fn check(md: &pbrs_grpc::Metadata, where_: &str) -> Result<(), Status> {
        if md.get("y-drop").is_some() {
            return Err(Status::internal(format!("y-drop leaked to {where_}")));
        }
        if md.get("x-keep") != Some("v") {
            return Err(Status::internal(format!(
                "{where_} x-keep {:?}",
                md.get("x-keep")
            )));
        }
        if md.get_bin("x-trace-bin").as_deref() != Some(&[1u8][..]) {
            return Err(Status::internal(format!("{where_} x-trace-bin missing")));
        }
        Ok(())
    }
    check(request.metadata(), "handler")?;
    let (msg, parts) = request.into_message_and_parts();
    check(parts.metadata(), "parts")?;
    Ok(msg)
}

impl pbrs_grpc::Greeter for SeesHops {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let msg = hops_ok(request)?;
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let _stream = hops_ok(request)?;
        Ok(Response::new(common::reply("ada")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let msg = hops_ok(request)?;
        let name = common::name_of_request(&msg);
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(common::reply(name)).await.ok();
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _stream = hops_ok(request)?;
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(common::reply("ada")).await.ok();
        }));
        Ok(Response::new(stream))
    }
}

fn interceptor_retain_x_metadata(rpc: &mut Rpc) -> Result<(), Status> {
    rpc.metadata_mut().retain(|k| k.starts_with("x-"));
    Ok(())
}

fn hop_metadata<T>(mut request: Request<T>) -> Request<T> {
    request.metadata_mut().insert("x-keep", "v").expect("keep");
    request
        .metadata_mut()
        .insert("y-drop", "secret")
        .expect("drop");
    request
        .metadata_mut()
        .insert_bin("x-trace-bin", [1u8])
        .expect("bin");
    request
}

async fn assert_retained_x_metadata(client: &GreeterClient) {
    let reply = client
        .say_hello(hop_metadata(Request::new(req("ada"))))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let mut stream = client
        .server_hello(hop_metadata(Request::new(req("ada"))))
        .await
        .expect("server-stream")
        .into_inner();
    assert_eq!(
        name_of(&stream.message().await.expect("item").expect("first")),
        "ada"
    );
    let (tx, call) = client.client_hello(hop_metadata(Request::new(())));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let (tx, call) = client.stream_hello(hop_metadata(Request::new(())));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    assert_eq!(
        name_of(&inbound.message().await.expect("item").expect("first")),
        "ada"
    );
}

#[tokio::test]
async fn a_server_interceptor_injects_metadata_the_handler_sees() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(ActorEcho)
            .intercept(interceptor_inject_actor)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_injected_actor(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_server_interceptor_injects_metadata_the_handler_sees() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(ActorEcho)
            .intercept(interceptor_inject_actor)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_injected_actor(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_server_interceptor_injects_metadata_the_handler_sees() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(ActorEcho)
            .intercept(interceptor_inject_actor)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_injected_actor(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_server_interceptor_injects_metadata_the_handler_sees() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(ActorEcho)
            .intercept(interceptor_inject_actor)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_injected_actor(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_server_interceptor_injects_metadata_the_handler_sees() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(ActorEcho)
            .intercept(interceptor_inject_actor)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_injected_actor(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn a_server_interceptor_strips_metadata_before_the_handler() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesAuth)
            .intercept(interceptor_strip_authorization)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_stripped_authorization(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_server_interceptor_strips_metadata_before_the_handler() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesAuth)
            .intercept(interceptor_strip_authorization)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_stripped_authorization(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_server_interceptor_strips_metadata_before_the_handler() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesAuth)
            .intercept(interceptor_strip_authorization)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_stripped_authorization(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_server_interceptor_strips_metadata_before_the_handler() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesAuth)
            .intercept(interceptor_strip_authorization)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_stripped_authorization(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_server_interceptor_strips_metadata_before_the_handler() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesAuth)
            .intercept(interceptor_strip_authorization)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_stripped_authorization(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn a_server_interceptor_retains_a_subset_of_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesHops)
            .intercept(interceptor_retain_x_metadata)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_retained_x_metadata(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_server_interceptor_retains_a_subset_of_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesHops)
            .intercept(interceptor_retain_x_metadata)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_retained_x_metadata(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_server_interceptor_retains_a_subset_of_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesHops)
            .intercept(interceptor_retain_x_metadata)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_retained_x_metadata(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_server_interceptor_retains_a_subset_of_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesHops)
            .intercept(interceptor_retain_x_metadata)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_retained_x_metadata(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_server_interceptor_retains_a_subset_of_metadata() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesHops)
            .intercept(interceptor_retain_x_metadata)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_retained_x_metadata(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

fn interceptor_set_timeout_5s(rpc: &mut Rpc) -> Result<(), Status> {
    rpc.set_timeout(Duration::from_secs(5));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_interceptor_can_tighten_the_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .intercept(interceptor_server_set_timeout)
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_secs(5)),
        Duration::from_millis(500),
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tls_server_interceptor_can_tighten_the_deadline() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .intercept(interceptor_server_set_timeout)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_secs(5)),
        Duration::from_millis(500),
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_mtls_server_interceptor_can_tighten_the_deadline() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .intercept(interceptor_server_set_timeout)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_secs(5)),
        Duration::from_millis(500),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_unix_server_interceptor_can_tighten_the_deadline() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .intercept(interceptor_server_set_timeout)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_secs(5)),
        Duration::from_millis(500),
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_from_io_server_interceptor_can_tighten_the_deadline() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .intercept(interceptor_server_set_timeout)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_secs(5)),
        Duration::from_millis(500),
    )
    .await;
    server.abort();
}

fn take_interceptor_deadline_cap<T>(request: Request<T>) -> Result<T, Status> {
    let timeout = request
        .timeout()
        .ok_or_else(|| Status::internal("missing timeout duration"))?;
    if timeout != Duration::from_millis(20) {
        return Err(Status::internal(format!(
            "stamped timeout {timeout:?} is not the interceptor cap"
        )));
    }
    let peer = request
        .peer_timeout()
        .ok_or_else(|| Status::internal("missing client grpc-timeout"))?;
    if peer != Duration::from_secs(5) {
        return Err(Status::internal(format!(
            "peer timeout {peer:?} is not the client's 5s"
        )));
    }
    let (msg, parts) = request.into_message_and_parts();
    if parts.timeout() != Some(timeout) {
        return Err(Status::internal("parts timeout must match Request"));
    }
    if parts.peer_timeout() != Some(peer) {
        return Err(Status::internal("parts peer_timeout must match Request"));
    }
    if parts.rpc_timeout().is_some() {
        return Err(Status::internal("no server timeout overlay on this test"));
    }
    let deadline = parts
        .deadline()
        .ok_or_else(|| Status::internal("missing deadline Instant"))?;
    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
    if left > Duration::from_millis(50) {
        return Err(Status::internal(format!(
            "remaining {left:?} looks like the client 5s, not the interceptor cap"
        )));
    }
    Ok(msg)
}

fn interceptor_check_peer_and_tighten(rpc: &mut Rpc) -> Result<(), Status> {
    let peer = rpc.peer_timeout();
    if peer != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!("rpc peer timeout {peer:?}")));
    }
    rpc.set_timeout(Duration::from_millis(20));
    Ok(())
}

struct SeesCap;

impl Greeter for SeesCap {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let msg = take_interceptor_deadline_cap(request)?;
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let _ = take_interceptor_deadline_cap(request)?;
        Ok(Response::new(common::reply("ok")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _ = take_interceptor_deadline_cap(request)?;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _ = take_interceptor_deadline_cap(request)?;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }
}

async fn assert_handler_sees_interceptor_deadline(client: &GreeterClient) {
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_secs(5));
    let reply = client.say_hello(request).await.expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_secs(5));
    let _ = client.server_hello(request).await.expect("server-stream");

    let mut request = Request::new(());
    request.set_timeout(Duration::from_secs(5));
    let (tx, call) = client.client_hello(request);
    tx.close();
    let _ = call.await.expect("client-stream");

    let mut request = Request::new(());
    request.set_timeout(Duration::from_secs(5));
    let (tx, call) = client.stream_hello(request);
    tx.close();
    let _ = call.await.expect("bidi");
}

#[tokio::test]
async fn a_handler_sees_the_interceptor_deadline_on_request() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesCap)
            .intercept(interceptor_check_peer_and_tighten)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_handler_sees_interceptor_deadline(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_handler_sees_the_interceptor_deadline_on_request() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesCap)
            .intercept(interceptor_check_peer_and_tighten)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_handler_sees_interceptor_deadline(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_handler_sees_the_interceptor_deadline_on_request() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesCap)
            .intercept(interceptor_check_peer_and_tighten)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_handler_sees_interceptor_deadline(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_handler_sees_the_interceptor_deadline_on_request() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesCap)
            .intercept(interceptor_check_peer_and_tighten)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_handler_sees_interceptor_deadline(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_handler_sees_the_interceptor_deadline_on_request() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesCap)
            .intercept(interceptor_check_peer_and_tighten)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_handler_sees_interceptor_deadline(&client).await;
    server.abort();
}

fn interceptor_require_rpc_limits(rpc: &mut Rpc) -> Result<(), Status> {
    let want = test_message_limits();
    if rpc.limits() != want {
        return Err(Status::internal(format!("rpc limits {:?}", rpc.limits())));
    }
    Ok(())
}

fn take_handler_limits<T>(request: Request<T>) -> Result<T, Status> {
    let want = test_message_limits();
    if request.limits() != Some(want) {
        return Err(Status::internal(format!("limits {:?}", request.limits())));
    }
    let (msg, parts) = request.into_message_and_parts();
    if parts.limits() != Some(want) {
        return Err(Status::internal(format!(
            "parts limits {:?}",
            parts.limits()
        )));
    }
    Ok(msg)
}

struct SeesLimits;

impl Greeter for SeesLimits {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let msg = take_handler_limits(request)?;
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let _ = take_handler_limits(request)?;
        Ok(Response::new(common::reply("ok")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _ = take_handler_limits(request)?;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _ = take_handler_limits(request)?;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }
}

async fn assert_handler_sees_message_limits(client: &GreeterClient) {
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let _ = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream");
    let (tx, call) = client.client_hello(Request::new(()));
    tx.close();
    let _ = call.await.expect("client-stream");
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    let _ = call.await.expect("bidi");
    assert!(Request::new(req("ada")).limits().is_none());
}

#[tokio::test]
async fn interceptors_and_handlers_see_message_limits() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesLimits)
            .max_decoding_message_size(16)
            .max_encoding_message_size(32)
            .intercept(interceptor_require_rpc_limits)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_handler_sees_message_limits(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn tls_interceptors_and_handlers_see_message_limits() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesLimits)
            .max_decoding_message_size(16)
            .max_encoding_message_size(32)
            .intercept(interceptor_require_rpc_limits)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_handler_sees_message_limits(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn mtls_interceptors_and_handlers_see_message_limits() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesLimits)
            .max_decoding_message_size(16)
            .max_encoding_message_size(32)
            .intercept(interceptor_require_rpc_limits)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_handler_sees_message_limits(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_interceptors_and_handlers_see_message_limits() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesLimits)
            .max_decoding_message_size(16)
            .max_encoding_message_size(32)
            .intercept(interceptor_require_rpc_limits)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_handler_sees_message_limits(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn from_io_interceptors_and_handlers_see_message_limits() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesLimits)
            .max_decoding_message_size(16)
            .max_encoding_message_size(32)
            .intercept(interceptor_require_rpc_limits)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_handler_sees_message_limits(&client).await;
    server.abort();
}

fn interceptor_require_greeter_path(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.service() != "helloworld.Greeter" {
        return Err(Status::internal(format!("rpc service {}", rpc.service())));
    }
    let want = format!("/helloworld.Greeter/{}", rpc.method());
    if rpc.path() != want {
        return Err(Status::internal(format!(
            "rpc path {} != {want}",
            rpc.path()
        )));
    }
    Ok(())
}

fn check_handler_path<T>(
    request: &Request<T>,
    want_path: &str,
    want_method: &str,
) -> Result<(), Status> {
    if request.path() != Some(want_path) {
        return Err(Status::internal(format!("path {:?}", request.path())));
    }
    if request.service() != Some("helloworld.Greeter") {
        return Err(Status::internal(format!("service {:?}", request.service())));
    }
    if request.method() != Some(want_method) {
        return Err(Status::internal(format!("method {:?}", request.method())));
    }
    Ok(())
}

fn take_handler_path<T>(
    request: Request<T>,
    want_path: &'static str,
    want_method: &'static str,
) -> Result<T, Status> {
    check_handler_path(&request, want_path, want_method)?;
    let (msg, parts) = request.into_message_and_parts();
    if parts.path() != Some(want_path) {
        return Err(Status::internal(format!("parts path {:?}", parts.path())));
    }
    if parts.service() != Some("helloworld.Greeter") {
        return Err(Status::internal(format!(
            "parts service {:?}",
            parts.service()
        )));
    }
    if parts.method() != Some(want_method) {
        return Err(Status::internal(format!(
            "parts method {:?}",
            parts.method()
        )));
    }
    Ok(msg)
}

struct SeesPath;

impl Greeter for SeesPath {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let msg = take_handler_path(request, "/helloworld.Greeter/SayHello", "SayHello")?;
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let _ = take_handler_path(request, "/helloworld.Greeter/ClientHello", "ClientHello")?;
        let mut reply = HelloReply::new();
        reply.set_message("path");
        Ok(Response::new(reply))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _ = take_handler_path(request, "/helloworld.Greeter/ServerHello", "ServerHello")?;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _ = take_handler_path(request, "/helloworld.Greeter/StreamHello", "StreamHello")?;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }
}

async fn assert_handler_sees_method_path(client: &GreeterClient) {
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let _ = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream");
    let (tx, call) = client.client_hello(Request::new(()));
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "path");
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    let _ = call.await.expect("bidi");
    assert!(Request::new(req("ada")).path().is_none());
    assert!(Request::new(req("ada")).service().is_none());
    assert!(Request::new(req("ada")).method().is_none());
}

#[tokio::test]
async fn interceptors_and_handlers_see_the_method_path() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesPath)
            .intercept(interceptor_require_greeter_path)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_handler_sees_method_path(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn tls_interceptors_and_handlers_see_the_method_path() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesPath)
            .intercept(interceptor_require_greeter_path)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_handler_sees_method_path(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn mtls_interceptors_and_handlers_see_the_method_path() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesPath)
            .intercept(interceptor_require_greeter_path)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_handler_sees_method_path(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_interceptors_and_handlers_see_the_method_path() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesPath)
            .intercept(interceptor_require_greeter_path)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_handler_sees_method_path(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn from_io_interceptors_and_handlers_see_the_method_path() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesPath)
            .intercept(interceptor_require_greeter_path)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_handler_sees_method_path(&client).await;
    server.abort();
}

fn interceptor_see_client_deadline(rpc: &mut Rpc) -> Result<(), Status> {
    let peer = rpc.peer_timeout();
    if peer != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!("peer timeout {peer:?}")));
    }
    rpc.set_timeout(Duration::from_secs(1));
    let effective = rpc.effective_timeout();
    if effective != Some(Duration::from_secs(1)) {
        return Err(Status::internal(format!("effective {effective:?}")));
    }
    let Some(deadline) = rpc.deadline() else {
        return Err(Status::internal("missing deadline"));
    };
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining > Duration::from_secs(1) {
        return Err(Status::internal(format!(
            "remaining too long {remaining:?}"
        )));
    }
    if remaining.is_zero() {
        return Err(Status::internal("deadline already passed"));
    }
    Ok(())
}

fn interceptor_require_no_deadline(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.peer_timeout().is_some() {
        return Err(Status::internal("unexpected peer timeout"));
    }
    if rpc.rpc_timeout().is_some() {
        return Err(Status::internal("unexpected server timeout overlay"));
    }
    if rpc.effective_timeout().is_some() {
        return Err(Status::internal("unexpected effective timeout"));
    }
    if rpc.deadline().is_some() {
        return Err(Status::internal("unexpected deadline"));
    }
    Ok(())
}

#[tokio::test]
async fn a_server_interceptor_cannot_extend_the_client_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_millis(50)),
        Duration::from_millis(150),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_tls_server_interceptor_cannot_extend_the_client_deadline() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_millis(50)),
        Duration::from_millis(150),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_server_interceptor_cannot_extend_the_client_deadline() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_millis(50)),
        Duration::from_millis(150),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_server_interceptor_cannot_extend_the_client_deadline() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_millis(50)),
        Duration::from_millis(150),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_server_interceptor_cannot_extend_the_client_deadline() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .intercept(interceptor_set_timeout_5s)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_deadline_quickly_on_every_shape(
        &client,
        Some(Duration::from_millis(50)),
        Duration::from_millis(150),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_server_interceptor_sees_the_client_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_see_client_deadline)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    echo_every_shape(&client, Some(Duration::from_secs(5))).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_server_interceptor_sees_the_client_deadline() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_see_client_deadline)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    echo_every_shape(&client, Some(Duration::from_secs(5))).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_server_interceptor_sees_the_client_deadline() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_see_client_deadline)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    echo_every_shape(&client, Some(Duration::from_secs(5))).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_server_interceptor_sees_the_client_deadline() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_see_client_deadline)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    echo_every_shape(&client, Some(Duration::from_secs(5))).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_server_interceptor_sees_the_client_deadline() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_see_client_deadline)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    echo_every_shape(&client, Some(Duration::from_secs(5))).await;
    server.abort();
}

#[tokio::test]
async fn a_server_interceptor_sees_a_missing_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_no_deadline)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_server_interceptor_sees_a_missing_deadline() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_no_deadline)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_server_interceptor_sees_a_missing_deadline() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_no_deadline)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_server_interceptor_sees_a_missing_deadline() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_no_deadline)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_server_interceptor_sees_a_missing_deadline() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(interceptor_require_no_deadline)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    echo_every_shape(&client, None).await;
    server.abort();
}

fn check_server_timeout_overlay<T>(request: &Request<T>) -> Result<(), Status> {
    if request.rpc_timeout() != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!(
            "rpc_timeout {:?}",
            request.rpc_timeout()
        )));
    }
    if request.peer_timeout() != Some(Duration::from_secs(30)) {
        return Err(Status::internal(format!(
            "peer_timeout {:?}",
            request.peer_timeout()
        )));
    }
    if request.timeout() != Some(Duration::from_secs(1)) {
        return Err(Status::internal(format!("timeout {:?}", request.timeout())));
    }
    Ok(())
}

fn interceptor_see_overlay_and_tighten(rpc: &mut Rpc) -> Result<(), Status> {
    if rpc.rpc_timeout() != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!(
            "rpc overlay {:?}",
            rpc.rpc_timeout()
        )));
    }
    if rpc.peer_timeout() != Some(Duration::from_secs(30)) {
        return Err(Status::internal(format!("peer {:?}", rpc.peer_timeout())));
    }
    if rpc.effective_timeout() != Some(Duration::from_secs(5)) {
        return Err(Status::internal(format!(
            "effective {:?}",
            rpc.effective_timeout()
        )));
    }
    rpc.set_timeout(Duration::from_secs(1));
    if rpc.rpc_timeout() != Some(Duration::from_secs(5)) {
        return Err(Status::internal("overlay vanished after set_timeout"));
    }
    if rpc.timeout() != Some(Duration::from_secs(1)) {
        return Err(Status::internal(format!("cap {:?}", rpc.timeout())));
    }
    if rpc.effective_timeout() != Some(Duration::from_secs(1)) {
        return Err(Status::internal(format!(
            "tightened {:?}",
            rpc.effective_timeout()
        )));
    }
    let shown = format!("{rpc:?}");
    if !shown.contains("rpc_timeout: Some(") {
        return Err(Status::internal(format!("rpc debug {shown}")));
    }
    Ok(())
}

struct SeesOverlay;

impl Greeter for SeesOverlay {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        check_server_timeout_overlay(&request)?;
        let (msg, parts) = request.into_message_and_parts();
        if parts.rpc_timeout() != Some(Duration::from_secs(5)) {
            return Err(Status::internal(format!(
                "parts rpc_timeout {:?}",
                parts.rpc_timeout()
            )));
        }
        if parts.peer_timeout() != Some(Duration::from_secs(30)) {
            return Err(Status::internal("parts peer_timeout must match Request"));
        }
        if parts.timeout() != Some(Duration::from_secs(1)) {
            return Err(Status::internal("parts timeout must match Request"));
        }
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        check_server_timeout_overlay(&request)?;
        let mut reply = HelloReply::new();
        reply.set_message("overlay");
        Ok(Response::new(reply))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        check_server_timeout_overlay(&request)?;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        check_server_timeout_overlay(&request)?;
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }
}

async fn assert_handler_sees_server_timeout_overlay(client: &GreeterClient) {
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_secs(30));
    let reply = client.say_hello(request).await.expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    let mut stream_req = Request::new(());
    stream_req.set_timeout(Duration::from_secs(30));
    let (tx, call) = client.client_hello(stream_req);
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "overlay");
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_secs(30));
    let _ = client.server_hello(request).await.expect("server-stream");
    let mut stream_req = Request::new(());
    stream_req.set_timeout(Duration::from_secs(30));
    let (tx, call) = client.stream_hello(stream_req);
    tx.close();
    let _ = call.await.expect("bidi");
    assert!(Request::new(req("ada")).rpc_timeout().is_none());
}

#[tokio::test]
async fn interceptors_and_handlers_see_the_server_timeout_overlay() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesOverlay)
            .timeout(Duration::from_secs(5))
            .intercept(interceptor_see_overlay_and_tighten)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_handler_sees_server_timeout_overlay(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn tls_interceptors_and_handlers_see_the_server_timeout_overlay() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesOverlay)
            .timeout(Duration::from_secs(5))
            .intercept(interceptor_see_overlay_and_tighten)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_handler_sees_server_timeout_overlay(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn mtls_interceptors_and_handlers_see_the_server_timeout_overlay() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesOverlay)
            .timeout(Duration::from_secs(5))
            .intercept(interceptor_see_overlay_and_tighten)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_handler_sees_server_timeout_overlay(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_interceptors_and_handlers_see_the_server_timeout_overlay() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesOverlay)
            .timeout(Duration::from_secs(5))
            .intercept(interceptor_see_overlay_and_tighten)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_handler_sees_server_timeout_overlay(&GreeterClient::new(unix_channel(&path).await))
        .await;
    task.abort();
}

#[tokio::test]
async fn from_io_interceptors_and_handlers_see_the_server_timeout_overlay() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesOverlay)
            .timeout(Duration::from_secs(5))
            .intercept(interceptor_see_overlay_and_tighten)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_handler_sees_server_timeout_overlay(&client).await;
    server.abort();
}

#[tokio::test]
async fn a_server_interceptor_can_reject_with_typed_status_details() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|_rpc: &mut Rpc| {
                let mut info = pbrs_grpc::pb::ErrorInfo::new();
                info.set_reason("API_DISABLED");
                info.set_domain("example.com");
                Err(Status::with_error_details(
                    Code::FailedPrecondition,
                    "api disabled",
                    [pbrs_grpc::Any::pack(&info)?],
                )?)
            })
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let unary = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("unary");
    assert_api_disabled(&unary);
    assert_api_disabled(
        &client
            .server_hello(Request::new(req("ada")))
            .await
            .expect_err("server-stream"),
    );
    let (tx, call) = client.client_hello(Request::new(()));
    tx.close();
    assert_api_disabled(&call.await.expect_err("client-stream"));
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    assert_api_disabled(&call.await.expect_err("bidi"));

    task.abort();
}

fn assert_api_disabled(err: &Status) {
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    assert_eq!(err.message(), "api disabled");
    let info = err
        .rpc()
        .expect("google.rpc.Status")
        .details()
        .get(0)
        .expect("one Any")
        .unpack::<pbrs_grpc::pb::ErrorInfo>()
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "API_DISABLED");
    assert_eq!(info.domain().to_str().unwrap_or(""), "example.com");
    let details = err.error_details().expect("ErrorDetails");
    let unpacked = details.error_info.expect("ErrorInfo");
    assert_eq!(unpacked.reason().to_str().unwrap_or(""), "API_DISABLED");
    assert_eq!(unpacked.domain().to_str().unwrap_or(""), "example.com");
}

fn one_ok_one_exhausted(codes: [Code; 2], what: &str) {
    assert!(
        codes.contains(&Code::Ok) && codes.contains(&Code::ResourceExhausted),
        "{what}: one Ok and one RESOURCE_EXHAUSTED, got {codes:?}"
    );
}

async fn assert_rpc_cap(client: &GreeterClient) {
    let (a, b) = tokio::join!(
        client.say_hello(Request::new(req("a"))),
        client.say_hello(Request::new(req("b"))),
    );
    one_ok_one_exhausted(
        [
            a.map(|_| Code::Ok).unwrap_or_else(|e| e.code()),
            b.map(|_| Code::Ok).unwrap_or_else(|e| e.code()),
        ],
        "unary",
    );

    let (c, d) = tokio::join!(client.server_hello(Request::new(req("c"))), async {
        let (tx, call) = client.stream_hello(Request::new(()));
        drop(tx);
        call.await
    });
    one_ok_one_exhausted(
        [
            c.map(|_| Code::Ok).unwrap_or_else(|e| e.code()),
            d.map(|_| Code::Ok).unwrap_or_else(|e| e.code()),
        ],
        "server-stream/bidi",
    );

    let (e, f) = tokio::join!(
        async {
            let (tx, call) = client.client_hello(Request::new(()));
            drop(tx);
            call.await
        },
        client.say_hello(Request::new(req("g"))),
    );
    one_ok_one_exhausted(
        [
            e.map(|_| Code::Ok).unwrap_or_else(|e| e.code()),
            f.map(|_| Code::Ok).unwrap_or_else(|e| e.code()),
        ],
        "client-stream",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extra_rpcs_are_refused_when_the_process_cap_is_hit() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_concurrent_rpcs(1)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_rpc_cap(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_extra_rpcs_are_refused_when_the_process_cap_is_hit() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_concurrent_rpcs(1)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_rpc_cap(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_extra_rpcs_are_refused_when_the_process_cap_is_hit() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_concurrent_rpcs(1)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_rpc_cap(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_extra_rpcs_are_refused_when_the_process_cap_is_hit() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_concurrent_rpcs(1)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_rpc_cap(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_extra_rpcs_are_refused_when_the_process_cap_is_hit() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_concurrent_rpcs(1)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_rpc_cap(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn outbound_rpcs_send_a_kernel_user_agent() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_kernel_user_agent(channel(addr).await).await;
    task.abort();
}

fn require_kernel_user_agent(rpc: &mut Rpc) -> Result<(), Status> {
    let ua = rpc.metadata().get("user-agent").unwrap_or("");
    if !ua.starts_with("pbrs-grpc/") {
        return Err(Status::invalid_argument(format!("user-agent {ua:?}")));
    }
    Ok(())
}

async fn assert_kernel_user_agent(ch: Channel) {
    echo_every_shape(&GreeterClient::new(ch), None).await;
}

#[tokio::test]
async fn tls_outbound_rpcs_send_a_kernel_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_kernel_user_agent(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_outbound_rpcs_send_a_kernel_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_kernel_user_agent(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_outbound_rpcs_send_a_kernel_user_agent() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_kernel_user_agent(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_outbound_rpcs_send_a_kernel_user_agent() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_kernel_user_agent(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

fn require_prefixed_user_agent(rpc: &mut Rpc) -> Result<(), Status> {
    let ua = rpc.metadata().get("user-agent").unwrap_or("");
    if !ua.starts_with("inventory/2.1 ") || !ua.contains("pbrs-grpc/") {
        return Err(Status::invalid_argument(format!("user-agent {ua:?}")));
    }
    Ok(())
}

async fn assert_prefixed_user_agent(ch: Channel) {
    let ch = ch.user_agent("inventory/2.1").expect("user-agent");
    echo_every_shape(&GreeterClient::new(ch), None).await;
}

async fn assert_user_agent_not_overridable(ch: Channel) {
    echo_every_shape(
        &GreeterClient::new(ch).intercept(|call: &mut Outgoing<'_>| {
            call.metadata_mut().insert("user-agent", "evil-agent")?;
            Ok(())
        }),
        None,
    )
    .await;
}

/// Answers without ever reading the request stream. Inbound messages are
/// decoded on the handler's task, so a handler that ignores them must still
/// terminate the RPC rather than leaving the client blocked on the window.
struct Deaf;

impl pbrs_grpc::Greeter for Deaf {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("deaf"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message("ignored your stream");
        Ok(Response::new(reply))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("deaf"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handler_that_ignores_its_request_stream_still_answers() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Deaf).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let (tx, call) = client.client_hello(Request::new(()));

    // Push more than fits in any buffer, from another task, so a hang here
    // would show up as the test timing out rather than as a deadlock.
    let sender = tokio::spawn(async move {
        for i in 0..512 {
            if tx.send(req(&format!("n{i}"))).await.is_err() {
                break;
            }
        }
    });

    let reply = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .expect("must not hang")
        .expect("must answer");
    assert_eq!(name_of(reply.get_ref()), "ignored your stream");
    sender.abort();

    let (tx, call) = client.stream_hello(Request::new(()));
    let sender = tokio::spawn(async move {
        for i in 0..512 {
            if tx.send(req(&format!("n{i}"))).await.is_err() {
                break;
            }
        }
    });
    let mut inbound = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .expect("bidi must not hang")
        .expect("bidi must answer")
        .into_inner();
    assert!(
        inbound.message().await.expect("end").is_none(),
        "Deaf bidi returns an empty stream without reading inbound"
    );
    sender.abort();
    task.abort();
}

async fn assert_deaf_handler_answers(client: &GreeterClient) {
    let (tx, call) = client.client_hello(Request::new(()));
    let sender = tokio::spawn(async move {
        for i in 0..512 {
            if tx.send(req(&format!("n{i}"))).await.is_err() {
                break;
            }
        }
    });
    let reply = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .expect("must not hang")
        .expect("must answer");
    assert_eq!(name_of(reply.get_ref()), "ignored your stream");
    sender.abort();

    let (tx, call) = client.stream_hello(Request::new(()));
    let sender = tokio::spawn(async move {
        for i in 0..512 {
            if tx.send(req(&format!("n{i}"))).await.is_err() {
                break;
            }
        }
    });
    let mut inbound = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .expect("bidi must not hang")
        .expect("bidi must answer")
        .into_inner();
    assert!(
        inbound.message().await.expect("end").is_none(),
        "Deaf bidi returns an empty stream without reading inbound"
    );
    sender.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_handler_that_ignores_its_request_stream_still_answers() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Deaf)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_deaf_handler_answers(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_handler_that_ignores_its_request_stream_still_answers() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Deaf)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_deaf_handler_answers(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_handler_that_ignores_its_request_stream_still_answers() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Deaf).serve_unix(sock).await.ok();
    });
    assert_deaf_handler_answers(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_handler_that_ignores_its_request_stream_still_answers() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Deaf)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_deaf_handler_answers(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn config_flows_from_the_generated_server_to_the_router() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_plus_test_with_decode_cap()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_add_service_decode_cap(
        &GreeterClient::new(channel(addr).await),
        &TestServiceClient::new(channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_config_flows_from_the_generated_server_to_the_router() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_plus_test_with_decode_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_add_service_decode_cap(
        &GreeterClient::new(tls_channel(addr).await),
        &TestServiceClient::new(tls_channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_config_flows_from_the_generated_server_to_the_router() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_plus_test_with_decode_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_add_service_decode_cap(
        &GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        &TestServiceClient::new(tls_channel_with(addr, client_tls).await),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_config_flows_from_the_generated_server_to_the_router() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        greeter_plus_test_with_decode_cap()
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_add_service_decode_cap(
        &GreeterClient::new(unix_channel(&path).await),
        &TestServiceClient::new(unix_channel(&path).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_config_flows_from_the_generated_server_to_the_router() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        greeter_plus_test_with_decode_cap()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let ch = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_add_service_decode_cap(&GreeterClient::new(ch.clone()), &TestServiceClient::new(ch))
        .await;
    server.abort();
}

#[tokio::test]
async fn encode_config_flows_from_the_generated_server_to_the_router() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_plus_test_with_encode_cap()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_add_service_encode_cap(
        &GreeterClient::new(channel(addr).await),
        &TestServiceClient::new(channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_encode_config_flows_from_the_generated_server_to_the_router() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_plus_test_with_encode_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_add_service_encode_cap(
        &GreeterClient::new(tls_channel(addr).await),
        &TestServiceClient::new(tls_channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_encode_config_flows_from_the_generated_server_to_the_router() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_plus_test_with_encode_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_add_service_encode_cap(
        &GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        &TestServiceClient::new(tls_channel_with(addr, client_tls).await),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_encode_config_flows_from_the_generated_server_to_the_router() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        greeter_plus_test_with_encode_cap()
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_add_service_encode_cap(
        &GreeterClient::new(unix_channel(&path).await),
        &TestServiceClient::new(unix_channel(&path).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_encode_config_flows_from_the_generated_server_to_the_router() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        greeter_plus_test_with_encode_cap()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let ch = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_add_service_encode_cap(&GreeterClient::new(ch.clone()), &TestServiceClient::new(ch))
        .await;
    server.abort();
}

#[test]
fn http2_tuning_knobs_are_fluent_on_server_and_router() {
    let server = GreeterServer::new(Echo)
        .initial_stream_window_size(3 * 1024 * 1024)
        .max_frame_size(65_536);
    let dbg = format!("{server:?}");
    assert!(dbg.contains("3145728"), "{dbg}");
    assert!(dbg.contains("65536"), "{dbg}");

    let router = Router::new()
        .add_service(GreeterServer::new(Echo))
        .initial_connection_window_size(7 * 1024 * 1024)
        .max_header_list_size(4096)
        .max_send_buffer_size(123_456)
        .max_pending_accept_reset_streams(3);
    let dbg = format!("{router:?}");
    assert!(dbg.contains("7340032"), "{dbg}");
    assert!(dbg.contains("4096"), "{dbg}");
    assert!(dbg.contains("123456"), "{dbg}");
    assert!(dbg.contains("max_pending_accept_reset_streams: 3"), "{dbg}");
}

async fn assert_dead_channel_redials(client: &GreeterClient) {
    // The first attempt can still land on the dying connection (`ready`
    // succeeded, then GOAWAY). Unary and server-streaming retry that redial
    // once; this loop covers a rebound listener that is not yet accepting.
    let after = until_ok("unary after", || {
        client.say_hello(Request::new(req("after")))
    })
    .await;
    assert_eq!(name_of(after.get_ref()), "after");
    echo_every_shape(client, None).await;
}

async fn assert_dead_channel_fails_fast(client: &GreeterClient) {
    tokio::time::timeout(Duration::from_secs(2), assert_gone_on_every_shape(client))
        .await
        .expect("reconnect to a closed port hung");
}

async fn assert_lazy_fails_fast(client: GreeterClient) {
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("fail-fast hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
async fn serve_unix_echo_unlink(path: std::path::PathBuf) -> tokio::task::JoinHandle<()> {
    let mut last = Status::unavailable("bind");
    for _ in 0..100 {
        let sock = path.clone();
        let handle = tokio::spawn(async move {
            GreeterServer::new(Echo).serve_unix_unlink(sock).await.ok();
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        if !handle.is_finished() {
            return handle;
        }
        last = Status::unavailable("unix unlink bind failed");
    }
    panic!("unix rebind: {last}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_channel_redials_the_same_address() {
    let (addr, client, guard) = spawn_greeter(Echo).await.expect("spawn");
    echo_every_shape(&client, None).await;
    drop(guard);
    let _guard = serve_at(addr, Echo, ServerConfig::default())
        .await
        .expect("rebind");
    assert_dead_channel_redials(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_a_dead_channel_redials_the_same_address() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn({
        let tls = tls.clone();
        async move {
            GreeterServer::new(Echo)
                .serve_tls_with_shutdown(listener, std::future::pending(), tls)
                .await
                .ok();
        }
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    echo_every_shape(&client, None).await;
    task.abort();
    let _rebind = serve_tls_at(addr, tls).await.expect("rebind");
    assert_dead_channel_redials(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_a_dead_channel_redials_the_same_address() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn({
        let tls = tls.clone();
        async move {
            GreeterServer::new(Echo)
                .serve_tls_with_shutdown(listener, std::future::pending(), tls)
                .await
                .ok();
        }
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    echo_every_shape(&client, None).await;
    task.abort();
    let _rebind = serve_tls_at(addr, tls).await.expect("rebind");
    assert_dead_channel_redials(&client).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_a_dead_channel_redials_the_same_path() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    echo_every_shape(&client, None).await;
    task.abort();
    let _rebind = serve_unix_echo_unlink(path).await;
    assert_dead_channel_redials(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_channel_fails_fast_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_shutdown(listener, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    echo_every_shape(&client, None).await;
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");
    assert_dead_channel_fails_fast(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_a_dead_channel_fails_fast_when_nothing_is_listening() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(
                listener,
                async {
                    shutdown_rx.await.ok();
                },
                tls,
            )
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    echo_every_shape(&client, None).await;
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");
    assert_dead_channel_fails_fast(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_a_dead_channel_fails_fast_when_nothing_is_listening() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(
                listener,
                async {
                    shutdown_rx.await.ok();
                },
                tls,
            )
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    echo_every_shape(&client, None).await;
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");
    assert_dead_channel_fails_fast(&client).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_a_dead_channel_fails_fast_when_nothing_is_listening() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_until_shutdown(sock, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    echo_every_shape(&client, None).await;
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");
    assert_dead_channel_fails_fast(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_lazy_fails_fast_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);
    assert_lazy_fails_fast(GreeterClient::new(
        Channel::connect_lazy(addr).expect("lazy"),
    ))
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_connect_lazy_fails_fast_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_lazy_fails_fast(GreeterClient::new(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
    ))
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_connect_lazy_fails_fast_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_lazy_fails_fast(GreeterClient::new(
        Channel::connect_tls_lazy(addr, client_tls).expect("lazy"),
    ))
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    let client = GreeterClient::new(channel);
    // Creating a Call does not start the RPC; first poll does. Drive all
    // four shapes long enough to prove they are retrying, then bind.
    wait_then_complete_every_shape(&client, true, async {
        serve_at(addr, Echo, ServerConfig::default())
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy").wait_for_ready();
    let client = GreeterClient::new(channel);
    wait_then_complete_every_shape(&client, false, async {
        serve_at(addr, Echo, ServerConfig::default())
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    let client = GreeterClient::new(channel);
    wait_then_complete_every_shape(&client, true, async {
        serve_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let client = GreeterClient::new(channel);
    wait_then_complete_every_shape(&client, false, async {
        serve_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    let client = GreeterClient::new(channel);
    wait_then_complete_every_shape(&client, true, async {
        serve_tls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let client = GreeterClient::new(channel);
    wait_then_complete_every_shape(&client, false, async {
        serve_tls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let client = GreeterClient::new(channel);
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_opt_out_every_shape(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    let client = GreeterClient::new(channel);
    let timeout = Duration::from_millis(80);
    let min = Duration::from_millis(50);
    let max = Duration::from_secs(2);
    assert_deadline_in(
        client.say_hello(stamp_wait_deadline(Request::new(req("x")), timeout)),
        min,
        max,
    )
    .await;
    assert_deadline_in(
        client.server_hello(stamp_wait_deadline(Request::new(req("x")), timeout)),
        min,
        max,
    )
    .await;
    let (tx, call) = client.client_hello(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
    let (tx, call) = client.stream_hello(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let client = GreeterClient::new(channel);
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_opt_out_every_shape(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    let client = GreeterClient::new(channel);
    let timeout = Duration::from_millis(80);
    let min = Duration::from_millis(50);
    let max = Duration::from_secs(2);
    assert_deadline_in(
        client.say_hello(stamp_wait_deadline(Request::new(req("x")), timeout)),
        min,
        max,
    )
    .await;
    assert_deadline_in(
        client.server_hello(stamp_wait_deadline(Request::new(req("x")), timeout)),
        min,
        max,
    )
    .await;
    let (tx, call) = client.client_hello(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
    let (tx, call) = client.stream_hello(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy").wait_for_ready();
    let client = GreeterClient::new(channel);
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_opt_out_every_shape(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    let client = GreeterClient::new(channel);
    let timeout = Duration::from_millis(80);
    let min = Duration::from_millis(50);
    let max = Duration::from_secs(2);
    assert_deadline_in(
        client.say_hello(stamp_wait_deadline(Request::new(req("x")), timeout)),
        min,
        max,
    )
    .await;
    assert_deadline_in(
        client.server_hello(stamp_wait_deadline(Request::new(req("x")), timeout)),
        min,
        max,
    )
    .await;
    let (tx, call) = client.client_hello(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
    let (tx, call) = client.stream_hello(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
}

fn assert_handshake_timed_out(err: &Status, started: Instant) {
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(
        err.message().contains("timed out"),
        "expected timeout status, got {err}"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(50),
        "timed out too fast: {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "timed out too slow: {:?}",
        started.elapsed()
    );
}

fn assert_connect_refused_fast(err: &Status, started: Instant) {
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "refused connect took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_times_out_when_the_peer_never_speaks_http2() {
    let (addr, listener) = bind().await;
    let started = Instant::now();
    let err = Channel::connect_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_millis(80)),
    )
    .await
    .expect_err("handshake should time out");
    assert_handshake_timed_out(&err, started);
    drop(listener);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_connect_times_out_when_the_peer_never_speaks() {
    let (addr, listener) = bind().await;
    let started = Instant::now();
    let err = Channel::connect_tls_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_millis(80)),
        ClientTls::ca("localhost", CA).expect("client tls"),
    )
    .await
    .expect_err("handshake should time out");
    assert_handshake_timed_out(&err, started);
    drop(listener);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_connect_times_out_when_the_peer_never_speaks() {
    let (addr, listener) = bind().await;
    let started = Instant::now();
    let err = Channel::connect_tls_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_millis(80)),
        ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client"),
    )
    .await
    .expect_err("handshake should time out");
    assert_handshake_timed_out(&err, started);
    drop(listener);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_to_a_closed_port_fails_fast() {
    let (addr, listener) = bind().await;
    drop(listener);
    let started = Instant::now();
    let err = Channel::connect_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_secs(20)),
    )
    .await
    .expect_err("closed port");
    assert_connect_refused_fast(&err, started);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_connect_to_a_closed_port_fails_fast() {
    let (addr, listener) = bind().await;
    drop(listener);
    let started = Instant::now();
    let err = Channel::connect_tls_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_secs(20)),
        ClientTls::ca("localhost", CA).expect("client tls"),
    )
    .await
    .expect_err("closed port");
    assert_connect_refused_fast(&err, started);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_connect_to_a_closed_port_fails_fast() {
    let (addr, listener) = bind().await;
    drop(listener);
    let started = Instant::now();
    let err = Channel::connect_tls_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_secs(20)),
        ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client"),
    )
    .await
    .expect_err("closed port");
    assert_connect_refused_fast(&err, started);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_connect_to_a_missing_path_fails_fast() {
    let (path, _guard) = unix_test_path();
    let started = Instant::now();
    let err = Channel::connect_unix_with(
        &path,
        ChannelConfig::new().connect_timeout(Duration::from_secs(20)),
    )
    .await
    .expect_err("missing path");
    assert_connect_refused_fast(&err, started);
}

async fn assert_mute_does_not_stop<M>(client: GreeterClient, mute: M) {
    echo_every_shape(&client, None).await;
    drop(mute);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mute_tcp_peer_does_not_stop_the_server_serving() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .handshake_timeout(Duration::from_millis(80))
            .serve_listener(listener)
            .await
            .ok();
    });
    let mute = tokio::net::TcpStream::connect(addr)
        .await
        .expect("mute connect");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_mute_does_not_stop(GreeterClient::new(channel(addr).await), mute).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mute_tls_peer_does_not_stop_the_server_serving() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .handshake_timeout(Duration::from_millis(80))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let mute = tokio::net::TcpStream::connect(addr)
        .await
        .expect("mute connect");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_mute_does_not_stop(GreeterClient::new(tls_channel(addr).await), mute).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mute_mtls_peer_does_not_stop_the_server_serving() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .handshake_timeout(Duration::from_millis(80))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let mute = tokio::net::TcpStream::connect(addr)
        .await
        .expect("mute connect");
    tokio::time::sleep(Duration::from_millis(150)).await;
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_mute_does_not_stop(
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
        mute,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mute_unix_peer_does_not_stop_the_server_serving() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .handshake_timeout(Duration::from_millis(80))
            .serve_unix(sock)
            .await
            .ok();
    });
    let mut last = None;
    let mute = {
        let mut found = None;
        for _ in 0..80 {
            match tokio::net::UnixStream::connect(&path).await {
                Ok(stream) => {
                    found = Some(stream);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("mute unix: {last:?}"))
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_mute_does_not_stop(GreeterClient::new(unix_channel(&path).await), mute).await;
    task.abort();
}

async fn assert_age_goaway_redials(client: GreeterClient) {
    echo_every_shape(&client, None).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        echo_every_shape(&client, Some(Duration::from_secs(5))),
    )
    .await
    .expect("redial hung");
}

async fn assert_idle_goaway_redials(client: GreeterClient) {
    echo_every_shape(&client, None).await;
    // Keepalive PINGs must not reset idle. Wait well past the idle cap.
    tokio::time::sleep(Duration::from_millis(250)).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        echo_every_shape(&client, Some(Duration::from_secs(5))),
    )
    .await
    .expect("redial hung");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_connection_age_goaway_then_the_channel_redials() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_age_goaway_redials(GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_max_connection_age_goaway_then_the_channel_redials() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_age_goaway_redials(GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_max_connection_age_goaway_then_the_channel_redials() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_age_goaway_redials(GreeterClient::new(tls_channel_with(addr, client_tls).await)).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_max_connection_age_goaway_then_the_channel_redials() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_age_goaway_redials(GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_connection_idle_goaway_then_the_channel_redials() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age_grace(Duration::from_secs(2))
            .max_connection_idle(Duration::from_millis(80))
            .keep_alive_interval(Duration::from_millis(20))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_idle_goaway_redials(GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_max_connection_idle_goaway_then_the_channel_redials() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age_grace(Duration::from_secs(2))
            .max_connection_idle(Duration::from_millis(80))
            .keep_alive_interval(Duration::from_millis(20))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_idle_goaway_redials(GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_max_connection_idle_goaway_then_the_channel_redials() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age_grace(Duration::from_secs(2))
            .max_connection_idle(Duration::from_millis(80))
            .keep_alive_interval(Duration::from_millis(20))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_idle_goaway_redials(GreeterClient::new(tls_channel_with(addr, client_tls).await)).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_max_connection_idle_goaway_then_the_channel_redials() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_connection_age_grace(Duration::from_secs(2))
            .max_connection_idle(Duration::from_millis(80))
            .keep_alive_interval(Duration::from_millis(20))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_idle_goaway_redials(GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

async fn assert_age_lets_in_flight_finish(client: GreeterClient) {
    let mut call = client.say_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("Slow returned before age: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    let reply = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("in-flight RPC hung past grace")
        .expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_connection_age_lets_in_flight_rpcs_finish() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_age_lets_in_flight_finish(GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_max_connection_age_lets_in_flight_rpcs_finish() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_age_lets_in_flight_finish(GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_max_connection_age_lets_in_flight_rpcs_finish() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_age_lets_in_flight_finish(GreeterClient::new(tls_channel_with(addr, client_tls).await))
        .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_max_connection_age_lets_in_flight_rpcs_finish() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_age_lets_in_flight_finish(GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_max_connection_age_lets_in_flight_rpcs_finish() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_age(Duration::from_millis(80))
            .max_connection_age_grace(Duration::from_secs(2))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_age_lets_in_flight_finish(GreeterClient::new(channel)).await;
    server.abort();
}

async fn assert_idle_lets_in_flight_finish(client: GreeterClient) {
    let mut call = client.say_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("Slow returned before idle: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    let reply = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("in-flight RPC hung past idle")
        .expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_connection_idle_lets_in_flight_rpcs_finish() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_idle(Duration::from_millis(50))
            .max_connection_age_grace(Duration::from_millis(1))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_idle_lets_in_flight_finish(GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_max_connection_idle_lets_in_flight_rpcs_finish() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_idle(Duration::from_millis(50))
            .max_connection_age_grace(Duration::from_millis(1))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_idle_lets_in_flight_finish(GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_max_connection_idle_lets_in_flight_rpcs_finish() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_idle(Duration::from_millis(50))
            .max_connection_age_grace(Duration::from_millis(1))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_idle_lets_in_flight_finish(GreeterClient::new(tls_channel_with(addr, client_tls).await))
        .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_max_connection_idle_lets_in_flight_rpcs_finish() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_idle(Duration::from_millis(50))
            .max_connection_age_grace(Duration::from_millis(1))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_idle_lets_in_flight_finish(GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_max_connection_idle_lets_in_flight_rpcs_finish() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .max_connection_idle(Duration::from_millis(50))
            .max_connection_age_grace(Duration::from_millis(1))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_idle_lets_in_flight_finish(GreeterClient::new(channel)).await;
    server.abort();
}

async fn channel_with(addr: SocketAddr, cfg: ChannelConfig) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_with(addr, cfg).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}");
}

struct CountingIncoming {
    listener: TcpListener,
    n: Arc<AtomicUsize>,
}

impl Incoming for CountingIncoming {
    type Io = tokio::net::TcpStream;

    async fn accept(&mut self) -> pbrs_grpc::IncomingAccept<Self::Io> {
        match self.listener.accept().await {
            Ok((io, addr)) => {
                self.n.fetch_add(1, Ordering::Relaxed);
                Some(Ok((io, Some(addr))))
            }
            Err(e) => Some(Err(Status::unavailable(e.to_string()))),
        }
    }
}

fn client_idle_cfg() -> ChannelConfig {
    ChannelConfig::new()
        .max_connection_idle(Duration::from_millis(80))
        .keep_alive_interval(Duration::from_millis(20))
}

fn stamp_idle_peer(
    peers: Arc<Mutex<HashSet<SocketAddr>>>,
) -> impl Fn(&mut Rpc) -> Result<(), Status> {
    move |rpc: &mut Rpc| {
        let Some(addr) = rpc.remote_addr() else {
            return Err(Status::internal("idle probe missing remote_addr"));
        };
        peers.lock().expect("peers").insert(addr);
        Ok(())
    }
}

async fn assert_client_idle_closes(client: &GreeterClient, accepts: &AtomicUsize) {
    assert_eq!(accepts.load(Ordering::Relaxed), 1, "dial is one accept");
    echo_every_shape(client, None).await;
    assert_eq!(accepts.load(Ordering::Relaxed), 1, "rpcs reuse the dial");
    tokio::time::sleep(Duration::from_millis(250)).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        echo_every_shape(client, Some(Duration::from_secs(5))),
    )
    .await
    .expect("redial hung");
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        2,
        "idle must tear down the socket so the next RPC dials again"
    );
}

async fn assert_client_idle_closes_peers(
    client: &GreeterClient,
    peers: &Mutex<HashSet<SocketAddr>>,
) {
    echo_every_shape(client, None).await;
    assert_eq!(peers.lock().expect("peers").len(), 1, "dial is one peer");
    tokio::time::sleep(Duration::from_millis(250)).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        echo_every_shape(client, Some(Duration::from_secs(5))),
    )
    .await
    .expect("redial hung");
    assert_eq!(
        peers.lock().expect("peers").len(),
        2,
        "idle must tear down the socket so the next RPC dials again"
    );
}

async fn assert_client_idle_in_flight(client: GreeterClient) {
    let mut call = client.say_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("Slow returned before client idle: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    let reply = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("in-flight RPC hung past client idle")
        .expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");
}

#[cfg(unix)]
struct CountingUnixIncoming {
    listener: tokio::net::UnixListener,
    n: Arc<AtomicUsize>,
}

#[cfg(unix)]
impl Incoming for CountingUnixIncoming {
    type Io = tokio::net::UnixStream;

    async fn accept(&mut self) -> pbrs_grpc::IncomingAccept<Self::Io> {
        match self.listener.accept().await {
            Ok((io, _)) => {
                self.n.fetch_add(1, Ordering::Relaxed);
                Some(Ok((io, None)))
            }
            Err(e) => Some(Err(Status::unavailable(e.to_string()))),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_max_connection_idle_closes_the_socket() {
    let accepts = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let n = Arc::clone(&accepts);
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_incoming(CountingIncoming { listener, n })
            .await
            .ok();
    });
    let client = GreeterClient::new(channel_with(addr, client_idle_cfg()).await);
    assert_client_idle_closes(&client, &accepts).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_client_max_connection_idle_closes_the_socket() {
    let peers = Arc::new(Mutex::new(HashSet::new()));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let seen = Arc::clone(&peers);
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(stamp_idle_peer(seen))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = GreeterClient::new(tls_channel_cfg(addr, client_tls, client_idle_cfg()).await);
    assert_client_idle_closes_peers(&client, &peers).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_client_max_connection_idle_closes_the_socket() {
    let peers = Arc::new(Mutex::new(HashSet::new()));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let seen = Arc::clone(&peers);
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(stamp_idle_peer(seen))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_cfg(addr, client_tls, client_idle_cfg()).await);
    assert_client_idle_closes_peers(&client, &peers).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_client_max_connection_idle_closes_the_socket() {
    let accepts = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let n = Arc::clone(&accepts);
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_incoming(CountingUnixIncoming { listener, n })
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel_with(&path, client_idle_cfg()).await);
    assert_client_idle_closes(&client, &accepts).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_max_connection_idle_lets_in_flight_rpcs_finish() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_listener(listener).await.ok();
    });
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    assert_client_idle_in_flight(GreeterClient::new(channel_with(addr, cfg).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_client_max_connection_idle_lets_in_flight_rpcs_finish() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    assert_client_idle_in_flight(GreeterClient::new(
        tls_channel_cfg(addr, client_tls, cfg).await,
    ))
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_client_max_connection_idle_lets_in_flight_rpcs_finish() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    assert_client_idle_in_flight(GreeterClient::new(
        tls_channel_cfg(addr, client_tls, cfg).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_client_max_connection_idle_lets_in_flight_rpcs_finish() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow).serve_unix(sock).await.ok();
    });
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    assert_client_idle_in_flight(GreeterClient::new(unix_channel_with(&path, cfg).await)).await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_max_connection_idle_lets_in_flight_rpcs_finish() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io_with(
        client_io,
        "localhost",
        ChannelConfig::new().max_connection_idle(Duration::from_millis(50)),
    )
    .await
    .expect("from_io");
    assert_client_idle_in_flight(GreeterClient::new(channel)).await;
    server.abort();
}

struct DelayedStream;

impl pbrs_grpc::Greeter for DelayedStream {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("delayed-stream"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("delayed-stream"))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let name = request
            .get_ref()
            .name()
            .to_str()
            .unwrap_or_default()
            .to_owned();
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let mut reply = HelloReply::new();
            reply.set_message(name);
            tx.send(reply).await.ok();
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("delayed-stream"))
    }
}

async fn assert_client_idle_holds_server_stream(client: GreeterClient) {
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let reply = tokio::time::timeout(Duration::from_secs(5), stream.message())
        .await
        .expect("stream hung")
        .expect("stream status")
        .expect("one message");
    assert_eq!(name_of(&reply), "ada");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_idle_holds_a_server_stream() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(DelayedStream)
            .serve_listener(listener)
            .await
            .ok();
    });
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    assert_client_idle_holds_server_stream(GreeterClient::new(channel_with(addr, cfg).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_client_idle_holds_a_server_stream() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(DelayedStream)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    assert_client_idle_holds_server_stream(GreeterClient::new(
        tls_channel_cfg(addr, client_tls, cfg).await,
    ))
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_client_idle_holds_a_server_stream() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(DelayedStream)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    assert_client_idle_holds_server_stream(GreeterClient::new(
        tls_channel_cfg(addr, client_tls, cfg).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_client_idle_holds_a_server_stream() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(DelayedStream)
            .serve_unix(sock)
            .await
            .ok();
    });
    let cfg = ChannelConfig::new().max_connection_idle(Duration::from_millis(50));
    assert_client_idle_holds_server_stream(GreeterClient::new(unix_channel_with(&path, cfg).await))
        .await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_idle_holds_a_server_stream() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(DelayedStream)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io_with(
        client_io,
        "localhost",
        ChannelConfig::new().max_connection_idle(Duration::from_millis(50)),
    )
    .await
    .expect("from_io");
    assert_client_idle_holds_server_stream(GreeterClient::new(channel)).await;
    server.abort();
}

#[cfg(unix)]
struct UnixSockGuard(std::path::PathBuf);

#[cfg(unix)]
impl Drop for UnixSockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(unix)]
fn unix_test_path() -> (std::path::PathBuf, UnixSockGuard) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pbrs-grpc-{}-{}.sock",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    (path.clone(), UnixSockGuard(path))
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_socket_serves() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.remote_addr().is_some() || rpc.local_addr().is_some() {
                    return Err(Status::internal("unix has no std::net::SocketAddr"));
                }
                if rpc.peer_identity().is_some() {
                    return Err(Status::internal("unix has no TLS client certificate"));
                }
                let Some(cred) = rpc.peer_cred() else {
                    return Err(Status::internal("unix missing peer_cred"));
                };
                if cred.pid() != Some(std::process::id()) {
                    return Err(Status::internal(format!(
                        "unix pid {:?} want {}",
                        cred.pid(),
                        std::process::id()
                    )));
                }
                if rpc.scheme() != Some("http") {
                    return Err(Status::internal(format!("unix scheme {:?}", rpc.scheme())));
                }
                Ok(())
            })
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let channel = Channel::connect_unix(&path).await.expect("connect");
    echo_every_shape(&GreeterClient::new(channel), None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_send_compressed_gzips_every_shape() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .send_compressed()
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let channel = Channel::connect_unix(&path)
        .await
        .expect("connect")
        .send_compressed();
    gzip_every_shape(&GreeterClient::new(channel)).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_interceptor_rejects_with_typed_status() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()))
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(Channel::connect_unix(&path).await.expect("connect"));
    assert_greeter_blocked_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_client_interceptor_rejects_with_typed_status() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(Channel::connect_unix(&path).await.expect("connect"))
        .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_greeter_blocked_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_client_interceptor_sees_every_shape_context() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_context)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(Channel::connect_unix(&path).await.expect("connect"))
        .intercept(stamp_outgoing_context);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_generated_handler_sees_unix_peer_cred() {
    struct SeesUnix;

    impl Greeter for SeesUnix {
        async fn say_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            let msg = sees_unix(request)?;
            Ok(Response::new(common::reply(common::name_of_request(&msg))))
        }

        async fn client_hello(
            &self,
            request: Request<pbrs_grpc::Streaming<HelloRequest>>,
        ) -> Result<Response<HelloReply>, Status> {
            let _ = sees_unix(request)?;
            Ok(Response::new(common::reply("ada")))
        }

        async fn server_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
            let msg = sees_unix(request)?;
            Ok(echo_named_stream(common::name_of_request(&msg)))
        }

        async fn stream_hello(
            &self,
            request: Request<pbrs_grpc::Streaming<HelloRequest>>,
        ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
            let _ = sees_unix(request)?;
            Ok(echo_named_stream("ada".into()))
        }
    }

    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesUnix)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[cfg(unix)]
async fn unix_channel(path: &std::path::Path) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_unix(path).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("connect unix {}: {last:?}", path.display());
}

#[cfg(unix)]
async fn unix_channel_with(path: &std::path::Path, cfg: ChannelConfig) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_unix_with(path, cfg).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("connect unix {}: {last:?}", path.display());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_until_shutdown_serves_then_drains() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_until_shutdown(sock, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("unix drain hung")
        .expect("join");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_unlink_until_shutdown_replaces_leftover_then_drains() {
    let (path, _guard) = unix_test_path();
    let leftover = tokio::net::UnixListener::bind(&path).expect("stale");
    drop(leftover);
    let sock = path.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_unix_unlink_until_shutdown(sock, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("unix unlink drain hung")
        .expect("join");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_interceptor_sees_unix_localhost_authority() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.authority() != Some("localhost") {
                    return Err(Status::internal(format!(
                        "unix server authority {:?}",
                        rpc.authority()
                    )));
                }
                if rpc.scheme() != Some("http") {
                    return Err(Status::internal(format!(
                        "unix https_scheme must stay http, got {:?}",
                        rpc.scheme()
                    )));
                }
                Ok(())
            })
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let channel = Channel::connect_unix(&path)
        .await
        .expect("connect")
        .https_scheme();
    assert_eq!(channel.authority(), "localhost");
    assert_eq!(channel.scheme(), "http");
    let client = GreeterClient::new(channel);
    assert_eq!(client.authority(), "localhost");
    assert_eq!(client.scheme(), "http");
    assert!(
        client.grpc_user_agent().starts_with("pbrs-grpc/"),
        "{}",
        client.grpc_user_agent()
    );
    let client = client.intercept(|call: &mut Outgoing<'_>| {
        if call.authority() != "localhost" {
            return Err(Status::internal(format!(
                "authority {} want localhost",
                call.authority()
            )));
        }
        if call.scheme() != "http" {
            return Err(Status::internal(format!(
                "unix https_scheme must stay http, got {}",
                call.scheme()
            )));
        }
        Ok(())
    });
    echo_every_shape(&client, None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_lazy_fails_fast_when_nothing_is_listening() {
    let (path, _guard) = unix_test_path();
    assert_lazy_fails_fast(GreeterClient::new(
        Channel::connect_unix_lazy(&path).expect("lazy"),
    ))
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_wait_for_ready_completes_once_the_server_listens() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path).expect("lazy");
    let client = GreeterClient::new(channel);
    wait_then_complete_every_shape(&client, true, async {
        let sock = path.clone();
        tokio::spawn(async move {
            GreeterServer::new(Echo).serve_unix(sock).await.ok();
        })
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_channel_wait_for_ready_completes_once_the_server_listens() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    let client = GreeterClient::new(channel);
    wait_then_complete_every_shape(&client, false, async {
        let sock = path.clone();
        tokio::spawn(async move {
            GreeterServer::new(Echo).serve_unix(sock).await.ok();
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = TestServiceClient::connect_lazy(addr).expect("lazy");
    wait_then_complete_test(&client, true, async {
        serve_test_at(addr).await.expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = TestServiceClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_test(&client, false, async {
        serve_test_at(addr).await.expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tls_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    wait_then_complete_test(&client, true, async {
        serve_test_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tls_channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_test(&client, false, async {
        serve_test_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mtls_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    wait_then_complete_test(&client, true, async {
        serve_test_tls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mtls_channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_test(&client, false, async {
        serve_test_tls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_unix_wait_for_ready_completes_once_the_server_listens() {
    let (path, _guard) = unix_test_path();
    let client = TestServiceClient::connect_unix_lazy(&path).expect("lazy");
    wait_then_complete_test(&client, true, async {
        let sock = path.clone();
        tokio::spawn(async move {
            TestServiceServer::new(InteropTestService)
                .serve_unix(sock)
                .await
                .ok();
        })
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_unix_channel_wait_for_ready_completes_once_the_server_listens() {
    let (path, _guard) = unix_test_path();
    let client = TestServiceClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_test(&client, false, async {
        let sock = path.clone();
        tokio::spawn(async move {
            TestServiceServer::new(InteropTestService)
                .serve_unix(sock)
                .await
                .ok();
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = TestServiceClient::connect_lazy(addr)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_test(&client, false, async {
        serve_test_at(addr).await.expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_tls_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_test(&client, false, async {
        serve_test_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_mtls_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_test(&client, false, async {
        serve_test_tls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_unix_client_interceptor_can_set_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let client = TestServiceClient::connect_unix_lazy(&path)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_test(&client, false, async {
        let sock = path.clone();
        tokio::spawn(async move {
            TestServiceServer::new(InteropTestService)
                .serve_unix(sock)
                .await
                .ok();
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = TestServiceClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_test_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_tls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_test_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_mtls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_test_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_unix_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let client = TestServiceClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_test_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    wait_then_complete_reverser(&channel, true, async {
        serve_reverser_at(addr).await.expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy").wait_for_ready();
    wait_then_complete_reverser(&channel, false, async {
        serve_reverser_at(addr).await.expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_tls_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    wait_then_complete_reverser(&channel, true, async {
        serve_reverser_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_tls_channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_reverser(&channel, false, async {
        serve_reverser_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_mtls_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    wait_then_complete_reverser(&channel, true, async {
        serve_reverser_mtls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_mtls_channel_wait_for_ready_completes_once_the_server_listens() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_reverser(&channel, false, async {
        serve_reverser_mtls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_unix_wait_for_ready_completes_once_the_server_listens() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path).expect("lazy");
    wait_then_complete_reverser(&channel, true, async {
        let sock = path.clone();
        tokio::spawn(async move {
            Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
                .serve_unix(sock)
                .await
                .ok();
        })
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_unix_channel_wait_for_ready_completes_once_the_server_listens() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    wait_then_complete_reverser(&channel, false, async {
        let sock = path.clone();
        tokio::spawn(async move {
            Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
                .serve_unix(sock)
                .await
                .ok();
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reverser_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel =
        Channel::connect_lazy(addr)
            .expect("lazy")
            .intercept(|call: &mut Outgoing<'_>| {
                call.set_wait_for_ready(true);
                Ok(())
            });
    wait_then_complete_reverser(&channel, false, async {
        serve_reverser_at(addr).await.expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reverser_tls_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_reverser(&channel, false, async {
        serve_reverser_tls_at(addr, ServerTls::new(server_identity()).expect("server tls"))
            .await
            .expect("serve")
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reverser_mtls_client_interceptor_can_set_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(true);
            Ok(())
        });
    wait_then_complete_reverser(&channel, false, async {
        serve_reverser_mtls_at(
            addr,
            ServerTls::mtls(server_identity(), CA).expect("mtls server"),
        )
        .await
        .expect("serve")
    })
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reverser_unix_client_interceptor_can_set_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let channel =
        Channel::connect_unix_lazy(&path)
            .expect("lazy")
            .intercept(|call: &mut Outgoing<'_>| {
                call.set_wait_for_ready(true);
                Ok(())
            });
    wait_then_complete_reverser(&channel, false, async {
        let sock = path.clone();
        tokio::spawn(async move {
            Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
                .serve_unix(sock)
                .await
                .ok();
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reverser_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reverser_err_every_shape(&channel, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reverser_tls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reverser_err_every_shape(&channel, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reverser_mtls_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reverser_err_every_shape(&channel, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reverser_unix_client_interceptor_can_opt_out_of_channel_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready()
        .intercept(|call: &mut Outgoing<'_>| {
            call.set_wait_for_ready(false);
            Ok(())
        });
    let started = Instant::now();
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_reverser_err_every_shape(&channel, Code::Unavailable),
    )
    .await
    .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_request_can_opt_out_of_channel_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    let client = GreeterClient::new(channel);
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_opt_out_every_shape(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_wait_for_ready_times_out_when_nothing_is_listening() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path).expect("lazy");
    let client = GreeterClient::new(channel);
    let timeout = Duration::from_millis(80);
    let min = Duration::from_millis(50);
    let max = Duration::from_secs(2);
    assert_deadline_in(
        client.say_hello(stamp_wait_deadline(Request::new(req("x")), timeout)),
        min,
        max,
    )
    .await;
    assert_deadline_in(
        client.server_hello(stamp_wait_deadline(Request::new(req("x")), timeout)),
        min,
        max,
    )
    .await;
    let (tx, call) = client.client_hello(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
    let (tx, call) = client.stream_hello(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = TestServiceClient::connect_lazy(addr)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_test_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client = TestServiceClient::connect_lazy(addr).expect("lazy");
    assert_test_wait_deadline(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tls_request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_test_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tls_wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    assert_test_wait_deadline(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mtls_request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_test_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mtls_wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = TestServiceClient::connect_tls_lazy(addr, client_tls).expect("lazy");
    assert_test_wait_deadline(&client).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_unix_request_can_opt_out_of_channel_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let client = TestServiceClient::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_test_opt_out(&client))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_unix_wait_for_ready_times_out_when_nothing_is_listening() {
    let (path, _guard) = unix_test_path();
    let client = TestServiceClient::connect_unix_lazy(&path).expect("lazy");
    assert_test_wait_deadline(&client).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy").wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_reverser_opt_out(&channel))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let channel = Channel::connect_lazy(addr).expect("lazy");
    assert_reverser_wait_deadline(&channel).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_tls_request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_reverser_opt_out(&channel))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_tls_wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    assert_reverser_wait_deadline(&channel).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_mtls_request_can_opt_out_of_channel_wait_for_ready() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_reverser_opt_out(&channel))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_mtls_wait_for_ready_times_out_when_nothing_is_listening() {
    let (addr, listener) = bind().await;
    drop(listener);

    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = Channel::connect_tls_lazy(addr, client_tls).expect("lazy");
    assert_reverser_wait_deadline(&channel).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_unix_request_can_opt_out_of_channel_wait_for_ready() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path)
        .expect("lazy")
        .wait_for_ready();
    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(2), assert_reverser_opt_out(&channel))
        .await
        .expect("opt-out hung");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "opt-out fail-fast took {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reverser_unix_wait_for_ready_times_out_when_nothing_is_listening() {
    let (path, _guard) = unix_test_path();
    let channel = Channel::connect_unix_lazy(&path).expect("lazy");
    assert_reverser_wait_deadline(&channel).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_fails_on_a_leftover_socket() {
    let (path, _guard) = unix_test_path();
    let leftover = tokio::net::UnixListener::bind(&path).expect("stale");
    drop(leftover);
    let err = GreeterServer::new(Echo)
        .serve_unix(&path)
        .await
        .expect_err("stale path should fail");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_unlink_replaces_a_leftover_socket() {
    let (path, _guard) = unix_test_path();
    let leftover = tokio::net::UnixListener::bind(&path).expect("stale");
    drop(leftover);
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix_unlink(sock).await.ok();
    });
    let mut last = None;
    let channel = {
        let mut found = None;
        for _ in 0..80 {
            match Channel::connect_unix(&path).await {
                Ok(channel) => {
                    found = Some(channel);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("connect after unlink: {last:?}"))
    };
    echo_every_shape(&GreeterClient::new(channel), None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_unlink_does_not_steal_a_live_socket() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let live = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    let channel = {
        let mut last = None;
        let mut found = None;
        for _ in 0..80 {
            match Channel::connect_unix(&path).await {
                Ok(channel) => {
                    found = Some(channel);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("live listener never came up: {last:?}"))
    };
    let err = GreeterServer::new(Echo)
        .serve_unix_unlink(&path)
        .await
        .expect_err("must not steal a live socket");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    echo_every_shape(&GreeterClient::new(channel), None).await;
    live.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_unix_unlink_does_not_steal_when_the_backlog_is_full() {
    let (path, _guard) = unix_test_path();
    let sock =
        socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).expect("socket");
    sock.bind(&socket2::SockAddr::unix(&path).expect("addr"))
        .expect("bind");
    sock.listen(1).expect("listen");
    let mut held = Vec::new();
    for _ in 0..8 {
        match tokio::time::timeout(
            Duration::from_millis(20),
            tokio::net::UnixStream::connect(&path),
        )
        .await
        {
            Ok(Ok(stream)) => held.push(stream),
            Ok(Err(_)) | Err(_) => break,
        }
    }
    let err = GreeterServer::new(Echo)
        .serve_unix_unlink(&path)
        .await
        .expect_err("full backlog is a live listener");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    assert!(path.exists(), "must not unlink a live inode");
    drop(held);
    drop(sock);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_connect_times_out_when_the_peer_never_speaks_http2() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let started = Instant::now();
    let err = Channel::connect_unix_with(
        &path,
        ChannelConfig::new().connect_timeout(Duration::from_millis(80)),
    )
    .await
    .expect_err("handshake should time out");
    assert_handshake_timed_out(&err, started);
    drop(listener);
}

async fn assert_server_timeout_expires(ch: Channel) {
    assert_deadline_quickly_on_every_shape(
        &GreeterClient::new(ch),
        None,
        Duration::from_millis(150),
    )
    .await;
}

async fn assert_server_timeout_caps(ch: Channel) {
    assert_deadline_quickly_on_every_shape(
        &GreeterClient::new(ch),
        Some(Duration::from_secs(5)),
        Duration::from_millis(500),
    )
    .await;
}

#[tokio::test]
async fn a_server_timeout_expires_when_the_client_sends_none() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .timeout(Duration::from_millis(50))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_server_timeout_expires(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_server_timeout_expires_when_the_client_sends_none() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .timeout(Duration::from_millis(50))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_server_timeout_expires(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_server_timeout_expires_when_the_client_sends_none() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .timeout(Duration::from_millis(50))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_server_timeout_expires(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_server_timeout_expires_when_the_client_sends_none() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .timeout(Duration::from_millis(50))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_server_timeout_expires(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_server_timeout_expires_when_the_client_sends_none() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .timeout(Duration::from_millis(50))
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_server_timeout_expires(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_server_timeout_caps_a_longer_client_deadline() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Slow))
            .timeout(Duration::from_millis(50))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_server_timeout_caps(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_server_timeout_caps_a_longer_client_deadline() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Slow))
            .timeout(Duration::from_millis(50))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_server_timeout_caps(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_server_timeout_caps_a_longer_client_deadline() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Slow))
            .timeout(Duration::from_millis(50))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_server_timeout_caps(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_server_timeout_caps_a_longer_client_deadline() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Slow))
            .timeout(Duration::from_millis(50))
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_server_timeout_caps(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_server_timeout_caps_a_longer_client_deadline() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Slow))
            .timeout(Duration::from_millis(50))
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_server_timeout_caps(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

fn refuse_connect_cfg() -> ChannelConfig {
    ChannelConfig::new().connect_timeout(Duration::from_millis(300))
}

async fn assert_cap_refuses_then_echo(
    first: Channel,
    second: Result<Channel, Status>,
    reconnect: impl std::future::Future<Output = Channel>,
) {
    let err = second.expect_err("second connection should be refused");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(first);
    echo_every_shape(&GreeterClient::new(reconnect.await), None).await;
}

#[tokio::test]
async fn extra_connections_are_refused_when_the_cap_is_hit() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_listener(listener)
            .await
            .ok();
    });
    let first = channel(addr).await;
    assert_cap_refuses_then_echo(
        first,
        Channel::connect_with(addr, refuse_connect_cfg()).await,
        channel(addr),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_extra_connections_are_refused_when_the_cap_is_hit() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let first = tls_channel_with(addr, client_tls.clone()).await;
    assert_cap_refuses_then_echo(
        first,
        Channel::connect_tls_with(addr, refuse_connect_cfg(), client_tls.clone()).await,
        tls_channel_with(addr, client_tls),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_extra_connections_are_refused_when_the_cap_is_hit() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let first = tls_channel_with(addr, client_tls.clone()).await;
    assert_cap_refuses_then_echo(
        first,
        Channel::connect_tls_with(addr, refuse_connect_cfg(), client_tls.clone()).await,
        tls_channel_with(addr, client_tls),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_extra_connections_are_refused_when_the_cap_is_hit() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_unix(sock)
            .await
            .ok();
    });
    let first = unix_channel(&path).await;
    assert_cap_refuses_then_echo(
        first,
        Channel::connect_unix_with(&path, refuse_connect_cfg()).await,
        unix_channel(&path),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tcp_keepalive_still_serves() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .tcp_keepalive(Duration::from_secs(15))
            .serve_listener(listener)
            .await
            .ok();
    });
    let cfg = ChannelConfig::new().tcp_keepalive(Duration::from_secs(15));
    let mut last = None;
    let connected = {
        let mut found = None;
        for _ in 0..80 {
            match Channel::connect_with(addr, cfg).await {
                Ok(channel) => {
                    found = Some(channel);
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        found.unwrap_or_else(|| panic!("connect with tcp keepalive: {last:?}"))
    };
    echo_every_shape(&GreeterClient::new(connected), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_tcp_keepalive_still_serves() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .tcp_keepalive(Duration::from_secs(15))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = tls_channel_cfg(
        addr,
        client_tls,
        ChannelConfig::new().tcp_keepalive(Duration::from_secs(15)),
    )
    .await;
    echo_every_shape(&GreeterClient::new(channel), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_tcp_keepalive_still_serves() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .tcp_keepalive(Duration::from_secs(15))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = tls_channel_cfg(
        addr,
        client_tls,
        ChannelConfig::new().tcp_keepalive(Duration::from_secs(15)),
    )
    .await;
    echo_every_shape(&GreeterClient::new(channel), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_keepalive_still_serves() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .keep_alive_interval(Duration::from_millis(50))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let channel = tls_channel_cfg(
        addr,
        client_tls,
        ChannelConfig::new().keep_alive_interval(Duration::from_millis(50)),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    echo_every_shape(&GreeterClient::new(channel), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_keepalive_still_serves() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .keep_alive_interval(Duration::from_millis(50))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let channel = tls_channel_cfg(
        addr,
        client_tls,
        ChannelConfig::new().keep_alive_interval(Duration::from_millis(50)),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    echo_every_shape(&GreeterClient::new(channel), None).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_keepalive_still_serves() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .keep_alive_interval(Duration::from_millis(50))
            .serve_unix(sock)
            .await
            .ok();
    });
    let channel = unix_channel_with(
        &path,
        ChannelConfig::new().keep_alive_interval(Duration::from_millis(50)),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    echo_every_shape(&GreeterClient::new(channel), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_keepalive_still_serves() {
    let (client_io, server_io) = duplex_pair();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .keep_alive_interval(Duration::from_millis(50))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io_with(
        client_io,
        "localhost",
        ChannelConfig::new().keep_alive_interval(Duration::from_millis(50)),
    )
    .await
    .expect("from_io");
    tokio::time::sleep(Duration::from_millis(120)).await;
    echo_every_shape(&GreeterClient::new(channel), None).await;
    task.abort();
}

fn duplex_pair() -> (tokio::io::DuplexStream, tokio::io::DuplexStream) {
    tokio::io::duplex(1024 * 1024)
}

struct OneIncoming<IO> {
    io: Option<IO>,
    remote: Option<SocketAddr>,
}

impl<IO> Incoming for OneIncoming<IO>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    type Io = IO;

    fn accept(
        &mut self,
    ) -> impl std::future::Future<Output = pbrs_grpc::IncomingAccept<IO>> + Send {
        let io = self.io.take();
        let remote = self.remote;
        async move {
            match io {
                Some(io) => Some(Ok((io, remote))),
                None => std::future::pending().await,
            }
        }
    }
}

#[tokio::test]
async fn from_io_round_trips_without_tcp() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert!(format!("{channel:?}").contains("once"), "{channel:?}");
    echo_every_shape(&GreeterClient::new(channel), None).await;
    server.abort();
}

#[tokio::test]
async fn from_io_send_compressed_gzips_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .send_compressed()
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io")
        .send_compressed();
    gzip_every_shape(&GreeterClient::new(channel)).await;
    server.abort();
}

#[tokio::test]
async fn from_io_interceptor_rejects_with_typed_status() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|_rpc: &mut Rpc| Err(interceptor_blocked()))
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_greeter_blocked_every_shape(&client).await;
    server.abort();
}

#[tokio::test]
async fn from_io_client_interceptor_rejects_with_typed_status() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(|_: &mut Outgoing<'_>| Err(interceptor_blocked()));
    assert_greeter_blocked_every_shape(&client).await;
    server.abort();
}

#[tokio::test]
async fn from_io_client_interceptor_sees_every_shape_context() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_context)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(stamp_outgoing_context);
    echo_every_shape(&client, None).await;
    server.abort();
}

#[tokio::test]
async fn tls_client_interceptor_sees_every_shape_context() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_context)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await).intercept(stamp_outgoing_context);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_client_interceptor_sees_every_shape_context() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_stamped_context)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(stamp_outgoing_context);
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn tls_handlers_return_typed_status_on_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(FailGreeter)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_greeter_blocked_every_shape(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn mtls_handlers_return_typed_status_on_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(FailGreeter)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_greeter_blocked_every_shape(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_handlers_return_typed_status_on_every_shape() {
    let (path, _guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let task = tokio::spawn(async move {
        GreeterServer::new(FailGreeter)
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    assert_greeter_blocked_every_shape(&GreeterClient::new(
        Channel::connect_unix(&path).await.expect("connect"),
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_handlers_return_typed_status_on_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(FailGreeter)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_greeter_blocked_every_shape(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn tls_typed_google_rpc_status_after_a_streamed_message() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(TypedAfterHeaders)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_typed_status_after_streamed_message(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn mtls_typed_google_rpc_status_after_a_streamed_message() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(TypedAfterHeaders)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_typed_status_after_streamed_message(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_typed_google_rpc_status_after_a_streamed_message() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(TypedAfterHeaders)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_typed_status_after_streamed_message(&GreeterClient::new(unix_channel(&path).await))
        .await;
    task.abort();
}

#[tokio::test]
async fn from_io_typed_google_rpc_status_after_a_streamed_message() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(TypedAfterHeaders)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_typed_status_after_streamed_message(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

#[tokio::test]
async fn from_io_authority_is_visible_to_interceptors() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.authority() != Some("my-svc") {
                    return Err(Status::internal(format!(
                        "server authority {:?}",
                        rpc.authority()
                    )));
                }
                if rpc.remote_addr().is_some() || rpc.local_addr().is_some() {
                    return Err(Status::internal("from_io must not invent TCP addrs"));
                }
                if rpc.peer_identity().is_some() {
                    return Err(Status::internal("from_io must not invent a TLS identity"));
                }
                if rpc.peer_cred().is_some() {
                    return Err(Status::internal("from_io must not invent unix credentials"));
                }
                if rpc.scheme() != Some("http") {
                    return Err(Status::internal(format!(
                        "from_io scheme {:?}",
                        rpc.scheme()
                    )));
                }
                Ok(())
            })
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "my-svc")
        .await
        .expect("from_io");
    let client = GreeterClient::new(channel).intercept(|call: &mut Outgoing<'_>| {
        if call.authority() != "my-svc" {
            return Err(Status::internal(format!(
                "client authority {}",
                call.authority()
            )));
        }
        Ok(())
    });
    echo_every_shape(&client, None).await;
    server.abort();
}

#[tokio::test]
async fn from_io_https_scheme_is_visible_to_interceptors() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.scheme() != Some("https") {
                    return Err(Status::internal(format!(
                        "from_io https scheme {:?}",
                        rpc.scheme()
                    )));
                }
                if rpc.metadata().get("x-scheme") != Some("https") {
                    return Err(Status::internal(format!(
                        "x-scheme {:?}",
                        rpc.metadata().get("x-scheme")
                    )));
                }
                Ok(())
            })
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_eq!(channel.scheme(), "http");
    let client = GreeterClient::new(channel).https_scheme();
    assert_eq!(client.scheme(), "https");
    assert_eq!(client.authority(), "localhost");
    assert_eq!(client.channel().scheme(), "https");
    assert!(
        client.grpc_user_agent().starts_with("pbrs-grpc/"),
        "{}",
        client.grpc_user_agent()
    );
    let client = client.intercept(|call: &mut Outgoing<'_>| {
        if call.scheme() != "https" {
            return Err(Status::internal(format!("scheme {}", call.scheme())));
        }
        let scheme = call.scheme();
        call.metadata_mut().set("x-scheme", scheme)?;
        Ok(())
    });
    echo_every_shape(&client, None).await;
    server.abort();
}

struct SeesFromIo {
    authority: &'static str,
    scheme: &'static str,
}

impl Greeter for SeesFromIo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let msg = sees_from_io(request, self.authority, self.scheme)?;
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let _ = sees_from_io(request, self.authority, self.scheme)?;
        Ok(Response::new(common::reply("ada")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let msg = sees_from_io(request, self.authority, self.scheme)?;
        Ok(echo_named_stream(common::name_of_request(&msg)))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let _ = sees_from_io(request, self.authority, self.scheme)?;
        Ok(echo_named_stream("ada".into()))
    }
}

#[tokio::test]
async fn a_generated_handler_sees_from_io_identity() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesFromIo {
            authority: "my-svc",
            scheme: "http",
        })
        .serve_connection(server_io)
        .await
        .ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "my-svc")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_generated_handler_sees_from_io_https_scheme() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesFromIo {
            authority: "localhost",
            scheme: "https",
        })
        .serve_connection(server_io)
        .await
        .ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        )
        .https_scheme(),
        None,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn https_scheme_is_a_noop_on_tcp() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.scheme() != Some("http") {
                    return Err(Status::internal(format!("tcp scheme {:?}", rpc.scheme())));
                }
                Ok(())
            })
            .serve_listener(listener)
            .await
            .ok();
    });
    let channel = channel(addr).await.https_scheme();
    assert_eq!(channel.scheme(), "http");
    let want_authority = channel.authority().to_owned();
    let client = GreeterClient::new(channel);
    assert_eq!(client.scheme(), "http");
    assert_eq!(client.authority(), want_authority);
    assert!(
        client.grpc_user_agent().starts_with("pbrs-grpc/"),
        "{}",
        client.grpc_user_agent()
    );
    let client = client.intercept(|call: &mut Outgoing<'_>| {
        if call.scheme() != "http" {
            return Err(Status::internal(format!("scheme {}", call.scheme())));
        }
        Ok(())
    });
    echo_every_shape(&client, None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_idle_close_cannot_redial() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io_with(
        client_io,
        "localhost",
        ChannelConfig::new().max_connection_idle(Duration::from_millis(80)),
    )
    .await
    .expect("from_io");
    let client = GreeterClient::new(channel);
    echo_every_shape(&client, None).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("idle close hung");
    server.abort();
}

#[tokio::test]
async fn from_io_cannot_redial() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    let client = GreeterClient::new(channel);
    echo_every_shape(&client, None).await;
    server.abort();
    let err = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match client.say_hello(Request::new(req("b"))).await {
                Ok(_) => tokio::time::sleep(Duration::from_millis(5)).await,
                Err(e) => return e,
            }
        }
    })
    .await
    .expect("should become unavailable");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    tokio::time::timeout(
        Duration::from_secs(2),
        assert_err_on_every_shape(&client, Code::Unavailable),
    )
    .await
    .expect("once channel cannot redial");
}

#[tokio::test]
async fn serve_with_incoming_accepts_a_duplex() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_incoming(OneIncoming {
                io: Some(server_io),
                remote: None,
            })
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    echo_every_shape(&GreeterClient::new(channel), None).await;
    server.abort();
}

#[tokio::test]
async fn incoming_default_peer_copies_the_accept_addr() {
    let remote: SocketAddr = "203.0.113.1:7".parse().expect("remote");
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(move |rpc: &mut Rpc| {
                if rpc.remote_addr() != Some(remote) {
                    return Err(Status::internal(format!(
                        "remote {:?} want {remote}",
                        rpc.remote_addr()
                    )));
                }
                if rpc.local_addr().is_some() {
                    return Err(Status::internal(
                        "default Incoming must not invent local_addr",
                    ));
                }
                if rpc.peer_identity().is_some() {
                    return Err(Status::internal(
                        "default Incoming must not invent identity",
                    ));
                }
                if rpc.peer_cred().is_some() {
                    return Err(Status::internal(
                        "default Incoming must not invent peer_cred",
                    ));
                }
                if rpc.scheme() != Some("http") {
                    return Err(Status::internal(format!(
                        "default Incoming scheme {:?}",
                        rpc.scheme()
                    )));
                }
                Ok(())
            })
            .serve_with_incoming(OneIncoming {
                io: Some(server_io),
                remote: Some(remote),
            })
            .await
            .ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

struct StampedIncoming<IO> {
    io: Option<IO>,
    accept_remote: SocketAddr,
}

impl<IO> Incoming for StampedIncoming<IO>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    type Io = IO;

    fn accept(
        &mut self,
    ) -> impl std::future::Future<Output = pbrs_grpc::IncomingAccept<IO>> + Send {
        let io = self.io.take();
        let remote = self.accept_remote;
        async move {
            match io {
                Some(io) => Some(Ok((io, Some(remote)))),
                None => std::future::pending().await,
            }
        }
    }

    fn peer(&self, io: &Self::Io, remote: Option<SocketAddr>) -> ConnectionInfo {
        let _ = (self, io, remote);
        ConnectionInfo::new()
            .with_remote_addr("192.0.2.1:8".parse().expect("remote"))
            .with_local_addr("127.0.0.1:9".parse().expect("local"))
            .with_peer_identity(PeerIdentity::from_der_certs([b"leaf"]).expect("leaf"))
            .with_peer_cred(PeerCred::new(42, 43, Some(44)))
            .with_scheme("https")
    }
}

#[tokio::test]
async fn incoming_peer_stamps_connection_facts() {
    struct SeesIncoming;

    impl Greeter for SeesIncoming {
        async fn say_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            let msg = sees_incoming(request)?;
            Ok(Response::new(common::reply(common::name_of_request(&msg))))
        }

        async fn client_hello(
            &self,
            request: Request<pbrs_grpc::Streaming<HelloRequest>>,
        ) -> Result<Response<HelloReply>, Status> {
            let _ = sees_incoming(request)?;
            Ok(Response::new(common::reply("ada")))
        }

        async fn server_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
            let msg = sees_incoming(request)?;
            Ok(echo_named_stream(common::name_of_request(&msg)))
        }

        async fn stream_hello(
            &self,
            request: Request<pbrs_grpc::Streaming<HelloRequest>>,
        ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
            let _ = sees_incoming(request)?;
            Ok(echo_named_stream("ada".into()))
        }
    }

    let accept_remote: SocketAddr = "203.0.113.1:7".parse().expect("accept");
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesIncoming)
            .intercept(|rpc: &mut Rpc| {
                if rpc.remote_addr() != Some("192.0.2.1:8".parse().expect("remote")) {
                    return Err(Status::internal(format!(
                        "interceptor remote {:?}",
                        rpc.remote_addr()
                    )));
                }
                if rpc.local_addr() != Some("127.0.0.1:9".parse().expect("local")) {
                    return Err(Status::internal(format!(
                        "interceptor local {:?}",
                        rpc.local_addr()
                    )));
                }
                if rpc.peer_identity().and_then(|id| id.leaf()) != Some(b"leaf") {
                    return Err(Status::internal("interceptor missing identity"));
                }
                if rpc.peer_cred() != Some(PeerCred::new(42, 43, Some(44))) {
                    return Err(Status::internal(format!(
                        "interceptor cred {:?}",
                        rpc.peer_cred()
                    )));
                }
                if rpc.scheme() != Some("https") {
                    return Err(Status::internal(format!(
                        "interceptor scheme {:?}",
                        rpc.scheme()
                    )));
                }
                Ok(())
            })
            .serve_with_incoming(StampedIncoming {
                io: Some(server_io),
                accept_remote,
            })
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    echo_every_shape(&client, None).await;
    server.abort();
}

/// Sleeps long enough that a cancelled caller would otherwise leave it running.
struct Hang {
    started: Arc<AtomicUsize>,
    finished: Arc<AtomicUsize>,
}

impl Hang {
    async fn hang(&self) {
        self.started.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.finished.fetch_add(1, Ordering::Relaxed);
    }
}

impl pbrs_grpc::Greeter for Hang {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.hang().await;
        let mut reply = HelloReply::new();
        reply.set_message(request.get_ref().name());
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        self.hang().await;
        Err(Status::internal("handler should have been dropped"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        self.hang().await;
        Err(Status::internal("handler should have been dropped"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        self.hang().await;
        Err(Status::internal("handler should have been dropped"))
    }
}

async fn assert_cancel_after_begin_is_cancelled_not_ok(client: &GreeterClient) {
    let (tx, call) = client.client_hello(Request::new(()));
    let handle = call.handle();
    handle.cancel();
    // Hold `tx` until the call settles. Dropping it is a half-close, which
    // can complete as OK before the RST is observed.
    let err = call.await.expect_err("cancel_after_begin");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    drop(tx);
}

#[tokio::test]
async fn cancel_after_begin_is_cancelled_not_ok() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    assert_cancel_after_begin_is_cancelled_not_ok(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn tls_cancel_after_begin_is_cancelled_not_ok() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_cancel_after_begin_is_cancelled_not_ok(&GreeterClient::new(tls_channel(addr).await))
        .await;
    task.abort();
}

#[tokio::test]
async fn mtls_cancel_after_begin_is_cancelled_not_ok() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_cancel_after_begin_is_cancelled_not_ok(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_cancel_after_begin_is_cancelled_not_ok() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    assert_cancel_after_begin_is_cancelled_not_ok(&GreeterClient::new(unix_channel(&path).await))
        .await;
    task.abort();
}

#[tokio::test]
async fn from_io_cancel_after_begin_is_cancelled_not_ok() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_cancel_after_begin_is_cancelled_not_ok(&GreeterClient::new(channel)).await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_reset_drops_the_handler() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = Hang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    assert_reset_drops_handler(
        client.say_hello(Request::new(req("ada"))),
        &started,
        &finished,
    )
    .await;
    assert_reset_drops_handler(
        client.server_hello(Request::new(req("ada"))),
        &started,
        &finished,
    )
    .await;
    let (tx, call) = client.client_hello(Request::new(()));
    assert_reset_drops_handler(call, &started, &finished).await;
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_reset_drops_handler(call, &started, &finished).await;
    drop(tx);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_call_drops_the_handler() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = Hang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    assert_drop_call_drops_handler(
        client.say_hello(Request::new(req("ada"))),
        &started,
        &finished,
    )
    .await;
    assert_drop_call_drops_handler(
        client.server_hello(Request::new(req("ada"))),
        &started,
        &finished,
    )
    .await;
    let (tx, call) = client.client_hello(Request::new(()));
    assert_drop_call_drops_handler(call, &started, &finished).await;
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_drop_call_drops_handler(call, &started, &finished).await;
    drop(tx);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_hanging_handler_drops_on_call_drop_and_reset() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let hang = Hang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_hang_drop_and_reset_on_every_shape(&client, &started, &finished).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_hanging_handler_drops_on_call_drop_and_reset() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let hang = Hang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_hang_drop_and_reset_on_every_shape(&client, &started, &finished).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_hanging_handler_drops_on_call_drop_and_reset() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let hang = Hang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_hang_drop_and_reset_on_every_shape(&client, &started, &finished).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_hanging_handler_drops_on_call_drop_and_reset() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let hang = Hang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_hang_drop_and_reset_on_every_shape(&client, &started, &finished).await;
    server.abort();
}

async fn assert_drop_call_drops_handler<T>(
    mut call: Call<T>,
    started: &AtomicUsize,
    finished: &AtomicUsize,
) {
    started.store(0, Ordering::Relaxed);
    tokio::select! {
        biased;
        _ = &mut call => panic!("Hang returned before drop"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    drop(call);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        finished.load(Ordering::Relaxed),
        0,
        "dropping the Call should RST the stream and drop the handler"
    );
}

async fn assert_reset_drops_handler<T>(
    mut call: Call<T>,
    started: &AtomicUsize,
    finished: &AtomicUsize,
) {
    started.store(0, Ordering::Relaxed);
    let handle = call.handle();
    tokio::select! {
        biased;
        _ = &mut call => panic!("Hang returned before cancel"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    handle.cancel();
    let err = match call.await {
        Ok(_) => panic!("cancelled Call resolved Ok"),
        Err(status) => status,
    };
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        finished.load(Ordering::Relaxed),
        0,
        "CallHandle cancel should RST the stream and drop the handler"
    );
}

async fn assert_hang_drop_and_reset_on_every_shape(
    client: &GreeterClient,
    started: &AtomicUsize,
    finished: &AtomicUsize,
) {
    assert_drop_call_drops_handler(
        client.say_hello(Request::new(req("ada"))),
        started,
        finished,
    )
    .await;
    assert_drop_call_drops_handler(
        client.server_hello(Request::new(req("ada"))),
        started,
        finished,
    )
    .await;
    let (tx, call) = client.client_hello(Request::new(()));
    assert_drop_call_drops_handler(call, started, finished).await;
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_drop_call_drops_handler(call, started, finished).await;
    drop(tx);

    assert_reset_drops_handler(
        client.say_hello(Request::new(req("ada"))),
        started,
        finished,
    )
    .await;
    assert_reset_drops_handler(
        client.server_hello(Request::new(req("ada"))),
        started,
        finished,
    )
    .await;
    let (tx, call) = client.client_hello(Request::new(()));
    assert_reset_drops_handler(call, started, finished).await;
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_reset_drops_handler(call, started, finished).await;
    drop(tx);
}

/// Spawns a child that waits on [`Request::cancelled`], then hangs.
struct SpawnHang {
    started: Arc<AtomicUsize>,
    finished: Arc<AtomicUsize>,
    child_done: Arc<AtomicUsize>,
}

impl SpawnHang {
    fn spawn_child<T>(&self, request: &Request<T>) {
        self.started.fetch_add(1, Ordering::Relaxed);
        let child_done = Arc::clone(&self.child_done);
        let cancelled = request.cancelled();
        drop(tokio::spawn(async move {
            cancelled.await;
            child_done.fetch_add(1, Ordering::Relaxed);
        }));
    }

    fn start<T>(&self, request: &Request<T>) -> Result<(), Status> {
        if request.is_cancelled() {
            return Err(Status::internal("cancelled before the handler ran"));
        }
        self.spawn_child(request);
        Ok(())
    }

    async fn hang_until_dropped(&self) -> Status {
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.finished.fetch_add(1, Ordering::Relaxed);
        Status::internal("handler should have been dropped")
    }
}

impl pbrs_grpc::Greeter for SpawnHang {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.start(&request)?;
        Err(self.hang_until_dropped().await)
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        self.start(&request)?;
        Err(self.hang_until_dropped().await)
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        self.start(&request)?;
        Err(self.hang_until_dropped().await)
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        self.start(&request)?;
        Err(self.hang_until_dropped().await)
    }
}

struct SpawnOk {
    child_done: Arc<AtomicUsize>,
}

impl SpawnOk {
    fn spawn_child<T>(&self, request: &Request<T>) {
        let child_done = Arc::clone(&self.child_done);
        let cancelled = request.cancelled();
        drop(tokio::spawn(async move {
            cancelled.await;
            child_done.fetch_add(1, Ordering::Relaxed);
        }));
    }
}

impl pbrs_grpc::Greeter for SpawnOk {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.spawn_child(&request);
        Ok(Response::new(common::reply(common::name_of_request(
            request.get_ref(),
        ))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        self.spawn_child(&request);
        Ok(Response::new(common::reply("ok")))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        self.spawn_child(&request);
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        self.spawn_child(&request);
        Ok(Response::new(pbrs_grpc::Streaming::empty()))
    }
}

async fn wait_flag(flag: &AtomicUsize) {
    for _ in 0..80 {
        if flag.load(Ordering::Relaxed) >= 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("spawned work never observed Request::cancelled");
}

async fn assert_deadline_dropped_spawned<T>(
    call: Call<T>,
    started: &AtomicUsize,
    finished: &AtomicUsize,
    child_done: &AtomicUsize,
) {
    started.store(0, Ordering::Relaxed);
    child_done.store(0, Ordering::Relaxed);
    let err = match call.await {
        Ok(_) => panic!("expected deadline"),
        Err(status) => status,
    };
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    wait_flag(child_done).await;
    assert_eq!(
        finished.load(Ordering::Relaxed),
        0,
        "handler should have been dropped, not run to completion"
    );
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
}

async fn assert_deadline_on_every_shape(
    client: &GreeterClient,
    started: &AtomicUsize,
    finished: &AtomicUsize,
    child_done: &AtomicUsize,
) {
    assert_deadline_dropped_spawned(
        client.say_hello(Request::new(req("ada"))),
        started,
        finished,
        child_done,
    )
    .await;
    assert_deadline_dropped_spawned(
        client.server_hello(Request::new(req("ada"))),
        started,
        finished,
        child_done,
    )
    .await;
    let (tx, call) = client.client_hello(Request::new(()));
    assert_deadline_dropped_spawned(call, started, finished, child_done).await;
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_deadline_dropped_spawned(call, started, finished, child_done).await;
    drop(tx);
}

async fn assert_spawned_cancel_dropped<T: std::fmt::Debug>(
    mut call: Call<T>,
    started: &AtomicUsize,
    finished: &AtomicUsize,
    child_done: &AtomicUsize,
) {
    started.store(0, Ordering::Relaxed);
    finished.store(0, Ordering::Relaxed);
    child_done.store(0, Ordering::Relaxed);
    tokio::select! {
        biased;
        result = &mut call => panic!("SpawnHang returned before cancel: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    call.handle().cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(child_done).await;
    assert_eq!(
        finished.load(Ordering::Relaxed),
        0,
        "handler should have been dropped"
    );
}

async fn assert_spawned_cancel_on_every_shape(
    client: &GreeterClient,
    started: &AtomicUsize,
    finished: &AtomicUsize,
    child_done: &AtomicUsize,
) {
    assert_spawned_cancel_dropped(
        client.say_hello(Request::new(req("ada"))),
        started,
        finished,
        child_done,
    )
    .await;
    assert_spawned_cancel_dropped(
        client.server_hello(Request::new(req("ada"))),
        started,
        finished,
        child_done,
    )
    .await;
    let (tx, call) = client.client_hello(Request::new(()));
    assert_spawned_cancel_dropped(call, started, finished, child_done).await;
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    assert_spawned_cancel_dropped(call, started, finished, child_done).await;
    drop(tx);
}

fn stamp_timeout<T>(mut request: Request<T>, timeout: Option<Duration>) -> Request<T> {
    if let Some(timeout) = timeout {
        request.set_timeout(timeout);
    }
    request
}

fn stamp_wait_ready<T>(
    mut request: Request<T>,
    wait_on_request: bool,
    timeout: Option<Duration>,
) -> Request<T> {
    if wait_on_request {
        request.set_wait_for_ready(true);
    }
    stamp_timeout(request, timeout)
}

fn stamp_opt_out<T>(mut request: Request<T>) -> Request<T> {
    request.set_wait_for_ready(false);
    request.set_timeout(Duration::from_secs(5));
    request
}

fn stamp_wait_deadline<T>(mut request: Request<T>, timeout: Duration) -> Request<T> {
    request.set_wait_for_ready(true);
    request.set_timeout(timeout);
    request
}

async fn echo_every_shape(client: &GreeterClient, timeout: Option<Duration>) {
    let reply = client
        .say_hello(stamp_timeout(Request::new(req("ada")), timeout))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let mut stream = client
        .server_hello(stamp_timeout(Request::new(req("ada")), timeout))
        .await
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert!(stream.message().await.expect("end").is_none());

    let (tx, call) = client.client_hello(stamp_timeout(Request::new(()), timeout));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let (tx, call) = client.stream_hello(stamp_timeout(Request::new(()), timeout));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert!(inbound.message().await.expect("end").is_none());
}

async fn echo_test_every_shape(client: &TestServiceClient) {
    client
        .empty_call(Request::new(Empty::new()))
        .await
        .expect("unary");

    let mut stream = client
        .streaming_output_call(Request::new(StreamingOutputCallRequest::new()))
        .await
        .expect("server-stream")
        .into_inner();
    assert!(
        stream.message().await.expect("end").is_none(),
        "empty StreamingOutputCall plan must end"
    );

    let (tx, call) = client.streaming_input_call(Request::new(()));
    tx.close();
    call.await.expect("client-stream");

    let (tx, call) = client.full_duplex_call(Request::new(()));
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    assert!(
        inbound.message().await.expect("end").is_none(),
        "empty FullDuplexCall must end"
    );
}

async fn gzip_test_every_shape(client: &TestServiceClient) {
    let empty = client
        .empty_call(Request::new(Empty::new()))
        .await
        .expect("unary");
    assert!(empty.compressed(), "EmptyCall gzip");
    assert_eq!(empty.encoding(), Some("gzip"), "{:?}", empty.encoding());

    let reply = client
        .streaming_output_call(Request::new(StreamingOutputCallRequest::new()))
        .await
        .expect("server-stream");
    assert_eq!(
        reply.encoding(),
        Some("gzip"),
        "StreamingOutputCall encoding"
    );
    let mut stream = reply.into_inner();
    assert!(
        stream.message().await.expect("end").is_none(),
        "empty StreamingOutputCall plan must end"
    );

    let (tx, call) = client.streaming_input_call(Request::new(()));
    assert!(tx.compress(), "StreamingInputCall StreamSender must gzip");
    tx.close();
    let summary = call.await.expect("client-stream");
    assert!(summary.compressed(), "StreamingInputCall reply gzip");
    assert_eq!(summary.encoding(), Some("gzip"));

    let (tx, call) = client.full_duplex_call(Request::new(()));
    assert!(tx.compress(), "FullDuplexCall StreamSender must gzip");
    let reply = call.await.expect("bidi");
    assert_eq!(reply.encoding(), Some("gzip"), "FullDuplexCall encoding");
    tx.close();
    let mut inbound = reply.into_inner();
    assert!(
        inbound.message().await.expect("end").is_none(),
        "empty FullDuplexCall must end"
    );
}

async fn wait_then_complete_reverser(
    channel: &Channel,
    wait_on_request: bool,
    start: impl std::future::Future,
) {
    let timeout = Some(Duration::from_secs(5));
    let mut unary = channel.unary::<HelloRequest, HelloReply>(
        "/demo.Reverser/Reverse",
        stamp_wait_ready(Request::new(req("stressed")), wait_on_request, timeout),
    );
    let mut server_stream = channel.server_streaming::<HelloRequest, HelloReply>(
        "/demo.Reverser/Server",
        stamp_wait_ready(Request::new(req("stressed")), wait_on_request, timeout),
    );
    let (tx_c, mut client_stream) = channel.client_streaming::<HelloRequest, HelloReply>(
        "/demo.Reverser/Client",
        stamp_wait_ready(Request::new(()), wait_on_request, timeout),
    );
    let (tx_b, mut bidi) = channel.bidi::<HelloRequest, HelloReply>(
        "/demo.Reverser/Bidi",
        stamp_wait_ready(Request::new(()), wait_on_request, timeout),
    );

    tokio::select! {
        biased;
        result = &mut unary => panic!("unary finished before the server listened: {result:?}"),
        result = &mut server_stream => panic!("server-stream finished before the server listened: {result:?}"),
        result = &mut client_stream => panic!("client-stream finished before the server listened: {result:?}"),
        result = &mut bidi => panic!("bidi finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let _guard = start.await;

    let reply = tokio::time::timeout(Duration::from_secs(2), unary)
        .await
        .expect("unary hung after listen")
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "desserts");

    let mut stream = tokio::time::timeout(Duration::from_secs(2), server_stream)
        .await
        .expect("server-stream hung after listen")
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "desserts");

    tx_c.send(req("stressed")).await.expect("send");
    tx_c.close();
    let reply = tokio::time::timeout(Duration::from_secs(2), client_stream)
        .await
        .expect("client-stream hung after listen")
        .expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "desserts");

    tx_b.send(req("stressed")).await.expect("send");
    tx_b.close();
    let mut inbound = tokio::time::timeout(Duration::from_secs(2), bidi)
        .await
        .expect("bidi hung after listen")
        .expect("bidi")
        .into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "desserts");
}

async fn echo_reverser_every_shape(channel: &Channel) {
    let reply: HelloReply = channel
        .unary("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect("unary")
        .into_inner();
    assert_eq!(name_of(&reply), "desserts");

    let mut stream = channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            Request::new(req("stressed")),
        )
        .await
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "desserts");
    assert!(stream.message().await.expect("end").is_none());

    let (tx, call) = channel
        .client_streaming::<HelloRequest, HelloReply>("/demo.Reverser/Client", Request::new(()));
    tx.send(req("stressed")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "desserts");

    let (tx, call) =
        channel.bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", Request::new(()));
    tx.send(req("stressed")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "desserts");
    assert!(inbound.message().await.expect("end").is_none());
}

async fn gzip_reverser_every_shape(channel: &Channel) {
    let reply = channel
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect("unary");
    assert!(reply.compressed(), "Reverse gzip");
    assert_eq!(reply.encoding(), Some("gzip"));
    assert_eq!(name_of(reply.get_ref()), "desserts");

    let reply = channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            Request::new(req("stressed")),
        )
        .await
        .expect("server-stream");
    assert_eq!(reply.encoding(), Some("gzip"), "Server encoding");
    let mut stream = reply.into_inner();
    let framed = stream.next_framed().await.expect("frame").expect("message");
    assert!(framed.compressed, "Server frames gzip");
    assert_eq!(name_of(&framed.message), "desserts");

    let (tx, call) = channel
        .client_streaming::<HelloRequest, HelloReply>("/demo.Reverser/Client", Request::new(()));
    assert!(tx.compress(), "Client StreamSender must gzip");
    tx.send(req("stressed")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert!(reply.compressed(), "Client reply gzip");
    assert_eq!(reply.encoding(), Some("gzip"));
    assert_eq!(name_of(reply.get_ref()), "desserts");

    let (tx, call) =
        channel.bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", Request::new(()));
    assert!(tx.compress(), "Bidi StreamSender must gzip");
    let reply = call.await.expect("bidi");
    assert_eq!(reply.encoding(), Some("gzip"), "Bidi encoding");
    let mut inbound = reply.into_inner();
    tx.send(req("stressed")).await.expect("send");
    let framed = inbound
        .next_framed()
        .await
        .expect("frame")
        .expect("message");
    assert!(framed.compressed, "Bidi frames gzip");
    assert_eq!(name_of(&framed.message), "desserts");
    tx.close();
    while inbound.message().await.expect("drain").is_some() {}
}

fn fat_test_payload() -> Payload {
    let mut p = Payload::new();
    p.set_body(vec![0u8; 64]);
    p
}

fn greeter_plus_test_with_decode_cap() -> Router {
    GreeterServer::new(Echo)
        .max_decoding_message_size(16)
        .add_service(TestServiceServer::new(InteropTestService))
}

async fn assert_greeter_oversize_every_shape(client: &GreeterClient) {
    let oversize = req(&"x".repeat(64));
    let err = client
        .say_hello(Request::new(oversize.clone()))
        .await
        .expect_err("unary over the server cap");
    assert_eq!(err.code(), Code::ResourceExhausted);
    match client.server_hello(Request::new(oversize.clone())).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("server-stream over the server cap must fail"),
        },
    }
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(oversize.clone()).await.expect("send");
    tx.close();
    let err = call.await.expect_err("client-stream over the server cap");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(oversize).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("bidi over the server cap must fail"),
        },
    }
}

async fn assert_add_service_decode_cap(greeter: &GreeterClient, test: &TestServiceClient) {
    echo_every_shape(greeter, None).await;
    echo_test_every_shape(test).await;
    assert_greeter_oversize_every_shape(greeter).await;
    assert_test_oversize_every_shape(test).await;
}

fn greeter_plus_test_with_encode_cap() -> Router {
    GreeterServer::new(Echo)
        .max_encoding_message_size(16)
        .add_service(TestServiceServer::new(InteropTestService))
}

fn fat_test_response() -> SimpleRequest {
    let mut r = SimpleRequest::new();
    r.set_response_size(64);
    r
}

fn fat_test_output_plan() -> StreamingOutputCallRequest {
    let mut r = StreamingOutputCallRequest::new();
    let mut p = ResponseParameters::new();
    p.set_size(64);
    r.response_parameters_mut().push(p);
    r
}

async fn assert_test_oversize_encode_every_shape(client: &TestServiceClient) {
    // EmptyCall and StreamingInputCall responses stay under the 16-byte cap.
    let err = client
        .unary_call(Request::new(fat_test_response()))
        .await
        .expect_err("unary encode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");

    match client
        .streaming_output_call(Request::new(fat_test_output_plan()))
        .await
    {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("server-stream over the encode cap must fail"),
        },
    }

    let (tx, call) = client.full_duplex_call(Request::new(()));
    tx.send(fat_test_output_plan()).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("bidi over the encode cap must fail"),
        },
    }
}

async fn assert_add_service_encode_cap(greeter: &GreeterClient, test: &TestServiceClient) {
    echo_every_shape(greeter, None).await;
    echo_test_every_shape(test).await;
    assert_greeter_oversize_every_shape(greeter).await;
    assert_test_oversize_encode_every_shape(test).await;
}

async fn assert_test_oversize_every_shape(client: &TestServiceClient) {
    // EmptyCall is smaller than the 16-byte cap; UnaryCall is the payload-bearing unary.
    let mut unary = SimpleRequest::new();
    unary.set_payload(fat_test_payload());
    let err = client
        .unary_call(Request::new(unary))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");

    let mut out = StreamingOutputCallRequest::new();
    out.set_payload(fat_test_payload());
    match client.streaming_output_call(Request::new(out)).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("server-stream over the server cap must fail"),
        },
    }

    let mut input = StreamingInputCallRequest::new();
    input.set_payload(fat_test_payload());
    let (tx, call) = client.streaming_input_call(Request::new(()));
    tx.send(input).await.expect("send");
    tx.close();
    let err = call.await.expect_err("client-stream over the server cap");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");

    let mut bidi = StreamingOutputCallRequest::new();
    bidi.set_payload(fat_test_payload());
    let (tx, call) = client.full_duplex_call(Request::new(()));
    tx.send(bidi).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("bidi over the server cap must fail"),
        },
    }
}

async fn wait_then_complete_every_shape(
    client: &GreeterClient,
    wait_on_request: bool,
    start: impl std::future::Future,
) {
    let timeout = Some(Duration::from_secs(5));
    let mut unary = client.say_hello(stamp_wait_ready(
        Request::new(req("late")),
        wait_on_request,
        timeout,
    ));
    let mut server_stream = client.server_hello(stamp_wait_ready(
        Request::new(req("late")),
        wait_on_request,
        timeout,
    ));
    let (tx_c, mut client_stream) =
        client.client_hello(stamp_wait_ready(Request::new(()), wait_on_request, timeout));
    let (tx_b, mut bidi) =
        client.stream_hello(stamp_wait_ready(Request::new(()), wait_on_request, timeout));

    tokio::select! {
        biased;
        result = &mut unary => panic!("unary finished before the server listened: {result:?}"),
        result = &mut server_stream => panic!("server-stream finished before the server listened: {result:?}"),
        result = &mut client_stream => panic!("client-stream finished before the server listened: {result:?}"),
        result = &mut bidi => panic!("bidi finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let _guard = start.await;

    let reply = tokio::time::timeout(Duration::from_secs(2), unary)
        .await
        .expect("unary hung after listen")
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "late");

    let mut stream = tokio::time::timeout(Duration::from_secs(2), server_stream)
        .await
        .expect("server-stream hung after listen")
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "late");

    tx_c.send(req("late")).await.expect("send");
    tx_c.close();
    let reply = tokio::time::timeout(Duration::from_secs(2), client_stream)
        .await
        .expect("client-stream hung after listen")
        .expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "late");

    tx_b.send(req("late")).await.expect("send");
    tx_b.close();
    let mut inbound = tokio::time::timeout(Duration::from_secs(2), bidi)
        .await
        .expect("bidi hung after listen")
        .expect("bidi")
        .into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "late");
}

async fn wait_then_complete_test(
    client: &TestServiceClient,
    wait_on_request: bool,
    start: impl std::future::Future,
) {
    let timeout = Some(Duration::from_secs(5));
    let mut unary = client.empty_call(stamp_wait_ready(
        Request::new(Empty::new()),
        wait_on_request,
        timeout,
    ));
    let mut server_stream = client.streaming_output_call(stamp_wait_ready(
        Request::new(StreamingOutputCallRequest::new()),
        wait_on_request,
        timeout,
    ));
    let (tx_c, mut client_stream) =
        client.streaming_input_call(stamp_wait_ready(Request::new(()), wait_on_request, timeout));
    let (tx_b, mut bidi) =
        client.full_duplex_call(stamp_wait_ready(Request::new(()), wait_on_request, timeout));

    tokio::select! {
        biased;
        result = &mut unary => panic!("unary finished before the server listened: {result:?}"),
        result = &mut server_stream => panic!("server-stream finished before the server listened: {result:?}"),
        result = &mut client_stream => panic!("client-stream finished before the server listened: {result:?}"),
        result = &mut bidi => panic!("bidi finished before the server listened: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(80)) => {}
    }

    let _guard = start.await;

    tokio::time::timeout(Duration::from_secs(2), unary)
        .await
        .expect("unary hung after listen")
        .expect("unary");

    let mut stream = tokio::time::timeout(Duration::from_secs(2), server_stream)
        .await
        .expect("server-stream hung after listen")
        .expect("server-stream")
        .into_inner();
    assert!(
        stream.message().await.expect("end").is_none(),
        "empty StreamingOutputCall plan must end"
    );

    tx_c.close();
    tokio::time::timeout(Duration::from_secs(2), client_stream)
        .await
        .expect("client-stream hung after listen")
        .expect("client-stream");

    tx_b.close();
    let mut inbound = tokio::time::timeout(Duration::from_secs(2), bidi)
        .await
        .expect("bidi hung after listen")
        .expect("bidi")
        .into_inner();
    assert!(
        inbound.message().await.expect("end").is_none(),
        "empty FullDuplexCall must end"
    );
}

fn echo_named_stream(name: String) -> Response<pbrs_grpc::Streaming<HelloReply>> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        tx.send(common::reply(name)).await.ok();
    }));
    Response::new(stream)
}

fn sees_http<T>(request: Request<T>) -> Result<T, Status> {
    let want_auth = format!(
        "127.0.0.1:{}",
        request
            .local_addr()
            .ok_or_else(|| Status::internal("missing local_addr"))?
            .port()
    );
    if request.authority() != Some(want_auth.as_str()) {
        return Err(Status::internal(format!(
            "authority {:?}",
            request.authority()
        )));
    }
    if request.scheme() != Some("http") {
        return Err(Status::internal(format!("scheme {:?}", request.scheme())));
    }
    if request.deadline().is_some() {
        return Err(Status::internal("no timeout, so no deadline Instant"));
    }
    if request.peer_cred().is_some() {
        return Err(Status::internal("tcp has no unix credentials"));
    }
    let (msg, parts) = request.into_message_and_parts();
    if parts.authority() != Some(want_auth.as_str()) || parts.scheme() != Some("http") {
        return Err(Status::internal("parts dropped http identity"));
    }
    if parts.peer_cred().is_some() {
        return Err(Status::internal("parts invented unix credentials"));
    }
    Ok(msg)
}

async fn sees_deadline<T>(request: &Request<T>) -> Result<(), Status> {
    let timeout = request
        .timeout()
        .ok_or_else(|| Status::internal("missing timeout duration"))?;
    if timeout < Duration::from_millis(150) || timeout > Duration::from_millis(250) {
        return Err(Status::internal(format!("timeout {timeout:?}")));
    }
    let deadline = request
        .deadline()
        .ok_or_else(|| Status::internal("missing deadline Instant"))?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
    if left >= timeout {
        return Err(Status::internal(format!(
            "remaining {left:?} not less than stamped {timeout:?}"
        )));
    }
    if request.timeout() != Some(timeout) {
        return Err(Status::internal("timeout duration must not shrink"));
    }
    Ok(())
}

#[cfg(unix)]
fn sees_unix<T>(request: Request<T>) -> Result<T, Status> {
    if request.remote_addr().is_some() || request.local_addr().is_some() {
        return Err(Status::internal("unix has no std::net::SocketAddr"));
    }
    if request.peer_identity().is_some() {
        return Err(Status::internal("unix has no TLS client certificate"));
    }
    if request.scheme() != Some("http") {
        return Err(Status::internal(format!("scheme {:?}", request.scheme())));
    }
    if request.authority() != Some("localhost") {
        return Err(Status::internal(format!(
            "authority {:?}",
            request.authority()
        )));
    }
    let Some(cred) = request.peer_cred() else {
        return Err(Status::internal("missing peer_cred"));
    };
    if cred.pid() != Some(std::process::id()) {
        return Err(Status::internal(format!(
            "pid {:?} want {}",
            cred.pid(),
            std::process::id()
        )));
    }
    let (msg, parts) = request.into_message_and_parts();
    if parts.peer_cred() != Some(cred) {
        return Err(Status::internal("parts dropped peer_cred"));
    }
    Ok(msg)
}

fn sees_from_io<T>(request: Request<T>, want_auth: &str, want_scheme: &str) -> Result<T, Status> {
    if request.remote_addr().is_some() || request.local_addr().is_some() {
        return Err(Status::internal("from_io must not invent TCP addrs"));
    }
    if request.peer_identity().is_some() {
        return Err(Status::internal("from_io must not invent a TLS identity"));
    }
    if request.peer_cred().is_some() {
        return Err(Status::internal("from_io must not invent unix credentials"));
    }
    if request.authority() != Some(want_auth) {
        return Err(Status::internal(format!(
            "authority {:?}",
            request.authority()
        )));
    }
    if request.scheme() != Some(want_scheme) {
        return Err(Status::internal(format!("scheme {:?}", request.scheme())));
    }
    let (msg, parts) = request.into_message_and_parts();
    if parts.authority() != Some(want_auth) || parts.scheme() != Some(want_scheme) {
        return Err(Status::internal("parts dropped from_io identity"));
    }
    if parts.remote_addr().is_some()
        || parts.local_addr().is_some()
        || parts.peer_identity().is_some()
        || parts.peer_cred().is_some()
    {
        return Err(Status::internal("parts invented peer facts"));
    }
    Ok(msg)
}

fn sees_incoming<T>(request: Request<T>) -> Result<T, Status> {
    let want_remote: SocketAddr = "192.0.2.1:8".parse().expect("remote");
    let want_local: SocketAddr = "127.0.0.1:9".parse().expect("local");
    let want_cred = PeerCred::new(42, 43, Some(44));
    if request.remote_addr() != Some(want_remote) {
        return Err(Status::internal(format!(
            "remote {:?}",
            request.remote_addr()
        )));
    }
    if request.local_addr() != Some(want_local) {
        return Err(Status::internal(format!(
            "local {:?}",
            request.local_addr()
        )));
    }
    if request.peer_identity().and_then(|id| id.leaf()) != Some(b"leaf") {
        return Err(Status::internal("missing stamped identity"));
    }
    if request.peer_cred() != Some(want_cred) {
        return Err(Status::internal(format!("cred {:?}", request.peer_cred())));
    }
    if request.scheme() != Some("https") {
        return Err(Status::internal(format!("scheme {:?}", request.scheme())));
    }
    let (msg, parts) = request.into_message_and_parts();
    if parts.remote_addr() != Some(want_remote)
        || parts.local_addr() != Some(want_local)
        || parts.peer_identity().and_then(|id| id.leaf()) != Some(b"leaf")
        || parts.peer_cred() != Some(want_cred)
        || parts.scheme() != Some("https")
    {
        return Err(Status::internal("parts dropped Incoming::peer facts"));
    }
    Ok(msg)
}

async fn gzip_every_shape(client: &GreeterClient) {
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");

    let (tx, call) = client.client_hello(Request::new(()));
    assert!(tx.compress(), "StreamSender must gzip");
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let (tx, call) = client.stream_hello(Request::new(()));
    assert!(tx.compress(), "bidi StreamSender must gzip");
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
}

async fn assert_server_gzip_every_shape(client: &GreeterClient) {
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert!(
        reply.compressed(),
        "server send_compressed plus client grpc-accept-encoding: gzip"
    );
    assert_eq!(
        reply.encoding(),
        Some("gzip"),
        "received unary must surface grpc-encoding"
    );
    assert_eq!(name_of(reply.get_ref()), "ada");

    let reply = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream");
    assert_eq!(
        reply.encoding(),
        Some("gzip"),
        "received server-stream must surface grpc-encoding"
    );
    let mut stream = reply.into_inner();
    let framed = stream.next_framed().await.expect("frame").expect("message");
    assert!(
        framed.compressed,
        "Server::send_compressed must gzip identity StreamSender::send frames"
    );
    assert_eq!(name_of(&framed.message), "ada");

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert!(
        reply.compressed(),
        "client-stream unary reply must be gzipped"
    );
    assert_eq!(reply.encoding(), Some("gzip"));
    assert_eq!(name_of(reply.get_ref()), "ada");

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("bidi");
    assert_eq!(reply.encoding(), Some("gzip"), "bidi must surface gzip");
    let mut inbound = reply.into_inner();
    let framed = inbound
        .next_framed()
        .await
        .expect("frame")
        .expect("message");
    assert!(
        framed.compressed,
        "bidi send_compressed frames must be gzip"
    );
    assert_eq!(name_of(&framed.message), "ada");
}

async fn assert_identity_encoding_every_shape(client: &GreeterClient) {
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert!(
        !reply.compressed(),
        "identity unary must not set Compressed-Flag"
    );
    assert!(
        reply.encoding().is_none(),
        "identity unary must not invent grpc-encoding"
    );
    assert_eq!(name_of(reply.get_ref()), "ada");

    let reply = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream");
    assert!(
        reply.encoding().is_none(),
        "identity stream must not invent grpc-encoding"
    );
    let mut stream = reply.into_inner();
    let framed = stream.next_framed().await.expect("frame").expect("message");
    assert!(
        !framed.compressed,
        "default server must leave identity StreamSender::send uncompressed"
    );
    assert_eq!(name_of(&framed.message), "ada");

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert!(!reply.compressed());
    assert!(reply.encoding().is_none());
    assert_eq!(name_of(reply.get_ref()), "ada");

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("bidi");
    assert!(reply.encoding().is_none());
    let mut inbound = reply.into_inner();
    let framed = inbound
        .next_framed()
        .await
        .expect("frame")
        .expect("message");
    assert!(!framed.compressed);
    assert_eq!(name_of(&framed.message), "ada");
}

async fn assert_deadline_quickly<T>(call: Call<T>, max_elapsed: Duration) {
    assert_deadline_in(call, Duration::ZERO, max_elapsed).await;
}

async fn assert_deadline_in<T>(call: Call<T>, min_elapsed: Duration, max_elapsed: Duration) {
    let started = Instant::now();
    let err = match call.await {
        Ok(_) => panic!("expected deadline"),
        Err(status) => status,
    };
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() >= min_elapsed,
        "deadline returned too fast: {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < max_elapsed,
        "deadline too slow: {:?}",
        started.elapsed()
    );
}

async fn assert_deadline_quickly_on_every_shape(
    client: &GreeterClient,
    timeout: Option<Duration>,
    max_elapsed: Duration,
) {
    assert_deadline_quickly(
        client.say_hello(stamp_timeout(Request::new(req("ada")), timeout)),
        max_elapsed,
    )
    .await;
    assert_deadline_quickly(
        client.server_hello(stamp_timeout(Request::new(req("ada")), timeout)),
        max_elapsed,
    )
    .await;
    let (tx, call) = client.client_hello(stamp_timeout(Request::new(()), timeout));
    assert_deadline_quickly(call, max_elapsed).await;
    drop(tx);
    let (tx, call) = client.stream_hello(stamp_timeout(Request::new(()), timeout));
    assert_deadline_quickly(call, max_elapsed).await;
    drop(tx);
}

async fn slow_every_shape(client: &GreeterClient, timeout: Option<Duration>) {
    let reply = client
        .say_hello(stamp_timeout(Request::new(req("ada")), timeout))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let mut stream = client
        .server_hello(stamp_timeout(Request::new(req("ada")), timeout))
        .await
        .expect("server-stream")
        .into_inner();
    assert!(stream.message().await.expect("end").is_none());

    let (tx, call) = client.client_hello(stamp_timeout(Request::new(()), timeout));
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), "ok");

    let (tx, call) = client.stream_hello(stamp_timeout(Request::new(()), timeout));
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    assert!(inbound.message().await.expect("end").is_none());
}

async fn assert_err_on_every_shape(client: &GreeterClient, want: Code) {
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), want, "{err}");
    let err = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect_err("server-stream");
    assert_eq!(err.code(), want, "{err}");
    let (tx, call) = client.client_hello(Request::new(()));
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), want, "{err}");
    drop(tx);
    let (tx, call) = client.stream_hello(Request::new(()));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), want, "{err}");
    drop(tx);
}

async fn assert_err_on_test_every_shape(client: &TestServiceClient, want: Code) {
    let err = client
        .empty_call(Request::new(Empty::new()))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), want, "{err}");
    let err = client
        .streaming_output_call(Request::new(StreamingOutputCallRequest::new()))
        .await
        .expect_err("server-stream");
    assert_eq!(err.code(), want, "{err}");
    let (tx, call) = client.streaming_input_call(Request::new(()));
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), want, "{err}");
    drop(tx);
    let (tx, call) = client.full_duplex_call(Request::new(()));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), want, "{err}");
    drop(tx);
}

async fn assert_test_blocked_every_shape(client: &TestServiceClient) {
    assert_interceptor_blocked(
        &client
            .empty_call(Request::new(Empty::new()))
            .await
            .expect_err("unary"),
    );
    match client
        .streaming_output_call(Request::new(StreamingOutputCallRequest::new()))
        .await
    {
        Err(err) => assert_interceptor_blocked(&err),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_interceptor_blocked(&err),
            Ok(_) => panic!("server-stream interceptor reject must fail"),
        },
    }
    let (tx, call) = client.streaming_input_call(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("client-stream"));
    drop(tx);
    let (tx, call) = client.full_duplex_call(Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

async fn assert_reverser_err_every_shape(channel: &Channel, want: Code) {
    let err = channel
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), want, "{err}");
    let err = channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            Request::new(req("stressed")),
        )
        .await
        .expect_err("server-stream");
    assert_eq!(err.code(), want, "{err}");
    let (tx, call) = channel
        .client_streaming::<HelloRequest, HelloReply>("/demo.Reverser/Client", Request::new(()));
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), want, "{err}");
    drop(tx);
    let (tx, call) =
        channel.bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", Request::new(()));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), want, "{err}");
    drop(tx);
}

async fn assert_reverser_blocked_every_shape(channel: &Channel) {
    let err = channel
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect_err("unary");
    assert_interceptor_blocked(&err);
    let err = channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            Request::new(req("stressed")),
        )
        .await
        .expect_err("server-stream");
    assert_interceptor_blocked(&err);
    let (tx, call) = channel
        .client_streaming::<HelloRequest, HelloReply>("/demo.Reverser/Client", Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("client-stream"));
    drop(tx);
    let (tx, call) =
        channel.bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", Request::new(()));
    assert_interceptor_blocked(&call.await.expect_err("bidi"));
    drop(tx);
}

async fn assert_opt_out_every_shape(client: &GreeterClient) {
    let err = client
        .say_hello(stamp_opt_out(Request::new(req("nope"))))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = client
        .server_hello(stamp_opt_out(Request::new(req("nope"))))
        .await
        .expect_err("server-stream");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let (tx, call) = client.client_hello(stamp_opt_out(Request::new(())));
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(tx);
    let (tx, call) = client.stream_hello(stamp_opt_out(Request::new(())));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(tx);
}

async fn assert_test_opt_out(client: &TestServiceClient) {
    let err = client
        .empty_call(stamp_opt_out(Request::new(Empty::new())))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = client
        .streaming_output_call(stamp_opt_out(Request::new(
            StreamingOutputCallRequest::new(),
        )))
        .await
        .expect_err("server-stream");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let (tx, call) = client.streaming_input_call(stamp_opt_out(Request::new(())));
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(tx);
    let (tx, call) = client.full_duplex_call(stamp_opt_out(Request::new(())));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(tx);
}

async fn assert_test_wait_deadline(client: &TestServiceClient) {
    let timeout = Duration::from_millis(80);
    let min = Duration::from_millis(50);
    let max = Duration::from_secs(2);
    assert_deadline_in(
        client.empty_call(stamp_wait_deadline(Request::new(Empty::new()), timeout)),
        min,
        max,
    )
    .await;
    assert_deadline_in(
        client.streaming_output_call(stamp_wait_deadline(
            Request::new(StreamingOutputCallRequest::new()),
            timeout,
        )),
        min,
        max,
    )
    .await;
    let (tx, call) = client.streaming_input_call(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
    let (tx, call) = client.full_duplex_call(stamp_wait_deadline(Request::new(()), timeout));
    assert_deadline_in(call, min, max).await;
    drop(tx);
}

async fn assert_reverser_opt_out(channel: &Channel) {
    let err = channel
        .unary::<HelloRequest, HelloReply>(
            "/demo.Reverser/Reverse",
            stamp_opt_out(Request::new(req("nope"))),
        )
        .await
        .expect_err("unary");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            stamp_opt_out(Request::new(req("nope"))),
        )
        .await
        .expect_err("server-stream");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let (tx, call) = channel.client_streaming::<HelloRequest, HelloReply>(
        "/demo.Reverser/Client",
        stamp_opt_out(Request::new(())),
    );
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(tx);
    let (tx, call) = channel
        .bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", stamp_opt_out(Request::new(())));
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(tx);
}

async fn assert_reverser_wait_deadline(channel: &Channel) {
    let timeout = Duration::from_millis(80);
    let min = Duration::from_millis(50);
    let max = Duration::from_secs(2);
    assert_deadline_in(
        channel.unary::<HelloRequest, HelloReply>(
            "/demo.Reverser/Reverse",
            stamp_wait_deadline(Request::new(req("x")), timeout),
        ),
        min,
        max,
    )
    .await;
    assert_deadline_in(
        channel.server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            stamp_wait_deadline(Request::new(req("x")), timeout),
        ),
        min,
        max,
    )
    .await;
    let (tx, call) = channel.client_streaming::<HelloRequest, HelloReply>(
        "/demo.Reverser/Client",
        stamp_wait_deadline(Request::new(()), timeout),
    );
    assert_deadline_in(call, min, max).await;
    drop(tx);
    let (tx, call) = channel.bidi::<HelloRequest, HelloReply>(
        "/demo.Reverser/Bidi",
        stamp_wait_deadline(Request::new(()), timeout),
    );
    assert_deadline_in(call, min, max).await;
    drop(tx);
}

fn stamp_gone<T>(mut request: Request<T>) -> Request<T> {
    request.set_timeout(Duration::from_millis(200));
    request
}

fn assert_gone(err: &Status) {
    assert!(
        matches!(
            err.code(),
            Code::Unavailable | Code::DeadlineExceeded | Code::Cancelled
        ),
        "{err}"
    );
}

async fn assert_gone_on_every_shape(client: &GreeterClient) {
    let err = client
        .say_hello(stamp_gone(Request::new(req("gone"))))
        .await
        .expect_err("unary");
    assert_gone(&err);
    let err = client
        .server_hello(stamp_gone(Request::new(req("gone"))))
        .await
        .expect_err("server-stream");
    assert_gone(&err);
    let (tx, call) = client.client_hello(stamp_gone(Request::new(())));
    let err = call.await.expect_err("client-stream");
    assert_gone(&err);
    drop(tx);
    let (tx, call) = client.stream_hello(stamp_gone(Request::new(())));
    let err = call.await.expect_err("bidi");
    assert_gone(&err);
    drop(tx);
}

async fn wait_half_close_drained<T: std::fmt::Debug>(call: &mut Call<T>, drained: &AtomicUsize) {
    tokio::select! {
        biased;
        result = call => panic!("call returned before drain: {result:?}"),
        () = async {
            for _ in 0..80 {
                if drained.load(Ordering::Relaxed) >= 1 {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("handler never finished reading the half-closed stream");
        } => {}
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawned_work_stops_when_the_client_cancels() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut call = client.say_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("SpawnHang returned before cancel: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    call.handle().cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(&child_done).await;
    assert_eq!(
        finished.load(Ordering::Relaxed),
        0,
        "handler should have been dropped"
    );
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawned_streaming_work_stops_when_the_client_cancels() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, mut call) = client.client_hello(Request::new(()));
    tokio::select! {
        biased;
        result = &mut call => panic!("SpawnHang returned before cancel: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    call.handle().cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(&child_done).await;
    assert_eq!(finished.load(Ordering::Relaxed), 0);
    drop(tx);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawned_server_streaming_work_stops_when_the_client_cancels() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut call = client.server_hello(Request::new(req("ada")));
    tokio::select! {
        biased;
        result = &mut call => panic!("SpawnHang returned before cancel: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    call.handle().cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(&child_done).await;
    assert_eq!(finished.load(Ordering::Relaxed), 0);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawned_bidi_work_stops_when_the_client_cancels() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, mut call) = client.stream_hello(Request::new(()));
    tokio::select! {
        biased;
        result = &mut call => panic!("SpawnHang returned before cancel: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    assert!(
        started.load(Ordering::Relaxed) >= 1,
        "handler should have started"
    );
    call.handle().cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(&child_done).await;
    assert_eq!(finished.load(Ordering::Relaxed), 0);
    drop(tx);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_spawned_work_stops_when_the_client_cancels() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_spawned_cancel_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_spawned_work_stops_when_the_client_cancels() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_spawned_cancel_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_spawned_work_stops_when_the_client_cancels() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_spawned_cancel_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_spawned_work_stops_when_the_client_cancels() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_spawned_cancel_on_every_shape(&client, &started, &finished, &child_done).await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadline_cancels_a_server_stream_before_headers() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_millis(80));
    let err = client.server_hello(request).await.expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    wait_flag(&child_done).await;
    assert_eq!(finished.load(Ordering::Relaxed), 0);
    assert!(started.load(Ordering::Relaxed) >= 1);
    task.abort();
}

fn interceptor_server_set_timeout(rpc: &mut Rpc) -> Result<(), Status> {
    rpc.set_timeout(Duration::from_millis(20));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawned_work_stops_when_the_deadline_fires() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .intercept(interceptor_server_set_timeout)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_spawned_work_stops_when_the_deadline_fires() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .intercept(interceptor_server_set_timeout)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_spawned_work_stops_when_the_deadline_fires() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .intercept(interceptor_server_set_timeout)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_spawned_work_stops_when_the_deadline_fires() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .intercept(interceptor_server_set_timeout)
            .serve_unix(sock)
            .await
            .ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_spawned_work_stops_when_the_deadline_fires() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(hang)
            .intercept(interceptor_server_set_timeout)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_deadline_on_every_shape(&client, &started, &finished, &child_done).await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawned_work_stops_when_the_rpc_completes() {
    let child_done = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = SpawnOk {
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    wait_flag(&child_done).await;

    child_done.store(0, Ordering::Relaxed);
    let _ = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream");
    wait_flag(&child_done).await;

    child_done.store(0, Ordering::Relaxed);
    let (tx, call) = client.client_hello(Request::new(()));
    tx.close();
    let _ = call.await.expect("client-stream");
    wait_flag(&child_done).await;

    child_done.store(0, Ordering::Relaxed);
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    let _ = call.await.expect("bidi");
    wait_flag(&child_done).await;
    task.abort();
}

async fn assert_spawned_complete_on_every_shape(client: &GreeterClient, child_done: &AtomicUsize) {
    child_done.store(0, Ordering::Relaxed);
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), "ada");
    wait_flag(child_done).await;

    child_done.store(0, Ordering::Relaxed);
    let _ = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream");
    wait_flag(child_done).await;

    child_done.store(0, Ordering::Relaxed);
    let (tx, call) = client.client_hello(Request::new(()));
    tx.close();
    let _ = call.await.expect("client-stream");
    wait_flag(child_done).await;

    child_done.store(0, Ordering::Relaxed);
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    let _ = call.await.expect("bidi");
    wait_flag(child_done).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_spawned_work_stops_when_the_rpc_completes() {
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = SpawnOk {
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_spawned_complete_on_every_shape(&client, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_spawned_work_stops_when_the_rpc_completes() {
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = SpawnOk {
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_spawned_complete_on_every_shape(&client, &child_done).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_spawned_work_stops_when_the_rpc_completes() {
    let child_done = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = SpawnOk {
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_spawned_complete_on_every_shape(&client, &child_done).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_spawned_work_stops_when_the_rpc_completes() {
    let child_done = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = SpawnOk {
        child_done: Arc::clone(&child_done),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_spawned_complete_on_every_shape(&client, &child_done).await;
    server.abort();
}

/// Watches [`Request::cancelled`] while a separate task produces the stream.
///
/// `go` stays false until the client has read the first message, so the
/// producer cannot drain (and fire cancel) before that assertion.
struct SpawnStream {
    cancelled: Arc<AtomicUsize>,
    go: tokio::sync::watch::Receiver<bool>,
}

impl pbrs_grpc::Greeter for SpawnStream {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("spawn-stream"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("spawn-stream"))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        if request.is_cancelled() {
            return Err(Status::internal("cancelled before the handler ran"));
        }
        let fired = Arc::clone(&self.cancelled);
        let cancelled = request.cancelled();
        drop(tokio::spawn(async move {
            cancelled.await;
            fired.fetch_add(1, Ordering::Relaxed);
        }));
        let (tx, stream) = pbrs_grpc::Streaming::channel(8);
        let mut go = self.go.clone();
        drop(tokio::spawn(async move {
            let mut first = HelloReply::new();
            first.set_message("0");
            if tx.send(first).await.is_err() {
                return;
            }
            if go.wait_for(|open| *open).await.is_err() {
                return;
            }
            for i in 1..3 {
                let mut reply = HelloReply::new();
                reply.set_message(format!("{i}"));
                if tx.send(reply).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("spawn-stream"))
    }
}

async fn assert_streaming_producer_is_not_cancelled_when_the_handler_returns(
    client: GreeterClient,
    go: tokio::sync::watch::Sender<bool>,
    cancelled: &AtomicUsize,
) {
    // Keep the client so this stays about cancellation, not connection lifetime.
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "0");
    assert_eq!(
        cancelled.load(Ordering::Relaxed),
        0,
        "Request::cancelled must wait until the stream drains"
    );
    go.send(true).expect("producer is waiting");
    let mut n = 1;
    while let Some(msg) = stream.message().await.expect("item") {
        assert_eq!(name_of(&msg), format!("{n}"));
        n += 1;
    }
    assert_eq!(n, 3, "producer must outlive handler return");
    wait_flag(cancelled).await;
    drop(client);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_streaming_producer_is_not_cancelled_when_the_handler_returns() {
    let (svc, go, cancelled) = spawn_stream_svc();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    assert_streaming_producer_is_not_cancelled_when_the_handler_returns(
        GreeterClient::new(channel(addr).await),
        go,
        &cancelled,
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_streaming_producer_is_not_cancelled_when_the_handler_returns() {
    let (svc, go, cancelled) = spawn_stream_svc();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_streaming_producer_is_not_cancelled_when_the_handler_returns(
        GreeterClient::new(tls_channel(addr).await),
        go,
        &cancelled,
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_streaming_producer_is_not_cancelled_when_the_handler_returns() {
    let (svc, go, cancelled) = spawn_stream_svc();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_streaming_producer_is_not_cancelled_when_the_handler_returns(
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
        go,
        &cancelled,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_streaming_producer_is_not_cancelled_when_the_handler_returns() {
    let (svc, go, cancelled) = spawn_stream_svc();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    assert_streaming_producer_is_not_cancelled_when_the_handler_returns(
        GreeterClient::new(unix_channel(&path).await),
        go,
        &cancelled,
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_streaming_producer_is_not_cancelled_when_the_handler_returns() {
    let (svc, go, cancelled) = spawn_stream_svc();
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_streaming_producer_is_not_cancelled_when_the_handler_returns(
        GreeterClient::new(channel),
        go,
        &cancelled,
    )
    .await;
    server.abort();
}

async fn assert_dropping_the_channel_does_not_kill_a_live_stream(
    client: GreeterClient,
    go: tokio::sync::watch::Sender<bool>,
) {
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "0");
    drop(client);
    go.send(true).expect("producer is waiting");
    let mut n = 1;
    while let Some(msg) = stream.message().await.expect("item") {
        assert_eq!(name_of(&msg), format!("{n}"));
        n += 1;
    }
    assert_eq!(n, 3, "stream must outlive dropping the client");
}

fn spawn_stream_svc() -> (
    SpawnStream,
    tokio::sync::watch::Sender<bool>,
    Arc<AtomicUsize>,
) {
    let cancelled = Arc::new(AtomicUsize::new(0));
    let (go, go_rx) = tokio::sync::watch::channel(false);
    (
        SpawnStream {
            cancelled: Arc::clone(&cancelled),
            go: go_rx,
        },
        go,
        cancelled,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_client_does_not_kill_a_live_stream() {
    let (svc, go, _) = spawn_stream_svc();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    assert_dropping_the_channel_does_not_kill_a_live_stream(
        GreeterClient::new(channel(addr).await),
        go,
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_dropping_the_client_does_not_kill_a_live_stream() {
    let (svc, go, _) = spawn_stream_svc();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_dropping_the_channel_does_not_kill_a_live_stream(
        GreeterClient::new(tls_channel(addr).await),
        go,
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_dropping_the_client_does_not_kill_a_live_stream() {
    let (svc, go, _) = spawn_stream_svc();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_dropping_the_channel_does_not_kill_a_live_stream(
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
        go,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_dropping_the_client_does_not_kill_a_live_stream() {
    let (svc, go, _) = spawn_stream_svc();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    assert_dropping_the_channel_does_not_kill_a_live_stream(
        GreeterClient::new(unix_channel(&path).await),
        go,
    )
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_dropping_the_client_does_not_kill_a_live_stream() {
    let (svc, go, _) = spawn_stream_svc();
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_dropping_the_channel_does_not_kill_a_live_stream(GreeterClient::new(channel), go).await;
    server.abort();
}

/// Sends one message, then waits until the client leaves.
///
/// Unlike [`SpawnStream`], this producer never sends again. Drain must abort
/// on RST so `cancelled` / `closed` fire without a later status change.
struct WaitAfterFirst {
    left: Arc<AtomicUsize>,
}

impl pbrs_grpc::Greeter for WaitAfterFirst {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("wait-after-first"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("wait-after-first"))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let left = Arc::clone(&self.left);
        let cancelled = request.cancelled();
        let (tx, stream) = pbrs_grpc::Streaming::channel(8);
        drop(tokio::spawn(async move {
            let mut first = HelloReply::new();
            first.set_message("0");
            if tx.send(first).await.is_err() {
                left.fetch_add(1, Ordering::Relaxed);
                return;
            }
            tokio::select! {
                biased;
                () = cancelled => {}
                () = tx.closed() => {}
            }
            left.fetch_add(1, Ordering::Relaxed);
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("wait-after-first"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_server_stream_cancels_a_waiting_producer() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "0");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait for the client to leave"
    );
    drop(stream);
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_handle_cancels_a_live_server_stream_after_headers() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let call = client.server_hello(Request::new(req("ada")));
    let handle = call.handle();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "0");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until cancel"
    );
    handle.cancel();
    wait_flag(&left).await;
    drop(stream);
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadline_cancels_a_live_server_stream_after_headers() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_millis(80));
    let mut stream = client
        .server_hello(request)
        .await
        .expect("headers")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "0");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until the deadline"
    );
    wait_flag(&left).await;
    drop(stream);
    drop(client);
    task.abort();
}

/// Echoes one bidi message, then waits until the client leaves.
struct BidiWaitAfterFirst {
    left: Arc<AtomicUsize>,
}

impl pbrs_grpc::Greeter for BidiWaitAfterFirst {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("bidi-wait-after-first"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("bidi-wait-after-first"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("bidi-wait-after-first"))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let left = Arc::clone(&self.left);
        let cancelled = request.cancelled();
        let mut inbound = request.into_inner();
        let (tx, stream) = pbrs_grpc::Streaming::channel(8);
        drop(tokio::spawn(async move {
            match inbound.message().await {
                Ok(Some(msg)) => {
                    let mut reply = HelloReply::new();
                    reply.set_message(msg.name());
                    if tx.send(reply).await.is_err() {
                        left.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                }
                Ok(None) | Err(_) => {
                    left.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            tokio::select! {
                biased;
                () = cancelled => {}
                () = tx.closed() => {}
            }
            left.fetch_add(1, Ordering::Relaxed);
        }));
        Ok(Response::new(stream))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_bidi_stream_cancels_a_waiting_producer() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait for the client to leave"
    );
    drop(stream);
    wait_flag(&left).await;
    drop(tx);
    drop(client);
    task.abort();
}

async fn assert_drop_server_stream_cancels_producer(client: &GreeterClient, left: &AtomicUsize) {
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "0");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait for the client to leave"
    );
    drop(stream);
    wait_flag(left).await;
}

async fn assert_drop_bidi_stream_cancels_producer(client: &GreeterClient, left: &AtomicUsize) {
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait for the client to leave"
    );
    drop(stream);
    wait_flag(left).await;
    drop(tx);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_dropping_a_received_stream_cancels_a_waiting_producer() {
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_drop_server_stream_cancels_producer(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_drop_bidi_stream_cancels_producer(&client, &left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_dropping_a_received_stream_cancels_a_waiting_producer() {
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_drop_server_stream_cancels_producer(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_drop_bidi_stream_cancels_producer(&client, &left).await;
    drop(client);
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_dropping_a_received_stream_cancels_a_waiting_producer() {
    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_drop_server_stream_cancels_producer(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_drop_bidi_stream_cancels_producer(&client, &left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_dropping_a_received_stream_cancels_a_waiting_producer() {
    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_drop_server_stream_cancels_producer(&client, &left).await;
    drop(client);
    server.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_drop_bidi_stream_cancels_producer(&client, &left).await;
    drop(client);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_handle_cancels_a_live_bidi_stream_after_headers() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, call) = client.stream_hello(Request::new(()));
    let handle = call.handle();
    tx.send(req("ada")).await.expect("send");
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until cancel"
    );
    handle.cancel();
    wait_flag(&left).await;
    drop(stream);
    drop(tx);
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_handle_cancels_a_bidi_stream_after_the_sender_closes() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, call) = client.stream_hello(Request::new(()));
    let handle = call.handle();
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until cancel"
    );
    handle.cancel();
    wait_flag(&left).await;
    drop(stream);
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadline_cancels_a_bidi_stream_after_the_sender_closes() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(());
    request.set_timeout(Duration::from_millis(80));
    let (tx, call) = client.stream_hello(request);
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until the deadline"
    );
    wait_flag(&left).await;
    drop(stream);
    drop(client);
    task.abort();
}

/// Drains the client stream, then waits until the client leaves.
struct ClientStreamWaitAfterClose {
    drained: Arc<AtomicUsize>,
    left: Arc<AtomicUsize>,
}

impl pbrs_grpc::Greeter for ClientStreamWaitAfterClose {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("wait-after-close"))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let (mut stream, parts) = request.into_message_and_parts();
        while stream.message().await?.is_some() {}
        self.drained.fetch_add(1, Ordering::Relaxed);
        // Spawned work, not the handler body: RST drops a pending handler.
        let left = Arc::clone(&self.left);
        let cancelled = parts.cancelled();
        drop(tokio::spawn(async move {
            cancelled.await;
            left.fetch_add(1, Ordering::Relaxed);
        }));
        tokio::time::sleep(Duration::from_secs(5)).await;
        Err(Status::internal("handler should have been dropped"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("wait-after-close"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("wait-after-close"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_handle_cancels_client_streaming_after_the_sender_closes() {
    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, mut call) = client.client_hello(Request::new(()));
    let handle = call.handle();
    tx.send(req("ada")).await.expect("send");
    tx.close();
    wait_half_close_drained(&mut call, &drained).await;
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "handler must wait until cancel"
    );
    handle.cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_call_cancels_client_streaming_after_the_sender_closes() {
    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, mut call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    wait_half_close_drained(&mut call, &drained).await;
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "handler must wait until drop"
    );
    drop(call);
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadline_cancels_client_streaming_after_the_sender_closes() {
    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(());
    request.set_timeout(Duration::from_millis(200));
    let (tx, mut call) = client.client_hello(request);
    tx.send(req("ada")).await.expect("send");
    tx.close();
    wait_half_close_drained(&mut call, &drained).await;
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "handler must wait until the deadline"
    );
    let err = call.await.expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

struct ClientStreamFailAfterOne {
    left: Arc<AtomicUsize>,
}

impl Greeter for ClientStreamFailAfterOne {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("fail-after-one"))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let cancelled = request.cancelled();
        let left = Arc::clone(&self.left);
        drop(tokio::spawn(async move {
            cancelled.await;
            left.fetch_add(1, Ordering::Relaxed);
        }));
        let mut inbound = request.into_inner();
        let first = inbound
            .message()
            .await?
            .ok_or_else(|| Status::internal("empty"))?;
        if first.name().to_str().unwrap_or("") != "ada" {
            return Err(Status::internal("unexpected name"));
        }
        std::future::pending().await
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("fail-after-one"))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let cancelled = request.cancelled();
        let left = Arc::clone(&self.left);
        drop(tokio::spawn(async move {
            cancelled.await;
            left.fetch_add(1, Ordering::Relaxed);
        }));
        let mut inbound = request.into_inner();
        let first = inbound
            .message()
            .await?
            .ok_or_else(|| Status::internal("empty"))?;
        if first.name().to_str().unwrap_or("") != "ada" {
            return Err(Status::internal("unexpected name"));
        }
        std::future::pending().await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_a_client_stream_after_a_message_is_that_status_not_internal() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.fail(stream_abort_status()).await;
    assert_stream_abort(&call.await.expect_err("fail"));
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

fn stream_abort_status() -> Status {
    let mut info = pbrs_grpc::pb::ErrorInfo::new();
    info.set_reason("STREAM_ABORTED");
    info.set_domain("example.com");
    Status::with_error_details(
        Code::NotFound,
        "gone",
        [pbrs_grpc::pb::Any::pack(&info).expect("pack")],
    )
    .expect("details")
}

fn assert_stream_abort(err: &Status) {
    assert_eq!(err.code(), Code::NotFound, "{err}");
    assert_eq!(err.message(), "gone");
    let info = err
        .error_details()
        .expect("details")
        .error_info
        .expect("ErrorInfo");
    assert_eq!(info.reason().to_str().unwrap_or(""), "STREAM_ABORTED");
}

/// Client-streaming: no request-side `grpc-status`; the Call gets `status`.
async fn assert_client_stream_sender_fail(client: &GreeterClient) {
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.fail(stream_abort_status()).await;
    assert_stream_abort(&call.await.expect_err("fail"));
}

/// Bidi before headers: the Call gets `status`, not `UNAVAILABLE` from RST.
async fn assert_bidi_sender_fail_before_headers(client: &GreeterClient) {
    for _ in 0..24 {
        let (tx, call) = client.stream_hello(Request::new(()));
        tx.send(req("ada")).await.expect("send");
        tx.fail(stream_abort_status()).await;
        assert_stream_abort(&call.await.expect_err("fail"));
    }
}

/// Bidi after headers: inbound Streaming sees CANCELLED, not `status`.
async fn assert_bidi_sender_fail_after_headers(client: &GreeterClient) {
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    tx.fail(stream_abort_status()).await;
    let err = stream.message().await.expect_err("reset after fail");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    assert_ne!(
        err.message(),
        "gone",
        "fail after headers must not surface the request status on Streaming: {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_a_bidi_stream_before_headers_is_that_status_not_unavailable() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    // Repeat: RST can surface as UNAVAILABLE on the same poll as fail's oneshot.
    for _ in 0..24 {
        let (tx, call) = client.stream_hello(Request::new(()));
        tx.send(req("ada")).await.expect("send");
        tx.fail(stream_abort_status()).await;
        assert_stream_abort(&call.await.expect_err("fail"));
    }
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_a_bidi_stream_after_headers_is_cancelled_not_that_status() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    tx.fail(stream_abort_status()).await;
    let err = stream.message().await.expect_err("reset after fail");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    assert_ne!(
        err.message(),
        "gone",
        "fail after headers must not surface the request status on Streaming: {err}"
    );
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_failing_a_request_stream_is_that_status_or_cancelled() {
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_client_stream_sender_fail(&client).await;
    assert_bidi_sender_fail_before_headers(&client).await;
    wait_flag(&left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_bidi_sender_fail_after_headers(&client).await;
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_failing_a_request_stream_is_that_status_or_cancelled() {
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_client_stream_sender_fail(&client).await;
    assert_bidi_sender_fail_before_headers(&client).await;
    wait_flag(&left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_bidi_sender_fail_after_headers(&client).await;
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_failing_a_request_stream_is_that_status_or_cancelled() {
    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_client_stream_sender_fail(&client).await;
    assert_bidi_sender_fail_before_headers(&client).await;
    wait_flag(&left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_bidi_sender_fail_after_headers(&client).await;
    wait_flag(&left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_failing_a_request_stream_is_that_status_or_cancelled() {
    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_client_stream_sender_fail(&client).await;
    assert_bidi_sender_fail_before_headers(&client).await;
    wait_flag(&left).await;
    drop(client);
    server.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_bidi_sender_fail_after_headers(&client).await;
    wait_flag(&left).await;
    drop(client);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_handle_cancels_a_bidi_stream_before_headers() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let (tx, mut call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tokio::select! {
        biased;
        result = &mut call => panic!("hang returned before cancel: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    call.handle().cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(&left).await;
    drop(tx);
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadline_cancels_a_bidi_stream_before_headers() {
    let left = Arc::new(AtomicUsize::new(0));
    let (addr, listener) = bind().await;
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_listener(listener).await.ok();
    });
    let client = GreeterClient::new(channel(addr).await);
    let mut request = Request::new(());
    request.set_timeout(Duration::from_millis(80));
    let (tx, call) = client.stream_hello(request);
    tx.send(req("ada")).await.expect("send");
    let err = call.await.expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    wait_flag(&left).await;
    drop(tx);
    drop(client);
    task.abort();
}

async fn assert_request_deadline_before_headers_on_every_shape(
    client: &GreeterClient,
    started: &AtomicUsize,
    finished: &AtomicUsize,
    child_done: &AtomicUsize,
) {
    let mut unary = Request::new(req("ada"));
    unary.set_timeout(Duration::from_millis(80));
    assert_deadline_dropped_spawned(client.say_hello(unary), started, finished, child_done).await;
    let mut server = Request::new(req("ada"));
    server.set_timeout(Duration::from_millis(80));
    assert_deadline_dropped_spawned(client.server_hello(server), started, finished, child_done)
        .await;
    let mut inbound = Request::new(());
    inbound.set_timeout(Duration::from_millis(80));
    let (tx, call) = client.client_hello(inbound);
    assert_deadline_dropped_spawned(call, started, finished, child_done).await;
    drop(tx);
    let mut bidi = Request::new(());
    bidi.set_timeout(Duration::from_millis(80));
    let (tx, call) = client.stream_hello(bidi);
    assert_deadline_dropped_spawned(call, started, finished, child_done).await;
    drop(tx);
}

async fn assert_deadline_cancels_live_server_stream(client: &GreeterClient, left: &AtomicUsize) {
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_millis(80));
    let mut stream = client
        .server_hello(request)
        .await
        .expect("headers")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "0");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until the deadline"
    );
    wait_flag(left).await;
    drop(stream);
}

async fn assert_deadline_cancels_bidi_after_close(client: &GreeterClient, left: &AtomicUsize) {
    let mut request = Request::new(());
    request.set_timeout(Duration::from_millis(80));
    let (tx, call) = client.stream_hello(request);
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until the deadline"
    );
    wait_flag(left).await;
    drop(stream);
}

async fn assert_deadline_cancels_client_stream_after_close(
    client: &GreeterClient,
    drained: &AtomicUsize,
    left: &AtomicUsize,
) {
    let mut request = Request::new(());
    request.set_timeout(Duration::from_millis(200));
    let (tx, mut call) = client.client_hello(request);
    tx.send(req("ada")).await.expect("send");
    tx.close();
    wait_half_close_drained(&mut call, drained).await;
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "handler must wait until the deadline"
    );
    let err = call.await.expect_err("deadline");
    assert_eq!(err.code(), Code::DeadlineExceeded, "{err}");
    wait_flag(left).await;
}

async fn assert_call_handle_cancels_live_server_stream(client: &GreeterClient, left: &AtomicUsize) {
    let call = client.server_hello(Request::new(req("ada")));
    let handle = call.handle();
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "0");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until cancel"
    );
    handle.cancel();
    wait_flag(left).await;
    drop(stream);
}

async fn assert_call_handle_cancels_live_bidi(client: &GreeterClient, left: &AtomicUsize) {
    let (tx, call) = client.stream_hello(Request::new(()));
    let handle = call.handle();
    tx.send(req("ada")).await.expect("send");
    let mut stream = call.await.expect("headers").into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), "ada");
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "producer must wait until cancel"
    );
    handle.cancel();
    wait_flag(left).await;
    drop(stream);
    drop(tx);
}

async fn assert_call_handle_cancels_bidi_before_headers(
    client: &GreeterClient,
    left: &AtomicUsize,
) {
    let (tx, mut call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tokio::select! {
        biased;
        result = &mut call => panic!("hang returned before cancel: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(40)) => {}
    }
    call.handle().cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(left).await;
    drop(tx);
}

async fn assert_call_handle_cancels_client_stream_after_close(
    client: &GreeterClient,
    drained: &AtomicUsize,
    left: &AtomicUsize,
) {
    let (tx, mut call) = client.client_hello(Request::new(()));
    let handle = call.handle();
    tx.send(req("ada")).await.expect("send");
    tx.close();
    wait_half_close_drained(&mut call, drained).await;
    assert_eq!(
        left.load(Ordering::Relaxed),
        0,
        "handler must wait until cancel"
    );
    handle.cancel();
    let err = call.await.expect_err("cancelled");
    assert_eq!(err.code(), Code::Cancelled, "{err}");
    wait_flag(left).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_call_handle_cancels_streams() {
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_call_handle_cancels_live_server_stream(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_call_handle_cancels_live_bidi(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_call_handle_cancels_bidi_before_headers(&client, &left).await;
    drop(client);
    task.abort();

    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_call_handle_cancels_client_stream_after_close(&client, &drained, &left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_call_handle_cancels_streams() {
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_call_handle_cancels_live_server_stream(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_call_handle_cancels_live_bidi(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_call_handle_cancels_bidi_before_headers(&client, &left).await;
    drop(client);
    task.abort();

    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_call_handle_cancels_client_stream_after_close(&client, &drained, &left).await;
    drop(client);
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_call_handle_cancels_streams() {
    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_call_handle_cancels_live_server_stream(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_call_handle_cancels_live_bidi(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_call_handle_cancels_bidi_before_headers(&client, &left).await;
    drop(client);
    task.abort();

    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_call_handle_cancels_client_stream_after_close(&client, &drained, &left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_call_handle_cancels_streams() {
    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_call_handle_cancels_live_server_stream(&client, &left).await;
    drop(client);
    server.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_call_handle_cancels_live_bidi(&client, &left).await;
    drop(client);
    server.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = ClientStreamFailAfterOne {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_call_handle_cancels_bidi_before_headers(&client, &left).await;
    drop(client);
    server.abort();

    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_call_handle_cancels_client_stream_after_close(&client, &drained, &left).await;
    drop(client);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_deadline_rsts_streams_before_and_after_headers() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_request_deadline_before_headers_on_every_shape(
        &client,
        &started,
        &finished,
        &child_done,
    )
    .await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_deadline_cancels_live_server_stream(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_deadline_cancels_bidi_after_close(&client, &left).await;
    drop(client);
    task.abort();

    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await);
    assert_deadline_cancels_client_stream_after_close(&client, &drained, &left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_deadline_rsts_streams_before_and_after_headers() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_request_deadline_before_headers_on_every_shape(
        &client,
        &started,
        &finished,
        &child_done,
    )
    .await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_deadline_cancels_live_server_stream(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_deadline_cancels_bidi_after_close(&client, &left).await;
    drop(client);
    task.abort();

    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await);
    assert_deadline_cancels_client_stream_after_close(&client, &drained, &left).await;
    drop(client);
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_deadline_rsts_streams_before_and_after_headers() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(hang).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_request_deadline_before_headers_on_every_shape(
        &client,
        &started,
        &finished,
        &child_done,
    )
    .await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_deadline_cancels_live_server_stream(&client, &left).await;
    drop(client);
    task.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_deadline_cancels_bidi_after_close(&client, &left).await;
    drop(client);
    task.abort();

    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let task = tokio::spawn(async move {
        GreeterServer::new(svc).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await);
    assert_deadline_cancels_client_stream_after_close(&client, &drained, &left).await;
    drop(client);
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_deadline_rsts_streams_before_and_after_headers() {
    let started = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicUsize::new(0));
    let child_done = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let hang = SpawnHang {
        started: Arc::clone(&started),
        finished: Arc::clone(&finished),
        child_done: Arc::clone(&child_done),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(hang)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_request_deadline_before_headers_on_every_shape(
        &client,
        &started,
        &finished,
        &child_done,
    )
    .await;
    drop(client);
    server.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = WaitAfterFirst {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_deadline_cancels_live_server_stream(&client, &left).await;
    drop(client);
    server.abort();

    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = BidiWaitAfterFirst {
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_deadline_cancels_bidi_after_close(&client, &left).await;
    drop(client);
    server.abort();

    let drained = Arc::new(AtomicUsize::new(0));
    let left = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = duplex_pair();
    let svc = ClientStreamWaitAfterClose {
        drained: Arc::clone(&drained),
        left: Arc::clone(&left),
    };
    let server = tokio::spawn(async move {
        GreeterServer::new(svc)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    );
    assert_deadline_cancels_client_stream_after_close(&client, &drained, &left).await;
    drop(client);
    server.abort();
}

/// Refuses a request that was not gzipped.
struct GzipProbe;

fn require_gzip_unary(request: Request<HelloRequest>) -> Result<HelloRequest, Status> {
    if !request.compressed() {
        return Err(Status::invalid_argument("expected gzip"));
    }
    let encoding = request.encoding().map(str::to_owned);
    let (msg, parts) = request.into_message_and_parts();
    if !parts.compressed() {
        return Err(Status::internal("parts dropped Compressed-Flag"));
    }
    if encoding.as_deref() != Some("gzip") || parts.encoding() != Some("gzip") {
        return Err(Status::internal(format!(
            "gzip encoding {:?} parts {:?}",
            encoding,
            parts.encoding()
        )));
    }
    Ok(msg)
}

async fn require_gzip_inbound(
    request: Request<pbrs_grpc::Streaming<HelloRequest>>,
) -> Result<HelloReply, Status> {
    if request.compressed() {
        return Err(Status::internal(
            "streaming Request.compressed is the unary first-frame flag",
        ));
    }
    if request.encoding() != Some("gzip") {
        return Err(Status::invalid_argument("expected gzip"));
    }
    let mut stream = request.into_inner();
    let framed = stream
        .next_framed()
        .await?
        .ok_or_else(|| Status::internal("empty gzip stream"))?;
    if !framed.compressed {
        return Err(Status::internal(
            "gzip frame missing per-message Compressed-Flag",
        ));
    }
    Ok(common::reply(common::name_of_request(&framed.message)))
}

impl pbrs_grpc::Greeter for GzipProbe {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let msg = require_gzip_unary(request)?;
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(require_gzip_inbound(request).await?))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let msg = require_gzip_unary(request)?;
        let name = common::name_of_request(&msg);
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(common::reply(name)).await.ok();
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let reply = require_gzip_inbound(request).await?;
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(reply).await.ok();
        }));
        Ok(Response::new(stream))
    }
}

#[tokio::test]
async fn a_prefixed_user_agent_is_sent() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_prefixed_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_prefixed_user_agent(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_prefixed_user_agent_is_sent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_prefixed_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_prefixed_user_agent(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_prefixed_user_agent_is_sent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_prefixed_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_prefixed_user_agent(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_prefixed_user_agent_is_sent() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_prefixed_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_prefixed_user_agent(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_prefixed_user_agent_is_sent() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_prefixed_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_prefixed_user_agent(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn metadata_cannot_override_the_kernel_user_agent() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_user_agent_not_overridable(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn tls_metadata_cannot_override_the_kernel_user_agent() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_user_agent_not_overridable(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_metadata_cannot_override_the_kernel_user_agent() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_user_agent_not_overridable(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_metadata_cannot_override_the_kernel_user_agent() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_user_agent_not_overridable(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_metadata_cannot_override_the_kernel_user_agent() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(require_kernel_user_agent)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_user_agent_not_overridable(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

fn gzip_router() -> Router {
    Router::new()
        .add_service(GreeterServer::new(Echo))
        .send_compressed()
}

async fn assert_router_gzips(ch: Channel) {
    assert_server_gzip_every_shape(&GreeterClient::new(ch)).await;
}

#[tokio::test]
async fn the_server_gzips_when_configured_and_the_client_accepts() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        gzip_router().serve_listener(listener).await.ok();
    });
    assert_router_gzips(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn tls_server_gzips_when_configured_and_the_client_accepts() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        gzip_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_router_gzips(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_server_gzips_when_configured_and_the_client_accepts() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        gzip_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_router_gzips(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_server_gzips_when_configured_and_the_client_accepts() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        gzip_router().serve_unix(sock).await.ok();
    });
    assert_router_gzips(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_server_gzips_when_configured_and_the_client_accepts() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        gzip_router().serve_connection(server_io).await.ok();
    });
    assert_router_gzips(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn server_send_compressed_gzips_streaming_send() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    let reply = GreeterClient::new(channel(addr).await)
        .server_hello(Request::new(req("ada")))
        .await
        .expect("stream");
    assert_eq!(
        reply.encoding(),
        Some("gzip"),
        "received stream must surface grpc-encoding"
    );
    let mut stream = reply.into_inner();
    let framed = stream.next_framed().await.expect("frame").expect("message");
    assert!(
        framed.compressed,
        "Server::send_compressed must gzip identity StreamSender::send frames"
    );
    assert_eq!(name_of(&framed.message), "ada");
    task.abort();
}

async fn assert_default_identity(ch: Channel) {
    assert_identity_encoding_every_shape(&GreeterClient::new(ch)).await;
}

#[tokio::test]
async fn identity_streaming_send_does_not_advertise_gzip() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    assert_default_identity(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn tls_identity_streaming_send_does_not_advertise_gzip() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_default_identity(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_identity_streaming_send_does_not_advertise_gzip() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_default_identity(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_identity_streaming_send_does_not_advertise_gzip() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    assert_default_identity(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_identity_streaming_send_does_not_advertise_gzip() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_default_identity(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

async fn assert_client_gzips(ch: Channel) {
    gzip_every_shape(&GreeterClient::new(ch)).await;
}

#[tokio::test]
async fn the_client_gzips_when_configured() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_client_gzips(channel(addr).await.send_compressed()).await;
    task.abort();
}

#[tokio::test]
async fn tls_client_gzips_when_configured() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_client_gzips(tls_channel(addr).await.send_compressed()).await;
    task.abort();
}

#[tokio::test]
async fn mtls_client_gzips_when_configured() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_client_gzips(tls_channel_with(addr, client_tls).await.send_compressed()).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_client_gzips_when_configured() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe).serve_unix(sock).await.ok();
    });
    assert_client_gzips(unix_channel(&path).await.send_compressed()).await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_gzips_when_configured() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_client_gzips(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .send_compressed(),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_set_compress() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(|call: &mut Outgoing<'_>| {
        call.set_compress(true);
        Ok(())
    });
    gzip_every_shape(&client).await;
    task.abort();
}

fn interceptor_set_compress(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.set_compress(true);
    Ok(())
}

#[tokio::test]
async fn a_tls_client_interceptor_can_set_compress() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await).intercept(interceptor_set_compress);
    gzip_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_can_set_compress() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(interceptor_set_compress);
    gzip_every_shape(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_can_set_compress() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(GzipProbe).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await).intercept(interceptor_set_compress);
    gzip_every_shape(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_can_set_compress() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(GzipProbe)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(interceptor_set_compress);
    gzip_every_shape(&client).await;
    server.abort();
}

async fn assert_request_gzip_opt_out(ch: Channel) {
    let client = GreeterClient::new(ch);
    let mut request = Request::new(req("ada"));
    request.set_compress(false);
    let reply = client.say_hello(request).await.expect("opt out");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let mut stream_req = Request::new(());
    stream_req.set_compress(false);
    let (tx, call) = client.client_hello(stream_req);
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("opt-out stream");
    assert_eq!(name_of(reply.get_ref()), "gzip");

    let mut server_req = Request::new(req("ada"));
    server_req.set_compress(false);
    let mut stream = client
        .server_hello(server_req)
        .await
        .expect("opt-out server-stream")
        .into_inner();
    let reply = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "ada");

    let mut bidi_req = Request::new(());
    bidi_req.set_compress(false);
    let (tx, call) = client.stream_hello(bidi_req);
    assert!(!tx.compress(), "opt-out must stamp StreamSender");
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("opt-out bidi").into_inner();
    let reply = inbound.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "gzip");
}

#[tokio::test]
async fn a_request_can_opt_out_of_channel_send_compressed() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_request_gzip_opt_out(channel(addr).await.send_compressed()).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_request_can_opt_out_of_channel_send_compressed() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_request_gzip_opt_out(tls_channel(addr).await.send_compressed()).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_request_can_opt_out_of_channel_send_compressed() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_request_gzip_opt_out(tls_channel_with(addr, client_tls).await.send_compressed()).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_request_can_opt_out_of_channel_send_compressed() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip).serve_unix(sock).await.ok();
    });
    assert_request_gzip_opt_out(unix_channel(&path).await.send_compressed()).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_request_can_opt_out_of_channel_send_compressed() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_request_gzip_opt_out(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .send_compressed(),
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_opt_out_of_channel_send_compressed() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await.send_compressed())
        .intercept(interceptor_opt_out_compress);
    assert_sees_gzip_opt_out(&client).await;
    task.abort();
}

fn interceptor_opt_out_compress(call: &mut Outgoing<'_>) -> Result<(), Status> {
    call.set_compress(false);
    Ok(())
}

async fn assert_sees_gzip_opt_out(client: &GreeterClient) {
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("opt out");
    assert_eq!(name_of(reply.get_ref()), "ada");

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("opt-out stream");
    assert_eq!(name_of(reply.get_ref()), "gzip");

    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("opt-out server-stream")
        .into_inner();
    let reply = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "ada");

    let (tx, call) = client.stream_hello(Request::new(()));
    assert!(
        !tx.compress(),
        "interceptor opt-out must stamp StreamSender"
    );
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("opt-out bidi").into_inner();
    let reply = inbound.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "gzip");
}

#[tokio::test]
async fn a_tls_client_interceptor_can_opt_out_of_channel_send_compressed() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await.send_compressed())
        .intercept(interceptor_opt_out_compress);
    assert_sees_gzip_opt_out(&client).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_can_opt_out_of_channel_send_compressed() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await.send_compressed())
        .intercept(interceptor_opt_out_compress);
    assert_sees_gzip_opt_out(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_can_opt_out_of_channel_send_compressed() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await.send_compressed())
        .intercept(interceptor_opt_out_compress);
    assert_sees_gzip_opt_out(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_can_opt_out_of_channel_send_compressed() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io")
            .send_compressed(),
    )
    .intercept(interceptor_opt_out_compress);
    assert_sees_gzip_opt_out(&client).await;
    server.abort();
}

#[tokio::test]
async fn a_client_interceptor_can_gzip_request_streams() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = GreeterClient::new(channel(addr).await).intercept(interceptor_set_compress);
    gzip_request_streams(&client).await;
    task.abort();
}

async fn gzip_request_streams(client: &GreeterClient) {
    let (tx, call) = client.client_hello(Request::new(()));
    assert!(tx.compress(), "interceptor must stamp StreamSender");
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("gzip stream");
    assert_eq!(name_of(reply.get_ref()), "gzip");

    let (tx, call) = client.stream_hello(Request::new(()));
    assert!(tx.compress(), "interceptor must stamp bidi StreamSender");
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("gzip bidi").into_inner();
    let reply = inbound.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "gzip");
}

#[tokio::test]
async fn a_tls_client_interceptor_can_gzip_request_streams() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client = GreeterClient::new(tls_channel(addr).await).intercept(interceptor_set_compress);
    gzip_request_streams(&client).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_client_interceptor_can_gzip_request_streams() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = GreeterClient::new(tls_channel_with(addr, client_tls).await)
        .intercept(interceptor_set_compress);
    gzip_request_streams(&client).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_client_interceptor_can_gzip_request_streams() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip).serve_unix(sock).await.ok();
    });
    let client = GreeterClient::new(unix_channel(&path).await).intercept(interceptor_set_compress);
    gzip_request_streams(&client).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_client_interceptor_can_gzip_request_streams() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let client = GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .intercept(interceptor_set_compress);
    gzip_request_streams(&client).await;
    server.abort();
}

struct OptOutGzip;

fn overlay_gzips<T>(request: &Request<T>) -> Result<(), Status> {
    if !request.compresses_outbound() {
        return Err(Status::internal("request overlay should gzip"));
    }
    Ok(())
}

fn identity_reply(name: &str) -> Response<HelloReply> {
    let mut resp = Response::new(common::reply(name));
    resp.set_compress(false);
    resp
}

fn identity_stream(name: String) -> Response<pbrs_grpc::Streaming<HelloReply>> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(1);
    drop(tokio::spawn(async move {
        tx.send(common::reply(name)).await.ok();
    }));
    let mut resp = Response::new(stream);
    resp.set_compress(false);
    resp
}

impl pbrs_grpc::Greeter for OptOutGzip {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        overlay_gzips(&request)?;
        Ok(identity_reply(&common::name_of_request(request.get_ref())))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        overlay_gzips(&request)?;
        Ok(identity_reply("ada"))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        overlay_gzips(&request)?;
        Ok(identity_stream(common::name_of_request(request.get_ref())))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        overlay_gzips(&request)?;
        Ok(identity_stream("ada".into()))
    }
}

fn interceptor_require_server_gzip(rpc: &mut Rpc) -> Result<(), Status> {
    if !rpc.compresses_outbound() {
        return Err(Status::internal("server overlay should gzip"));
    }
    Ok(())
}

async fn assert_handler_gzip_opt_out(ch: Channel) {
    assert_identity_encoding_every_shape(&GreeterClient::new(ch)).await;
}

#[tokio::test]
async fn a_handler_can_opt_out_of_server_send_compressed() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(OptOutGzip)
            .send_compressed()
            .intercept(interceptor_require_server_gzip)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_handler_gzip_opt_out(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_handler_can_opt_out_of_server_send_compressed() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(OptOutGzip)
            .send_compressed()
            .intercept(interceptor_require_server_gzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_handler_gzip_opt_out(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_handler_can_opt_out_of_server_send_compressed() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(OptOutGzip)
            .send_compressed()
            .intercept(interceptor_require_server_gzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_handler_gzip_opt_out(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_handler_can_opt_out_of_server_send_compressed() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(OptOutGzip)
            .send_compressed()
            .intercept(interceptor_require_server_gzip)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_handler_gzip_opt_out(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_handler_can_opt_out_of_server_send_compressed() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(OptOutGzip)
            .send_compressed()
            .intercept(interceptor_require_server_gzip)
            .serve_connection(server_io)
            .await
            .ok();
    });
    assert_handler_gzip_opt_out(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

struct SeesGzip;

fn sees_gzip_unary(request: Request<HelloRequest>) -> Result<HelloRequest, Status> {
    if !request.accepts_gzip() {
        return Err(Status::internal("kernel client advertises gzip"));
    }
    if request.compresses_outbound() {
        return Err(Status::internal("default server does not gzip"));
    }
    let encoding = request.encoding().map(str::to_owned);
    let compressed = request.compressed();
    let (msg, parts) = request.into_message_and_parts();
    if !parts.accepts_gzip() {
        return Err(Status::internal("parts dropped accepts_gzip"));
    }
    if parts.compresses_outbound() {
        return Err(Status::internal("parts invented compresses_outbound"));
    }
    if parts.encoding() != encoding.as_deref() {
        return Err(Status::internal(format!(
            "parts encoding {:?}",
            parts.encoding()
        )));
    }
    if parts.compressed() != compressed {
        return Err(Status::internal("parts dropped Compressed-Flag"));
    }
    match (encoding.as_deref(), compressed) {
        (None, false) | (Some("gzip"), true) => {}
        other => {
            return Err(Status::internal(format!("unary gzip facts {other:?}")));
        }
    }
    Ok(msg)
}

async fn sees_gzip_inbound(
    request: Request<pbrs_grpc::Streaming<HelloRequest>>,
) -> Result<HelloReply, Status> {
    if request.compressed() {
        return Err(Status::internal(
            "streaming Request.compressed is the unary first-frame flag",
        ));
    }
    if !request.accepts_gzip() {
        return Err(Status::internal("kernel client advertises gzip"));
    }
    if request.compresses_outbound() {
        return Err(Status::internal("default server does not gzip"));
    }
    let encoding = request.encoding().map(str::to_owned);
    let mut stream = request.into_inner();
    match encoding.as_deref() {
        None => {
            if let Some(framed) = stream.next_framed().await? {
                if framed.compressed {
                    return Err(Status::internal("identity frame was gzipped"));
                }
            }
        }
        Some("gzip") => {
            let framed = stream
                .next_framed()
                .await?
                .ok_or_else(|| Status::internal("empty gzip stream"))?;
            if !framed.compressed {
                return Err(Status::internal(
                    "gzip frame missing per-message Compressed-Flag",
                ));
            }
        }
        other => {
            return Err(Status::internal(format!("stream encoding {other:?}")));
        }
    }
    let mut reply = HelloReply::new();
    reply.set_message("gzip");
    Ok(reply)
}

impl pbrs_grpc::Greeter for SeesGzip {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let msg = sees_gzip_unary(request)?;
        Ok(Response::new(common::reply(common::name_of_request(&msg))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(sees_gzip_inbound(request).await?))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let msg = sees_gzip_unary(request)?;
        let name = common::name_of_request(&msg);
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(common::reply(name)).await.ok();
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let reply = sees_gzip_inbound(request).await?;
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(reply).await.ok();
        }));
        Ok(Response::new(stream))
    }
}

fn interceptor_require_accepts_gzip(rpc: &mut Rpc) -> Result<(), Status> {
    if !rpc.accepts_gzip() {
        return Err(Status::internal("rpc accepts_gzip"));
    }
    Ok(())
}

async fn assert_handler_sees_gzip_headers(identity: &GreeterClient, gzip: &GreeterClient) {
    assert!(!identity.compresses_outbound());
    assert!(gzip.compresses_outbound());

    let reply = identity
        .say_hello(Request::new(req("ada")))
        .await
        .expect("identity unary");
    assert!(
        reply.encoding().is_none(),
        "identity unary reply must not invent grpc-encoding"
    );
    assert_eq!(name_of(reply.get_ref()), "ada");

    let reply = gzip
        .say_hello(Request::new(req("ada")))
        .await
        .expect("gzip unary");
    assert!(
        reply.encoding().is_none(),
        "SeesGzip does not gzip the reply"
    );
    assert_eq!(name_of(reply.get_ref()), "ada");

    let (tx, call) = identity.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let reply = call.await.expect("identity stream");
    assert!(
        reply.encoding().is_none(),
        "identity client-stream reply must not invent grpc-encoding"
    );
    assert_eq!(name_of(reply.get_ref()), "gzip");

    let (tx, call) = gzip.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send gzip");
    tx.close();
    let reply = call.await.expect("gzip stream");
    assert_eq!(name_of(reply.get_ref()), "gzip");

    let mut stream = identity
        .server_hello(Request::new(req("ada")))
        .await
        .expect("identity server-stream")
        .into_inner();
    let reply = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "ada");

    let mut stream = gzip
        .server_hello(Request::new(req("ada")))
        .await
        .expect("gzip server-stream")
        .into_inner();
    let reply = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "ada");

    let (tx, call) = identity.stream_hello(Request::new(()));
    assert!(!tx.compress());
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("identity bidi").into_inner();
    let reply = inbound.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "gzip");

    let (tx, call) = gzip.stream_hello(Request::new(()));
    assert!(tx.compress(), "channel overlay must stamp StreamSender");
    tx.send(req("ada")).await.expect("send gzip");
    tx.close();
    let mut inbound = call.await.expect("gzip bidi").into_inner();
    let reply = inbound.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&reply), "gzip");
}

#[tokio::test]
async fn a_handler_sees_gzip_headers_and_the_unary_compressed_flag() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .intercept(interceptor_require_accepts_gzip)
            .serve_listener(listener)
            .await
            .ok();
    });
    let identity = GreeterClient::new(channel(addr).await);
    let gzip = GreeterClient::new(channel(addr).await.send_compressed());
    assert_handler_sees_gzip_headers(&identity, &gzip).await;
    task.abort();
}

#[tokio::test]
async fn a_tls_handler_sees_gzip_headers_and_the_unary_compressed_flag() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .intercept(interceptor_require_accepts_gzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let identity = GreeterClient::new(tls_channel(addr).await);
    let gzip = GreeterClient::new(tls_channel(addr).await.send_compressed());
    assert_handler_sees_gzip_headers(&identity, &gzip).await;
    task.abort();
}

#[tokio::test]
async fn an_mtls_handler_sees_gzip_headers_and_the_unary_compressed_flag() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .intercept(interceptor_require_accepts_gzip)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let identity = GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await);
    let gzip = GreeterClient::new(tls_channel_with(addr, client_tls).await.send_compressed());
    assert_handler_sees_gzip_headers(&identity, &gzip).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn a_unix_handler_sees_gzip_headers_and_the_unary_compressed_flag() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .intercept(interceptor_require_accepts_gzip)
            .serve_unix(sock)
            .await
            .ok();
    });
    let identity = GreeterClient::new(unix_channel(&path).await);
    let gzip = GreeterClient::new(unix_channel(&path).await.send_compressed());
    assert_handler_sees_gzip_headers(&identity, &gzip).await;
    task.abort();
}

#[tokio::test]
async fn a_from_io_handler_sees_gzip_headers_and_the_unary_compressed_flag() {
    let (identity_io, identity_server) = duplex_pair();
    let identity_task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .intercept(interceptor_require_accepts_gzip)
            .serve_connection(identity_server)
            .await
            .ok();
    });
    let identity = GreeterClient::new(
        Channel::from_io(identity_io, "localhost")
            .await
            .expect("from_io identity"),
    );
    let (gzip_io, gzip_server) = duplex_pair();
    let gzip_task = tokio::spawn(async move {
        GreeterServer::new(SeesGzip)
            .intercept(interceptor_require_accepts_gzip)
            .serve_connection(gzip_server)
            .await
            .ok();
    });
    let gzip = GreeterClient::new(
        Channel::from_io(gzip_io, "localhost")
            .await
            .expect("from_io gzip")
            .send_compressed(),
    );
    assert_handler_sees_gzip_headers(&identity, &gzip).await;
    identity_task.abort();
    gzip_task.abort();
}

#[test]
fn server_and_router_config_is_readable_and_cloneable() {
    let svc = GreeterServer::new(Echo).timeout(Duration::from_secs(3));
    assert_eq!(
        svc.server_config().rpc_timeout(),
        Some(Duration::from_secs(3))
    );
    assert_eq!(svc.rpc_timeout(), Some(Duration::from_secs(3)));
    assert!(!svc.compresses_outbound());
    assert!(svc.clone().send_compressed().compresses_outbound());
    let server = Server::new(svc.clone()).timeout(Duration::from_secs(9));
    assert_eq!(
        server.server_config().rpc_timeout(),
        Some(Duration::from_secs(9))
    );
    assert_eq!(server.rpc_timeout(), Some(Duration::from_secs(9)));
    assert_eq!(
        server.clone().server_config().rpc_timeout(),
        Some(Duration::from_secs(9))
    );
    let router = Router::new()
        .add_service(svc)
        .timeout(Duration::from_secs(2));
    assert_eq!(
        router.server_config().rpc_timeout(),
        Some(Duration::from_secs(2))
    );
    assert_eq!(router.rpc_timeout(), Some(Duration::from_secs(2)));
    assert!(!router.compresses_outbound());
    assert!(router.clone().send_compressed().compresses_outbound());
    assert_eq!(
        router.clone().server_config().rpc_timeout(),
        Some(Duration::from_secs(2))
    );
}

async fn assert_official_cases(client: &TestServiceClient, cases: &[&str]) {
    for case in cases {
        pbrs_grpc::run_case(client, case)
            .await
            .unwrap_or_else(|err| panic!("{case}: {err}"));
    }
}

const OFFICIAL_COMPRESSED_CASES: &[&str] = &[
    "client_compressed_unary",
    "server_compressed_unary",
    "client_compressed_streaming",
    "server_compressed_streaming",
];

const OFFICIAL_UNCOMPRESSED_CASES: &[&str] = &[
    "empty_unary",
    "large_unary",
    "client_streaming",
    "server_streaming",
    "ping_pong",
    "empty_stream",
    "cancel_after_begin",
    "cancel_after_first_response",
    "timeout_on_sleeping_server",
    "custom_metadata",
    "status_code_and_message",
    "special_status_message",
    "unimplemented_method",
    "unimplemented_service",
];

#[tokio::test]
async fn official_compressed_interop_cases_pass_against_the_kernel_server() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_official_cases(
        &TestServiceClient::new(channel(addr).await),
        OFFICIAL_COMPRESSED_CASES,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_official_compressed_interop_cases_pass_against_the_kernel_server() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_official_cases(
        &TestServiceClient::new(tls_channel(addr).await),
        OFFICIAL_COMPRESSED_CASES,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_official_compressed_interop_cases_pass_against_the_kernel_server() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_official_cases(
        &TestServiceClient::new(tls_channel_with(addr, client_tls).await),
        OFFICIAL_COMPRESSED_CASES,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_official_compressed_interop_cases_pass_against_the_kernel_server() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_official_cases(
        &TestServiceClient::new(unix_channel(&path).await),
        OFFICIAL_COMPRESSED_CASES,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_official_compressed_interop_cases_pass_against_the_kernel_server() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_official_cases(&TestServiceClient::new(channel), OFFICIAL_COMPRESSED_CASES).await;
    server.abort();
}

#[tokio::test]
async fn official_uncompressed_interop_cases_pass_against_the_kernel_server() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_official_cases(
        &TestServiceClient::new(channel(addr).await),
        OFFICIAL_UNCOMPRESSED_CASES,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_official_uncompressed_interop_cases_pass_against_the_kernel_server() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_official_cases(
        &TestServiceClient::new(tls_channel(addr).await),
        OFFICIAL_UNCOMPRESSED_CASES,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_official_uncompressed_interop_cases_pass_against_the_kernel_server() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_official_cases(
        &TestServiceClient::new(tls_channel_with(addr, client_tls).await),
        OFFICIAL_UNCOMPRESSED_CASES,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_official_uncompressed_interop_cases_pass_against_the_kernel_server() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_official_cases(
        &TestServiceClient::new(unix_channel(&path).await),
        OFFICIAL_UNCOMPRESSED_CASES,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_official_uncompressed_interop_cases_pass_against_the_kernel_server() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_official_cases(
        &TestServiceClient::new(channel),
        OFFICIAL_UNCOMPRESSED_CASES,
    )
    .await;
    server.abort();
}

const TRAILER_BIN: &str = "x-grpc-test-echo-trailing-bin";
const HEADER_ASCII: &str = "x-grpc-test-echo-initial";

struct TrailerEcho;

fn stamp_ok_trailers<T>(resp: &mut Response<T>) {
    resp.metadata_mut()
        .insert(HEADER_ASCII, "ok")
        .expect("ascii");
    resp.trailers_mut()
        .insert_bin(TRAILER_BIN, [0x00, 0x01])
        .expect("bin");
}

fn named_reply(name: String) -> HelloReply {
    let mut reply = HelloReply::new();
    reply.set_message(name);
    reply
}

fn trailer_stream(name: String) -> Response<pbrs_grpc::Streaming<HelloReply>> {
    let (tx, stream) = pbrs_grpc::Streaming::channel(4);
    drop(tokio::spawn(async move {
        tx.send(named_reply(name)).await.ok();
    }));
    let mut resp = Response::new(stream);
    stamp_ok_trailers(&mut resp);
    resp
}

impl Greeter for TrailerEcho {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        let mut resp = Response::new(named_reply(name));
        stamp_ok_trailers(&mut resp);
        Ok(resp)
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut resp = Response::new(named_reply("ada".into()));
        stamp_ok_trailers(&mut resp);
        Ok(resp)
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        Ok(trailer_stream(name))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Ok(trailer_stream("ada".into()))
    }
}

struct TrailerFail;

impl Greeter for TrailerFail {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("trailers"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("trailers"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.fail(Status::not_found("gone")).await;
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let (tx, stream) = pbrs_grpc::Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.fail(Status::not_found("gone")).await;
        }));
        Ok(Response::new(stream))
    }
}

fn assert_ok_headers_and_bin_trailers<T>(resp: &Response<T>, shape: &str) {
    assert_eq!(
        resp.metadata().get(HEADER_ASCII),
        Some("ok"),
        "{shape} initial header"
    );
    assert!(
        resp.metadata().get_bin(TRAILER_BIN).is_none(),
        "{shape} -bin trailer must not appear as headers"
    );
}

async fn assert_ok_path_custom_bin_trailers(client: &GreeterClient) {
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_ok_headers_and_bin_trailers(&resp, "unary");
    assert_eq!(
        resp.trailers().get_bin(TRAILER_BIN).as_deref(),
        Some([0x00, 0x01].as_slice()),
        "unary trailers"
    );

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let resp = call.await.expect("client-stream");
    assert_ok_headers_and_bin_trailers(&resp, "client-stream");
    assert_eq!(
        resp.trailers().get_bin(TRAILER_BIN).as_deref(),
        Some([0x00, 0x01].as_slice()),
        "client-stream trailers"
    );

    let resp = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream");
    assert_ok_headers_and_bin_trailers(&resp, "server-stream");
    let mut stream = resp.into_inner();
    // Do not drain first: trailers() must wait for EOS itself.
    let trailers = stream.trailers().await.expect("wait");
    assert_eq!(
        trailers.get_bin(TRAILER_BIN).as_deref(),
        Some([0x00, 0x01].as_slice()),
        "server-stream trailers without drain"
    );

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let resp = call.await.expect("bidi");
    assert_ok_headers_and_bin_trailers(&resp, "bidi");
    let mut inbound = resp.into_inner();
    let trailers = inbound.trailers().await.expect("bidi wait");
    assert_eq!(
        trailers.get_bin(TRAILER_BIN).as_deref(),
        Some([0x00, 0x01].as_slice()),
        "bidi trailers without drain"
    );

    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("server-stream drain")
        .into_inner();
    let msg = stream.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&msg), "ada");
    assert!(stream.message().await.expect("end").is_none());
    let trailers = stream.trailers().await.expect("after drain");
    assert_eq!(
        trailers.get_bin(TRAILER_BIN).as_deref(),
        Some([0x00, 0x01].as_slice()),
        "server-stream trailers after drain"
    );

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi drain").into_inner();
    let msg = inbound.message().await.expect("msg").expect("item");
    assert_eq!(name_of(&msg), "ada");
    assert!(inbound.message().await.expect("end").is_none());
    let trailers = inbound.trailers().await.expect("bidi after drain");
    assert_eq!(
        trailers.get_bin(TRAILER_BIN).as_deref(),
        Some([0x00, 0x01].as_slice()),
        "bidi trailers after drain"
    );
}

async fn assert_streaming_trailers_surface_a_trailing_status(client: &GreeterClient) {
    let mut stream = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect("headers")
        .into_inner();
    let err = stream.trailers().await.expect_err("status");
    assert_eq!(err.code(), Code::NotFound, "{err}");

    let (tx, call) = client.stream_hello(Request::new(()));
    drop(tx);
    let mut inbound = call.await.expect("bidi headers").into_inner();
    let err = inbound.trailers().await.expect_err("bidi status");
    assert_eq!(err.code(), Code::NotFound, "{err}");
}

#[tokio::test]
async fn tls_ok_path_custom_bin_trailers_not_headers() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(TrailerEcho)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_ok_path_custom_bin_trailers(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn mtls_ok_path_custom_bin_trailers_not_headers() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(TrailerEcho)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_ok_path_custom_bin_trailers(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_ok_path_custom_bin_trailers_not_headers() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(TrailerEcho).serve_unix(sock).await.ok();
    });
    assert_ok_path_custom_bin_trailers(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn from_io_ok_path_custom_bin_trailers_not_headers() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(TrailerEcho)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_ok_path_custom_bin_trailers(&GreeterClient::new(channel)).await;
    server.abort();
}

#[tokio::test]
async fn tls_streaming_trailers_surface_a_trailing_status() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(TrailerFail)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_streaming_trailers_surface_a_trailing_status(&GreeterClient::new(
        tls_channel(addr).await,
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_streaming_trailers_surface_a_trailing_status() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(TrailerFail)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_streaming_trailers_surface_a_trailing_status(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_streaming_trailers_surface_a_trailing_status() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(TrailerFail).serve_unix(sock).await.ok();
    });
    assert_streaming_trailers_surface_a_trailing_status(&GreeterClient::new(
        unix_channel(&path).await,
    ))
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_streaming_trailers_surface_a_trailing_status() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(TrailerFail)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_streaming_trailers_surface_a_trailing_status(&GreeterClient::new(channel)).await;
    server.abort();
}

async fn assert_client_encode_cap_every_shape(client: &GreeterClient) {
    let oversize = req(&"x".repeat(64));
    let err = client
        .say_hello(Request::new(oversize.clone()))
        .await
        .expect_err("unary encode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    match client.server_hello(Request::new(oversize.clone())).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(_) => panic!("server-stream client encode cap must fail before headers"),
    }
    let (tx, call) = client.client_hello(Request::new(()));
    let err = tx
        .send(oversize.clone())
        .await
        .expect_err("client-stream send");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    drop(call);
    let (tx, call) = client.stream_hello(Request::new(()));
    let err = tx.send(oversize).await.expect_err("bidi send");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    drop(call);
}

async fn assert_client_decode_cap_every_shape(client: &GreeterClient) {
    let oversize = req(&"x".repeat(64));
    let err = client
        .say_hello(Request::new(oversize.clone()))
        .await
        .expect_err("unary decode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    match client.server_hello(Request::new(oversize.clone())).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("server-stream client decode cap must fail"),
        },
    }
    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(oversize.clone()).await.expect("send");
    tx.close();
    let err = call.await.expect_err("client-stream decode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(oversize).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("bidi client decode cap must fail"),
        },
    }
}

async fn assert_client_message_caps(channel: Channel) {
    assert_client_encode_cap_every_shape(
        &GreeterClient::new(channel.clone()).max_encoding_message_size(16),
    )
    .await;
    assert_client_decode_cap_every_shape(
        &GreeterClient::new(channel).max_decoding_message_size(16),
    )
    .await;
}

#[tokio::test]
async fn tls_client_message_caps_are_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_client_message_caps(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_client_message_caps_are_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_client_message_caps(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_client_message_caps_are_resource_exhausted() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    assert_client_message_caps(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_message_caps_are_resource_exhausted() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_client_message_caps(channel).await;
    server.abort();
}

fn pool_cfg() -> ChannelConfig {
    ChannelConfig::new().connections(4)
}

async fn assert_pool_serves(channel: Channel) {
    let client = GreeterClient::new(channel);
    echo_every_shape(&client, None).await;
    let mut hs = Vec::new();
    for i in 0..16u32 {
        let c = client.clone();
        hs.push(tokio::spawn(async move {
            let label = format!("n{i}");
            let resp = c.say_hello(Request::new(req(&label))).await.expect("unary");
            name_of(&resp.into_inner())
        }));
    }
    let mut got = Vec::new();
    for h in hs {
        got.push(h.await.expect("join"));
    }
    got.sort();
    let mut want: Vec<String> = (0..16u32).map(|i| format!("n{i}")).collect();
    want.sort();
    assert_eq!(got, want);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tls_connection_pool_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_pool_serves(tls_channel_cfg(addr, client_tls, pool_cfg()).await).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mtls_connection_pool_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_pool_serves(tls_channel_cfg(addr, client_tls, pool_cfg()).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unix_connection_pool_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    assert_pool_serves(unix_channel_with(&path, pool_cfg()).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_pool_config_is_still_one_duplex() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io_with(client_io, "localhost", pool_cfg())
        .await
        .expect("from_io");
    echo_every_shape(&GreeterClient::new(channel), None).await;
    server.abort();
}

fn pool_against_cap() -> ChannelConfig {
    refuse_connect_cfg().connections(2)
}

#[tokio::test]
async fn connection_pool_is_refused_when_the_cap_is_hit() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_listener(listener)
            .await
            .ok();
    });
    let first = channel(addr).await;
    assert_cap_refuses_then_echo(
        first,
        Channel::connect_with(addr, pool_against_cap()).await,
        channel(addr),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_connection_pool_is_refused_when_the_cap_is_hit() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let first = tls_channel_with(addr, client_tls.clone()).await;
    assert_cap_refuses_then_echo(
        first,
        Channel::connect_tls_with(addr, pool_against_cap(), client_tls.clone()).await,
        tls_channel_with(addr, client_tls),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_connection_pool_is_refused_when_the_cap_is_hit() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let first = tls_channel_with(addr, client_tls.clone()).await;
    assert_cap_refuses_then_echo(
        first,
        Channel::connect_tls_with(addr, pool_against_cap(), client_tls.clone()).await,
        tls_channel_with(addr, client_tls),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_connection_pool_is_refused_when_the_cap_is_hit() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .max_concurrent_connections(1)
            .serve_unix(sock)
            .await
            .ok();
    });
    let first = unix_channel(&path).await;
    assert_cap_refuses_then_echo(
        first,
        Channel::connect_unix_with(&path, pool_against_cap()).await,
        unix_channel(&path),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_raw_status_details_round_trip() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(RichFail)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_raw_status_details_every_shape(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn mtls_raw_status_details_round_trip() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(RichFail)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_raw_status_details_every_shape(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_raw_status_details_round_trip() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(RichFail).serve_unix(sock).await.ok();
    });
    assert_raw_status_details_every_shape(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn from_io_raw_status_details_round_trip() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(RichFail)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_raw_status_details_every_shape(&GreeterClient::new(channel)).await;
    server.abort();
}

fn dial_encode_cap() -> ChannelConfig {
    ChannelConfig::new().max_encoding_message_size(16)
}

fn dial_decode_cap() -> ChannelConfig {
    ChannelConfig::new().max_decoding_message_size(16)
}

async fn assert_dial_message_caps(encode: Channel, decode: Channel) {
    assert_client_encode_cap_every_shape(&GreeterClient::new(encode)).await;
    assert_client_decode_cap_every_shape(&GreeterClient::new(decode)).await;
}

#[tokio::test]
async fn channel_config_message_caps_are_resource_exhausted() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    assert_dial_message_caps(
        channel_cfg(addr, dial_encode_cap()).await,
        channel_cfg(addr, dial_decode_cap()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_channel_config_message_caps_are_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_dial_message_caps(
        tls_channel_cfg(addr, client_tls.clone(), dial_encode_cap()).await,
        tls_channel_cfg(addr, client_tls, dial_decode_cap()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_channel_config_message_caps_are_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_dial_message_caps(
        tls_channel_cfg(addr, client_tls.clone(), dial_encode_cap()).await,
        tls_channel_cfg(addr, client_tls, dial_decode_cap()).await,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_channel_config_message_caps_are_resource_exhausted() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    assert_dial_message_caps(
        unix_channel_with(&path, dial_encode_cap()).await,
        unix_channel_with(&path, dial_decode_cap()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_channel_config_message_caps_are_resource_exhausted() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_connection(s1).await.ok();
    });
    let encode = Channel::from_io_with(c1, "localhost", dial_encode_cap())
        .await
        .expect("from_io encode");
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_connection(s2).await.ok();
    });
    let decode = Channel::from_io_with(c2, "localhost", dial_decode_cap())
        .await
        .expect("from_io decode");
    assert_dial_message_caps(encode, decode).await;
    server1.abort();
    server2.abort();
}

const GREETER_UNARY: &str = "/helloworld.Greeter/SayHello";
const GREETER_SERVER: &str = "/helloworld.Greeter/ServerHello";
const GREETER_CLIENT: &str = "/helloworld.Greeter/ClientHello";
const GREETER_BIDI: &str = "/helloworld.Greeter/StreamHello";

async fn assert_channel_encode_cap_every_shape(channel: &Channel) {
    let oversize = req(&"x".repeat(64));
    let err = channel
        .unary::<HelloRequest, HelloReply>(GREETER_UNARY, Request::new(oversize.clone()))
        .await
        .expect_err("unary encode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    match channel
        .server_streaming::<HelloRequest, HelloReply>(
            GREETER_SERVER,
            Request::new(oversize.clone()),
        )
        .await
    {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(_) => panic!("server-stream Channel encode cap must fail before headers"),
    }
    let (tx, call) =
        channel.client_streaming::<HelloRequest, HelloReply>(GREETER_CLIENT, Request::new(()));
    let err = tx
        .send(oversize.clone())
        .await
        .expect_err("client-stream send");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    drop(call);
    let (tx, call) = channel.bidi::<HelloRequest, HelloReply>(GREETER_BIDI, Request::new(()));
    let err = tx.send(oversize).await.expect_err("bidi send");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    drop(call);
}

async fn assert_channel_decode_cap_every_shape(channel: &Channel) {
    let oversize = req(&"x".repeat(64));
    let err = channel
        .unary::<HelloRequest, HelloReply>(GREETER_UNARY, Request::new(oversize.clone()))
        .await
        .expect_err("unary decode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    match channel
        .server_streaming::<HelloRequest, HelloReply>(
            GREETER_SERVER,
            Request::new(oversize.clone()),
        )
        .await
    {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("server-stream Channel decode cap must fail"),
        },
    }
    let (tx, call) =
        channel.client_streaming::<HelloRequest, HelloReply>(GREETER_CLIENT, Request::new(()));
    tx.send(oversize.clone()).await.expect("send");
    tx.close();
    let err = call.await.expect_err("client-stream decode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    let (tx, call) = channel.bidi::<HelloRequest, HelloReply>(GREETER_BIDI, Request::new(()));
    tx.send(oversize).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("bidi Channel decode cap must fail"),
        },
    }
}

async fn assert_channel_call_message_caps(channel: Channel) {
    assert_channel_encode_cap_every_shape(&channel.clone().max_encoding_message_size(16)).await;
    assert_channel_decode_cap_every_shape(&channel.max_decoding_message_size(16)).await;
}

#[tokio::test]
async fn channel_call_message_caps_are_resource_exhausted() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    assert_channel_call_message_caps(channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn tls_channel_call_message_caps_are_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_channel_call_message_caps(tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_channel_call_message_caps_are_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_channel_call_message_caps(tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_channel_call_message_caps_are_resource_exhausted() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    assert_channel_call_message_caps(unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_channel_call_message_caps_are_resource_exhausted() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_channel_call_message_caps(channel).await;
    server.abort();
}

async fn assert_test_client_encode_cap(client: &TestServiceClient) {
    let mut unary = SimpleRequest::new();
    unary.set_payload(fat_test_payload());
    let err = client
        .unary_call(Request::new(unary))
        .await
        .expect_err("unary encode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");

    let mut out = StreamingOutputCallRequest::new();
    out.set_payload(fat_test_payload());
    match client.streaming_output_call(Request::new(out)).await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(_) => panic!("server-stream TestService client encode cap must fail before headers"),
    }

    let mut input = StreamingInputCallRequest::new();
    input.set_payload(fat_test_payload());
    let (tx, call) = client.streaming_input_call(Request::new(()));
    let err = tx.send(input).await.expect_err("client-stream send");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    drop(call);

    let mut bidi = StreamingOutputCallRequest::new();
    bidi.set_payload(fat_test_payload());
    let (tx, call) = client.full_duplex_call(Request::new(()));
    let err = tx.send(bidi).await.expect_err("bidi send");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    drop(call);
}

async fn assert_test_client_decode_cap(client: &TestServiceClient) {
    // EmptyCall / StreamingInputCall replies stay under 16 bytes.
    let err = client
        .unary_call(Request::new(fat_test_response()))
        .await
        .expect_err("unary decode");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");

    match client
        .streaming_output_call(Request::new(fat_test_output_plan()))
        .await
    {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("server-stream TestService client decode cap must fail"),
        },
    }

    let (tx, call) = client.full_duplex_call(Request::new(()));
    tx.send(fat_test_output_plan()).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("bidi TestService client decode cap must fail"),
        },
    }
}

async fn assert_test_client_message_caps(client: TestServiceClient) {
    assert_test_client_encode_cap(&client.clone().max_encoding_message_size(16)).await;
    assert_test_client_decode_cap(&client.max_decoding_message_size(16)).await;
}

#[tokio::test]
async fn test_service_client_message_caps_are_resource_exhausted() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_test_client_message_caps(TestServiceClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_client_message_caps_are_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_test_client_message_caps(TestServiceClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_client_message_caps_are_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_test_client_message_caps(TestServiceClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_client_message_caps_are_resource_exhausted() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_test_client_message_caps(TestServiceClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_client_message_caps_are_resource_exhausted() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    let channel = Channel::from_io(client_io, "localhost")
        .await
        .expect("from_io");
    assert_test_client_message_caps(TestServiceClient::new(channel)).await;
    server.abort();
}

fn encode_message_limits() -> MessageLimits {
    MessageLimits::new().with_max_encoding(16)
}

fn decode_message_limits() -> MessageLimits {
    MessageLimits::new().with_max_decoding(16)
}

async fn assert_combined_message_limits_caps(channel: Channel) {
    assert_client_encode_cap_every_shape(
        &GreeterClient::new(channel.clone()).message_limits(encode_message_limits()),
    )
    .await;
    assert_client_decode_cap_every_shape(
        &GreeterClient::new(channel).message_limits(decode_message_limits()),
    )
    .await;
}

fn dial_encode_limits() -> ChannelConfig {
    ChannelConfig::new().message_limits(encode_message_limits())
}

fn dial_decode_limits() -> ChannelConfig {
    ChannelConfig::new().message_limits(decode_message_limits())
}

#[tokio::test]
async fn message_limits_setter_is_resource_exhausted() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    assert_combined_message_limits_caps(channel(addr).await).await;
    assert_dial_message_caps(
        channel_cfg(addr, dial_encode_limits()).await,
        channel_cfg(addr, dial_decode_limits()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_message_limits_setter_is_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_combined_message_limits_caps(tls_channel(addr).await).await;
    assert_dial_message_caps(
        tls_channel_cfg(addr, client_tls.clone(), dial_encode_limits()).await,
        tls_channel_cfg(addr, client_tls, dial_decode_limits()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_message_limits_setter_is_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_combined_message_limits_caps(tls_channel_with(addr, client_tls.clone()).await).await;
    assert_dial_message_caps(
        tls_channel_cfg(addr, client_tls.clone(), dial_encode_limits()).await,
        tls_channel_cfg(addr, client_tls, dial_decode_limits()).await,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_message_limits_setter_is_resource_exhausted() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_unix(sock).await.ok();
    });
    assert_combined_message_limits_caps(unix_channel(&path).await).await;
    assert_dial_message_caps(
        unix_channel_with(&path, dial_encode_limits()).await,
        unix_channel_with(&path, dial_decode_limits()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_message_limits_setter_is_resource_exhausted() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_connection(s1).await.ok();
    });
    let live = Channel::from_io(c1, "localhost")
        .await
        .expect("from_io live");
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_connection(s2).await.ok();
    });
    let encode = Channel::from_io_with(c2, "localhost", dial_encode_limits())
        .await
        .expect("from_io encode");
    let (c3, s3) = duplex_pair();
    let server3 = tokio::spawn(async move {
        GreeterServer::new(Echo).serve_connection(s3).await.ok();
    });
    let decode = Channel::from_io_with(c3, "localhost", dial_decode_limits())
        .await
        .expect("from_io decode");
    assert_combined_message_limits_caps(live).await;
    assert_dial_message_caps(encode, decode).await;
    server1.abort();
    server2.abort();
    server3.abort();
}

fn server_decode_limits() -> MessageLimits {
    MessageLimits::new().with_max_decoding(16)
}

fn greeter_server_limits() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).message_limits(server_decode_limits())
}

fn greeter_config_limits() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).config(ServerConfig::new().message_limits(server_decode_limits()))
}

fn greeter_router_limits() -> Router {
    Router::new()
        .message_limits(server_decode_limits())
        .add_service(GreeterServer::new(Echo))
}

fn reverser_plain_limits() -> Server<Reverser> {
    Server::new(Reverser::new(Arc::new(AtomicUsize::new(0)))).message_limits(server_decode_limits())
}

fn reverser_mtls_limits() -> Server<Reverser> {
    Server::new(Reverser::mtls(
        Arc::new(AtomicUsize::new(0)),
        client_identity().certificates().next().expect("leaf"),
    ))
    .message_limits(server_decode_limits())
}

async fn assert_reverser_server_oversize(channel: &Channel) {
    let oversize = req(&"x".repeat(64));
    let err = channel
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", Request::new(oversize.clone()))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    match channel
        .server_streaming::<HelloRequest, HelloReply>(
            "/demo.Reverser/Server",
            Request::new(oversize.clone()),
        )
        .await
    {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("server-stream over the server cap must fail"),
        },
    }
    let (tx, call) = channel
        .client_streaming::<HelloRequest, HelloReply>("/demo.Reverser/Client", Request::new(()));
    tx.send(oversize.clone()).await.expect("send");
    tx.close();
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), Code::ResourceExhausted, "{err}");
    let (tx, call) =
        channel.bidi::<HelloRequest, HelloReply>("/demo.Reverser/Bidi", Request::new(()));
    tx.send(oversize).await.expect("send");
    tx.close();
    match call.await {
        Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
        Ok(resp) => match resp.into_inner().message().await {
            Err(err) => assert_eq!(err.code(), Code::ResourceExhausted, "{err}"),
            Ok(_) => panic!("bidi over the server cap must fail"),
        },
    }
}

#[tokio::test]
async fn server_message_limits_is_resource_exhausted() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_server_limits().serve_listener(listener).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_config_limits().serve_listener(listener).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_router_limits().serve_listener(listener).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_plain_limits().serve_listener(listener).await.ok();
    });
    assert_reverser_server_oversize(&channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn tls_server_message_limits_is_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_server_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_config_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_router_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_plain_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_reverser_server_oversize(&tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_server_message_limits_is_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_server_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_config_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_router_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_mtls_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reverser_server_oversize(&tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_server_message_limits_is_resource_exhausted() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        greeter_server_limits().serve_unix(sock).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        greeter_config_limits().serve_unix(sock).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        greeter_router_limits().serve_unix(sock).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        reverser_plain_limits().serve_unix(sock).await.ok();
    });
    assert_reverser_server_oversize(&unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_server_message_limits_is_resource_exhausted() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        greeter_server_limits().serve_connection(s1).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        Channel::from_io(c1, "localhost")
            .await
            .expect("from_io wrap"),
    ))
    .await;
    server1.abort();
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        greeter_config_limits().serve_connection(s2).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        Channel::from_io(c2, "localhost")
            .await
            .expect("from_io config"),
    ))
    .await;
    server2.abort();
    let (c3, s3) = duplex_pair();
    let server3 = tokio::spawn(async move {
        greeter_router_limits().serve_connection(s3).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        Channel::from_io(c3, "localhost")
            .await
            .expect("from_io router"),
    ))
    .await;
    server3.abort();
    let (c4, s4) = duplex_pair();
    let server4 = tokio::spawn(async move {
        reverser_plain_limits().serve_connection(s4).await.ok();
    });
    assert_reverser_server_oversize(
        &Channel::from_io(c4, "localhost")
            .await
            .expect("from_io reverser"),
    )
    .await;
    server4.abort();
}

fn server_encode_limits() -> MessageLimits {
    MessageLimits::new().with_max_encoding(16)
}

fn greeter_server_encode_limits() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).message_limits(server_encode_limits())
}

fn greeter_config_encode_limits() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).config(ServerConfig::new().message_limits(server_encode_limits()))
}

fn test_server_encode_limits() -> TestServiceServer<InteropTestService> {
    TestServiceServer::new(InteropTestService).message_limits(server_encode_limits())
}

#[tokio::test]
async fn server_message_limits_encode_is_resource_exhausted() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_server_encode_limits()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_config_encode_limits()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_server_encode_limits()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_test_oversize_encode_every_shape(&TestServiceClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn tls_server_message_limits_encode_is_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_server_encode_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_config_encode_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_server_encode_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_test_oversize_encode_every_shape(&TestServiceClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn mtls_server_message_limits_encode_is_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_server_encode_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        greeter_config_encode_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_server_encode_limits()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_test_oversize_encode_every_shape(&TestServiceClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_server_message_limits_encode_is_resource_exhausted() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        greeter_server_encode_limits().serve_unix(sock).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        greeter_config_encode_limits().serve_unix(sock).await.ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        test_server_encode_limits().serve_unix(sock).await.ok();
    });
    assert_test_oversize_encode_every_shape(&TestServiceClient::new(unix_channel(&path).await))
        .await;
    task.abort();
}

#[tokio::test]
async fn from_io_server_message_limits_encode_is_resource_exhausted() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        greeter_server_encode_limits()
            .serve_connection(s1)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        Channel::from_io(c1, "localhost")
            .await
            .expect("from_io wrap"),
    ))
    .await;
    server1.abort();
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        greeter_config_encode_limits()
            .serve_connection(s2)
            .await
            .ok();
    });
    assert_greeter_oversize_every_shape(&GreeterClient::new(
        Channel::from_io(c2, "localhost")
            .await
            .expect("from_io config"),
    ))
    .await;
    server2.abort();
    let (c3, s3) = duplex_pair();
    let server3 = tokio::spawn(async move {
        test_server_encode_limits().serve_connection(s3).await.ok();
    });
    assert_test_oversize_encode_every_shape(&TestServiceClient::new(
        Channel::from_io(c3, "localhost")
            .await
            .expect("from_io test"),
    ))
    .await;
    server3.abort();
}

fn header_list_cap_server() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).max_header_list_size(1024)
}

fn flood_hello() -> Request<HelloRequest> {
    let mut request = Request::new(req("ada"));
    request
        .metadata_mut()
        .insert("x-flood", "v".repeat(4096))
        .expect("meta");
    request
}

async fn assert_header_flood_then_echo(flood: GreeterClient, healthy: GreeterClient) {
    let _ = tokio::time::timeout(Duration::from_secs(2), flood.say_hello(flood_hello())).await;
    echo_every_shape(&healthy, None).await;
}

#[tokio::test]
async fn header_list_cap_refuses_oversize_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_server().serve_listener(listener).await.ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(channel(addr).await),
        GreeterClient::new(channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(tls_channel(addr).await),
        GreeterClient::new(tls_channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_header_flood_then_echo(
        GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_header_list_cap_refuses_oversize_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        header_list_cap_server().serve_unix(sock).await.ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(unix_channel(&path).await),
        GreeterClient::new(unix_channel(&path).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_header_list_cap_refuses_oversize_metadata() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        header_list_cap_server().serve_connection(s1).await.ok();
    });
    let flood = GreeterClient::new(Channel::from_io(c1, "localhost").await.expect("flood"));
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        header_list_cap_server().serve_connection(s2).await.ok();
    });
    let healthy = GreeterClient::new(Channel::from_io(c2, "localhost").await.expect("healthy"));
    assert_header_flood_then_echo(flood, healthy).await;
    server1.abort();
    server2.abort();
}

async fn assert_test_combined_message_limits(client: TestServiceClient) {
    assert_test_client_encode_cap(&client.clone().message_limits(encode_message_limits())).await;
    assert_test_client_decode_cap(&client.message_limits(decode_message_limits())).await;
}

async fn test_cfg(addr: SocketAddr, cfg: ChannelConfig) -> TestServiceClient {
    let mut last = None;
    for _ in 0..80 {
        match TestServiceClient::connect_with(addr, cfg).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

async fn test_tls_cfg(addr: SocketAddr, tls: ClientTls, cfg: ChannelConfig) -> TestServiceClient {
    let mut last = None;
    for _ in 0..80 {
        match TestServiceClient::connect_tls_with(addr, cfg, tls.clone()).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

#[cfg(unix)]
async fn test_unix_cfg(path: &std::path::Path, cfg: ChannelConfig) -> TestServiceClient {
    let mut last = None;
    for _ in 0..80 {
        match TestServiceClient::connect_unix_with(path, cfg).await {
            Ok(client) => return client,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}")
}

async fn assert_test_dial_message_limits(encode: TestServiceClient, decode: TestServiceClient) {
    assert_test_client_encode_cap(&encode).await;
    assert_test_client_decode_cap(&decode).await;
}

#[tokio::test]
async fn test_service_message_limits_setter_is_resource_exhausted() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_test_combined_message_limits(TestServiceClient::new(channel(addr).await)).await;
    assert_test_dial_message_limits(
        test_cfg(addr, dial_encode_limits()).await,
        test_cfg(addr, dial_decode_limits()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_message_limits_setter_is_resource_exhausted() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_test_combined_message_limits(TestServiceClient::new(tls_channel(addr).await)).await;
    assert_test_dial_message_limits(
        test_tls_cfg(addr, client_tls.clone(), dial_encode_limits()).await,
        test_tls_cfg(addr, client_tls, dial_decode_limits()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_message_limits_setter_is_resource_exhausted() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_test_combined_message_limits(TestServiceClient::new(
        tls_channel_with(addr, client_tls.clone()).await,
    ))
    .await;
    assert_test_dial_message_limits(
        test_tls_cfg(addr, client_tls.clone(), dial_encode_limits()).await,
        test_tls_cfg(addr, client_tls, dial_decode_limits()).await,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_message_limits_setter_is_resource_exhausted() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_unix(sock)
            .await
            .ok();
    });
    assert_test_combined_message_limits(TestServiceClient::new(unix_channel(&path).await)).await;
    assert_test_dial_message_limits(
        test_unix_cfg(&path, dial_encode_limits()).await,
        test_unix_cfg(&path, dial_decode_limits()).await,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_message_limits_setter_is_resource_exhausted() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(s1)
            .await
            .ok();
    });
    let live = TestServiceClient::from_io(c1, "localhost")
        .await
        .expect("from_io live");
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(s2)
            .await
            .ok();
    });
    let encode = TestServiceClient::from_io_with(c2, "localhost", dial_encode_limits())
        .await
        .expect("from_io encode");
    let (c3, s3) = duplex_pair();
    let server3 = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(s3)
            .await
            .ok();
    });
    let decode = TestServiceClient::from_io_with(c3, "localhost", dial_decode_limits())
        .await
        .expect("from_io decode");
    assert_test_combined_message_limits(live).await;
    assert_test_dial_message_limits(encode, decode).await;
    server1.abort();
    server2.abort();
    server3.abort();
}

fn header_list_cap_config() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).config(ServerConfig::new().max_header_list_size(1024))
}

fn header_list_cap_router() -> Router {
    Router::new()
        .max_header_list_size(1024)
        .add_service(GreeterServer::new(Echo))
}

#[tokio::test]
async fn header_list_config_and_router_refuse_oversize_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_config().serve_listener(listener).await.ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(channel(addr).await),
        GreeterClient::new(channel(addr).await),
    )
    .await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_router().serve_listener(listener).await.ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(channel(addr).await),
        GreeterClient::new(channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_header_list_config_and_router_refuse_oversize_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(tls_channel(addr).await),
        GreeterClient::new(tls_channel(addr).await),
    )
    .await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(tls_channel(addr).await),
        GreeterClient::new(tls_channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_header_list_config_and_router_refuse_oversize_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_header_flood_then_echo(
        GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
    )
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        header_list_cap_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_header_flood_then_echo(
        GreeterClient::new(tls_channel_with(addr, client_tls.clone()).await),
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_header_list_config_and_router_refuse_oversize_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        header_list_cap_config().serve_unix(sock).await.ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(unix_channel(&path).await),
        GreeterClient::new(unix_channel(&path).await),
    )
    .await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        header_list_cap_router().serve_unix(sock).await.ok();
    });
    assert_header_flood_then_echo(
        GreeterClient::new(unix_channel(&path).await),
        GreeterClient::new(unix_channel(&path).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_header_list_config_and_router_refuse_oversize_metadata() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        header_list_cap_config().serve_connection(s1).await.ok();
    });
    let flood = GreeterClient::new(Channel::from_io(c1, "localhost").await.expect("flood"));
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        header_list_cap_config().serve_connection(s2).await.ok();
    });
    let healthy = GreeterClient::new(Channel::from_io(c2, "localhost").await.expect("healthy"));
    assert_header_flood_then_echo(flood, healthy).await;
    server1.abort();
    server2.abort();
    let (c3, s3) = duplex_pair();
    let server3 = tokio::spawn(async move {
        header_list_cap_router().serve_connection(s3).await.ok();
    });
    let flood = GreeterClient::new(
        Channel::from_io(c3, "localhost")
            .await
            .expect("flood router"),
    );
    let (c4, s4) = duplex_pair();
    let server4 = tokio::spawn(async move {
        header_list_cap_router().serve_connection(s4).await.ok();
    });
    let healthy = GreeterClient::new(
        Channel::from_io(c4, "localhost")
            .await
            .expect("healthy router"),
    );
    assert_header_flood_then_echo(flood, healthy).await;
    server3.abort();
    server4.abort();
}

fn flood_meta<T>(mut resp: Response<T>) -> Response<T> {
    resp.metadata_mut()
        .insert("x-flood", "v".repeat(4096))
        .expect("meta");
    resp
}

/// Echoes like [`Echo`], but stamps oversize response headers.
struct FloodHeaders;

impl Greeter for FloodHeaders {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(flood_meta(Response::new(common::reply(name_of_request(
            request.get_ref(),
        )))))
    }

    async fn client_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut stream = request.into_inner();
        let mut names = Vec::new();
        while let Some(msg) = stream.message().await? {
            names.push(name_of_request(&msg));
        }
        Ok(flood_meta(Response::new(common::reply(names.join(",")))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let name = name_of_request(request.get_ref());
        let (tx, stream) = pbrs_grpc::Streaming::channel(4);
        drop(tokio::spawn(async move {
            for part in name.split(',') {
                if tx.send(common::reply(part.to_string())).await.is_err() {
                    break;
                }
            }
        }));
        Ok(flood_meta(Response::new(stream)))
    }

    async fn stream_hello(
        &self,
        request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = pbrs_grpc::Streaming::channel(4);
        drop(tokio::spawn(async move {
            loop {
                match inbound.message().await {
                    Ok(Some(msg)) => {
                        if tx.send(common::reply(name_of_request(&msg))).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(status) => {
                        tx.fail(status).await;
                        break;
                    }
                }
            }
        }));
        Ok(flood_meta(Response::new(stream)))
    }
}

fn flood_headers_server() -> GreeterServer<FloodHeaders> {
    GreeterServer::new(FloodHeaders)
}

fn client_header_list_cap() -> ChannelConfig {
    ChannelConfig::new().max_header_list_size(1024)
}

async fn assert_capped_client_refuses_flood(client: &GreeterClient) {
    let err = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("unary");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let err = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect_err("server-stream");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let (tx, call) = client.client_hello(Request::new(()));
    tx.close();
    let err = call.await.expect_err("client-stream");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    let (tx, call) = client.stream_hello(Request::new(()));
    tx.close();
    let err = call.await.expect_err("bidi");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
}

async fn assert_client_header_list_cap(capped: GreeterClient, healthy: GreeterClient) {
    assert_capped_client_refuses_flood(&capped).await;
    echo_every_shape(&healthy, None).await;
}

#[tokio::test]
async fn channel_config_header_list_cap_refuses_oversize_response_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        flood_headers_server().serve_listener(listener).await.ok();
    });
    assert_client_header_list_cap(
        GreeterClient::new(channel_cfg(addr, client_header_list_cap()).await),
        GreeterClient::new(channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_channel_config_header_list_cap_refuses_oversize_response_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        flood_headers_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_client_header_list_cap(
        GreeterClient::new(
            tls_channel_cfg(addr, client_tls.clone(), client_header_list_cap()).await,
        ),
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_channel_config_header_list_cap_refuses_oversize_response_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        flood_headers_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_client_header_list_cap(
        GreeterClient::new(
            tls_channel_cfg(addr, client_tls.clone(), client_header_list_cap()).await,
        ),
        GreeterClient::new(tls_channel_with(addr, client_tls).await),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_channel_config_header_list_cap_refuses_oversize_response_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        flood_headers_server().serve_unix(sock).await.ok();
    });
    assert_client_header_list_cap(
        GreeterClient::new(unix_channel_with(&path, client_header_list_cap()).await),
        GreeterClient::new(unix_channel(&path).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_channel_config_header_list_cap_refuses_oversize_response_metadata() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        flood_headers_server().serve_connection(s1).await.ok();
    });
    let capped = GreeterClient::new(
        Channel::from_io_with(c1, "localhost", client_header_list_cap())
            .await
            .expect("from_io capped"),
    );
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        flood_headers_server().serve_connection(s2).await.ok();
    });
    let healthy = GreeterClient::new(
        Channel::from_io(c2, "localhost")
            .await
            .expect("from_io healthy"),
    );
    assert_client_header_list_cap(capped, healthy).await;
    server1.abort();
    server2.abort();
}

fn stream_cap_server() -> GreeterServer<Slow> {
    GreeterServer::new(Slow).max_concurrent_streams(1)
}

fn both_ok(
    a: Result<impl std::fmt::Debug, Status>,
    b: Result<impl std::fmt::Debug, Status>,
    what: &str,
) {
    assert!(
        a.is_ok() && b.is_ok(),
        "{what}: both RPCs must complete under the stream cap, got {a:?} {b:?}"
    );
}

async fn assert_stream_cap(client: &GreeterClient) {
    let started = Instant::now();
    let (a, b) = tokio::join!(
        client.say_hello(Request::new(req("a"))),
        client.say_hello(Request::new(req("b"))),
    );
    both_ok(a, b, "unary");
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "stream cap must serialize Slow unary handlers, got {:?}",
        started.elapsed()
    );

    let started = Instant::now();
    let (c, d) = tokio::join!(client.server_hello(Request::new(req("c"))), async {
        let (tx, call) = client.stream_hello(Request::new(()));
        drop(tx);
        call.await
    });
    both_ok(c, d, "server-stream/bidi");
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "stream cap must serialize Slow streaming handlers, got {:?}",
        started.elapsed()
    );

    let started = Instant::now();
    let (e, f) = tokio::join!(
        async {
            let (tx, call) = client.client_hello(Request::new(()));
            drop(tx);
            call.await
        },
        client.say_hello(Request::new(req("g"))),
    );
    both_ok(e, f, "client-stream");
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "stream cap must serialize Slow client-stream handlers, got {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extra_rpcs_wait_when_the_stream_cap_is_hit() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_server().serve_listener(listener).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_extra_rpcs_wait_when_the_stream_cap_is_hit() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_stream_cap(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_extra_rpcs_wait_when_the_stream_cap_is_hit() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_stream_cap(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_extra_rpcs_wait_when_the_stream_cap_is_hit() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        stream_cap_server().serve_unix(sock).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_extra_rpcs_wait_when_the_stream_cap_is_hit() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        stream_cap_server().serve_connection(server_io).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

fn stream_cap_config() -> GreeterServer<Slow> {
    GreeterServer::new(Slow).config(ServerConfig::new().max_concurrent_streams(1))
}

fn stream_cap_router() -> Router {
    Router::new()
        .max_concurrent_streams(1)
        .add_service(GreeterServer::new(Slow))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extra_rpcs_wait_when_the_stream_cap_config_and_router_are_hit() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_config().serve_listener(listener).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_router().serve_listener(listener).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_extra_rpcs_wait_when_the_stream_cap_config_and_router_are_hit() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_stream_cap(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_stream_cap(&GreeterClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_extra_rpcs_wait_when_the_stream_cap_config_and_router_are_hit() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_stream_cap(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        stream_cap_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_stream_cap(&GreeterClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_extra_rpcs_wait_when_the_stream_cap_config_and_router_are_hit() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        stream_cap_config().serve_unix(sock).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        stream_cap_router().serve_unix(sock).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_extra_rpcs_wait_when_the_stream_cap_config_and_router_are_hit() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        stream_cap_config().serve_connection(server_io).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io config"),
    ))
    .await;
    server.abort();
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        stream_cap_router().serve_connection(server_io).await.ok();
    });
    assert_stream_cap(&GreeterClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io router"),
    ))
    .await;
    server.abort();
}

fn test_header_list_cap() -> TestServiceServer<InteropTestService> {
    TestServiceServer::new(InteropTestService).max_header_list_size(1024)
}

fn flood_empty() -> Request<Empty> {
    let mut request = Request::new(Empty::new());
    request
        .metadata_mut()
        .insert("x-flood", "v".repeat(4096))
        .expect("meta");
    request
}

async fn assert_test_header_flood_then_echo(flood: TestServiceClient, healthy: TestServiceClient) {
    let _ = tokio::time::timeout(Duration::from_secs(2), flood.empty_call(flood_empty())).await;
    echo_test_every_shape(&healthy).await;
}

#[tokio::test]
async fn test_service_header_list_cap_refuses_oversize_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_header_list_cap().serve_listener(listener).await.ok();
    });
    assert_test_header_flood_then_echo(
        TestServiceClient::new(channel(addr).await),
        TestServiceClient::new(channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_header_list_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_test_header_flood_then_echo(
        TestServiceClient::new(tls_channel(addr).await),
        TestServiceClient::new(tls_channel(addr).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_header_list_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_test_header_flood_then_echo(
        TestServiceClient::new(tls_channel_with(addr, client_tls.clone()).await),
        TestServiceClient::new(tls_channel_with(addr, client_tls).await),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_header_list_cap_refuses_oversize_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        test_header_list_cap().serve_unix(sock).await.ok();
    });
    assert_test_header_flood_then_echo(
        TestServiceClient::new(unix_channel(&path).await),
        TestServiceClient::new(unix_channel(&path).await),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_header_list_cap_refuses_oversize_metadata() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        test_header_list_cap().serve_connection(s1).await.ok();
    });
    let flood = TestServiceClient::new(
        Channel::from_io(c1, "localhost")
            .await
            .expect("from_io flood"),
    );
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        test_header_list_cap().serve_connection(s2).await.ok();
    });
    let healthy = TestServiceClient::new(
        Channel::from_io(c2, "localhost")
            .await
            .expect("from_io healthy"),
    );
    assert_test_header_flood_then_echo(flood, healthy).await;
    server1.abort();
    server2.abort();
}

fn client_stream_settings() -> ChannelConfig {
    ChannelConfig::new().max_concurrent_streams(1)
}

fn slow_uncapped() -> GreeterServer<Slow> {
    GreeterServer::new(Slow)
}

async fn assert_client_stream_settings_do_not_serialize(client: &GreeterClient) {
    let started = Instant::now();
    let (a, b) = tokio::join!(
        client.say_hello(Request::new(req("a"))),
        client.say_hello(Request::new(req("b"))),
    );
    both_ok(a, b, "unary");
    assert!(
        started.elapsed() < Duration::from_millis(350),
        "client SETTINGS_MAX_CONCURRENT_STREAMS must not serialize Slow unary handlers, got {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extra_rpcs_do_not_wait_when_client_stream_settings_do_not_serialize() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        slow_uncapped().serve_listener(listener).await.ok();
    });
    assert_client_stream_settings_do_not_serialize(&GreeterClient::new(
        channel_cfg(addr, client_stream_settings()).await,
    ))
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_extra_rpcs_do_not_wait_when_client_stream_settings_do_not_serialize() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        slow_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    assert_client_stream_settings_do_not_serialize(&GreeterClient::new(
        tls_channel_cfg(addr, client_tls, client_stream_settings()).await,
    ))
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_extra_rpcs_do_not_wait_when_client_stream_settings_do_not_serialize() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        slow_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_client_stream_settings_do_not_serialize(&GreeterClient::new(
        tls_channel_cfg(addr, client_tls, client_stream_settings()).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_extra_rpcs_do_not_wait_when_client_stream_settings_do_not_serialize() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        slow_uncapped().serve_unix(sock).await.ok();
    });
    assert_client_stream_settings_do_not_serialize(&GreeterClient::new(
        unix_channel_with(&path, client_stream_settings()).await,
    ))
    .await;
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn from_io_extra_rpcs_do_not_wait_when_client_stream_settings_do_not_serialize() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        slow_uncapped().serve_connection(server_io).await.ok();
    });
    assert_client_stream_settings_do_not_serialize(&GreeterClient::new(
        Channel::from_io_with(client_io, "localhost", client_stream_settings())
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

fn reverser_header_list_cap() -> Server<Reverser> {
    Server::new(Reverser::new(Arc::new(AtomicUsize::new(0)))).max_header_list_size(1024)
}

fn reverser_mtls_header_list_cap() -> Server<Reverser> {
    Server::new(Reverser::mtls(
        Arc::new(AtomicUsize::new(0)),
        client_identity().certificates().next().expect("leaf"),
    ))
    .max_header_list_size(1024)
}

fn flood_reverse() -> Request<HelloRequest> {
    let mut request = Request::new(req("stressed"));
    request
        .metadata_mut()
        .insert("x-flood", "v".repeat(4096))
        .expect("meta");
    request
}

async fn assert_reverser_header_flood_then_echo(flood: Channel, healthy: Channel) {
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        flood.unary::<HelloRequest, HelloReply>("/demo.Reverser/Reverse", flood_reverse()),
    )
    .await;
    echo_reverser_every_shape(&healthy).await;
}

#[tokio::test]
async fn reverser_header_list_cap_refuses_oversize_metadata() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_header_list_cap()
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_reverser_header_flood_then_echo(channel(addr).await, channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn tls_reverser_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_header_list_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    assert_reverser_header_flood_then_echo(tls_channel(addr).await, tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_reverser_header_list_cap_refuses_oversize_metadata() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_mtls_header_list_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    assert_reverser_header_flood_then_echo(
        tls_channel_with(addr, client_tls.clone()).await,
        tls_channel_with(addr, client_tls).await,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_reverser_header_list_cap_refuses_oversize_metadata() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        reverser_header_list_cap().serve_unix(sock).await.ok();
    });
    assert_reverser_header_flood_then_echo(unix_channel(&path).await, unix_channel(&path).await)
        .await;
    task.abort();
}

#[tokio::test]
async fn from_io_reverser_header_list_cap_refuses_oversize_metadata() {
    let (c1, s1) = duplex_pair();
    let server1 = tokio::spawn(async move {
        reverser_header_list_cap().serve_connection(s1).await.ok();
    });
    let flood = Channel::from_io(c1, "localhost")
        .await
        .expect("from_io flood");
    let (c2, s2) = duplex_pair();
    let server2 = tokio::spawn(async move {
        reverser_header_list_cap().serve_connection(s2).await.ok();
    });
    let healthy = Channel::from_io(c2, "localhost")
        .await
        .expect("from_io healthy");
    assert_reverser_header_flood_then_echo(flood, healthy).await;
    server1.abort();
    server2.abort();
}

fn frame_size_server() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).max_frame_size(16 * 1024)
}

#[tokio::test]
async fn frame_size_still_serves_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_server().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_frame_size_still_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_frame_size_still_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_frame_size_still_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        frame_size_server().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_frame_size_still_serves_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        frame_size_server().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn frame_size_config() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).config(ServerConfig::new().max_frame_size(16 * 1024))
}

fn frame_size_router() -> Router {
    Router::new()
        .max_frame_size(16 * 1024)
        .add_service(GreeterServer::new(Echo))
}

#[tokio::test]
async fn frame_size_config_and_router_still_serve_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_config().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_router().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_frame_size_config_and_router_still_serve_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_frame_size_config_and_router_still_serve_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        frame_size_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_frame_size_config_and_router_still_serve_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        frame_size_config().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        frame_size_router().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_frame_size_config_and_router_still_serve_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        frame_size_config().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io config"),
        ),
        None,
    )
    .await;
    server.abort();
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        frame_size_router().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io router"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn client_frame_settings() -> ChannelConfig {
    ChannelConfig::new().max_frame_size(16 * 1024)
}

fn echo_uncapped() -> GreeterServer<Echo> {
    GreeterServer::new(Echo)
}

#[tokio::test]
async fn client_frame_settings_still_serve_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped().serve_listener(listener).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(channel_cfg(addr, client_frame_settings()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_client_frame_settings_still_serve_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    echo_every_shape(
        &GreeterClient::new(tls_channel_cfg(addr, client_tls, client_frame_settings()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_client_frame_settings_still_serve_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_cfg(addr, client_tls, client_frame_settings()).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_client_frame_settings_still_serve_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        echo_uncapped().serve_unix(sock).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(unix_channel_with(&path, client_frame_settings()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_frame_settings_still_serve_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        echo_uncapped().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io_with(client_io, "localhost", client_frame_settings())
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn window_size_server() -> GreeterServer<Echo> {
    GreeterServer::new(Echo)
        .initial_stream_window_size(64 * 1024)
        .initial_connection_window_size(128 * 1024)
}

#[tokio::test]
async fn window_size_still_serves_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_server().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_window_size_still_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_window_size_still_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_window_size_still_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        window_size_server().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_window_size_still_serves_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        window_size_server().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn window_size_config() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).config(
        ServerConfig::new()
            .initial_stream_window_size(64 * 1024)
            .initial_connection_window_size(128 * 1024),
    )
}

fn window_size_router() -> Router {
    Router::new()
        .initial_stream_window_size(64 * 1024)
        .initial_connection_window_size(128 * 1024)
        .add_service(GreeterServer::new(Echo))
}

#[tokio::test]
async fn window_size_config_and_router_still_serve_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_config().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_router().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_window_size_config_and_router_still_serve_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_window_size_config_and_router_still_serve_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        window_size_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_window_size_config_and_router_still_serve_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        window_size_config().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        window_size_router().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_window_size_config_and_router_still_serve_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        window_size_config().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io config"),
        ),
        None,
    )
    .await;
    server.abort();
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        window_size_router().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io router"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn client_window_settings() -> ChannelConfig {
    ChannelConfig::new()
        .initial_stream_window_size(64 * 1024)
        .initial_connection_window_size(128 * 1024)
}

#[tokio::test]
async fn client_window_settings_still_serve_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped().serve_listener(listener).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(channel_cfg(addr, client_window_settings()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_client_window_settings_still_serve_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    echo_every_shape(
        &GreeterClient::new(tls_channel_cfg(addr, client_tls, client_window_settings()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_client_window_settings_still_serve_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_cfg(addr, client_tls, client_window_settings()).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_client_window_settings_still_serve_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        echo_uncapped().serve_unix(sock).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(unix_channel_with(&path, client_window_settings()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_window_settings_still_serve_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        echo_uncapped().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io_with(client_io, "localhost", client_window_settings())
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn test_frame_size() -> TestServiceServer<InteropTestService> {
    TestServiceServer::new(InteropTestService).max_frame_size(16 * 1024)
}

#[tokio::test]
async fn test_service_frame_size_still_serves_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_frame_size().serve_listener(listener).await.ok();
    });
    echo_test_every_shape(&TestServiceClient::new(channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_tls_frame_size_still_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_frame_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_test_every_shape(&TestServiceClient::new(tls_channel(addr).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_mtls_frame_size_still_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_frame_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_test_every_shape(&TestServiceClient::new(
        tls_channel_with(addr, client_tls).await,
    ))
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn test_service_unix_frame_size_still_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        test_frame_size().serve_unix(sock).await.ok();
    });
    echo_test_every_shape(&TestServiceClient::new(unix_channel(&path).await)).await;
    task.abort();
}

#[tokio::test]
async fn test_service_from_io_frame_size_still_serves_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        test_frame_size().serve_connection(server_io).await.ok();
    });
    echo_test_every_shape(&TestServiceClient::new(
        Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    ))
    .await;
    server.abort();
}

fn reverser_frame_size() -> Server<Reverser> {
    Server::new(Reverser::new(Arc::new(AtomicUsize::new(0)))).max_frame_size(16 * 1024)
}

fn reverser_mtls_frame_size() -> Server<Reverser> {
    Server::new(Reverser::mtls(
        Arc::new(AtomicUsize::new(0)),
        client_identity().certificates().next().expect("leaf"),
    ))
    .max_frame_size(16 * 1024)
}

#[tokio::test]
async fn reverser_frame_size_still_serves_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_frame_size().serve_listener(listener).await.ok();
    });
    echo_reverser_every_shape(&channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn tls_reverser_frame_size_still_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_frame_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_reverser_every_shape(&tls_channel(addr).await).await;
    task.abort();
}

#[tokio::test]
async fn mtls_reverser_frame_size_still_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_mtls_frame_size()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_reverser_every_shape(&tls_channel_with(addr, client_tls).await).await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_reverser_frame_size_still_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        reverser_frame_size().serve_unix(sock).await.ok();
    });
    echo_reverser_every_shape(&unix_channel(&path).await).await;
    task.abort();
}

#[tokio::test]
async fn from_io_reverser_frame_size_still_serves_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        reverser_frame_size().serve_connection(server_io).await.ok();
    });
    echo_reverser_every_shape(
        &Channel::from_io(client_io, "localhost")
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

fn send_buffer_server() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).max_send_buffer_size(16 * 1024)
}

#[tokio::test]
async fn send_buffer_still_serves_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_server().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_send_buffer_still_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_send_buffer_still_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_send_buffer_still_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        send_buffer_server().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_send_buffer_still_serves_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        send_buffer_server().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn send_buffer_config() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).config(ServerConfig::new().max_send_buffer_size(16 * 1024))
}

fn send_buffer_router() -> Router {
    Router::new()
        .max_send_buffer_size(16 * 1024)
        .add_service(GreeterServer::new(Echo))
}

#[tokio::test]
async fn send_buffer_config_and_router_still_serve_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_config().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_router().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_send_buffer_config_and_router_still_serve_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_send_buffer_config_and_router_still_serve_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        send_buffer_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_send_buffer_config_and_router_still_serve_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        send_buffer_config().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        send_buffer_router().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_send_buffer_config_and_router_still_serve_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        send_buffer_config().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io config"),
        ),
        None,
    )
    .await;
    server.abort();
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        send_buffer_router().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io router"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn client_send_buffer() -> ChannelConfig {
    ChannelConfig::new().max_send_buffer_size(16 * 1024)
}

#[tokio::test]
async fn client_send_buffer_still_serves_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped().serve_listener(listener).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(channel_cfg(addr, client_send_buffer()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_client_send_buffer_still_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    echo_every_shape(
        &GreeterClient::new(tls_channel_cfg(addr, client_tls, client_send_buffer()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_client_send_buffer_still_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_cfg(addr, client_tls, client_send_buffer()).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_client_send_buffer_still_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        echo_uncapped().serve_unix(sock).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(unix_channel_with(&path, client_send_buffer()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_send_buffer_still_serves_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        echo_uncapped().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io_with(client_io, "localhost", client_send_buffer())
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn client_pending_reset() -> ChannelConfig {
    ChannelConfig::new().max_pending_accept_reset_streams(1)
}

#[tokio::test]
async fn client_pending_reset_still_serves_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped().serve_listener(listener).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(channel_cfg(addr, client_pending_reset()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_client_pending_reset_still_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    echo_every_shape(
        &GreeterClient::new(tls_channel_cfg(addr, client_tls, client_pending_reset()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_client_pending_reset_still_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        echo_uncapped()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_cfg(addr, client_tls, client_pending_reset()).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_client_pending_reset_still_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        echo_uncapped().serve_unix(sock).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(unix_channel_with(&path, client_pending_reset()).await),
        None,
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_client_pending_reset_still_serves_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        echo_uncapped().serve_connection(server_io).await.ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io_with(client_io, "localhost", client_pending_reset())
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

fn test_conn_cap() -> TestServiceServer<InteropTestService> {
    TestServiceServer::new(InteropTestService).max_concurrent_connections(1)
}

async fn assert_test_cap_refuses_then_echo(
    first: TestServiceClient,
    second: Result<TestServiceClient, Status>,
    reconnect: impl std::future::Future<Output = TestServiceClient>,
) {
    let err = second.expect_err("pool larger than the accept-loop cap should fail");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(first);
    echo_test_every_shape(&reconnect.await).await;
}

#[tokio::test]
async fn test_service_pool_against_cap_is_unavailable() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_conn_cap().serve_listener(listener).await.ok();
    });
    let first = TestServiceClient::new(channel(addr).await);
    assert_test_cap_refuses_then_echo(
        first,
        TestServiceClient::connect_with(addr, pool_against_cap()).await,
        async { TestServiceClient::new(channel(addr).await) },
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_test_service_pool_against_cap_is_unavailable() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_conn_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let first = TestServiceClient::new(tls_channel_with(addr, client_tls.clone()).await);
    assert_test_cap_refuses_then_echo(
        first,
        TestServiceClient::connect_tls_with(addr, pool_against_cap(), client_tls.clone()).await,
        async move { TestServiceClient::new(tls_channel_with(addr, client_tls).await) },
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_test_service_pool_against_cap_is_unavailable() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        test_conn_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let first = TestServiceClient::new(tls_channel_with(addr, client_tls.clone()).await);
    assert_test_cap_refuses_then_echo(
        first,
        TestServiceClient::connect_tls_with(addr, pool_against_cap(), client_tls.clone()).await,
        async move { TestServiceClient::new(tls_channel_with(addr, client_tls).await) },
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_test_service_pool_against_cap_is_unavailable() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        test_conn_cap().serve_unix(sock).await.ok();
    });
    let first = TestServiceClient::new(unix_channel(&path).await);
    let reconnect_path = path.clone();
    assert_test_cap_refuses_then_echo(
        first,
        TestServiceClient::connect_unix_with(&path, pool_against_cap()).await,
        async move { TestServiceClient::new(unix_channel(&reconnect_path).await) },
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_test_service_pool_config_is_still_one_duplex() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_test_every_shape(
        &TestServiceClient::from_io_with(client_io, "localhost", pool_cfg())
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

fn reverser_conn_cap() -> Server<Reverser> {
    Server::new(Reverser::new(Arc::new(AtomicUsize::new(0)))).max_concurrent_connections(1)
}

fn reverser_mtls_conn_cap() -> Server<Reverser> {
    Server::new(Reverser::mtls(
        Arc::new(AtomicUsize::new(0)),
        client_identity().certificates().next().expect("leaf"),
    ))
    .max_concurrent_connections(1)
}

async fn assert_reverser_cap_refuses_then_echo(
    first: Channel,
    second: Result<Channel, Status>,
    reconnect: impl std::future::Future<Output = Channel>,
) {
    let err = second.expect_err("pool larger than the accept-loop cap should fail");
    assert_eq!(err.code(), Code::Unavailable, "{err}");
    drop(first);
    echo_reverser_every_shape(&reconnect.await).await;
}

#[tokio::test]
async fn reverser_pool_against_cap_is_unavailable() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_conn_cap().serve_listener(listener).await.ok();
    });
    let first = channel(addr).await;
    assert_reverser_cap_refuses_then_echo(
        first,
        Channel::connect_with(addr, pool_against_cap()).await,
        channel(addr),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn tls_reverser_pool_against_cap_is_unavailable() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_conn_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let first = tls_channel_with(addr, client_tls.clone()).await;
    assert_reverser_cap_refuses_then_echo(
        first,
        Channel::connect_tls_with(addr, pool_against_cap(), client_tls.clone()).await,
        tls_channel_with(addr, client_tls),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn mtls_reverser_pool_against_cap_is_unavailable() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        reverser_mtls_conn_cap()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let first = tls_channel_with(addr, client_tls.clone()).await;
    assert_reverser_cap_refuses_then_echo(
        first,
        Channel::connect_tls_with(addr, pool_against_cap(), client_tls.clone()).await,
        tls_channel_with(addr, client_tls),
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_reverser_pool_against_cap_is_unavailable() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        reverser_conn_cap().serve_unix(sock).await.ok();
    });
    let first = unix_channel(&path).await;
    assert_reverser_cap_refuses_then_echo(
        first,
        Channel::connect_unix_with(&path, pool_against_cap()).await,
        unix_channel(&path),
    )
    .await;
    task.abort();
}

#[tokio::test]
async fn from_io_reverser_pool_config_is_still_one_duplex() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        Server::new(Reverser::new(Arc::new(AtomicUsize::new(0))))
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_reverser_every_shape(
        &Channel::from_io_with(client_io, "localhost", pool_cfg())
            .await
            .expect("from_io"),
    )
    .await;
    server.abort();
}

fn pending_reset_server() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).max_pending_accept_reset_streams(1)
}

fn pending_reset_config() -> GreeterServer<Echo> {
    GreeterServer::new(Echo).config(ServerConfig::new().max_pending_accept_reset_streams(1))
}

fn pending_reset_router() -> Router {
    Router::new()
        .max_pending_accept_reset_streams(1)
        .add_service(GreeterServer::new(Echo))
}

#[tokio::test]
async fn pending_reset_cap_still_serves_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_server().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_pending_reset_cap_still_serves_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_pending_reset_cap_still_serves_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_server()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_pending_reset_cap_still_serves_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        pending_reset_server().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_pending_reset_cap_still_serves_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        pending_reset_server()
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn pending_reset_config_and_router_still_serve_every_shape() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_config().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_router().serve_listener(listener).await.ok();
    });
    echo_every_shape(&GreeterClient::new(channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn tls_pending_reset_config_and_router_still_serve_every_shape() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    echo_every_shape(&GreeterClient::new(tls_channel(addr).await), None).await;
    task.abort();
}

#[tokio::test]
async fn mtls_pending_reset_config_and_router_still_serve_every_shape() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_config()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        pending_reset_router()
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    echo_every_shape(
        &GreeterClient::new(tls_channel_with(addr, client_tls).await),
        None,
    )
    .await;
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_pending_reset_config_and_router_still_serve_every_shape() {
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        pending_reset_config().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
    let (path, _guard) = unix_test_path();
    let sock = path.clone();
    let task = tokio::spawn(async move {
        pending_reset_router().serve_unix(sock).await.ok();
    });
    echo_every_shape(&GreeterClient::new(unix_channel(&path).await), None).await;
    task.abort();
}

#[tokio::test]
async fn from_io_pending_reset_config_and_router_still_serve_every_shape() {
    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        pending_reset_config()
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();

    let (client_io, server_io) = duplex_pair();
    let server = tokio::spawn(async move {
        pending_reset_router()
            .serve_connection(server_io)
            .await
            .ok();
    });
    echo_every_shape(
        &GreeterClient::new(
            Channel::from_io(client_io, "localhost")
                .await
                .expect("from_io"),
        ),
        None,
    )
    .await;
    server.abort();
}
