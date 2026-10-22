//! Compressed message flags must agree with the RPC's negotiated coding.
//! Registry h2 supplies an independent peer for both directions.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "bounded wire-protocol integration fixtures"
)]

use bytes::Bytes;
use http::{HeaderMap, Method};
use pbrs_grpc::hello::{Greeter, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{
    Channel, Code, CompressionAlgorithm, Request, Response, Server, Status, Streaming,
};
use std::future::Future;
use std::time::Duration;

const UNARY: &str = "/helloworld.Greeter/SayHello";
const SERVER_STREAM: &str = "/helloworld.Greeter/ServerHello";
const CLIENT_STREAM: &str = "/helloworld.Greeter/ClientHello";
const BIDI: &str = "/helloworld.Greeter/StreamHello";
const MESSAGE: &[u8] = b"\x0a\x05hello";

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("wire fixture completed within five seconds")
}

fn frame(compressed: bool) -> Bytes {
    let payload = if compressed {
        CompressionAlgorithm::Gzip.encode(MESSAGE).expect("gzip")
    } else {
        MESSAGE.to_vec()
    };
    pbrs_grpc::codec::encode(&payload, compressed).expect("frame")
}

struct Echo;

impl Echo {
    fn reply() -> Response<HelloReply> {
        let mut reply = HelloReply::new();
        reply.set_message("hello");
        Response::new(reply)
    }

    async fn stream() -> Result<Response<Streaming<HelloReply>>, Status> {
        let (send, stream) = Streaming::channel(1);
        send.send(Self::reply().into_inner()).await?;
        send.close();
        Ok(Response::new(stream))
    }

    async fn read(request: Request<Streaming<HelloRequest>>) -> Result<(), Status> {
        let mut stream = request.into_inner();
        let request = stream
            .message()
            .await?
            .ok_or_else(|| Status::internal("missing fixture message"))?;
        assert_eq!(request.name().to_str().expect("UTF-8 request"), "hello");
        assert!(stream.message().await?.is_none());
        Ok(())
    }
}

impl Greeter for Echo {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Self::reply())
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Self::stream().await
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Self::read(request).await?;
        Ok(Self::reply())
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Self::read(request).await?;
        Self::stream().await
    }
}

#[tokio::test]
async fn server_checks_compressed_request_flags_for_every_call_shape() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        Server::new(GreeterServer::new(Echo))
            .serve_connection(server)
            .await
            .expect("native server");
    });
    let (mut sender, connection) = bounded(h2::client::handshake(client))
        .await
        .expect("raw client handshake");
    let driver = tokio::spawn(connection);
    for path in [UNARY, SERVER_STREAM, CLIENT_STREAM, BIDI] {
        for coding in [None, Some("identity"), Some("IDENTITY"), Some("gzip")] {
            for compressed in [false, true] {
                let mut request = http::Request::builder()
                    .method(Method::POST)
                    .uri(format!("http://localhost{path}"))
                    .header("content-type", "application/grpc")
                    .header("te", "trailers");
                if let Some(coding) = coding {
                    request = request.header("grpc-encoding", coding);
                }
                sender = bounded(sender.ready()).await.expect("ready raw client");
                let (response, mut send) = sender
                    .send_request(request.body(()).expect("request"), false)
                    .expect("send request");
                send.send_data(frame(compressed), true)
                    .expect("request DATA");
                let response = bounded(response).await.expect("response headers");
                let mut status = response.headers().get("grpc-status").cloned();
                let mut body = response.into_body();
                while let Some(chunk) = bounded(body.data()).await {
                    let chunk = chunk.expect("response DATA");
                    body.flow_control()
                        .release_capacity(chunk.len())
                        .expect("return response capacity");
                }
                if let Some(trailers) = bounded(body.trailers()).await.expect("trailers") {
                    status = trailers.get("grpc-status").cloned().or(status);
                }
                let expected = if compressed && coding != Some("gzip") {
                    "13"
                } else {
                    "0"
                };
                assert_eq!(
                    status.expect("grpc-status"),
                    expected,
                    "{path}, coding {coding:?}, compressed {compressed}"
                );
            }
        }
    }
    driver.abort();
    server.abort();
}

#[tokio::test]
async fn client_checks_compressed_response_flags_for_unary_and_streaming() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let peer = tokio::spawn(async move {
        let mut connection = h2::server::handshake(server)
            .await
            .expect("raw server handshake");
        let mut handlers = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                result = handlers.join_next(), if !handlers.is_empty() => {
                    result.expect("handler result").expect("raw response handler");
                }
                incoming = connection.accept() => {
                    let Some(incoming) = incoming else { break };
                    let (request, mut respond) = incoming.expect("raw request");
                    let coding = request.headers().get("x-fixture-encoding").cloned();
                    let compressed = request.headers().get("x-fixture-compressed")
                        .is_some_and(|value| value == "yes");
                    handlers.spawn(async move {
                        let mut body = request.into_body();
                        while let Some(chunk) = body.data().await {
                            let chunk = chunk.expect("request DATA");
                            body.flow_control().release_capacity(chunk.len()).expect("request credit");
                        }
                        let mut response = http::Response::builder().header("content-type", "application/grpc");
                        if let Some(coding) = coding {
                            response = response.header("grpc-encoding", coding);
                        }
                        let mut send = respond.send_response(response.body(()).expect("response"), false)
                            .expect("response headers");
                        send.send_data(frame(compressed), false).expect("response DATA");
                        let mut trailers = HeaderMap::new();
                        trailers.insert("grpc-status", "0".parse().expect("status"));
                        send.send_trailers(trailers).expect("response trailers");
                    });
                }
            }
        }
    });
    let channel = bounded(Channel::from_io(client, "localhost"))
        .await
        .expect("native client handshake");
    for path in [UNARY, SERVER_STREAM] {
        for coding in [None, Some("identity"), Some("IDENTITY"), Some("gzip")] {
            for compressed in [false, true] {
                let mut message = HelloRequest::new();
                message.set_name("request");
                let mut request = Request::new(message);
                if let Some(coding) = coding {
                    request
                        .metadata_mut()
                        .insert("x-fixture-encoding", coding)
                        .expect("fixture encoding");
                }
                if compressed {
                    request
                        .metadata_mut()
                        .insert("x-fixture-compressed", "yes")
                        .expect("fixture compression flag");
                }
                let result = if path == UNARY {
                    bounded(channel.unary::<HelloRequest, HelloReply>(path, request))
                        .await
                        .map(|response| response.into_inner())
                } else {
                    let mut stream = bounded(
                        channel.server_streaming::<HelloRequest, HelloReply>(path, request),
                    )
                    .await
                    .expect("response headers")
                    .into_inner();
                    bounded(stream.message())
                        .await
                        .map(|message| message.expect("fixture reply"))
                };
                if compressed && coding != Some("gzip") {
                    assert_eq!(
                        result.expect_err("unnegotiated compressed message").code(),
                        Code::Internal
                    );
                } else {
                    let reply = result.expect("valid per-message compression");
                    assert_eq!(reply.message().to_str().expect("UTF-8 response"), "hello");
                }
            }
        }
    }
    peer.abort();
}
