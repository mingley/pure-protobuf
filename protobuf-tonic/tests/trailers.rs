use protobuf_tonic::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use std::net::SocketAddr;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

const TRAILER_KEY: &str = "x-pbrs-trail";
const TRAILER_VAL: &str = "ok";

struct WithTrailers;

impl Greeter for WithTrailers {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        let mut reply = HelloReply::new();
        reply.set_message(name);
        let mut response = Response::new(reply);
        response
            .metadata_mut()
            .insert(TRAILER_KEY, TRAILER_VAL.parse().unwrap());
        Ok(response)
    }

    async fn client_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("trailers test"))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("trailers test"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("trailers test"))
    }
}

async fn spawn_with_trailers() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(WithTrailers))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn unary_response_metadata_round_trip() {
    let addr = spawn_with_trailers().await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let resp = client
        .say_hello(Request::new(req))
        .await
        .expect("unary with metadata");
    let got = resp
        .metadata()
        .get(TRAILER_KEY)
        .expect("missing response metadata")
        .to_str()
        .unwrap();
    assert_eq!(got, TRAILER_VAL);
    assert_eq!(resp.into_inner().message().to_str().unwrap_or(""), "ada");
}
