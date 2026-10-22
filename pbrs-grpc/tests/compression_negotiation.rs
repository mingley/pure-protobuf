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

fn codings() -> impl Iterator<Item = Option<&'static str>> {
    [
        None,
        Some("identity"),
        Some("IDENTITY"),
        Some("gzip"),
        Some("deflate"),
    ]
    .into_iter()
    .chain(cfg!(feature = "zstd").then_some(Some("zstd")))
}

fn frame(compressed: bool, coding: Option<&str>) -> Bytes {
    let payload = if compressed {
        let algorithm = coding
            .and_then(CompressionAlgorithm::parse)
            .unwrap_or_default();
        if algorithm == CompressionAlgorithm::Deflate {
            algorithm.encode(MESSAGE).expect("zlib stream")
        } else {
            // Split protobuf bytes across independent gzip members or Zstd
            // frames. A first-frame decoder would see truncated protobuf.
            let (first, second) = MESSAGE.split_at(3);
            let mut payload = algorithm.encode(first).expect("first compressed part");
            if algorithm.name() == "zstd" {
                // Legal skippable metadata between standard Zstd frames.
                payload.extend_from_slice(&0x184d2a50u32.to_le_bytes());
                payload.extend_from_slice(&3u32.to_le_bytes());
                payload.extend_from_slice(b"pad");
            }
            payload.extend_from_slice(&algorithm.encode(second).expect("second compressed part"));
            payload
        }
    } else {
        MESSAGE.to_vec()
    };
    pbrs_grpc::codec::encode(&payload, compressed).expect("frame")
}

fn split_points(wire: &Bytes, compressed: bool, coding: Option<&str>) -> Vec<usize> {
    if compressed && coding.and_then(CompressionAlgorithm::parse).is_some() {
        (0..=wire.len()).collect()
    } else {
        vec![wire.len()]
    }
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
        for coding in codings() {
            for compressed in [false, true] {
                let wire = frame(compressed, coding);
                for split in split_points(&wire, compressed, coding) {
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
                    send.send_data(wire.slice(..split), false)
                        .expect("first request DATA");
                    send.send_data(wire.slice(split..), true)
                        .expect("last request DATA");
                    let response = bounded(response).await.expect("response headers");
                    let mut status = response.headers().get("grpc-status").cloned();
                    let mut body = response.into_body();
                    while let Some(chunk) = bounded(body.data()).await {
                        let chunk = chunk.expect("response DATA");
                        body.flow_control()
                            .release_capacity(chunk.len())
                            .expect("response credit");
                    }
                    if let Some(trailers) = bounded(body.trailers()).await.expect("trailers") {
                        status = trailers.get("grpc-status").cloned().or(status);
                    }
                    let expected =
                        if compressed && coding.and_then(CompressionAlgorithm::parse).is_none() {
                            "13"
                        } else {
                            "0"
                        };
                    assert_eq!(
                        status.expect("grpc-status"),
                        expected,
                        "{path}, coding {coding:?}, compressed {compressed}, split {split}"
                    );
                }
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
                    let coding = request.headers().get("x-fixture-encoding")
                        .map(|value| value.to_str().expect("encoding token").to_owned());
                    let split = request.headers().get("x-fixture-split").expect("response split")
                        .to_str().expect("split text").parse::<usize>().expect("split number");
                    let compressed = request.headers().get("x-fixture-compressed")
                        .is_some_and(|value| value == "yes");
                    handlers.spawn(async move {
                        let mut body = request.into_body();
                        while let Some(chunk) = body.data().await {
                            let chunk = chunk.expect("request DATA");
                            body.flow_control().release_capacity(chunk.len()).expect("request credit");
                        }
                        let mut response = http::Response::builder().header("content-type", "application/grpc");
                        if let Some(coding) = &coding {
                            response = response.header("grpc-encoding", coding);
                        }
                        let mut send = respond.send_response(response.body(()).expect("response"), false)
                            .expect("response headers");
                        let wire = frame(compressed, coding.as_deref());
                        send.send_data(wire.slice(..split), false).expect("first response DATA");
                        send.send_data(wire.slice(split..), false).expect("last response DATA");
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
        for coding in codings() {
            for compressed in [false, true] {
                let wire = frame(compressed, coding);
                for split in split_points(&wire, compressed, coding) {
                    let mut message = HelloRequest::new();
                    message.set_name("request");
                    let mut request = Request::new(message);
                    if let Some(coding) = coding {
                        request
                            .metadata_mut()
                            .insert("x-fixture-encoding", coding)
                            .expect("encoding");
                    }
                    if compressed {
                        request
                            .metadata_mut()
                            .insert("x-fixture-compressed", "yes")
                            .expect("flag");
                    }
                    request
                        .metadata_mut()
                        .insert("x-fixture-split", split.to_string())
                        .expect("split");
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
                        match bounded(stream.message()).await {
                            Ok(Some(reply)) => {
                                assert!(
                                    bounded(stream.message())
                                        .await
                                        .expect("clean response end")
                                        .is_none()
                                );
                                Ok(reply)
                            }
                            Ok(None) => panic!("missing fixture reply"),
                            Err(status) => Err(status),
                        }
                    };
                    if compressed && coding.and_then(CompressionAlgorithm::parse).is_none() {
                        assert_eq!(
                            result.expect_err("unnegotiated compressed message").code(),
                            Code::Internal
                        );
                    } else {
                        let reply = result.expect("valid per-message compression");
                        assert_eq!(
                            reply.message().to_str().expect("UTF-8 response"),
                            "hello",
                            "{path}, coding {coding:?}, compressed {compressed}, split {split}"
                        );
                    }
                }
            }
        }
    }
    peer.abort();
}
