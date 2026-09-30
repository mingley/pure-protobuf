//! Provider and policy compatibility in an isolated consuming application.

#![allow(
    clippy::disallowed_types,
    reason = "rustls resolver/verifier callbacks are synchronous; these short mutex guards never cross an await"
)]

#[path = "../../common/mod.rs"]
mod common;

use common::{Echo, ServerGuard, name_of, req};
use pbrs_grpc::hello::{GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::tower_server::TonicServiceExt;
use pbrs_grpc::{Channel, ClientTls, Code, Identity, Request, ServerTls};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

const CA: &str = include_str!("../../tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("../../tls_data/server.crt");
const SERVER_KEY: &str = include_str!("../../tls_data/server.key");
const CLIENT_CERT: &str = include_str!("../../tls_data/client.crt");
const CLIENT_KEY: &str = include_str!("../../tls_data/client.key");
const ROTATION_CA: &str = include_str!("../data/ca.pem");
const FIRST_CERT: &str = include_str!("../data/first.crt");
const FIRST_KEY: &str = include_str!("../data/first.key");
const SECOND_CERT: &str = include_str!("../data/second.crt");
const SECOND_KEY: &str = include_str!("../data/second.key");

fn certs(pem: &str) -> Vec<CertificateDer<'static>> {
    rustls_pemfile::certs(&mut pem.as_bytes())
        .collect::<Result<_, _>>()
        .unwrap()
}

fn key(pem: &str) -> PrivateKeyDer<'static> {
    rustls_pemfile::private_key(&mut pem.as_bytes())
        .unwrap()
        .unwrap()
}

fn roots(pem: &str) -> rustls::RootCertStore {
    let mut store = rustls::RootCertStore::empty();
    for cert in certs(pem) {
        store.add(cert).unwrap();
    }
    store
}

fn ring() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn server_config(cert: &str, private_key: &str) -> rustls::ServerConfig {
    let verifier =
        rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots(CA)), ring())
            .build()
            .unwrap();
    let mut config = rustls::ServerConfig::builder_with_provider(ring())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs(cert), key(private_key))
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    config
}

fn client_config(ca: &str) -> rustls::ClientConfig {
    let mut config = rustls::ClientConfig::builder_with_provider(ring())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots(ca))
        .with_client_auth_cert(certs(CLIENT_CERT), key(CLIENT_KEY))
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    config
}

async fn serve(tls: ServerTls) -> (SocketAddr, ServerGuard) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .unwrap();
    });
    (addr, ServerGuard(task))
}

async fn every_shape(client: &GreeterClient) {
    assert_eq!(
        name_of(
            client
                .say_hello(Request::new(req("caller")))
                .await
                .unwrap()
                .get_ref()
        ),
        "caller"
    );
    let mut stream = client
        .server_hello(Request::new(req("caller")))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(name_of(&stream.message().await.unwrap().unwrap()), "caller");
    assert!(stream.message().await.unwrap().is_none());
    let (send, call) = client.client_hello(Request::new(()));
    send.send(req("caller")).await.unwrap();
    send.close();
    assert_eq!(name_of(call.await.unwrap().get_ref()), "caller");
    let (send, call) = client.stream_hello(Request::new(()));
    send.send(req("caller")).await.unwrap();
    send.close();
    let mut stream = call.await.unwrap().into_inner();
    assert_eq!(name_of(&stream.message().await.unwrap().unwrap()), "caller");
    assert!(stream.message().await.unwrap().is_none());
}

#[tokio::test]
async fn caller_ring_client_calls_graviola_mtls_every_shape() {
    let (addr, _server) =
        serve(ServerTls::mtls(Identity::from_pem(SERVER_CERT, SERVER_KEY).unwrap(), CA).unwrap())
            .await;
    let tls = ClientTls::from_rustls("localhost", Arc::new(client_config(CA))).unwrap();
    every_shape(&GreeterClient::connect_tls(addr, tls).await.unwrap()).await;
}

#[tokio::test]
async fn graviola_client_calls_caller_ring_mtls_every_shape() {
    let (addr, _server) =
        serve(ServerTls::from_rustls(Arc::new(server_config(SERVER_CERT, SERVER_KEY))).unwrap())
            .await;
    let tls = ClientTls::ca_mtls(
        "localhost",
        CA,
        Identity::from_pem(CLIENT_CERT, CLIENT_KEY).unwrap(),
    )
    .unwrap();
    every_shape(&GreeterClient::connect_tls(addr, tls).await.unwrap()).await;
}

#[tokio::test]
async fn caller_ring_client_calls_tonic_mtls_every_shape() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = tonic::transport::ServerTlsConfig::new()
        .identity(tonic::transport::Identity::from_pem(
            SERVER_CERT,
            SERVER_KEY,
        ))
        .client_ca_root(tonic::transport::Certificate::from_pem(CA));
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .tls_config(config)
            .unwrap()
            .add_service(GreeterServer::new(Echo).into_tonic_service())
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let _server = ServerGuard(task);
    let tls = ClientTls::from_rustls("localhost", Arc::new(client_config(CA))).unwrap();
    every_shape(&GreeterClient::connect_tls(addr, tls).await.unwrap()).await;
}

#[tokio::test]
async fn tonic_client_calls_caller_ring_mtls() {
    let (addr, _server) =
        serve(ServerTls::from_rustls(Arc::new(server_config(SERVER_CERT, SERVER_KEY))).unwrap())
            .await;
    let tls = tonic::transport::ClientTlsConfig::new()
        .domain_name("localhost")
        .ca_certificate(tonic::transport::Certificate::from_pem(CA))
        .identity(tonic::transport::Identity::from_pem(
            CLIENT_CERT,
            CLIENT_KEY,
        ));
    let channel = tonic::transport::Endpoint::from_shared(format!("https://{addr}"))
        .unwrap()
        .tls_config(tls)
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = tonic::client::Grpc::new(channel);
    client.ready().await.unwrap();
    let reply = client
        .unary(
            tonic::Request::new(req("caller")),
            tonic::codegen::http::uri::PathAndQuery::from_static("/helloworld.Greeter/SayHello"),
            protobuf_tonic::ProtobufCodec::<HelloRequest, HelloReply>::default(),
        )
        .await
        .unwrap();
    assert_eq!(name_of(reply.get_ref()), "caller");
}

#[derive(Debug)]
struct RotatingResolver {
    current: Mutex<Arc<CertifiedKey>>,
}

impl ResolvesServerCert for RotatingResolver {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.current.lock().unwrap().clone())
    }
}

#[derive(Debug)]
struct RecordingVerifier {
    inner: Arc<rustls::client::WebPkiServerVerifier>,
    leaves: Mutex<Vec<Vec<u8>>>,
    reject: AtomicBool,
}

impl ServerCertVerifier for RecordingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let verified = self
            .inner
            .verify_server_cert(end_entity, intermediates, name, ocsp, now)?;
        self.leaves.lock().unwrap().push(end_entity.to_vec());
        if self.reject.load(Ordering::SeqCst) {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        } else {
            Ok(verified)
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn recording_client(ca: &str) -> (ClientTls, Arc<RecordingVerifier>) {
    let verifier = Arc::new(RecordingVerifier {
        inner: rustls::client::WebPkiServerVerifier::builder_with_provider(
            Arc::new(roots(ca)),
            ring(),
        )
        .build()
        .unwrap(),
        leaves: Mutex::new(Vec::new()),
        reject: AtomicBool::new(false),
    });
    let mut config = rustls::ClientConfig::builder_with_provider(ring())
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_client_auth_cert(certs(CLIENT_CERT), key(CLIENT_KEY))
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    config.resumption = rustls::client::Resumption::disabled();
    (
        ClientTls::from_rustls("localhost", Arc::new(config)).unwrap(),
        verifier,
    )
}

#[tokio::test]
async fn caller_resolver_rotates_new_full_handshakes_without_rebuilding_server() {
    let first =
        Arc::new(CertifiedKey::from_der(certs(FIRST_CERT), key(FIRST_KEY), &ring()).unwrap());
    let second =
        Arc::new(CertifiedKey::from_der(certs(SECOND_CERT), key(SECOND_KEY), &ring()).unwrap());
    let resolver = Arc::new(RotatingResolver {
        current: Mutex::new(first),
    });
    let mut config = server_config(FIRST_CERT, FIRST_KEY);
    config.cert_resolver = resolver.clone();
    let (addr, _server) = serve(ServerTls::from_rustls(Arc::new(config)).unwrap()).await;
    let (tls, verifier) = recording_client(ROTATION_CA);
    let existing = GreeterClient::connect_tls(addr, tls.clone()).await.unwrap();
    every_shape(&existing).await;
    *resolver.current.lock().unwrap() = second;
    every_shape(&existing).await;
    let fresh = GreeterClient::connect_tls(addr, tls).await.unwrap();
    every_shape(&fresh).await;
    assert_eq!(
        *verifier.leaves.lock().unwrap(),
        vec![
            certs(FIRST_CERT)[0].to_vec(),
            certs(SECOND_CERT)[0].to_vec()
        ]
    );
}

#[tokio::test]
async fn caller_verifier_rejection_propagates_without_replacing_its_policy() {
    let (addr, _server) =
        serve(ServerTls::mtls(Identity::from_pem(SERVER_CERT, SERVER_KEY).unwrap(), CA).unwrap())
            .await;
    let (tls, verifier) = recording_client(CA);
    every_shape(&GreeterClient::connect_tls(addr, tls.clone()).await.unwrap()).await;
    verifier.reject.store(true, Ordering::SeqCst);
    let err = Channel::connect_tls(addr, tls)
        .await
        .expect_err("caller verifier rejected peer");
    assert_eq!(err.code(), Code::Unauthenticated);
    assert_eq!(verifier.leaves.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn caller_configs_still_require_negotiated_h2_in_both_directions() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut config = server_config(SERVER_CERT, SERVER_KEY);
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let _tls = tokio_rustls::TlsAcceptor::from(Arc::new(config))
            .accept(tcp)
            .await
            .unwrap();
    });
    let _raw_server = ServerGuard(task);
    let mut config = client_config(CA);
    config.alpn_protocols.push(b"http/1.1".to_vec());
    let err = Channel::connect_tls(
        addr,
        ClientTls::from_rustls("localhost", Arc::new(config)).unwrap(),
    )
    .await
    .expect_err("h2 was not negotiated");
    assert_eq!(err.code(), Code::Unauthenticated);

    let mut config = server_config(SERVER_CERT, SERVER_KEY);
    config.alpn_protocols = vec![b"http/1.1".to_vec(), b"h2".to_vec()];
    let (addr, _server) = serve(ServerTls::from_rustls(Arc::new(config)).unwrap()).await;
    let mut config = client_config(CA);
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .unwrap();
    assert_eq!(tls.get_ref().1.alpn_protocol(), Some(&b"http/1.1"[..]));
    let mut byte = [0u8; 1];
    let read = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::io::AsyncReadExt::read(&mut tls, &mut byte),
    )
    .await
    .unwrap();
    assert!(
        matches!(read, Ok(0) | Err(_)),
        "native server sent HTTP/2 bytes after non-h2 negotiation"
    );
}

#[tokio::test]
async fn caller_protocol_restrictions_are_preserved() {
    let verifier =
        rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots(CA)), ring())
            .build()
            .unwrap();
    let mut server = rustls::ServerConfig::builder_with_provider(ring())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs(SERVER_CERT), key(SERVER_KEY))
        .unwrap();
    server.alpn_protocols = vec![b"h2".to_vec()];
    let mut client = rustls::ClientConfig::builder_with_provider(ring())
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_root_certificates(roots(CA))
        .with_client_auth_cert(certs(CLIENT_CERT), key(CLIENT_KEY))
        .unwrap();
    client.alpn_protocols = vec![b"h2".to_vec()];
    let (addr, _server) = serve(ServerTls::from_rustls(Arc::new(server)).unwrap()).await;
    let err = Channel::connect_tls(
        addr,
        ClientTls::from_rustls("localhost", Arc::new(client)).unwrap(),
    )
    .await
    .expect_err("caller TLS versions have no intersection");
    assert_eq!(err.code(), Code::Unauthenticated);
}
