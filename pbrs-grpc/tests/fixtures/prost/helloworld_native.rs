mod helloworld {
    include!(concat!(env!("OUT_DIR"), "/helloworld.rs"));
    include!(concat!(
        env!("OUT_DIR"),
        "/helloworld/helloworld.pbrs_grpc.rs"
    ));
}

use helloworld::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Request, Response, Router, Status};

struct Svc;

impl Greeter for Svc {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(HelloReply {
            message: format!("hello {}", request.into_inner().name),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Status> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Svc))
            .serve_listener(listener)
            .await
            .ok();
    });
    let mut client = GreeterClient::new(Channel::connect(addr).await?);
    let unary = client
        .say_hello(Request::new(HelloRequest { name: "ada".into() }))
        .await?
        .into_inner();
    assert_eq!(unary.message, "hello ada");

    server.abort();
    println!("prost helloworld ok");
    Ok(())
}
