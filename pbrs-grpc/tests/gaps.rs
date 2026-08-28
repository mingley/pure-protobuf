//! OK-path custom trailers, DEADLINE_EXCEEDED, CANCELLED on shipped APIs.

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
    reason = "integration tests"
)]

mod common;

use common::{req, spawn_greeter};
use pbrs_grpc::hello::{Greeter, HelloReply, HelloRequest};
use pbrs_grpc::{Code, Request, Response, Status, Streaming};
use std::time::Duration;

const TRAILER_BIN: &str = "x-grpc-test-echo-trailing-bin";
const HEADER_ASCII: &str = "x-grpc-test-echo-initial";

struct TrailerEcho;

impl Greeter for TrailerEcho {
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
        let mut resp = Response::new(reply);
        resp.metadata_mut()
            .insert(HEADER_ASCII, "ok")
            .expect("ascii");
        resp.trailers_mut()
            .insert_bin(TRAILER_BIN, [0x00, 0x01])
            .expect("bin");
        Ok(resp)
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("gaps"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("gaps"))
    }

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("gaps"))
    }
}

struct Sleep;

impl Greeter for Sleep {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut reply = HelloReply::new();
        reply.set_message("late");
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("gaps"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("gaps"))
    }

    async fn stream_hello(
        &self,
        _request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("gaps"))
    }
}

#[tokio::test]
async fn ok_path_custom_bin_trailers_not_headers() {
    let (_addr, client, _guard) = spawn_greeter(TrailerEcho).await.expect("spawn");
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(resp.metadata().get(HEADER_ASCII), Some("ok"));
    assert!(
        resp.metadata().get_bin(TRAILER_BIN).is_none(),
        "-bin trailer must not appear as headers"
    );
    assert_eq!(
        resp.trailers().get_bin(TRAILER_BIN).as_deref(),
        Some([0x00, 0x01].as_slice())
    );
}

#[tokio::test]
async fn deadline_exceeded_on_sleeping_handler() {
    let (_addr, client, _guard) = spawn_greeter(Sleep).await.expect("spawn");
    let mut request = Request::new(req("ada"));
    request.set_timeout(Duration::from_millis(80));
    match client.say_hello(request).await {
        Err(err) => {
            assert_eq!(err.code(), Code::DeadlineExceeded);
            assert_eq!(err.code().to_i32(), 4);
        }
        Ok(_) => panic!("expected DEADLINE_EXCEEDED"),
    }
}

#[tokio::test]
async fn cancelled_on_client_cancel() {
    let (_addr, client, _guard) = spawn_greeter(Sleep).await.expect("spawn");
    let call = client.say_hello(Request::new(req("ada")));
    let handle = call.handle();
    drop(tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        handle.cancel();
    }));
    match call.await {
        Err(err) => {
            assert_eq!(err.code(), Code::Cancelled);
            assert_eq!(err.code().to_i32(), 1);
        }
        Ok(_) => panic!("expected CANCELLED"),
    }
}
