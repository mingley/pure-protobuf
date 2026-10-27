//! A peer may finish an RPC before the client queues an empty half-close.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "bounded in-memory transport regression assertions"
)]

use pbrs_grpc::{
    Code, Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest, Request, Response,
    Status, Streaming,
};
use std::time::Duration;

struct Early;

impl Greeter for Early {
    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        if request.metadata().get("reject") == Some("yes") {
            return Err(Status::resource_exhausted("early rejection"));
        }
        Ok(Response::new(HelloReply::new()))
    }

    async fn server_hello(
        &self,
        _: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Ok(Response::new(Streaming::empty()))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        if request.metadata().get("reject") == Some("yes") {
            return Err(Status::resource_exhausted("early rejection"));
        }
        Ok(Response::new(Streaming::empty()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_closed_upload_preserves_early_responses_and_sibling_recovery() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(GreeterServer::new(Early).serve_connection(server_io));
    let channel = pbrs_grpc::Channel::from_io(client_io, "localhost")
        .await
        .unwrap();
    let client = GreeterClient::new(channel);
    tokio::time::timeout(Duration::from_secs(30), async {
        for turn in 0..1000 {
            // Drop an unconsumed sibling response before opening the upload.
            drop(
                client
                    .server_hello(Request::new(HelloRequest::new()))
                    .await
                    .unwrap(),
            );
            let request = || {
                let mut request = Request::new(());
                if turn % 2 == 1 {
                    request.set_timeout(Duration::from_secs(2));
                }
                if turn % 3 == 1 {
                    request.metadata_mut().insert("reject", "yes").unwrap();
                }
                request
            };
            let (sender, call) = client.client_hello(request());
            sender.close();
            let result = call.await;
            if turn % 3 == 1 {
                assert_eq!(
                    result.expect_err("early rejection").code(),
                    Code::ResourceExhausted
                );
            } else {
                result.unwrap_or_else(|status| {
                    panic!(
                        "client-stream turn {turn}: {:?}: {}",
                        status.code(),
                        status.message()
                    )
                });
            }
            let (sender, call) = client.stream_hello(request());
            sender.close();
            let result = call.await;
            if turn % 3 == 1 {
                assert_eq!(
                    result.expect_err("early rejection").code(),
                    Code::ResourceExhausted
                );
            } else {
                let mut stream = result
                    .unwrap_or_else(|status| {
                        panic!(
                            "bidi turn {turn}: {:?}: {}",
                            status.code(),
                            status.message()
                        )
                    })
                    .into_inner();
                assert!(
                    stream.message().await.unwrap().is_none(),
                    "complete valid response"
                );
                stream.trailers().await.unwrap();
            }
        }
    })
    .await
    .expect("bounded repeated early responses");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reset_without_a_complete_response_is_still_an_error() {
    for reason in [
        h2::Reason::NO_ERROR,
        h2::Reason::INTERNAL_ERROR,
        h2::Reason::CANCEL,
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut connection = h2::server::handshake(socket).await.unwrap();
            let (_, mut respond) = connection.accept().await.unwrap().unwrap();
            let headers = http::Response::builder()
                .header("content-type", "application/grpc")
                .body(())
                .unwrap();
            let mut send = respond.send_response(headers, false).unwrap();
            send.send_reset(reason);
            // Drive the connection until the client observes the incomplete
            // response. A reset is never a replacement for gRPC OK trailers.
            let _ = connection.accept().await;
        });
        let channel = pbrs_grpc::Channel::connect(address).await.unwrap();
        let (sender, call) =
            channel.bidi::<HelloRequest, HelloReply>("/fixture.Early/Upload", Request::new(()));
        sender.close();
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            match call.await {
                Err(status) => status,
                Ok(response) => response
                    .into_inner()
                    .message()
                    .await
                    .expect_err("incomplete response"),
            }
        })
        .await
        .expect("reset observed within the call budget");
        assert_eq!(
            result.code(),
            if reason == h2::Reason::CANCEL {
                Code::Cancelled
            } else {
                Code::Internal
            }
        );
        peer.abort();
    }
}
