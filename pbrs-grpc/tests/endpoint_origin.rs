//! Logical request origins retain the dial target and transport identity.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "bounded raw HTTP/2 integration fixtures and assertions"
)]

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Uri};
use pbrs_grpc::{
    Channel, ChannelConfig, Code, Endpoint, HelloReply, HelloRequest, Request, Status,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

const PATH: &str = "/origin.Test/Call";
const BOUND: Duration = Duration::from_secs(5);

trait PeerIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> PeerIo for T {}

struct Peer {
    addr: SocketAddr,
    received: mpsc::UnboundedReceiver<Uri>,
    server_name: oneshot::Receiver<Option<String>>,
    task: Option<JoinHandle<()>>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

impl Peer {
    async fn start(tls: Option<Arc<rustls::ServerConfig>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (sent, received) = mpsc::unbounded_channel();
        let (name_tx, server_name) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let (io, name): (Box<dyn PeerIo>, _) = match tls {
                Some(config) => {
                    let io = tokio_rustls::TlsAcceptor::from(config)
                        .accept(tcp)
                        .await
                        .unwrap();
                    let name = io.get_ref().1.server_name().map(str::to_owned);
                    (Box::new(io), name)
                }
                None => (Box::new(tcp), None),
            };
            name_tx.send(name).unwrap();
            let mut connection = h2::server::handshake(io).await.unwrap();
            let mut replies = JoinSet::new();
            while let Some(Ok((request, mut respond))) = connection.accept().await {
                sent.send(request.uri().clone()).unwrap();
                replies.spawn(async move {
                    let mut body = request.into_body();
                    while let Some(chunk) = body.data().await {
                        let chunk = chunk.unwrap();
                        body.flow_control().release_capacity(chunk.len()).unwrap();
                    }
                    let response = http::Response::builder()
                        .status(200)
                        .header(http::header::CONTENT_TYPE, "application/grpc")
                        .body(())
                        .unwrap();
                    let mut stream = respond.send_response(response, false).unwrap();
                    stream
                        .send_data(Bytes::from_static(&[0, 0, 0, 0, 0]), false)
                        .unwrap();
                    let mut trailers = HeaderMap::new();
                    trailers.insert("grpc-status", HeaderValue::from_static("0"));
                    stream.send_trailers(trailers).unwrap();
                });
            }
            while let Some(reply) = replies.join_next().await {
                reply.unwrap();
            }
        });
        Self {
            addr,
            received,
            server_name,
            task: Some(task),
        }
    }

    fn endpoint(&self) -> Endpoint {
        Endpoint::from_shared(format!("http://{}/dial-path?ignored=1", self.addr)).unwrap()
    }

    async fn assert_requests(&mut self, scheme: &str, authority: &str, count: usize) {
        for _ in 0..count {
            let uri = tokio::time::timeout(BOUND, self.received.recv())
                .await
                .expect("request reached independent h2 peer")
                .expect("peer observation channel");
            assert_eq!(uri.scheme_str(), Some(scheme));
            assert_eq!(uri.authority().map(|a| a.as_str()), Some(authority));
            assert_eq!(uri.path_and_query().map(|p| p.as_str()), Some(PATH));
        }
    }

    async fn stop(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            if let Err(error) = task.await {
                assert!(error.is_cancelled(), "peer task failed: {error}");
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Connect {
    Eager,
    Lazy,
    Custom,
}

async fn connect(endpoint: &Endpoint, mode: Connect) -> Channel {
    match mode {
        Connect::Eager => endpoint.connect().await.unwrap(),
        Connect::Lazy => endpoint.connect_lazy().unwrap(),
        Connect::Custom => {
            let expected = endpoint.uri().clone();
            endpoint
                .connect_with_connector(move |target| async move {
                    assert_eq!(target, expected, "connector sees dial URI, not origin");
                    TcpStream::connect(target.authority().unwrap().as_str())
                        .await
                        .map_err(|e| Status::unavailable(e.to_string()))
                })
                .await
                .unwrap()
        }
    }
}

async fn all_native_shapes(channel: &Channel) {
    tokio::time::timeout(BOUND, native_shapes(channel))
        .await
        .expect("all four native call shapes complete");
}

async fn native_shapes(channel: &Channel) {
    channel
        .unary::<HelloRequest, HelloReply>(PATH, Request::new(HelloRequest::new()))
        .await
        .unwrap();
    let mut stream = channel
        .server_streaming::<HelloRequest, HelloReply>(PATH, Request::new(HelloRequest::new()))
        .await
        .unwrap()
        .into_inner();
    assert!(stream.message().await.unwrap().is_some());
    assert!(stream.message().await.unwrap().is_none());
    let (sender, call) =
        channel.client_streaming::<HelloRequest, HelloReply>(PATH, Request::new(()));
    sender.send(HelloRequest::new()).await.unwrap();
    sender.close();
    call.await.unwrap();
    let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(PATH, Request::new(()));
    sender.send(HelloRequest::new()).await.unwrap();
    sender.close();
    let mut stream = call.await.unwrap().into_inner();
    assert!(stream.message().await.unwrap().is_some());
    assert!(stream.message().await.unwrap().is_none());
}

fn require_plain_transport(call: &mut pbrs_grpc::Outgoing<'_>) -> Result<(), Status> {
    assert_eq!(call.scheme(), "http", "logical origin does not assert TLS");
    Ok(())
}

#[tokio::test]
async fn origins_preserve_method_paths_for_all_shapes_and_connection_modes() {
    // The same path and authority exercise cached standard headers around
    // custom-scheme requests on this single-thread runtime.
    for scheme in ["http", "custom", "https", "other", "http"] {
        for mode in [Connect::Eager, Connect::Lazy, Connect::Custom] {
            let mut peer = Peer::start(None).await;
            let endpoint = peer.endpoint().origin(
                format!("{scheme}://virtual.example:7443/ignored-origin?ignored=2")
                    .parse()
                    .unwrap(),
            );
            assert_eq!(
                endpoint.uri().authority().unwrap().as_str(),
                peer.addr.to_string()
            );
            let channel = connect(&endpoint, mode)
                .await
                .intercept(require_plain_transport);
            assert_eq!(channel.scheme(), "http");
            assert_eq!(channel.request_scheme().unwrap(), scheme);
            assert_eq!(channel.authority(), "virtual.example:7443");
            all_native_shapes(&channel).await;
            peer.assert_requests(scheme, "virtual.example:7443", 4)
                .await;
            drop(channel);
            peer.stop().await;
        }
    }
}

#[tokio::test]
async fn connected_authority_overrides_are_clone_local_and_keep_logical_scheme() {
    let mut peer = Peer::start(None).await;
    let channel = peer
        .endpoint()
        .origin("custom://original.example/path?ignored=1".parse().unwrap())
        .connect()
        .await
        .unwrap();
    let other = channel.clone().origin("replacement.example:8080").unwrap();
    assert_eq!(channel.authority(), "original.example");
    assert_eq!(other.authority(), "replacement.example:8080");
    assert_eq!(channel.scheme(), "http");
    assert_eq!(other.scheme(), "http");
    all_native_shapes(&channel).await;
    all_native_shapes(&other).await;
    all_native_shapes(&channel).await;
    peer.assert_requests("custom", "original.example", 4).await;
    peer.assert_requests("custom", "replacement.example:8080", 4)
        .await;
    peer.assert_requests("custom", "original.example", 4).await;
    drop(other);
    drop(channel);
    peer.stop().await;
}

static ENCODED: AtomicUsize = AtomicUsize::new(0);

struct Counted;
impl pbrs_grpc::CodecMessage for Counted {
    fn encoded_len(&self) -> usize {
        ENCODED.fetch_add(1, Ordering::SeqCst);
        0
    }
    fn encode_payload<W: pbrs::WireOut>(&self, _: &mut W) -> Result<(), Status> {
        ENCODED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn decode_payload(_: Bytes) -> Result<Self, Status> {
        Ok(Self)
    }
    fn empty() -> Self {
        Self
    }
}

#[tokio::test]
async fn invalid_origins_fail_calls_before_hooks_encoding_or_request_headers() {
    for origin in ["/relative", "authority.example:80"] {
        let mut peer = Peer::start(None).await;
        let channel = peer
            .endpoint()
            .origin(origin.parse().unwrap())
            .connect()
            .await
            .unwrap()
            .intercept(|_: &mut pbrs_grpc::Outgoing<'_>| {
                panic!("invalid origin must fail before client hooks")
            });
        let result = channel
            .unary::<Counted, Counted>(PATH, Request::new(Counted))
            .await;
        assert_eq!(result.err().unwrap().code(), Code::InvalidArgument);
        let result = channel
            .server_streaming::<Counted, Counted>(PATH, Request::new(Counted))
            .await;
        assert_eq!(result.err().unwrap().code(), Code::InvalidArgument);
        let (sender, call) = channel.client_streaming::<Counted, Counted>(PATH, Request::new(()));
        sender.close();
        assert_eq!(call.await.err().unwrap().code(), Code::InvalidArgument);
        let (sender, call) = channel.bidi::<Counted, Counted>(PATH, Request::new(()));
        sender.close();
        assert_eq!(call.await.err().unwrap().code(), Code::InvalidArgument);
        assert_eq!(ENCODED.load(Ordering::SeqCst), 0);
        assert!(
            channel.connected(),
            "invalid origin does not evict healthy IO"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(10), peer.received.recv())
                .await
                .is_err()
        );
        drop(channel);
        peer.stop().await;
    }
}

#[tokio::test]
async fn static_and_watched_balance_helpers_reject_origins_explicitly() {
    let endpoint = Endpoint::from_static("http://127.0.0.1:1")
        .unwrap()
        .origin("https://virtual.example".parse().unwrap());
    let error = Endpoint::balance_list([endpoint.clone()], ChannelConfig::default())
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(error.message().contains("origins"));
    let (_updates, endpoints) = tokio::sync::watch::channel(vec![endpoint]);
    let error = Endpoint::balance_channel(endpoints, ChannelConfig::default())
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(error.message().contains("origins"));
}

fn tls_configs() -> (Arc<rustls::ServerConfig>, Arc<rustls::ClientConfig>) {
    let provider = Arc::new(rustls_graviola::default_provider());
    let certs = rustls_pemfile::certs(&mut include_bytes!("tls_data/server.crt").as_slice())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = rustls_pemfile::private_key(&mut include_bytes!("tls_data/server.key").as_slice())
        .unwrap()
        .unwrap();
    let mut server = rustls::ServerConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    server.alpn_protocols = vec![b"h2".to_vec()];
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut include_bytes!("tls_data/ca.crt").as_slice()) {
        roots.add(cert.unwrap()).unwrap();
    }
    let mut client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    client.alpn_protocols = vec![b"h2".to_vec()];
    (Arc::new(server), Arc::new(client))
}

fn require_secure_transport(call: &mut pbrs_grpc::Outgoing<'_>) -> Result<(), Status> {
    assert_eq!(
        call.scheme(),
        "https",
        "origin cannot downgrade TLS context"
    );
    Ok(())
}

#[tokio::test]
async fn origin_does_not_change_tls_sni_verification_or_security_context() {
    for scheme in ["http", "custom", "https"] {
        let (server, client) = tls_configs();
        let mut peer = Peer::start(Some(server)).await;
        let endpoint = Endpoint::from_shared(format!("https://localhost:{}", peer.addr.port()))
            .unwrap()
            .origin(
                format!("{scheme}://not-in-certificate.example/ignored")
                    .parse()
                    .unwrap(),
            );
        let addr = peer.addr;
        let channel = endpoint
            .connect_with_connector(move |uri| async move {
                assert_eq!(uri.host(), Some("localhost"));
                let server_name =
                    rustls::pki_types::ServerName::try_from(uri.host().unwrap().to_owned())
                        .unwrap();
                let tcp = TcpStream::connect(addr).await.unwrap();
                tokio_rustls::TlsConnector::from(client)
                    .connect(server_name, tcp)
                    .await
                    .map_err(|error| Status::unavailable(error.to_string()))
            })
            .await
            .unwrap()
            .intercept(require_secure_transport);
        assert_eq!(channel.scheme(), "https");
        assert_eq!(channel.request_scheme().unwrap(), scheme);
        assert_eq!(channel.authority(), "not-in-certificate.example");
        all_native_shapes(&channel).await;
        assert_eq!(
            (&mut peer.server_name).await.unwrap().as_deref(),
            Some("localhost")
        );
        peer.assert_requests(scheme, "not-in-certificate.example", 4)
            .await;
        drop(channel);
        peer.stop().await;
    }
}

#[cfg(feature = "tonic")]
mod tonic_origin {
    use super::*;
    use bytes::Buf;
    use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

    #[derive(Clone, Default)]
    struct EmptyCodec;
    struct EmptyEncoder;
    struct EmptyDecoder;
    impl Codec for EmptyCodec {
        type Encode = ();
        type Decode = ();
        type Encoder = EmptyEncoder;
        type Decoder = EmptyDecoder;

        fn encoder(&mut self) -> Self::Encoder {
            EmptyEncoder
        }

        fn decoder(&mut self) -> Self::Decoder {
            EmptyDecoder
        }
    }

    impl Encoder for EmptyEncoder {
        type Item = ();
        type Error = tonic::Status;

        fn encode(&mut self, _: (), _: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl Decoder for EmptyDecoder {
        type Item = ();
        type Error = tonic::Status;

        fn decode(&mut self, input: &mut DecodeBuf<'_>) -> Result<Option<()>, Self::Error> {
            assert_eq!(input.remaining(), 0);
            Ok(Some(()))
        }
    }

    async fn all_shapes<T>(client: &mut tonic::client::Grpc<T>)
    where
        T: tonic::client::GrpcService<tonic::body::Body, ResponseBody = tonic::body::Body>,
        T::Error: Into<tonic::codegen::StdError> + std::fmt::Debug,
    {
        tokio::time::timeout(BOUND, shapes(client))
            .await
            .expect("all four tonic call shapes complete");
    }

    async fn shapes<T>(client: &mut tonic::client::Grpc<T>)
    where
        T: tonic::client::GrpcService<tonic::body::Body, ResponseBody = tonic::body::Body>,
        T::Error: Into<tonic::codegen::StdError> + std::fmt::Debug,
    {
        client.ready().await.unwrap();
        client
            .unary(
                tonic::Request::new(()),
                http::uri::PathAndQuery::from_static(PATH),
                EmptyCodec,
            )
            .await
            .unwrap();
        client.ready().await.unwrap();
        let mut stream = client
            .server_streaming(
                tonic::Request::new(()),
                http::uri::PathAndQuery::from_static(PATH),
                EmptyCodec,
            )
            .await
            .unwrap()
            .into_inner();
        assert!(stream.message().await.unwrap().is_some());
        assert!(stream.message().await.unwrap().is_none());
        client.ready().await.unwrap();
        client
            .client_streaming(
                tonic::Request::new(tokio_stream::iter([()])),
                http::uri::PathAndQuery::from_static(PATH),
                EmptyCodec,
            )
            .await
            .unwrap();
        client.ready().await.unwrap();
        let mut stream = client
            .streaming(
                tonic::Request::new(tokio_stream::iter([()])),
                http::uri::PathAndQuery::from_static(PATH),
                EmptyCodec,
            )
            .await
            .unwrap()
            .into_inner();
        assert!(stream.message().await.unwrap().is_some());
        assert!(stream.message().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn reference_tonic_uses_the_same_scheme_authority_and_method_paths() {
        for scheme in ["http", "https", "custom"] {
            let mut peer = Peer::start(None).await;
            let channel = tonic::transport::Endpoint::from_shared(format!("http://{}", peer.addr))
                .unwrap()
                .origin(
                    format!("{scheme}://virtual.example:7443/ignored?discard=1")
                        .parse()
                        .unwrap(),
                )
                .connect()
                .await
                .unwrap();
            let mut client = tonic::client::Grpc::new(channel);
            all_shapes(&mut client).await;
            peer.assert_requests(scheme, "virtual.example:7443", 4)
                .await;
            drop(client);
            peer.stop().await;
        }
    }

    #[tokio::test]
    async fn native_opaque_tonic_adapter_preserves_endpoint_origins() {
        for scheme in ["http", "https", "custom"] {
            let mut peer = Peer::start(None).await;
            let channel = peer
                .endpoint()
                .origin(
                    format!("{scheme}://virtual.example:7443/ignored?discard=1")
                        .parse()
                        .unwrap(),
                )
                .connect()
                .await
                .unwrap();
            assert_eq!(channel.scheme(), "http");
            let mut client = tonic::client::Grpc::new(channel);
            all_shapes(&mut client).await;
            peer.assert_requests(scheme, "virtual.example:7443", 4)
                .await;
            drop(client);
            peer.stop().await;
        }
    }

    #[tokio::test]
    async fn invalid_reference_and_native_opaque_origins_emit_no_headers() {
        for origin in ["/relative", "authority.example:80"] {
            let mut peer = Peer::start(None).await;
            let channel = tonic::transport::Endpoint::from_shared(format!("http://{}", peer.addr))
                .unwrap()
                .origin(origin.parse().unwrap())
                .connect()
                .await
                .unwrap();
            let mut client = tonic::client::Grpc::new(channel);
            client.ready().await.unwrap();
            assert!(
                client
                    .unary(
                        tonic::Request::new(()),
                        http::uri::PathAndQuery::from_static(PATH),
                        EmptyCodec
                    )
                    .await
                    .is_err()
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(10), peer.received.recv())
                    .await
                    .is_err()
            );
            drop(client);
            peer.stop().await;

            let mut peer = Peer::start(None).await;
            let channel = peer
                .endpoint()
                .origin(origin.parse().unwrap())
                .connect()
                .await
                .unwrap();
            let mut client = tonic::client::Grpc::new(channel);
            client.ready().await.unwrap();
            let error = client
                .unary(
                    tonic::Request::new(()),
                    http::uri::PathAndQuery::from_static(PATH),
                    EmptyCodec,
                )
                .await
                .unwrap_err();
            assert_eq!(error.code(), tonic::Code::InvalidArgument);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), peer.received.recv())
                    .await
                    .is_err()
            );
            drop(client);
            peer.stop().await;
        }
    }
}
