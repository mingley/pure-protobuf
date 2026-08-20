use protobuf_tonic::hello::{HelloReply, HelloRequest};
use protobuf_tonic::ProtobufCodec;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tonic::body::BoxBody;
use tonic::server::{NamedService, UnaryService};
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

const PATH: &str = "/helloworld.Greeter/SayHello";

#[derive(Clone, Default)]
struct Echo;

impl UnaryService<HelloRequest> for Echo {
    type Response = HelloReply;
    type Future = Pin<Box<dyn Future<Output = Result<Response<HelloReply>, Status>> + Send>>;

    fn call(&mut self, req: Request<HelloRequest>) -> Self::Future {
        let name = req.into_inner().name().to_str().unwrap_or("").to_string();
        Box::pin(async move {
            let mut reply = HelloReply::new();
            reply.set_message(name);
            Ok(Response::new(reply))
        })
    }
}

#[derive(Clone, Default)]
struct GreeterServer;

impl NamedService for GreeterServer {
    const NAME: &'static str = "helloworld.Greeter";
}

impl tonic::codegen::Service<http::Request<BoxBody>> for GreeterServer {
    type Response = http::Response<BoxBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<BoxBody>) -> Self::Future {
        Box::pin(async move {
            let mut grpc =
                tonic::server::Grpc::new(ProtobufCodec::<HelloReply, HelloRequest>::default());
            let resp = grpc.unary(Echo, req).await;
            Ok(resp)
        })
    }
}

async fn echo_once(addr: SocketAddr) -> String {
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut grpc = tonic::client::Grpc::new(channel);
    grpc.ready().await.expect("ready");
    let resp: Response<HelloReply> = grpc
        .unary(
            Request::new(req),
            PATH.parse().unwrap(),
            ProtobufCodec::<HelloRequest, HelloReply>::default(),
        )
        .await
        .expect("unary");
    resp.into_inner()
        .message()
        .to_str()
        .unwrap_or("")
        .to_string()
}

#[tokio::test]
async fn unary_echo_twice() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer)
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let a = echo_once(addr).await;
    let b = echo_once(addr).await;
    assert_eq!(a, "ada");
    assert_eq!(b, "ada");
    println!("ok {a}");
}
