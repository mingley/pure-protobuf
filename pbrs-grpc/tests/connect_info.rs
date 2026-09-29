//! Tonic-shaped connect-info request extensions.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    unreachable_pub,
    missing_docs,
    reason = "integration tests assert transport fixtures"
)]

mod common;

use common::{ServerGuard, name_of, req};
#[cfg(unix)]
use pbrs_grpc::compat::UdsConnectInfo;
use pbrs_grpc::compat::{Request, Response, Status, TcpConnectInfo, TlsConnectInfo};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{ClientTls, Identity, ServerTls};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

const CA: &str = include_str!("tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("tls_data/server.crt");
const SERVER_KEY: &str = include_str!("tls_data/server.key");
const CLIENT_CERT: &str = include_str!("tls_data/client.crt");
const CLIENT_KEY: &str = include_str!("tls_data/client.key");

enum ExpectedConnectInfo {
    Tcp,
    Tls {
        want_leaf: Option<Arc<[u8]>>,
    },
    #[cfg(unix)]
    Uds,
}

struct ConnectInfoGreeter {
    expected: ExpectedConnectInfo,
}

impl ConnectInfoGreeter {
    fn tcp() -> Self {
        Self {
            expected: ExpectedConnectInfo::Tcp,
        }
    }

    fn tls(want_leaf: Option<Arc<[u8]>>) -> Self {
        Self {
            expected: ExpectedConnectInfo::Tls { want_leaf },
        }
    }

    #[cfg(unix)]
    fn uds() -> Self {
        Self {
            expected: ExpectedConnectInfo::Uds,
        }
    }

    fn check<T>(&self, request: &Request<T>) -> Result<(), Status> {
        match &self.expected {
            ExpectedConnectInfo::Tcp => assert_tcp_connect_info(request),
            ExpectedConnectInfo::Tls { want_leaf } => {
                assert_tls_connect_info(request, want_leaf.as_deref())
            }
            #[cfg(unix)]
            ExpectedConnectInfo::Uds => assert_uds_connect_info(request),
        }
    }
}

impl Greeter for ConnectInfoGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.check(&request)?;
        Ok(Response::new(common::reply(name_of_request(&request))))
    }
}

fn name_of_request(request: &Request<HelloRequest>) -> String {
    request.get_ref().name().to_str().unwrap_or("").to_owned()
}

fn assert_tcp_connect_info<T>(request: &Request<T>) -> Result<(), Status> {
    let info = request
        .extensions()
        .get::<TcpConnectInfo>()
        .ok_or_else(|| Status::internal("missing TcpConnectInfo"))?;
    if request
        .extensions()
        .get::<TlsConnectInfo<TcpConnectInfo>>()
        .is_some()
    {
        return Err(Status::internal("cleartext TCP carried TlsConnectInfo"));
    }
    if info.remote_addr().is_none() || info.remote_addr() != request.remote_addr() {
        return Err(Status::internal("TcpConnectInfo remote_addr mismatch"));
    }
    if info.local_addr().is_none() || info.local_addr() != request.local_addr() {
        return Err(Status::internal("TcpConnectInfo local_addr mismatch"));
    }
    if request.peer_identity().is_some() || request.peer_cred().is_some() {
        return Err(Status::internal(
            "TCP invented peer identity or credentials",
        ));
    }
    Ok(())
}

fn assert_tls_connect_info<T>(
    request: &Request<T>,
    want_leaf: Option<&[u8]>,
) -> Result<(), Status> {
    if request.extensions().get::<TcpConnectInfo>().is_some() {
        return Err(Status::internal("TLS carried cleartext TcpConnectInfo"));
    }
    let info = request
        .extensions()
        .get::<TlsConnectInfo<TcpConnectInfo>>()
        .ok_or_else(|| Status::internal("missing TlsConnectInfo"))?;
    if info.get_ref().remote_addr() != request.remote_addr() {
        return Err(Status::internal("TlsConnectInfo remote_addr mismatch"));
    }
    if info.get_ref().local_addr() != request.local_addr() {
        return Err(Status::internal("TlsConnectInfo local_addr mismatch"));
    }
    match (
        want_leaf,
        info.peer_certs().and_then(pbrs_grpc::PeerIdentity::leaf),
        request
            .peer_identity()
            .and_then(pbrs_grpc::PeerIdentity::leaf),
    ) {
        (None, None, None) => Ok(()),
        (Some(want), Some(info_leaf), Some(request_leaf))
            if info_leaf == want && request_leaf == want =>
        {
            Ok(())
        }
        _ => Err(Status::internal("TlsConnectInfo peer cert mismatch")),
    }
}

#[cfg(unix)]
fn assert_uds_connect_info<T>(request: &Request<T>) -> Result<(), Status> {
    let info = request
        .extensions()
        .get::<UdsConnectInfo>()
        .ok_or_else(|| Status::internal("missing UdsConnectInfo"))?;
    if info.peer_addr().is_none() {
        return Err(Status::internal("UdsConnectInfo missing peer_addr"));
    }
    let Some(cred) = info.peer_cred() else {
        return Err(Status::internal("UdsConnectInfo missing peer_cred"));
    };
    if Some(cred) != request.peer_cred() {
        return Err(Status::internal("UdsConnectInfo peer_cred mismatch"));
    }
    if cred.pid() != Some(std::process::id()) {
        return Err(Status::internal(format!(
            "UdsConnectInfo pid {:?} want {}",
            cred.pid(),
            std::process::id()
        )));
    }
    if request.remote_addr().is_some() || request.local_addr().is_some() {
        return Err(Status::internal("UDS invented TCP addresses"));
    }
    Ok(())
}

async fn bind() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    (addr, listener)
}

async fn connect(addr: SocketAddr) -> GreeterClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match GreeterClient::connect(addr).await {
            Ok(client) => return client,
            Err(err) => {
                last = err;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

fn server_identity() -> Identity {
    Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity")
}

fn client_identity() -> Identity {
    Identity::from_pem(CLIENT_CERT, CLIENT_KEY).expect("client identity")
}

async fn connect_tls(addr: SocketAddr, tls: ClientTls) -> GreeterClient {
    let mut last = Status::unavailable("connect tls");
    for _ in 0..80 {
        match GreeterClient::connect_tls(addr, tls.clone()).await {
            Ok(client) => return client,
            Err(err) => {
                last = err;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect TLS to {addr}: {last}");
}

async fn call_unary(client: GreeterClient) {
    let reply = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("say hello");
    assert_eq!(name_of(reply.get_ref()), "ada");
}

#[tokio::test]
async fn tcp_handler_reads_tonic_connect_info_extension() {
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(ConnectInfoGreeter::tcp())
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    call_unary(connect(addr).await).await;
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
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pbrs-grpc-connect-info-{}.sock",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    (path.clone(), UnixSockGuard(path))
}

#[cfg(unix)]
#[tokio::test]
async fn uds_handler_reads_tonic_connect_info_extension() {
    let (path, _path_guard) = unix_test_path();
    let listener = tokio::net::UnixListener::bind(&path).expect("bind");
    let handle = tokio::spawn(async move {
        GreeterServer::new(ConnectInfoGreeter::uds())
            .serve_unix_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = GreeterClient::connect_unix(&path)
        .await
        .expect("connect UDS");
    call_unary(client).await;
}

#[tokio::test]
async fn tls_handler_reads_tonic_connect_info_extension() {
    let (addr, listener) = bind().await;
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let handle = tokio::spawn(async move {
        GreeterServer::new(ConnectInfoGreeter::tls(None))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = connect_tls(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    call_unary(client).await;
}

#[tokio::test]
async fn mtls_handler_reads_tonic_connect_info_extension() {
    let (addr, listener) = bind().await;
    let leaf = client_identity()
        .certificates()
        .next()
        .expect("client leaf")
        .into();
    let tls = ServerTls::mtls(server_identity(), CA).expect("server mtls");
    let handle = tokio::spawn(async move {
        GreeterServer::new(ConnectInfoGreeter::tls(Some(leaf)))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = connect_tls(
        addr,
        ClientTls::ca_mtls("localhost", CA, client_identity()).expect("client mtls"),
    )
    .await;
    call_unary(client).await;
}
