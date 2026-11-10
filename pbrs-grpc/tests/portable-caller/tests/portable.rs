//! Caller-owned TLS and no-downgrade controls for the portable transport.
//! This standalone test application opts into ring; the shipping crate does not.
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, ClientTls, Code, Request, Response, ServerTls, Status, Streaming};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

const CA: &str = include_str!("../../tls_data/ca.crt");
const OTHER_CA: &str = include_str!("../../tls_data/other.crt");
const SERVER_CERT: &str = include_str!("../../tls_data/server.crt");
const SERVER_KEY: &str = include_str!("../../tls_data/server.key");
const CLIENT_CERT: &str = include_str!("../../tls_data/client.crt");
const CLIENT_KEY: &str = include_str!("../../tls_data/client.key");

struct Echo;
impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(request.into_inner().name().to_str().unwrap().to_owned());
        Ok(Response::new(reply))
    }
    async fn client_hello(
        &self,
        _: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn server_hello(
        &self,
        _: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("fixture"))
    }
    async fn stream_hello(
        &self,
        _: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("fixture"))
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}
fn roots(pem: &str) -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
        roots.add(cert.unwrap()).unwrap();
    }
    roots
}
fn certs(pem: &str) -> Vec<rustls::pki_types::CertificateDer<'static>> {
    rustls_pemfile::certs(&mut pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
fn key(pem: &str) -> rustls::pki_types::PrivateKeyDer<'static> {
    rustls_pemfile::private_key(&mut pem.as_bytes())
        .unwrap()
        .unwrap()
}
fn server_config() -> rustls::ServerConfig {
    let crypto = provider();
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots(CA)),
        crypto.clone(),
    )
    .build()
    .unwrap();
    let mut config = rustls::ServerConfig::builder_with_provider(crypto)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs(SERVER_CERT), key(SERVER_KEY))
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    config
}
fn client_config(ca: &str, identity: bool) -> rustls::ClientConfig {
    let builder = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots(ca));
    let mut config = if identity {
        builder
            .with_client_auth_cert(certs(CLIENT_CERT), key(CLIENT_KEY))
            .unwrap()
    } else {
        builder.with_no_client_auth()
    };
    config.alpn_protocols = vec![b"h2".to_vec()];
    config
}
fn request(name: &str) -> Request<HelloRequest> {
    let mut req = HelloRequest::new();
    req.set_name(name);
    Request::new(req)
}

#[test]
fn caller_configs_require_h2_and_valid_name() {
    let mut server = server_config();
    server.alpn_protocols.clear();
    assert_eq!(
        ServerTls::from_rustls(Arc::new(server)).unwrap_err().code(),
        Code::InvalidArgument
    );
    let mut client = client_config(CA, true);
    client.alpn_protocols = vec![b"http/1.1".to_vec()];
    assert_eq!(
        ClientTls::from_rustls("localhost", Arc::new(client))
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        ClientTls::from_rustls("not a valid name", Arc::new(client_config(CA, true)))
            .unwrap_err()
            .code(),
        Code::InvalidArgument,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn plaintext_h2c_round_trip() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            GreeterServer::new(Echo)
                .serve_listener(listener)
                .await
                .unwrap();
        });
        let client = GreeterClient::connect(addr).await.unwrap();
        let reply = client.say_hello(request("h2c")).await.unwrap().into_inner();
        assert_eq!(reply.message().to_str().unwrap(), "h2c");
        task.abort();
    })
    .await
    .expect("bounded plaintext RPC");
}

#[tokio::test(flavor = "current_thread")]
async fn caller_owned_mutual_tls_and_rejection() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let server = ServerTls::from_rustls(Arc::new(server_config())).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            GreeterServer::new(Echo)
                .serve_tls_with_shutdown(listener, std::future::pending(), server)
                .await
                .unwrap();
        });

        let tls = ClientTls::from_rustls("localhost", Arc::new(client_config(CA, true))).unwrap();
        let client = GreeterClient::connect_tls(addr, tls).await.unwrap();
        let reply = client
            .say_hello(request("mtls"))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(reply.message().to_str().unwrap(), "mtls");

        for (name, ca, identity) in [
            ("localhost", OTHER_CA, true),
            ("wrong.example", CA, true),
            ("localhost", CA, false),
        ] {
            let tls = ClientTls::from_rustls(name, Arc::new(client_config(ca, identity))).unwrap();
            let failure = Channel::connect_tls(addr, tls).await;
            assert!(
                failure.is_err(),
                "untrusted name, CA, or missing client certificate"
            );
        }
        task.abort();
    })
    .await
    .expect("bounded caller-owned mTLS");
}

#[tokio::test(flavor = "current_thread")]
async fn tls_failure_does_not_retry_as_plaintext() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut prefix = [0u8; 24];
            let n = stream.read(&mut prefix).await.unwrap();
            assert!(n > 0);
            assert_eq!(prefix[0], 0x16, "first record must be TLS handshake");
            assert!(!prefix[..n].starts_with(b"PRI * HTTP/2.0"));
            drop(stream);
            assert!(
                tokio::time::timeout(Duration::from_millis(500), listener.accept())
                    .await
                    .is_err(),
                "TLS failure must not start a plaintext connection"
            );
        });
        let tls = ClientTls::from_rustls("localhost", Arc::new(client_config(CA, true))).unwrap();
        assert!(Channel::connect_tls(addr, tls).await.is_err());
        peer.await.unwrap();
    })
    .await
    .expect("bounded TLS no-downgrade check");
}
