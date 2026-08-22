use futures_util::StreamExt;
use protobuf_tonic::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use std::net::SocketAddr;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status, Streaming};

struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("streaming test"))
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("streaming test"))
    }

    type ServerHelloStream = ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("streaming test"))
    }

    type StreamHelloStream = ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            while let Some(Ok(msg)) = inbound.next().await {
                let mut reply = HelloReply::new();
                reply.set_message(msg.name().to_str().unwrap_or("").to_string());
                if tx.send(Ok(reply)).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

async fn echo_two(addr: SocketAddr) -> Vec<String> {
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");
    let mut client = GreeterClient::new(channel);
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    for name in ["ada", "bob"] {
        let mut req = HelloRequest::new();
        req.set_name(name);
        tx.send(req).await.unwrap();
    }
    drop(tx);
    let mut stream = client
        .stream_hello(Request::new(ReceiverStream::new(rx)))
        .await
        .expect("streaming");
    let mut out = Vec::new();
    while let Some(Ok(msg)) = stream.get_mut().next().await {
        out.push(msg.message().to_str().unwrap_or("").to_string());
    }
    out
}

#[tokio::test]
async fn streaming_echo_twice() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(Echo))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let a = echo_two(addr).await;
    let b = echo_two(addr).await;
    assert_eq!(a, vec!["ada".to_string(), "bob".to_string()]);
    assert_eq!(a, b);
    println!("ok {} {}", a[0], a[1]);
}
