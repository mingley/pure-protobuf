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

use common::{greeter_client, name_of, req, until_ok, Echo, ServerGuard};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{
    Channel, ChannelConfig, ClientTls, Code, Identity, Outgoing, Request, Response, Rpc, ServerTls,
    Status, Streaming,
};
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
        match GreeterClient::connect_tls(addr, tls.clone()).await {
            Ok(client) => return client,
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

async fn echo_every_shape(client: &GreeterClient, name: &str) {
    let reply = client
        .say_hello(Request::new(req(name)))
        .await
        .expect("unary");
    assert_eq!(name_of(reply.get_ref()), name);

    let mut stream = client
        .server_hello(Request::new(req(name)))
        .await
        .expect("server-stream")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), name);
    assert!(stream.message().await.expect("end").is_none());

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req(name)).await.expect("send");
    tx.close();
    let reply = call.await.expect("client-stream");
    assert_eq!(name_of(reply.get_ref()), name);

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req(name)).await.expect("send");
    tx.close();
    let mut inbound = call.await.expect("bidi").into_inner();
    let first = inbound
        .message()
        .await
        .expect("item")
        .expect("first message");
    assert_eq!(name_of(&first), name);
    assert!(inbound.message().await.expect("end").is_none());
}

fn echo_named_stream(name: String) -> Response<Streaming<HelloReply>> {
    let (tx, stream) = Streaming::channel(4);
    drop(tokio::spawn(async move {
        for part in name.split(',') {
            if tx.send(common::reply(part.to_string())).await.is_err() {
                break;
            }
        }
    }));
    Response::new(stream)
}

fn echo_bidi(mut inbound: Streaming<HelloRequest>) -> Response<Streaming<HelloReply>> {
    let (tx, stream) = Streaming::channel(4);
    drop(tokio::spawn(async move {
        loop {
            match inbound.message().await {
                Ok(Some(msg)) => {
                    if tx
                        .send(common::reply(common::name_of_request(&msg)))
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
    Response::new(stream)
}

async fn echo_client_stream(
    mut inbound: Streaming<HelloRequest>,
) -> Result<Response<HelloReply>, Status> {
    let mut names = Vec::new();
    while let Some(msg) = inbound.message().await? {
        names.push(common::name_of_request(&msg));
    }
    Ok(Response::new(common::reply(names.join(","))))
}

fn sees_https<T>(request: Request<T>) -> Result<T, Status> {
    if request.scheme() != Some("https") {
        return Err(Status::internal(format!("scheme {:?}", request.scheme())));
    }
    let Some(auth) = request.authority() else {
        return Err(Status::internal("missing authority"));
    };
    if !auth.starts_with("127.0.0.1:") {
        return Err(Status::internal(format!("authority {auth}")));
    }
    if request.local_addr().is_none() {
        return Err(Status::internal("missing local_addr"));
    }
    if request.peer_identity().is_some() {
        return Err(Status::internal("anonymous TLS has no client cert"));
    }
    if request.peer_cred().is_some() {
        return Err(Status::internal("tls has no unix credentials"));
    }
    let want_auth = auth.to_owned();
    let (msg, parts) = request.into_message_and_parts();
    if parts.scheme() != Some("https") || parts.authority() != Some(want_auth.as_str()) {
        return Err(Status::internal("parts dropped tls identity"));
    }
    if parts.peer_identity().is_some() || parts.peer_cred().is_some() {
        return Err(Status::internal("parts invented credentials"));
    }
    Ok(msg)
}

fn sees_mtls<T>(request: Request<T>, want: &[u8]) -> Result<T, Status> {
    match request.peer_identity().and_then(|id| id.leaf()) {
        Some(leaf) if leaf == want => {}
        Some(_) => return Err(Status::internal("wrong leaf")),
        None => return Err(Status::internal("missing peer identity")),
    }
    let (msg, parts) = request.into_message_and_parts();
    match parts.peer_identity().and_then(|id| id.leaf()) {
        Some(leaf) if leaf == want => {}
        _ => return Err(Status::internal("parts dropped peer identity")),
    }
    Ok(msg)
}

#[tokio::test]
async fn unary_over_tls() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, _guard) = serve_tls(tls).await;
    let client = tls_client(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    echo_every_shape(&client, "ada").await;
}

#[tokio::test]
async fn serve_tls_until_shutdown_serves_then_drains() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    drop(listener);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_until_shutdown(
                addr,
                async {
                    shutdown_rx.await.ok();
                },
                tls,
            )
            .await
            .ok();
    });
    let client = tls_client(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    echo_every_shape(&client, "ada").await;
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("tls drain hung")
        .expect("join");
}

#[tokio::test]
async fn tls_requests_use_the_https_scheme() {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    let seen = Arc::new(AtomicU8::new(0));
    let flag = Arc::clone(&seen);
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(move |rpc: &mut pbrs_grpc::Rpc| {
                if rpc.peer_cred().is_some() {
                    flag.store(7, Ordering::SeqCst);
                    return Ok(());
                }
                let n = match (rpc.scheme(), rpc.local_addr(), rpc.peer_identity()) {
                    (Some("https"), Some(_), None) => 2,
                    (Some("https"), Some(_), Some(_)) => 5,
                    (Some("https"), None, _) => 4,
                    (Some("http"), _, _) => 1,
                    _ => 3,
                };
                flag.store(n, Ordering::SeqCst);
                Ok(())
            })
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = tls_client(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    echo_every_shape(&client, "ada").await;
    assert_eq!(
        seen.load(Ordering::SeqCst),
        2,
        "TLS RPCs must send :scheme https and expose a TCP local_addr"
    );
}

#[tokio::test]
async fn a_tls_client_interceptor_sees_https_scheme() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .intercept(|rpc: &mut Rpc| {
                if rpc.metadata().get("x-scheme") != Some("https") {
                    return Err(Status::internal(format!(
                        "x-scheme {:?}",
                        rpc.metadata().get("x-scheme")
                    )));
                }
                Ok(())
            })
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = tls_client(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    assert_eq!(client.scheme(), "https");
    assert_eq!(client.channel().scheme(), "https");
    assert_eq!(client.authority(), client.channel().authority());
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
    echo_every_shape(&client, "ada").await;
}

#[tokio::test]
async fn tls_handlers_see_https_scheme_and_authority() {
    struct SeesHttps;

    impl Greeter for SeesHttps {
        async fn say_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            let msg = sees_https(request)?;
            Ok(Response::new(common::reply(common::name_of_request(&msg))))
        }

        async fn client_hello(
            &self,
            request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<HelloReply>, Status> {
            echo_client_stream(sees_https(request)?).await
        }

        async fn server_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            let msg = sees_https(request)?;
            Ok(echo_named_stream(common::name_of_request(&msg)))
        }

        async fn stream_hello(
            &self,
            request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            Ok(echo_bidi(sees_https(request)?))
        }
    }

    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(SeesHttps)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = tls_client(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    echo_every_shape(&client, "ada").await;
}

#[tokio::test]
async fn mtls_exposes_the_client_certificate() {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;

    struct SeesPeerCert {
        want: Vec<u8>,
    }

    impl Greeter for SeesPeerCert {
        async fn say_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            let msg = sees_mtls(request, &self.want)?;
            Ok(Response::new(common::reply(common::name_of_request(&msg))))
        }

        async fn client_hello(
            &self,
            request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<HelloReply>, Status> {
            echo_client_stream(sees_mtls(request, &self.want)?).await
        }

        async fn server_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            let msg = sees_mtls(request, &self.want)?;
            Ok(echo_named_stream(common::name_of_request(&msg)))
        }

        async fn stream_hello(
            &self,
            request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            Ok(echo_bidi(sees_mtls(request, &self.want)?))
        }
    }

    let want = client_identity()
        .certificates()
        .next()
        .expect("leaf")
        .to_vec();
    let seen = Arc::new(AtomicU8::new(0));
    let flag = Arc::clone(&seen);
    let intercept_want = want.clone();
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        GreeterServer::new(SeesPeerCert { want })
            .intercept(move |rpc: &mut Rpc| {
                let n = match rpc.peer_identity().and_then(|id| id.leaf()) {
                    Some(leaf) if leaf == intercept_want.as_slice() => 1,
                    Some(_) => 2,
                    None => 3,
                };
                flag.store(n, Ordering::SeqCst);
                Ok(())
            })
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client(addr, client_tls).await;
    echo_every_shape(&client, "grace").await;
    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "mTLS RPCs must expose the client certificate on Rpc"
    );
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
    // h2's handshake future returns after writing the preface. The kernel
    // then waits for the peer's SETTINGS, so a TLS server (which never
    // sends them) fails the dial at connect_timeout rather than looking
    // connected.
    let err = match Channel::connect_with(
        addr,
        ChannelConfig::new().connect_timeout(Duration::from_millis(80)),
    )
    .await
    {
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
    echo_every_shape(&client, "grace").await;
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
    echo_every_shape(&client, "ping").await;
}

#[tokio::test]
async fn cleartext_still_works() {
    let (addr, _guard) = common::spawn_greeter_server(pbrs_grpc::ServerConfig::default()).await;
    let client = greeter_client(addr).await;
    echo_every_shape(&client, "h2c").await;
}

async fn serve_tls_at(addr: SocketAddr, tls: ServerTls) -> ServerGuard {
    let mut last = None;
    for _ in 0..100 {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let handle = tokio::spawn(async move {
                    GreeterServer::new(Echo)
                        .serve_tls_with_shutdown(listener, std::future::pending(), tls)
                        .await
                        .ok();
                });
                return ServerGuard(handle);
            }
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    panic!("rebind {addr}: {last:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_tls_channel_redials() {
    let tls = ServerTls::new(server_identity()).expect("server tls");
    let (addr, guard) = serve_tls(tls).await;
    let client = tls_client(addr, ClientTls::ca("localhost", CA).expect("client tls")).await;
    echo_every_shape(&client, "before").await;

    drop(guard);
    let _guard = serve_tls_at(addr, ServerTls::new(server_identity()).expect("server tls")).await;

    // The first attempt can still land on the dying connection (`ready`
    // succeeded, then GOAWAY). Unary and server-streaming retry that redial
    // once; further attempts cover a rebound listener that is not yet accepting.
    let after = until_ok("tls unary after", || {
        client.say_hello(Request::new(req("after")))
    })
    .await;
    assert_eq!(name_of(after.get_ref()), "after");
    echo_every_shape(&client, "after").await;
}
