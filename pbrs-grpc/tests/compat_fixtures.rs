//! Runtime checks for tonic-shaped compat shims.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests assert fixture behavior"
)]

use pbrs_grpc::HelloReply;
use pbrs_grpc::compat::{self, IntoRequest, Request, Response, Status};

#[test]
fn compat_unary_request_conversion_matches_tonic_shape() {
    let request: Request<&'static str> = "hello".into_request();
    assert_eq!(*request.get_ref(), "hello");

    let request = Request::new("wrapped");
    let request: Request<&'static str> = request.into_request();
    assert_eq!(*request.get_ref(), "wrapped");
}

#[tokio::test]
async fn compat_response_stream_forwards_items_and_status() {
    fn reply(message: &str) -> HelloReply {
        let mut reply = HelloReply::new();
        reply.set_message(message);
        reply
    }

    let response = Response::new(compat::iter(vec![
        Ok::<_, Status>(reply("a")),
        Ok(reply("b")),
        Err(Status::unavailable("closed")),
    ]));
    let mut stream = compat::response_stream(response).into_inner();
    assert_eq!(stream.message().await.unwrap().unwrap().message(), "a");
    assert_eq!(stream.message().await.unwrap().unwrap().message(), "b");
    let err = stream.message().await.expect_err("status");
    assert_eq!(err.code(), pbrs_grpc::Code::Unavailable);
}
