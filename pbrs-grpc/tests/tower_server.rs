//! Tower server adapter coverage.

#![cfg(feature = "tower")]
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests"
)]

mod common;

use bytes::{Bytes, BytesMut};
use common::{Echo, name_of, req};
use http::{Request, Response};
use http_body::Body;
use http_body_util::Full;
use pbrs::{Parse, Serialize};
use pbrs_grpc::Router;
use pbrs_grpc::codec;
use pbrs_grpc::health::{HealthCheckRequest, HealthCheckResponse, ServingStatus};
use pbrs_grpc::hello::{FILE_DESCRIPTOR_SET, GreeterServer, HelloReply};
use pbrs_grpc::reflection::{ServerReflectionRequest, ServerReflectionResponse};
use std::pin::Pin;
use tower::ServiceExt;

async fn collect(
    response: Response<pbrs_grpc::tower_server::TowerBody>,
) -> (Bytes, Option<http::HeaderMap>) {
    let mut body = response.into_body();
    let mut data = BytesMut::new();
    let mut trailers = None;
    while let Some(frame) = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let frame = frame.expect("infallible body");
        match frame.into_data() {
            Ok(chunk) => data.extend_from_slice(&chunk),
            Err(frame) => {
                trailers = frame.into_trailers().ok();
            }
        }
    }
    (data.freeze(), trailers)
}

fn grpc_request(path: &'static str, payload: Vec<u8>) -> Request<Full<Bytes>> {
    Request::post(path)
        .header("content-type", "application/grpc")
        .body(Full::new(codec::encode(&payload, false).expect("frame")))
        .expect("request")
}

fn first_payload(data: Bytes) -> Bytes {
    let mut buf = BytesMut::from(data.as_ref());
    let frame = codec::pop(&mut buf)
        .expect("frame")
        .expect("one response frame");
    assert!(buf.is_empty());
    frame.payload
}

#[tokio::test]
async fn router_is_tower_service_for_greeter_health_and_reflection() {
    let (health, reporter) = pbrs_grpc::health::service();
    reporter.set_serving("helloworld.Greeter");
    let reflection = pbrs_grpc::reflection::Builder::new()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build()
        .expect("reflection");
    let service = Router::new()
        .add_service(GreeterServer::new(Echo))
        .add_service(health)
        .add_service(reflection)
        .into_tower_service();

    let response = service
        .clone()
        .oneshot(grpc_request(
            "/helloworld.Greeter/SayHello",
            Serialize::serialize(&req("tower")).expect("serialize"),
        ))
        .await
        .expect("tower service");
    assert_eq!(response.status(), http::StatusCode::OK);
    let (data, trailers) = collect(response).await;
    let reply = HelloReply::parse_bytes(first_payload(data)).expect("reply");
    assert_eq!(name_of(&reply), "tower");
    assert_eq!(
        trailers
            .as_ref()
            .and_then(|map| map.get("grpc-status"))
            .and_then(|value| value.to_str().ok()),
        Some("0")
    );

    let mut health_req = HealthCheckRequest::new();
    health_req.set_service("helloworld.Greeter");
    let response = service
        .clone()
        .oneshot(grpc_request(
            "/grpc.health.v1.Health/Check",
            Serialize::serialize(&health_req).expect("serialize health"),
        ))
        .await
        .expect("health");
    let (data, _) = collect(response).await;
    let health = HealthCheckResponse::parse_bytes(first_payload(data)).expect("health response");
    assert_eq!(health.status(), ServingStatus::Serving);

    let mut reflection_req = ServerReflectionRequest::new();
    reflection_req.set_list_services("");
    let response = service
        .oneshot(grpc_request(
            "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo",
            Serialize::serialize(&reflection_req).expect("serialize reflection"),
        ))
        .await
        .expect("reflection");
    let (data, _) = collect(response).await;
    let reflection =
        ServerReflectionResponse::parse_bytes(first_payload(data)).expect("reflection response");
    let names: Vec<String> = reflection
        .list_services_response()
        .service()
        .iter()
        .map(|service| service.name().to_str().unwrap_or_default().to_owned())
        .collect();
    assert!(names.iter().any(|name| name == "helloworld.Greeter"));
}
