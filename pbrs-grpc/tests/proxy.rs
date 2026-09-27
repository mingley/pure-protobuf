//! A1 HTTP CONNECT proxy tests (CH-09): tunneling, auth, NO_PROXY,
//! and TLS end-to-end through a test proxy. Process env is global, so
//! every env-mutating case holds `ENV_LOCK`; lib unit tests cover pure
//! parsing without env.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    unreachable_pub,
    missing_docs,
    unsafe_code,
    reason = "integration tests"
)]

mod common;

use common::{Echo, ServerGuard, name_of, req};
use pbrs_grpc::hello::{GreeterClient, GreeterServer};
use pbrs_grpc::{Channel, ClientTls, Identity, Request, Router, ServerTls};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Serializes process-env mutation across cases in this binary.
/// SAFETY of the `set_var`/`remove_var` calls below: this lock (plus no
/// other env use in this binary) means no thread observes a torn
/// environment; each case quiesces all dials before mutating.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const CA: &str = include_str!("tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("tls_data/server.crt");
const SERVER_KEY: &str = include_str!("tls_data/server.key");

/// What the test proxy observed, for assertions.
#[derive(Debug, Default)]
struct ProxyLog {
    /// CONNECT targets requested, in order.
    targets: Mutex<Vec<String>>,
    /// Proxy-Authorization values seen, in order (empty string = absent).
    auth: Mutex<Vec<String>>,
}

/// Tiny CONNECT proxy: optionally requires `want_auth`, dials the
/// target, answers 200, and relays both directions.
async fn run_proxy(listener: TcpListener, log: Arc<ProxyLog>, want_auth: Option<String>) {
    loop {
        let Ok((mut inbound, _)) = listener.accept().await else {
            return;
        };
        let log = log.clone();
        let want_auth = want_auth.clone();
        tokio::spawn(async move {
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                if head.len() > 8192 || inbound.read_exact(&mut byte).await.is_err() {
                    return;
                }
                head.push(byte[0]);
                if head.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&head);
            let mut lines = text.lines();
            let target = lines
                .next()
                .and_then(|request| request.split_whitespace().nth(1))
                .unwrap_or("")
                .to_string();
            let mut auth = String::new();
            for line in lines {
                if let Some(value) = line.strip_prefix("Proxy-Authorization:") {
                    auth = value.trim().to_string();
                }
            }
            log.targets.lock().expect("log").push(target.clone());
            log.auth.lock().expect("log").push(auth.clone());
            if want_auth.as_deref().is_some_and(|want| want != auth) {
                let _ = inbound
                    .write_all(b"HTTP/1.1 407 Proxy Auth Required\r\n\r\n")
                    .await;
                return;
            }
            let target_conn = match TcpStream::connect(&target).await {
                Ok(conn) => conn,
                Err(_) => {
                    let _ = inbound.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
                    return;
                }
            };
            if inbound
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .is_err()
            {
                return;
            }
            let mut target_conn = target_conn;
            let _ = tokio::io::copy_bidirectional(&mut inbound, &mut target_conn).await;
        });
    }
}

async fn start_proxy(want_auth: Option<String>) -> (SocketAddr, Arc<ProxyLog>) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("proxy bind");
    let addr = listener.local_addr().expect("proxy addr");
    let log = Arc::new(ProxyLog::default());
    tokio::spawn(run_proxy(listener, log.clone(), want_auth));
    (addr, log)
}

async fn serve() -> (SocketAddr, ServerGuard) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

fn set_env(key: &str, value: &str) {
    // SAFETY: `ENV_LOCK` serializes all env mutation in this binary and
    // every case quiesces dials first; no concurrent getenv can race.
    unsafe {
        std::env::set_var(key, value);
    }
}

fn remove_env(key: &str) {
    // SAFETY: same discipline as `set_env`.
    unsafe {
        std::env::remove_var(key);
    }
}

struct EnvGuard {
    saved: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    fn set(pairs: &[(&str, &str)]) -> Self {
        let mut saved = Vec::new();
        for (key, value) in pairs {
            saved.push(((*key).to_string(), std::env::var(key).ok()));
            set_env(key, value);
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                Some(previous) => set_env(&key, &previous),
                None => remove_env(&key),
            }
        }
    }
}

#[tokio::test]
async fn proxy_tunnels_plaintext_rpc() {
    let _env = ENV_LOCK.lock().await;
    let (addr, _server) = serve().await;
    let (proxy_addr, log) = start_proxy(None).await;
    let _guard = EnvGuard::set(&[
        ("HTTPS_PROXY", &format!("http://{proxy_addr}")),
        ("NO_PROXY", ""),
    ]);

    let channel = Channel::connect(format!("127.0.0.1:{}", addr.port()))
        .await
        .expect("connect via proxy");
    let client = GreeterClient::new(channel);
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary through proxy");
    assert_eq!(name_of(&resp.into_inner()), "ada");

    let targets = log.targets.lock().expect("log").clone();
    assert_eq!(targets, vec![addr.to_string()], "CONNECT went to proxy");
}

#[tokio::test]
async fn proxy_tunnels_hostname_target() {
    let _env = ENV_LOCK.lock().await;
    let (addr, _server) = serve().await;
    let (proxy_addr, log) = start_proxy(None).await;
    let _guard = EnvGuard::set(&[("HTTPS_PROXY", &proxy_addr.to_string()), ("NO_PROXY", "")]);

    // Name-based target exercises the `connect(host)` proxy path.
    let channel = Channel::connect(format!("localhost:{}", addr.port()))
        .await
        .expect("connect via proxy");
    let client = GreeterClient::new(channel);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary through proxy");

    let targets = log.targets.lock().expect("log").clone();
    assert_eq!(targets.len(), 1);
    assert!(
        targets[0].ends_with(&format!(":{}", addr.port())),
        "CONNECT target carries the port: {:?}",
        targets[0]
    );
}

#[tokio::test]
async fn proxy_no_proxy_bypasses() {
    let _env = ENV_LOCK.lock().await;
    let (addr, _server) = serve().await;
    // Dead proxy: any attempt to use it fails fast, proving the bypass.
    let _guard = EnvGuard::set(&[
        ("HTTPS_PROXY", "http://127.0.0.1:1"),
        ("NO_PROXY", "127.0.0.1, localhost"),
    ]);

    let channel = Channel::connect(addr).await.expect("direct connect");
    let client = GreeterClient::new(channel);
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary bypasses proxy");
    assert_eq!(name_of(&resp.into_inner()), "ada");
}

#[tokio::test]
async fn proxy_no_proxy_miss_uses_proxy_and_fails() {
    let _env = ENV_LOCK.lock().await;
    let (addr, _server) = serve().await;
    let _guard = EnvGuard::set(&[
        ("HTTPS_PROXY", "http://127.0.0.1:1"),
        ("NO_PROXY", "example.com"),
    ]);

    // 127.0.0.1:1 refuses: the dial must attempt the proxy and fail,
    // proving NO_PROXY did not wrongly bypass.
    let err = Channel::connect(addr).await.expect_err("proxy refused");
    assert_eq!(err.code(), pbrs_grpc::Code::Unavailable);
}

#[tokio::test]
async fn proxy_authenticates() {
    let _env = ENV_LOCK.lock().await;
    let (addr, _server) = serve().await;
    let (proxy_addr, log) = start_proxy(Some("Basic dXNlcjpwYXNz".to_string())).await;

    // Without credentials the proxy answers 407 and the RPC fails.
    let _guard = EnvGuard::set(&[
        ("HTTPS_PROXY", &format!("http://{proxy_addr}")),
        ("NO_PROXY", ""),
    ]);
    assert!(
        Channel::connect(addr).await.is_err(),
        "unauthenticated CONNECT must fail"
    );

    // With credentials in the URL the tunnel opens.
    drop(_guard);
    let _guard = EnvGuard::set(&[
        ("HTTPS_PROXY", &format!("http://user:pass@{proxy_addr}")),
        ("NO_PROXY", ""),
    ]);
    let channel = Channel::connect(addr).await.expect("authed connect");
    let client = GreeterClient::new(channel);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary through authed proxy");

    let auth = log.auth.lock().expect("log").clone();
    assert!(
        auth.iter().any(|seen| seen == "Basic dXNlcjpwYXNz"),
        "proxy saw basic auth: {auth:?}"
    );
}

#[tokio::test]
async fn proxy_tunnels_tls_end_to_end() {
    let _env = ENV_LOCK.lock().await;
    let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let tls = ServerTls::new(identity).expect("server tls");
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _server = ServerGuard(handle);

    let (proxy_addr, log) = start_proxy(None).await;
    let _guard = EnvGuard::set(&[("HTTPS_PROXY", &proxy_addr.to_string()), ("NO_PROXY", "")]);

    // The TLS handshake runs inside the tunnel; the proxy only relays.
    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
    let client = GreeterClient::connect_tls(addr, client_tls.clone())
        .await
        .expect("tls connect via proxy");
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("tls unary through proxy");
    assert_eq!(name_of(&resp.into_inner()), "ada");
    assert_eq!(log.targets.lock().expect("log").len(), 1);
}

#[tokio::test]
async fn user_timeout_env_does_not_break_dials() {
    let _env = ENV_LOCK.lock().await;
    let _guard = EnvGuard::set(&[("PBRS_TCP_USER_TIMEOUT_MS", "60000")]);
    let (addr, _server) = serve().await;
    // Applied on Linux (setsockopt), ignored elsewhere; either way the
    // dial path must stay healthy.
    let channel = Channel::connect(addr).await.expect("connect");
    let client = GreeterClient::new(channel);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
}
