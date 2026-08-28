//! TLS and mTLS over loopback, including the cases that must fail.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    unreachable_pub,
    missing_docs,
    reason = "integration tests"
)]

mod common;

use common::{greeter_client, name_of, req, Echo, ServerGuard};
use pbrs_grpc::hello::{GreeterClient, GreeterServer};
use pbrs_grpc::{Channel, ChannelConfig, ClientTls, Code, Identity, Request, ServerTls, Status};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

const CA: &str = include_str!("tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("tls_data/server.crt");
const SERVER_KEY: &str = include_str!("tls_data/server.key");
const CLIENT_CERT: &str = include_str!("tls_data/client.crt");
const CLIENT_KEY: &str = include_str!("tls_data/client.key");
const OTHER_CA: &str = include_str!("tls_data/other.crt");

fn server_identity() -> Identity {
    Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity")
}

fn client_identity() -> Identity {
    Identity::from_pem(CLIENT_CERT, CLIENT_KEY).expect("client identity")
}

async fn bind() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    (addr, listener)
}

async fn serve_tls(tls: ServerTls) -> (SocketAddr, ServerGuard) {
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

async fn tls_client(addr: SocketAddr, tls: ClientTls) -> GreeterClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect_tls(addr, tls.clone()).await {
            Ok(channel) => return GreeterClient::new(channel),
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

#[tokio::test]
async fn unary_over_tls() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, _guard) = serve_tls(tls).await;
    let client = tls_client(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "ada");
}

#[tokio::test]
async fn wrong_ca_is_unauthenticated() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, _guard) = serve_tls(tls).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let Err(err) = Channel::connect_tls(
        addr,
        ClientTls::ca("localhost", OTHER_CA).expect("wrong ca"),
    )
    .await
    else {
        panic!("wrong CA was accepted");
    };
    assert_eq!(err.code(), Code::Unauthenticated, "{err}");
}

#[tokio::test]
async fn h2c_client_cannot_speak_tls() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, _guard) = serve_tls(tls).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    // h2's handshake resolves after writing the preface, before the peer
    // speaks, so a successful connect is not proof the peer is HTTP/2.
    let err = match Channel::connect(addr).await {
        Ok(channel) => GreeterClient::new(channel)
            .say_hello(Request::new(req("x")))
            .await
            .expect_err("h2c RPC succeeded against a TLS server"),
        Err(e) => e,
    };
    assert!(
        matches!(
            err.code(),
            Code::Unavailable | Code::Unauthenticated | Code::Internal
        ),
        "{err}"
    );
}

#[tokio::test]
async fn mtls_requires_a_client_certificate() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, _guard) = serve_tls(tls).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let Err(err) = Channel::connect_tls(
        addr,
        ClientTls::ca("localhost", CA).expect("no client cert"),
    )
    .await
    else {
        panic!("missing client cert was accepted");
    };
    assert_eq!(err.code(), Code::Unauthenticated, "{err}");
}

#[tokio::test]
async fn mtls_unary() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, _guard) = serve_tls(tls).await;
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client(addr, client_tls).await;
    let reply = client
        .say_hello(Request::new(req("grace")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "grace");
}

#[tokio::test]
async fn keepalive_still_serves() {
    let (addr, _guard) = {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let handle = tokio::spawn(async move {
            GreeterServer::new(Echo)
                .config(
                    pbrs_grpc::ServerConfig::new().keep_alive_interval(Duration::from_millis(50)),
                )
                .serve_listener(listener)
                .await
                .ok();
        });
        (addr, ServerGuard(handle))
    };
    let mut last = Status::unavailable("connect");
    let client = {
        let mut c = None;
        for _ in 0..80 {
            match Channel::connect_with(
                addr,
                ChannelConfig::new().keep_alive_interval(Duration::from_millis(50)),
            )
            .await
            {
                Ok(channel) => {
                    c = Some(GreeterClient::new(channel));
                    break;
                }
                Err(e) => {
                    last = e;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        c.unwrap_or_else(|| panic!("connect: {last}"))
    };
    tokio::time::sleep(Duration::from_millis(120)).await;
    let reply = client
        .say_hello(Request::new(req("ping")))
        .await
        .expect("rpc after ping");
    assert_eq!(name_of(reply.get_ref()), "ping");
}

#[tokio::test]
async fn cleartext_still_works() {
    let (addr, _guard) = common::spawn_greeter_server(pbrs_grpc::ServerConfig::default()).await;
    let client = greeter_client(addr).await;
    let reply = client
        .say_hello(Request::new(req("h2c")))
        .await
        .expect("rpc");
    assert_eq!(name_of(reply.get_ref()), "h2c");
}
