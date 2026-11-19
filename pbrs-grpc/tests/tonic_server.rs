//! Mounting a native generated service in tonic's transport stack.

#![cfg(feature = "tonic")]
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test")]

mod common;

use bytes::Buf;
use common::{Echo, name_of, req};
use http::{Request as HttpRequest, Response as HttpResponse};
use pbrs_grpc::hello::{GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::tower_server::{TonicService, TonicServiceExt};
use std::convert::Infallible;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::body::Body;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::server::NamedService;
use tower::Service;

#[derive(Clone, Copy, Default)]
struct PbrsCodec<E, D>(PhantomData<fn() -> (E, D)>);

impl<E, D> Codec for PbrsCodec<E, D>
where
    E: pbrs::Serialize + Send + 'static,
    D: pbrs::Parse + pbrs::ClearAndParse + Default + Send + 'static,
{
    type Encode = E;
    type Decode = D;
    type Encoder = PbrsEncoder<E>;
    type Decoder = PbrsDecoder<D>;
    fn encoder(&mut self) -> Self::Encoder {
        PbrsEncoder(PhantomData)
    }
    fn decoder(&mut self) -> Self::Decoder {
        PbrsDecoder(PhantomData)
    }
}

#[derive(Clone, Copy, Default)]
struct PbrsEncoder<T>(PhantomData<fn() -> T>);
impl<T: pbrs::Serialize> Encoder for PbrsEncoder<T> {
    type Item = T;
    type Error = tonic::Status;
    fn encode(&mut self, item: T, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        pbrs::Serialize::encode(&item, dst)
            .map_err(|error| tonic::Status::internal(error.to_string()))
    }
}

#[derive(Clone, Copy, Default)]
struct PbrsDecoder<T>(PhantomData<fn() -> T>);
impl<T: pbrs::Parse + pbrs::ClearAndParse + Default> Decoder for PbrsDecoder<T> {
    type Item = T;
    type Error = tonic::Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<T>, Self::Error> {
        pbrs::Parse::parse_bytes(src.copy_to_bytes(src.remaining()))
            .map(Some)
            .map_err(|error| tonic::Status::internal(error.to_string()))
    }
}

#[derive(Clone)]
struct ConnectInfoProbe<S> {
    inner: S,
    observed: Arc<AtomicBool>,
}

impl<S: NamedService> NamedService for ConnectInfoProbe<S> {
    const NAME: &'static str = S::NAME;
}

impl<S> Service<HttpRequest<Body>> for ConnectInfoProbe<S>
where
    S: Service<HttpRequest<Body>, Response = HttpResponse<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = HttpResponse<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: HttpRequest<Body>) -> Self::Future {
        if let Some(info) = request
            .extensions()
            .get::<tonic::transport::server::TcpConnectInfo>()
        {
            self.observed.store(
                info.local_addr().is_some() && info.remote_addr().is_some(),
                Ordering::Release,
            );
        }
        Box::pin(self.inner.call(request))
    }
}

#[tokio::test]
async fn generated_service_mounts_in_tonic_and_exposes_tonic_connect_info() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let observed = Arc::new(AtomicBool::new(false));
    let service: TonicService<GreeterServer<Echo>> = GreeterServer::new(Echo).into_tonic_service();
    let service = ConnectInfoProbe {
        inner: service,
        observed: Arc::clone(&observed),
    };

    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .expect("tonic server");
    });

    let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
        .expect("endpoint")
        .connect()
        .await
        .expect("tonic client");
    let mut client = tonic::client::Grpc::new(channel);
    client.ready().await.expect("ready");
    let response = client
        .unary(
            tonic::Request::new(req("tonic mount")),
            http::uri::PathAndQuery::from_static("/helloworld.Greeter/SayHello"),
            PbrsCodec::<HelloRequest, HelloReply>::default(),
        )
        .await
        .expect("say hello");
    assert_eq!(name_of(response.get_ref()), "tonic mount");
    assert!(observed.load(Ordering::Acquire));
    server.abort();
}

#[tokio::test]
async fn tonic_client_exchanges_bidi_messages_before_closing_the_request() {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        let service = pbrs_grpc::ServiceExt::on_response(
            GreeterServer::new(Echo),
            |parts: &mut pbrs_grpc::ResponseParts| {
                parts.metadata_mut().set("x-bridge-head", "first")?;
                parts.trailers_mut().set("x-bridge-tail", "last")
            },
        );
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(service.into_tonic_service())
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .expect("tonic server");
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .expect("endpoint")
            .connect()
            .await
            .expect("tonic client");
        let mut client = tonic::client::Grpc::new(channel);
        let (upload, receiver) = tokio::sync::mpsc::channel(1);
        upload.send(req("first")).await.expect("first upload");
        client.ready().await.expect("ready");
        let response = client
            .streaming(
                tonic::Request::new(tokio_stream::wrappers::ReceiverStream::new(receiver)),
                http::uri::PathAndQuery::from_static("/helloworld.Greeter/StreamHello"),
                PbrsCodec::<HelloRequest, HelloReply>::default(),
            )
            .await
            .expect("bidi headers before request closes");
        assert_eq!(
            response.metadata().get("x-bridge-head").expect("header"),
            "first"
        );
        let mut replies = response.into_inner();
        let first = replies
            .message()
            .await
            .expect("first reply")
            .expect("message");
        assert_eq!(name_of(&first), "first");

        upload.send(req("second")).await.expect("second upload");
        let second = replies
            .message()
            .await
            .expect("second reply")
            .expect("message");
        assert_eq!(name_of(&second), "second");
        drop(upload);
        assert!(replies.message().await.expect("final status").is_none());
        let trailers = replies
            .trailers()
            .await
            .expect("final trailers")
            .expect("metadata");
        assert_eq!(trailers.get("x-bridge-tail").expect("trailer"), "last");
        server.abort();
    })
    .await
    .expect("bidirectional request and reply turn-taking completes");
}

#[test]
fn tonic_tls_connect_info_keeps_peer_certificate_api_available() {
    fn assert_peer_certs_api(
        info: &tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>,
    ) {
        let _ = info.peer_certs();
    }
    let _ = assert_peer_certs_api;
}
