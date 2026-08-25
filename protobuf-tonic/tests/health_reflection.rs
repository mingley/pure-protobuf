//! Greeter plus gRPC health (SERVING) and server reflection.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests are sync; generated fixtures live in the test crate"
)]
use futures_util::StreamExt;
use protobuf_tonic::hello::{
    Greeter, GreeterServer, HelloReply, HelloRequest, FILE_DESCRIPTOR_SET,
};
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::server::health_reporter;
use tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient;
use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;
use tonic_reflection::pb::v1::ServerReflectionRequest;
use tonic_reflection::server::Builder as ReflectionBuilder;

struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(HelloReply::new()))
    }

    async fn client_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("health"))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("health"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<HelloRequest>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("health"))
    }
}

#[tokio::test]
async fn health_serving_and_reflection_lists_greeter() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (reporter, health_service) = health_reporter();
    reporter.set_serving::<GreeterServer<Echo>>().await;
    let reflection = ReflectionBuilder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("reflection");
    tokio::spawn(async move {
        Server::builder()
            .add_service(health_service)
            .add_service(reflection)
            .add_service(GreeterServer::new(Echo))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let channel = Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .expect("connect");

    let mut health = HealthClient::new(channel.clone());
    let overall = health
        .check(Request::new(HealthCheckRequest {
            service: String::new(),
        }))
        .await
        .expect("health overall")
        .into_inner();
    assert_eq!(overall.status, ServingStatus::Serving as i32);
    let greeter = health
        .check(Request::new(HealthCheckRequest {
            service: "helloworld.Greeter".into(),
        }))
        .await
        .expect("health greeter")
        .into_inner();
    assert_eq!(greeter.status, ServingStatus::Serving as i32);
    println!("SERVING");

    let mut refl = ServerReflectionClient::new(channel);
    let outbound = tokio_stream::once(ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    });
    let mut inbound = refl
        .server_reflection_info(Request::new(outbound))
        .await
        .expect("reflection")
        .into_inner();
    let msg = inbound.next().await.expect("stream").expect("status");
    match msg.message_response {
        Some(MessageResponse::ListServicesResponse(services)) => {
            let names: Vec<_> = services.service.iter().map(|s| s.name.as_str()).collect();
            assert!(
                names.contains(&"helloworld.Greeter"),
                "missing helloworld.Greeter in {names:?}"
            );
            println!("helloworld.Greeter");
        }
        other => panic!("expected ListServicesResponse, got {other:?}"),
    }
}
