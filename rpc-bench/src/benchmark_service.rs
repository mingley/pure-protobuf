//! Implementation of the official gRPC `BenchmarkService`.
//!
//! Provides high-throughput unary and streaming data-plane procedures matching
//! the official gRPC benchmark specification.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    missing_docs,
    reason = "bench implementation"
)]

pub mod proto {
    #![allow(
        missing_docs,
        unused,
        clippy::all,
        clippy::pedantic,
        clippy::restriction,
        reason = "generated protobuf stubs"
    )]
    include!(concat!(env!("OUT_DIR"), "/benchmark_service.rs"));
}

pub mod control {
    #![allow(
        missing_docs,
        unused,
        clippy::all,
        clippy::pedantic,
        clippy::restriction,
        reason = "generated protobuf control stubs"
    )]
    include!(concat!(env!("OUT_DIR"), "/control.rs"));
}

pub mod stats {
    #![allow(
        missing_docs,
        unused,
        clippy::all,
        clippy::pedantic,
        clippy::restriction,
        reason = "generated protobuf stats stubs"
    )]
    include!(concat!(env!("OUT_DIR"), "/stats.rs"));
}

pub mod payloads {
    #![allow(
        missing_docs,
        unused,
        clippy::all,
        clippy::pedantic,
        clippy::restriction,
        reason = "generated protobuf payloads stubs"
    )]
    include!(concat!(env!("OUT_DIR"), "/payloads.rs"));
}

pub mod worker_service {
    #![allow(
        missing_docs,
        unused,
        clippy::all,
        clippy::pedantic,
        clippy::restriction,
        reason = "generated protobuf worker_service stubs"
    )]
    include!(concat!(env!("OUT_DIR"), "/worker_service.rs"));
}

pub use proto::{
    BenchmarkService, BenchmarkServiceClient, BenchmarkServiceServer, Payload, PayloadType,
    SimpleRequest, SimpleResponse,
};

use pbrs_grpc::{Code, Request, Response, Status, StreamSender, Streaming};

const ECHO_INITIAL: &str = "x-grpc-test-echo-initial";
const ECHO_TRAILING: &str = "x-grpc-test-echo-trailing-bin";

#[derive(Default)]
struct Echo {
    initial: Option<String>,
    trailing: Option<Vec<u8>>,
}

impl Echo {
    fn capture<T>(request: &Request<T>) -> Self {
        Self {
            initial: request.metadata().get(ECHO_INITIAL).map(str::to_owned),
            trailing: request.metadata().get_bin(ECHO_TRAILING),
        }
    }

    fn apply<T>(&self, response: &mut Response<T>) {
        if let Some(ref v) = self.initial {
            response.metadata_mut().insert(ECHO_INITIAL, v).ok();
        }
        if let Some(ref v) = self.trailing {
            response.trailers_mut().insert_bin(ECHO_TRAILING, v).ok();
        }
    }
}

/// Create a zero-filled `Payload` with length `size`.
pub fn zeros_payload(size: i32) -> Payload {
    let n = usize::try_from(size.max(0)).unwrap_or(0);
    let mut p = Payload::new();
    p.set_body(vec![0u8; n]);
    p
}

/// Generate a `SimpleResponse` answering a `SimpleRequest`.
pub fn make_response(req: &SimpleRequest) -> Result<SimpleResponse, Status> {
    if req.has_response_status() {
        let st = req.response_status();
        return Err(Status::new(
            Code::from_i32(st.code()),
            st.message().to_string(),
        ));
    }

    let mut resp = SimpleResponse::new();
    let resp_size = req.response_size();
    if resp_size > 0 {
        resp.set_payload(zeros_payload(resp_size));
    } else if req.has_payload() && !req.payload().body().is_empty() {
        resp.set_payload(req.payload().clone());
    } else {
        resp.set_payload(Payload::new());
    }
    Ok(resp)
}

/// Native implementation of the gRPC `BenchmarkService`.
#[derive(Clone, Default)]
pub struct BenchmarkServiceImpl;

/// Alias for `BenchmarkServiceImpl`.
pub type NativeBenchmarkService = BenchmarkServiceImpl;

async fn echo_stream(mut input: Streaming<SimpleRequest>, tx: StreamSender<SimpleResponse>) {
    loop {
        let item = match input.next_framed().await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(status) => {
                tx.fail(status).await;
                return;
            }
        };
        let req = item.message;
        if req.has_expect_compressed() && req.expect_compressed().value() && !item.compressed {
            tx.fail(Status::invalid_argument("request not compressed"))
                .await;
            return;
        }
        let resp_msg = match make_response(&req) {
            Ok(msg) => msg,
            Err(status) => {
                tx.fail(status).await;
                return;
            }
        };
        let send_res = if req.has_response_compressed() && req.response_compressed().value() {
            tx.send_compressed(resp_msg).await
        } else {
            tx.send(resp_msg).await
        };
        if send_res.is_err() || tx.is_closed() {
            break;
        }
    }
}

impl BenchmarkService for BenchmarkServiceImpl {
    /// UnaryCall: responds with payload of requested response_size.
    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let echo = Echo::capture(&request);
        let compressed = request.compressed();
        let inner = request.into_inner();
        if inner.has_expect_compressed() && inner.expect_compressed().value() && !compressed {
            return Err(Status::invalid_argument("request not compressed"));
        }
        let resp_msg = make_response(&inner)?;
        let mut resp = Response::new(resp_msg);
        echo.apply(&mut resp);
        if inner.has_response_compressed() && inner.response_compressed().value() {
            resp.set_compress(true);
        }
        Ok(resp)
    }

    /// StreamingCall: bidirectional ping-pong echo with requested size.
    /// An inbound stream error ends the response with that status, not OK.
    async fn streaming_call(
        &self,
        request: Request<Streaming<SimpleRequest>>,
    ) -> Result<Response<Streaming<SimpleResponse>>, Status> {
        let echo = Echo::capture(&request);
        let in_stream = request.into_inner();
        let (tx, out_stream) = Streaming::channel(32);

        tokio::spawn(echo_stream(in_stream, tx));

        let mut resp = Response::new(out_stream);
        echo.apply(&mut resp);
        Ok(resp)
    }

    /// StreamingFromClient: aggregates stream and returns response.
    async fn streaming_from_client(
        &self,
        request: Request<Streaming<SimpleRequest>>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let echo = Echo::capture(&request);
        let mut in_stream = request.into_inner();
        let mut total_bytes: usize = 0;
        let mut last_response_size: i32 = 0;
        let mut compress_response = false;

        while let Some(item) = in_stream.next_framed().await? {
            let req = item.message;
            if req.has_expect_compressed() && req.expect_compressed().value() && !item.compressed {
                return Err(Status::invalid_argument("request not compressed"));
            }
            if req.has_response_status() {
                let st = req.response_status();
                return Err(Status::new(
                    Code::from_i32(st.code()),
                    st.message().to_string(),
                ));
            }
            if req.response_size() > 0 {
                last_response_size = req.response_size();
            }
            if req.has_response_compressed() && req.response_compressed().value() {
                compress_response = true;
            }
            total_bytes = total_bytes.saturating_add(req.payload().body().len());
        }

        let mut resp_msg = SimpleResponse::new();
        let final_size = if last_response_size > 0 {
            last_response_size as usize
        } else {
            total_bytes
        };
        resp_msg.set_payload(zeros_payload(final_size as i32));

        let mut resp = Response::new(resp_msg);
        echo.apply(&mut resp);
        if compress_response {
            resp.set_compress(true);
        }
        Ok(resp)
    }

    /// StreamingFromServer: streams chunks of requested size.
    async fn streaming_from_server(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<Streaming<SimpleResponse>>, Status> {
        let echo = Echo::capture(&request);
        let compressed = request.compressed();
        let req = request.into_inner();
        if req.has_expect_compressed() && req.expect_compressed().value() && !compressed {
            return Err(Status::invalid_argument("request not compressed"));
        }
        if req.has_response_status() {
            let st = req.response_status();
            return Err(Status::new(
                Code::from_i32(st.code()),
                st.message().to_string(),
            ));
        }

        let resp_msg = make_response(&req)?;
        let compress = req.has_response_compressed() && req.response_compressed().value();
        let (tx, out_stream) = Streaming::channel(32);

        tokio::spawn(async move {
            loop {
                if tx.is_closed() {
                    break;
                }
                let send_res = if compress {
                    tx.send_compressed(resp_msg.clone()).await
                } else {
                    tx.send(resp_msg.clone()).await
                };
                if send_res.is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        });

        let mut resp = Response::new(out_stream);
        echo.apply(&mut resp);
        if compress {
            resp.set_compress(true);
        }
        Ok(resp)
    }

    /// StreamingBothWays: bidirectional echo with requested size.
    /// An inbound stream error ends the response with that status, not OK.
    async fn streaming_both_ways(
        &self,
        request: Request<Streaming<SimpleRequest>>,
    ) -> Result<Response<Streaming<SimpleResponse>>, Status> {
        let echo = Echo::capture(&request);
        let in_stream = request.into_inner();
        let (tx, out_stream) = Streaming::channel(32);

        tokio::spawn(echo_stream(in_stream, tx));

        let mut resp = Response::new(out_stream);
        echo.apply(&mut resp);
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_benchmark_service_unary() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            BenchmarkServiceServer::new(BenchmarkServiceImpl)
                .serve_listener(listener)
                .await
                .ok();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
        let client = BenchmarkServiceClient::new(channel);

        // Test with requested size = 1234
        let mut req = SimpleRequest::new();
        req.set_response_size(1234);
        let resp = client
            .unary_call(Request::new(req))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.payload().body().len(), 1234);

        // Test with requested size = 0
        let mut req_empty = SimpleRequest::new();
        req_empty.set_response_size(0);
        let resp_empty = client
            .unary_call(Request::new(req_empty))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp_empty.payload().body().len(), 0);
    }

    #[tokio::test]
    async fn test_benchmark_service_streaming_call() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            BenchmarkServiceServer::new(BenchmarkServiceImpl)
                .serve_listener(listener)
                .await
                .ok();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
        let client = BenchmarkServiceClient::new(channel);

        let (tx, call) = client.streaming_call(Request::new(()));
        let mut resp_stream = call.await.unwrap().into_inner();

        for size in [100, 250, 500] {
            let mut req = SimpleRequest::new();
            req.set_response_size(size);
            tx.send(req).await.unwrap();

            let resp = resp_stream.message().await.unwrap().unwrap();
            assert_eq!(resp.payload().body().len(), size as usize);
        }

        tx.close();
        assert!(resp_stream.message().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn request_stream_errors_are_not_reported_as_successful_responses() {
        let service = BenchmarkServiceImpl;
        for shape in ["StreamingCall", "StreamingBothWays"] {
            for sent_first in [false, true] {
                let (sender, input) = Streaming::<SimpleRequest>::channel(2);
                let response = if shape == "StreamingCall" {
                    service.streaming_call(Request::new(input)).await
                } else {
                    service.streaming_both_ways(Request::new(input)).await
                }
                .expect("stream setup");
                if sent_first {
                    let mut request = SimpleRequest::new();
                    request.set_response_size(8);
                    sender.send(request).await.expect("first request");
                }
                sender
                    .fail(Status::invalid_argument("request stream failed"))
                    .await;
                let mut output = response.into_inner();
                if sent_first {
                    let first = tokio::time::timeout(Duration::from_secs(1), output.message())
                        .await
                        .expect("first response stalled")
                        .expect("first response status")
                        .expect("first response message");
                    assert_eq!(first.payload().body().len(), 8);
                }
                let error = tokio::time::timeout(Duration::from_secs(1), output.message())
                    .await
                    .expect("response stream stalled")
                    .expect_err("request failure must not become an empty OK response");
                assert_eq!(error.code(), Code::InvalidArgument, "{shape}: {error}");
                assert_eq!(error.message(), "request stream failed");
            }
        }
    }

    #[tokio::test]
    async fn test_benchmark_service_streaming_both_ways() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            BenchmarkServiceServer::new(BenchmarkServiceImpl)
                .serve_listener(listener)
                .await
                .ok();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
        let client = BenchmarkServiceClient::new(channel);

        let (tx, call) = client.streaming_both_ways(Request::new(()));
        let mut resp_stream = call.await.unwrap().into_inner();

        for size in [64, 128, 256] {
            let mut req = SimpleRequest::new();
            req.set_response_size(size);
            tx.send(req).await.unwrap();

            let resp = resp_stream.message().await.unwrap().unwrap();
            assert_eq!(resp.payload().body().len(), size as usize);
        }

        tx.close();
        assert!(resp_stream.message().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_benchmark_service_streaming_from_client() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            BenchmarkServiceServer::new(BenchmarkServiceImpl)
                .serve_listener(listener)
                .await
                .ok();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
        let client = BenchmarkServiceClient::new(channel);

        let (tx, call) = client.streaming_from_client(Request::new(()));

        for _ in 0..4 {
            let mut req = SimpleRequest::new();
            req.set_payload(zeros_payload(50));
            tx.send(req).await.unwrap();
        }
        // Last request specifies response_size = 300
        let mut final_req = SimpleRequest::new();
        final_req.set_payload(zeros_payload(50));
        final_req.set_response_size(300);
        tx.send(final_req).await.unwrap();

        tx.close();
        let resp = call.await.unwrap().into_inner();
        assert_eq!(resp.payload().body().len(), 300);
    }

    #[tokio::test]
    async fn test_benchmark_service_streaming_from_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            BenchmarkServiceServer::new(BenchmarkServiceImpl)
                .serve_listener(listener)
                .await
                .ok();
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let channel = pbrs_grpc::Channel::connect(addr).await.unwrap();
        let client = BenchmarkServiceClient::new(channel);

        let mut req = SimpleRequest::new();
        req.set_response_size(512);

        let call = client.streaming_from_server(Request::new(req));
        let mut stream = call.await.unwrap().into_inner();

        // Read 5 chunks of requested size 512
        for _ in 0..5 {
            let msg = stream.message().await.unwrap().unwrap();
            assert_eq!(msg.payload().body().len(), 512);
        }
        // Drop the stream to end the server loop
        drop(stream);
    }
}
