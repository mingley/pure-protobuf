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
use http_body::{Body, Frame};
use http_body_util::{BodyExt, Full, StreamBody};
use pbrs::{Parse, Serialize};
use pbrs_grpc::Router;
use pbrs_grpc::codec;
use pbrs_grpc::health::{HealthCheckRequest, HealthCheckResponse, ServingStatus};
use pbrs_grpc::hello::{FILE_DESCRIPTOR_SET, GreeterServer, HelloReply};
use pbrs_grpc::reflection::{ServerReflectionRequest, ServerReflectionResponse};
use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
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

const BRIDGE_TEST_TIMEOUT: Duration = Duration::from_secs(8);

#[tokio::test]
async fn router_delivers_large_replies_past_the_bridge_window() {
    let service = Router::new()
        .add_service(GreeterServer::new(Echo))
        .on_response(|parts: &mut pbrs_grpc::ResponseParts| {
            parts.metadata_mut().set("x-bridge-head", "first")?;
            parts.trailers_mut().set("x-bridge-tail", "last")
        })
        .into_tower_service()
        .duplex_buffer(64 * 1024);
    let value = "x".repeat(256 * 1024);
    let request = grpc_request(
        "/helloworld.Greeter/SayHello",
        Serialize::serialize(&req(&value)).expect("serialize"),
    );

    let (data, trailers) = tokio::time::timeout(BRIDGE_TEST_TIMEOUT, async {
        let response = service.oneshot(request).await.expect("tower service");
        assert_eq!(
            response.headers().get("x-bridge-head").expect("header"),
            "first"
        );
        collect(response).await
    })
    .await
    .expect("the reply must finish without exhausting bridge credit");
    let reply = HelloReply::parse_bytes(first_payload(data)).expect("reply");
    assert_eq!(name_of(&reply), value);
    let trailers = trailers.expect("terminal status");
    assert_eq!(trailers.get("grpc-status").expect("status"), "0");
    assert_eq!(trailers.get("x-bridge-tail").expect("trailer"), "last");
}

#[tokio::test]
async fn router_rejects_an_unknown_method_without_waiting_for_upload() {
    let service = Router::new()
        .add_service(GreeterServer::new(Echo))
        .into_tower_service();
    let (upload, receiver) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(1);
    let request = Request::post("/helloworld.Greeter/Unknown")
        .header("content-type", "application/grpc")
        .body(StreamBody::new(ReceiverStream::new(receiver)))
        .expect("request");

    let response = tokio::time::timeout(BRIDGE_TEST_TIMEOUT, service.oneshot(request))
        .await
        .expect("the status must arrive while the upload is open")
        .expect("tower service");
    assert_eq!(response.headers().get("grpc-status").expect("status"), "12");
    assert!(response.body().is_end_stream());
    tokio::time::timeout(BRIDGE_TEST_TIMEOUT, upload.closed())
        .await
        .expect("the terminal response must drop the upload");
}

type Upload = tokio::sync::mpsc::Sender<Result<Frame<Bytes>, &'static str>>;

async fn start_echo_bidi(
    grpc_timeout: Option<&str>,
) -> (Upload, Response<pbrs_grpc::tower_server::TowerBody>) {
    let service = Router::new()
        .add_service(GreeterServer::new(Echo))
        .into_tower_service();
    let (upload, receiver) = tokio::sync::mpsc::channel(1);
    let frame = codec::encode(
        &Serialize::serialize(&req("first")).expect("serialize"),
        false,
    )
    .expect("frame");
    upload.send(Ok(Frame::data(frame))).await.expect("send");
    let mut request =
        Request::post("/helloworld.Greeter/StreamHello").header("content-type", "application/grpc");
    if let Some(timeout) = grpc_timeout {
        request = request.header("grpc-timeout", timeout);
    }
    let request = request
        .body(StreamBody::new(ReceiverStream::new(receiver)))
        .expect("request");

    let mut response = tokio::time::timeout(BRIDGE_TEST_TIMEOUT, service.oneshot(request))
        .await
        .expect("the reply must start while the upload is open")
        .expect("tower service");
    let data = tokio::time::timeout(BRIDGE_TEST_TIMEOUT, response.body_mut().frame())
        .await
        .expect("receive first data")
        .expect("first frame")
        .expect("infallible body")
        .into_data()
        .expect("data before terminal status");
    assert!(!data.is_empty());
    assert!(!upload.is_closed());
    (upload, response)
}

#[tokio::test]
async fn dropping_router_body_releases_pending_upload() {
    let (upload, response) = start_echo_bidi(None).await;
    drop(response);
    tokio::time::timeout(BRIDGE_TEST_TIMEOUT, upload.closed())
        .await
        .expect("dropping the response body must drop the upload");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_error_after_headers_arrives_in_trailers() {
    let (upload, response) = start_echo_bidi(None).await;
    upload
        .send(Err("late upload error\n%"))
        .await
        .expect("send error");
    let (_, trailers) = tokio::time::timeout(BRIDGE_TEST_TIMEOUT, collect(response))
        .await
        .expect("error must terminate the response");
    let trailers = trailers.expect("trailers");
    assert_eq!(trailers.get("grpc-status").expect("code"), "13");
    assert_eq!(
        trailers.get("grpc-message").expect("message"),
        "late upload error%0A%25"
    );
    tokio::time::timeout(BRIDGE_TEST_TIMEOUT, upload.closed())
        .await
        .expect("terminal error must drop the upload");
}

#[tokio::test(start_paused = true)]
async fn deadline_after_headers_ends_pending_upload() {
    let (upload, response) = start_echo_bidi(Some("1S")).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    let (_, trailers) = tokio::time::timeout(BRIDGE_TEST_TIMEOUT, collect(response))
        .await
        .expect("deadline must terminate the response");
    let trailers = trailers.expect("trailers");
    assert_eq!(trailers.get("grpc-status").expect("code"), "4");
    tokio::time::timeout(BRIDGE_TEST_TIMEOUT, upload.closed())
        .await
        .expect("deadline trailers must drop the upload");
}

#[tokio::test]
async fn request_decode_limit_rejects_before_upload_ends() {
    let service = pbrs_grpc::Server::new(GreeterServer::new(Echo))
        .max_decoding_message_size(8)
        .into_router()
        .into_tower_service();
    let (upload, receiver) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(1);
    let frame = codec::encode(
        &Serialize::serialize(&req("longer than eight bytes")).expect("serialize"),
        false,
    )
    .expect("frame");
    upload.send(Ok(Frame::data(frame))).await.expect("send");
    let request = Request::post("/helloworld.Greeter/SayHello")
        .header("content-type", "application/grpc")
        .body(StreamBody::new(ReceiverStream::new(receiver)))
        .expect("request");

    let response = tokio::time::timeout(BRIDGE_TEST_TIMEOUT, service.oneshot(request))
        .await
        .expect("must apply the message cap before upload ends")
        .expect("tower service");
    assert_eq!(response.headers().get("grpc-status").expect("status"), "8");
    assert!(response.body().is_end_stream());
    tokio::time::timeout(BRIDGE_TEST_TIMEOUT, upload.closed())
        .await
        .expect("rejection must drop the upload");
}

#[tokio::test]
async fn router_reports_upload_error_as_grpc_status() {
    let service = Router::new()
        .add_service(GreeterServer::new(Echo))
        .into_tower_service();
    let frames = futures_util::stream::iter([Err::<Frame<Bytes>, _>("upload failed\n%")]);
    let request = Request::post("/helloworld.Greeter/SayHello")
        .header("content-type", "application/grpc")
        .body(StreamBody::new(frames))
        .expect("request");

    let response = tokio::time::timeout(BRIDGE_TEST_TIMEOUT, service.oneshot(request))
        .await
        .expect("error reaches caller")
        .expect("tower service");
    assert_eq!(response.headers().get("grpc-status").expect("status"), "13");
    assert_eq!(
        response.headers().get("grpc-message").expect("message"),
        "upload failed%0A%25"
    );
    assert!(response.body().is_end_stream());
}
