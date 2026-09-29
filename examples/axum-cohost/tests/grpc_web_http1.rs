//! gRPC-Web over HTTP/1.1 co-hosting proof for the tower adapter.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods,
    reason = "example integration test"
)]

use bytes::{Buf, Bytes, BytesMut};
use http::{Request as HttpRequest, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use pbrs::{Parse, Serialize};
use pbrs_grpc::codec;
use pbrs_grpc::hello::{Greeter, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Request, Response, Router, Status};
use tokio::net::TcpListener;
use tower::ServiceBuilder;

struct GreeterSvc;

impl Greeter for GreeterSvc {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {}", request.get_ref().name()));
        Ok(Response::new(reply))
    }
}

#[tokio::test]
async fn grpc_web_http1_unary_call_succeeds_through_tower_recipe() {
    let service = Router::new()
        .add_service(GreeterServer::new(GreeterSvc))
        .into_tower_service();
    let service = ServiceBuilder::new()
        .layer(tonic_web::GrpcWebLayer::new())
        .service(service);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let serve = tokio::spawn(async move {
        let (io, _) = listener.accept().await.expect("accept");
        auto::Builder::new(TokioExecutor::new())
            .serve_connection(TokioIo::new(io), TowerToHyperService::new(service))
            .await
            .expect("serve connection");
    });

    let mut request = HelloRequest::new();
    request.set_name("web");
    let body = codec::encode(&Serialize::serialize(&request).expect("serialize"), false)
        .expect("grpc frame");
    let client: Client<HttpConnector, Full<Bytes>> = Client::builder(TokioExecutor::new())
        .http2_only(false)
        .build_http();
    let response = client
        .request(
            HttpRequest::post(format!("http://{addr}/helloworld.Greeter/SayHello"))
                .header("content-type", "application/grpc-web+proto")
                .header("x-grpc-web", "1")
                .body(Full::new(body))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let mut body = BytesMut::from(body.as_ref());
    let reply_frame = codec::pop(&mut body).expect("frame").expect("reply frame");
    assert!(!reply_frame.compressed);
    let reply = HelloReply::parse(&reply_frame.payload).expect("reply parse");
    assert_eq!(reply.message(), "hello web");
    let trailers = body.split().freeze();
    assert!(!trailers.is_empty(), "grpc-web trailers frame missing");
    assert_eq!(
        trailers.chunk().first().copied().unwrap_or_default() & 0x80,
        0x80
    );
    serve.abort();
}
