//! A terminal response must release an idle bidirectional request producer.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "integration tests"
)]

mod common;

use pbrs_grpc::hello::{Greeter, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Request, Response, ServerConfig, Status, Streaming};
use std::time::Duration;

struct Terminal;

impl Greeter for Terminal {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        common::Echo.say_hello(request).await
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        common::Echo.client_hello(request).await
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        common::Echo.server_hello(request).await
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let fail = request.metadata().get("x-fail").is_some();
        let (tx, replies) = Streaming::channel(1);
        drop(tokio::spawn(async move {
            tx.send(common::reply("first")).await.unwrap();
            if fail {
                let mut status = Status::failed_precondition("terminal");
                status
                    .metadata_mut()
                    .insert("x-terminal", "retained")
                    .unwrap();
                tx.fail(status).await;
            }
        }));
        Ok(Response::new(replies))
    }
}

#[tokio::test]
async fn terminal_bidi_responses_stop_idle_request_pumps_without_caller_cancellation() {
    let (addr, _guard) = common::serve(Terminal, ServerConfig::default())
        .await
        .unwrap();
    let channel = common::until_ok("connect", || async { Channel::connect(addr).await }).await;
    for fail in [false, true] {
        for trailers_only in [false, true] {
            let mut request = Request::new(());
            if fail {
                request.metadata_mut().insert("x-fail", "true").unwrap();
            }
            let (sender, call) = channel
                .bidi::<HelloRequest, HelloReply>("/helloworld.Greeter/StreamHello", request);
            let handle = call.handle();
            let mut replies = call.await.unwrap().into_inner();
            let result = if trailers_only {
                replies.trailers().await.map(|_| ())
            } else {
                assert_eq!(
                    common::name_of(&replies.message().await.unwrap().unwrap()),
                    "first"
                );
                replies
                    .message()
                    .await
                    .map(|message| assert!(message.is_none()))
            };
            if fail {
                let error = result.unwrap_err();
                assert_eq!(error.code(), Code::FailedPrecondition);
                assert_eq!(error.metadata().get("x-terminal"), Some("retained"));
            } else {
                result.unwrap();
            }
            // Keep the completed response alive: termination itself, rather
            // than Drop or an explicit cancellation, must release the sender.
            tokio::time::timeout(Duration::from_secs(2), sender.closed())
                .await
                .unwrap();
            assert!(!handle.is_cancelled());
        }
    }
}
