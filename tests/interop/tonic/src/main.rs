//! Tonic 0.14 mixed-peer interop test runner.
//!
//! Provides both server and client roles for `grpc.testing.TestService`
//! over `protobuf-tonic` / `pbrs-grpc`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    missing_docs,
    reason = "interop test binary"
)]

use bytes::Bytes;
use futures_util::StreamExt;
use http::{HeaderMap, HeaderName, HeaderValue, Request as HttpRequest, Response as HttpResponse};
use http_body::Frame;
use pbrs_grpc::testing::{
    EchoStatus, Empty, Payload, ResponseParameters, SimpleRequest, SimpleResponse,
    StreamingInputCallRequest, StreamingInputCallResponse, StreamingOutputCallRequest,
    StreamingOutputCallResponse,
};
use protobuf_tonic::ProtobufCodec;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tonic::body::Body;
use tonic::client::Grpc;
use tonic::server::{
    ClientStreamingService, Grpc as ServerGrpc, NamedService, ServerStreamingService,
    StreamingService, UnaryService,
};
use tonic::transport::{Channel, Server};
use tonic::{Code, Request, Response, Status};
use tower_service::Service;

const LARGE_REQ: usize = 271828;
const LARGE_RESP: usize = 314159;
const ECHO_INITIAL: &str = "x-grpc-test-echo-initial";
const ECHO_INITIAL_VAL: &str = "test_initial_metadata_value";
const ECHO_TRAILING_BIN: &str = "x-grpc-test-echo-trailing-bin";
const ECHO_TRAILING_BIN_VAL: &[u8] = &[0xab, 0xab, 0xab];

pin_project_lite::pin_project! {
    pub struct TrailersBody<B> {
        #[pin]
        inner: B,
        trailing: Option<HeaderMap>,
    }
}

impl<B> http_body::Body for TrailersBody<B>
where
    B: http_body::Body<Data = Bytes, Error = Status>,
{
    type Data = Bytes;
    type Error = Status;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        match this.inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if frame.is_trailers() {
                    let mut trailers = frame.into_trailers().unwrap();
                    if let Some(extra) = this.trailing.take() {
                        trailers.extend(extra);
                    }
                    Poll::Ready(Some(Ok(Frame::trailers(trailers))))
                } else {
                    Poll::Ready(Some(Ok(frame)))
                }
            }
            Poll::Ready(None) => {
                if let Some(extra) = this.trailing.take() {
                    Poll::Ready(Some(Ok(Frame::trailers(extra))))
                } else {
                    Poll::Ready(None)
                }
            }
            other => other,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream() && self.trailing.is_none()
    }
}

#[derive(Clone, Default)]
pub struct TestServiceServer;

impl NamedService for TestServiceServer {
    const NAME: &'static str = "grpc.testing.TestService";
}

impl<B> Service<HttpRequest<B>> for TestServiceServer
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<tonic::codegen::StdError> + Send + 'static,
{
    type Response = HttpResponse<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: HttpRequest<B>) -> Self::Future {
        let initial_md = req.headers().get(ECHO_INITIAL).cloned();
        let trailing_md = req.headers().get(ECHO_TRAILING_BIN).cloned();

        let wrap_body = move |res: HttpResponse<Body>| -> HttpResponse<Body> {
            if let Some(trailing_val) = trailing_md {
                let (parts, body) = res.into_parts();
                let mut map = HeaderMap::new();
                map.insert(HeaderName::from_static(ECHO_TRAILING_BIN), trailing_val);
                let new_body = Body::new(TrailersBody {
                    inner: body,
                    trailing: Some(map),
                });
                HttpResponse::from_parts(parts, new_body)
            } else {
                res
            }
        };

        Box::pin(async move {
            match req.uri().path() {
                "/grpc.testing.TestService/EmptyCall" => {
                    struct Svc(Option<HeaderValue>);
                    impl UnaryService<Empty> for Svc {
                        type Response = Empty;
                        type Future = Pin<Box<dyn Future<Output = Result<Response<Empty>, Status>> + Send>>;
                        fn call(&mut self, _req: Request<Empty>) -> Self::Future {
                            let initial = self.0.clone();
                            Box::pin(async move {
                                let mut resp = Response::new(Empty::new());
                                if let Some(v) = initial {
                                    if let Ok(s) = v.to_str() {
                                        if let Ok(mv) = s.parse() {
                                            resp.metadata_mut().insert(ECHO_INITIAL, mv);
                                        }
                                    }
                                }
                                Ok(resp)
                            })
                        }
                    }
                    let mut grpc = ServerGrpc::new(ProtobufCodec::<Empty, Empty>::default());
                    let res = grpc.unary(Svc(initial_md), req).await;
                    Ok(wrap_body(res))
                }
                "/grpc.testing.TestService/UnaryCall" | "/grpc.testing.TestService/CacheableUnaryCall" => {
                    struct Svc(Option<HeaderValue>);
                    impl UnaryService<SimpleRequest> for Svc {
                        type Response = SimpleResponse;
                        type Future = Pin<Box<dyn Future<Output = Result<Response<SimpleResponse>, Status>> + Send>>;
                        fn call(&mut self, req: Request<SimpleRequest>) -> Self::Future {
                            let initial = self.0.clone();
                            Box::pin(async move {
                                let inner = req.into_inner();
                                if inner.has_response_status() {
                                    let st = inner.response_status();
                                    return Err(Status::new(
                                        Code::from_i32(st.code()),
                                        st.message().to_string(),
                                    ));
                                }
                                let size = inner.response_size().max(0) as usize;
                                let mut resp_msg = SimpleResponse::new();
                                let mut p = Payload::new();
                                p.set_body(vec![0u8; size]);
                                resp_msg.set_payload(p);
                                let mut resp = Response::new(resp_msg);
                                if let Some(v) = initial {
                                    if let Ok(s) = v.to_str() {
                                        if let Ok(mv) = s.parse() {
                                            resp.metadata_mut().insert(ECHO_INITIAL, mv);
                                        }
                                    }
                                }
                                Ok(resp)
                            })
                        }
                    }
                    let mut grpc = ServerGrpc::new(ProtobufCodec::<SimpleResponse, SimpleRequest>::default());
                    let res = grpc.unary(Svc(initial_md), req).await;
                    Ok(wrap_body(res))
                }
                "/grpc.testing.TestService/StreamingInputCall" => {
                    struct Svc(Option<HeaderValue>);
                    impl ClientStreamingService<StreamingInputCallRequest> for Svc {
                        type Response = StreamingInputCallResponse;
                        type Future = Pin<Box<dyn Future<Output = Result<Response<StreamingInputCallResponse>, Status>> + Send>>;
                        fn call(&mut self, req: Request<tonic::Streaming<StreamingInputCallRequest>>) -> Self::Future {
                            let initial = self.0.clone();
                            Box::pin(async move {
                                let mut stream = req.into_inner();
                                let mut total: i32 = 0;
                                while let Some(msg) = stream.next().await {
                                    let msg = msg?;
                                    let n = msg.payload().body().len() as i32;
                                    total = total.saturating_add(n);
                                }
                                let mut resp_msg = StreamingInputCallResponse::new();
                                resp_msg.set_aggregated_payload_size(total);
                                let mut resp = Response::new(resp_msg);
                                if let Some(v) = initial {
                                    if let Ok(s) = v.to_str() {
                                        if let Ok(mv) = s.parse() {
                                            resp.metadata_mut().insert(ECHO_INITIAL, mv);
                                        }
                                    }
                                }
                                Ok(resp)
                            })
                        }
                    }
                    let mut grpc = ServerGrpc::new(ProtobufCodec::<StreamingInputCallResponse, StreamingInputCallRequest>::default());
                    let res = grpc.client_streaming(Svc(initial_md), req).await;
                    Ok(wrap_body(res))
                }
                "/grpc.testing.TestService/StreamingOutputCall" => {
                    struct Svc(Option<HeaderValue>);
                    impl ServerStreamingService<StreamingOutputCallRequest> for Svc {
                        type Response = StreamingOutputCallResponse;
                        type ResponseStream = ReceiverStream<Result<StreamingOutputCallResponse, Status>>;
                        type Future = Pin<Box<dyn Future<Output = Result<Response<Self::ResponseStream>, Status>> + Send>>;
                        fn call(&mut self, req: Request<StreamingOutputCallRequest>) -> Self::Future {
                            let initial = self.0.clone();
                            Box::pin(async move {
                                let inner = req.into_inner();
                                if inner.has_response_status() {
                                    let st = inner.response_status();
                                    return Err(Status::new(
                                        Code::from_i32(st.code()),
                                        st.message().to_string(),
                                    ));
                                }
                                let params: Vec<(i32, i32)> = inner
                                    .response_parameters()
                                    .iter()
                                    .map(|p| (p.size(), p.interval_us()))
                                    .collect();

                                let (tx, rx) = tokio::sync::mpsc::channel(8);
                                tokio::spawn(async move {
                                    for (size, interval_us) in params {
                                        if interval_us > 0 {
                                            tokio::time::sleep(Duration::from_micros(interval_us as u64)).await;
                                        }
                                        let mut msg = StreamingOutputCallResponse::new();
                                        let mut p = Payload::new();
                                        p.set_body(vec![0u8; size.max(0) as usize]);
                                        msg.set_payload(p);
                                        if tx.send(Ok(msg)).await.is_err() {
                                            break;
                                        }
                                    }
                                });
                                let mut resp = Response::new(ReceiverStream::new(rx));
                                if let Some(v) = initial {
                                    if let Ok(s) = v.to_str() {
                                        if let Ok(mv) = s.parse() {
                                            resp.metadata_mut().insert(ECHO_INITIAL, mv);
                                        }
                                    }
                                }
                                Ok(resp)
                            })
                        }
                    }
                    let mut grpc = ServerGrpc::new(ProtobufCodec::<StreamingOutputCallResponse, StreamingOutputCallRequest>::default());
                    let res = grpc.server_streaming(Svc(initial_md), req).await;
                    Ok(wrap_body(res))
                }
                "/grpc.testing.TestService/FullDuplexCall" | "/grpc.testing.TestService/HalfDuplexCall" => {
                    struct Svc(Option<HeaderValue>);
                    impl StreamingService<StreamingOutputCallRequest> for Svc {
                        type Response = StreamingOutputCallResponse;
                        type ResponseStream = ReceiverStream<Result<StreamingOutputCallResponse, Status>>;
                        type Future = Pin<Box<dyn Future<Output = Result<Response<Self::ResponseStream>, Status>> + Send>>;
                        fn call(&mut self, req: Request<tonic::Streaming<StreamingOutputCallRequest>>) -> Self::Future {
                            let initial = self.0.clone();
                            Box::pin(async move {
                                let mut inbound = req.into_inner();
                                let (tx, rx) = tokio::sync::mpsc::channel(8);
                                tokio::spawn(async move {
                                    while let Some(msg_res) = inbound.next().await {
                                        match msg_res {
                                            Ok(req_msg) => {
                                                if req_msg.has_response_status() {
                                                    let st = req_msg.response_status();
                                                    let _ = tx.send(Err(Status::new(
                                                        Code::from_i32(st.code()),
                                                        st.message().to_string(),
                                                    ))).await;
                                                    return;
                                                }
                                                let params: Vec<(i32, i32)> = req_msg
                                                    .response_parameters()
                                                    .iter()
                                                    .map(|p| (p.size(), p.interval_us()))
                                                    .collect();
                                                for (size, interval_us) in params {
                                                    if interval_us > 0 {
                                                        tokio::time::sleep(Duration::from_micros(interval_us as u64)).await;
                                                    }
                                                    let mut out = StreamingOutputCallResponse::new();
                                                    let mut p = Payload::new();
                                                    p.set_body(vec![0u8; size.max(0) as usize]);
                                                    out.set_payload(p);
                                                    if tx.send(Ok(out)).await.is_err() {
                                                        return;
                                                    }
                                                }
                                            }
                                            Err(status) => {
                                                let _ = tx.send(Err(status)).await;
                                                return;
                                            }
                                        }
                                    }
                                });
                                let mut resp = Response::new(ReceiverStream::new(rx));
                                if let Some(v) = initial {
                                    if let Ok(s) = v.to_str() {
                                        if let Ok(mv) = s.parse() {
                                            resp.metadata_mut().insert(ECHO_INITIAL, mv);
                                        }
                                    }
                                }
                                Ok(resp)
                            })
                        }
                    }
                    let mut grpc = ServerGrpc::new(ProtobufCodec::<StreamingOutputCallResponse, StreamingOutputCallRequest>::default());
                    let res = grpc.streaming(Svc(initial_md), req).await;
                    Ok(wrap_body(res))
                }
                "/grpc.testing.TestService/UnimplementedCall" => {
                    let mut response = HttpResponse::new(Body::default());
                    let headers = response.headers_mut();
                    headers.insert(Status::GRPC_STATUS, (Code::Unimplemented as i32).into());
                    headers.insert(http::header::CONTENT_TYPE, tonic::metadata::GRPC_CONTENT_TYPE);
                    Ok(response)
                }
                _ => {
                    let mut response = HttpResponse::new(Body::default());
                    let headers = response.headers_mut();
                    headers.insert(Status::GRPC_STATUS, (Code::Unimplemented as i32).into());
                    headers.insert(http::header::CONTENT_TYPE, tonic::metadata::GRPC_CONTENT_TYPE);
                    Ok(response)
                }
            }
        })
    }
}

#[derive(Clone)]
pub struct TestServiceClient {
    grpc: Grpc<Channel>,
}

impl TestServiceClient {
    pub async fn connect(host: &str, port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let uri = format!("http://{host}:{port}");
        let channel = Channel::from_shared(uri)?.connect().await?;
        Ok(Self {
            grpc: Grpc::new(channel),
        })
    }

    pub async fn empty_call(&mut self, req: Request<Empty>) -> Result<Response<Empty>, Status> {
        self.grpc
            .ready()
            .await
            .map_err(|e| Status::unknown(e.to_string()))?;
        let path = "/grpc.testing.TestService/EmptyCall".parse().unwrap();
        self.grpc
            .unary(req, path, ProtobufCodec::<Empty, Empty>::default())
            .await
    }

    pub async fn unary_call(
        &mut self,
        req: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        self.grpc
            .ready()
            .await
            .map_err(|e| Status::unknown(e.to_string()))?;
        let path = "/grpc.testing.TestService/UnaryCall".parse().unwrap();
        self.grpc
            .unary(
                req,
                path,
                ProtobufCodec::<SimpleRequest, SimpleResponse>::default(),
            )
            .await
    }

    pub async fn client_streaming<S>(
        &mut self,
        req: Request<S>,
    ) -> Result<Response<StreamingInputCallResponse>, Status>
    where
        S: tokio_stream::Stream<Item = StreamingInputCallRequest> + Send + 'static,
    {
        self.grpc
            .ready()
            .await
            .map_err(|e| Status::unknown(e.to_string()))?;
        let path = "/grpc.testing.TestService/StreamingInputCall"
            .parse()
            .unwrap();
        self.grpc
            .client_streaming(
                req,
                path,
                ProtobufCodec::<StreamingInputCallRequest, StreamingInputCallResponse>::default(),
            )
            .await
    }

    pub async fn server_streaming(
        &mut self,
        req: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<tonic::Streaming<StreamingOutputCallResponse>>, Status> {
        self.grpc
            .ready()
            .await
            .map_err(|e| Status::unknown(e.to_string()))?;
        let path = "/grpc.testing.TestService/StreamingOutputCall"
            .parse()
            .unwrap();
        self.grpc
            .server_streaming(
                req,
                path,
                ProtobufCodec::<StreamingOutputCallRequest, StreamingOutputCallResponse>::default(),
            )
            .await
    }

    pub async fn full_duplex_call<S>(
        &mut self,
        req: Request<S>,
    ) -> Result<Response<tonic::Streaming<StreamingOutputCallResponse>>, Status>
    where
        S: tokio_stream::Stream<Item = StreamingOutputCallRequest> + Send + 'static,
    {
        self.grpc
            .ready()
            .await
            .map_err(|e| Status::unknown(e.to_string()))?;
        let path = "/grpc.testing.TestService/FullDuplexCall"
            .parse()
            .unwrap();
        self.grpc
            .streaming(
                req,
                path,
                ProtobufCodec::<StreamingOutputCallRequest, StreamingOutputCallResponse>::default(),
            )
            .await
    }

    pub async fn unimplemented_call(
        &mut self,
        req: Request<Empty>,
    ) -> Result<Response<Empty>, Status> {
        self.grpc
            .ready()
            .await
            .map_err(|e| Status::unknown(e.to_string()))?;
        let path = "/grpc.testing.TestService/UnimplementedCall"
            .parse()
            .unwrap();
        self.grpc
            .unary(req, path, ProtobufCodec::<Empty, Empty>::default())
            .await
    }

    pub async fn unimplemented_service_call(
        &mut self,
        req: Request<Empty>,
    ) -> Result<Response<Empty>, Status> {
        self.grpc
            .ready()
            .await
            .map_err(|e| Status::unknown(e.to_string()))?;
        let path = "/grpc.testing.UnimplementedService/UnimplementedCall"
            .parse()
            .unwrap();
        self.grpc
            .unary(req, path, ProtobufCodec::<Empty, Empty>::default())
            .await
    }
}

pub async fn run_client_case(
    client: &mut TestServiceClient,
    case: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match case {
        "empty_unary" => empty_unary(client).await,
        "large_unary" => large_unary(client).await,
        "client_streaming" => client_streaming(client).await,
        "server_streaming" => server_streaming(client).await,
        "ping_pong" => ping_pong(client).await,
        "empty_stream" => empty_stream(client).await,
        "cancel_after_begin" => cancel_after_begin(client).await,
        "timeout_on_sleeping_server" => timeout_on_sleeping_server(client).await,
        "custom_metadata" => custom_metadata(client).await,
        "status_code_and_message" => status_code_and_message(client).await,
        "special_status_message" => special_status_message(client).await,
        "unimplemented_method" => unimplemented_method(client).await,
        "unimplemented_service" => unimplemented_service(client).await,
        other => Err(format!("unknown test case: {other}").into()),
    }
}

pub async fn empty_unary(client: &mut TestServiceClient) -> Result<(), Box<dyn std::error::Error>> {
    let req = Request::new(Empty::new());
    let _ = client.empty_call(req).await?;
    Ok(())
}

pub async fn large_unary(client: &mut TestServiceClient) -> Result<(), Box<dyn std::error::Error>> {
    let mut req = SimpleRequest::new();
    req.set_response_size(LARGE_RESP as i32);
    let mut payload = Payload::new();
    payload.set_body(vec![0u8; LARGE_REQ]);
    req.set_payload(payload);

    let resp = client.unary_call(Request::new(req)).await?;
    let body = resp.into_inner();
    let resp_len = body.payload().body().len();
    if resp_len != LARGE_RESP {
        return Err(format!("large_unary: expected {LARGE_RESP} bytes, got {resp_len}").into());
    }
    Ok(())
}

pub async fn client_streaming(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let sizes = [27182, 8, 1828, 45904];
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tokio::spawn(async move {
        for n in sizes {
            let mut req = StreamingInputCallRequest::new();
            let mut p = Payload::new();
            p.set_body(vec![0u8; n]);
            req.set_payload(p);
            if tx.send(req).await.is_err() {
                break;
            }
        }
    });

    let resp = client
        .client_streaming(Request::new(ReceiverStream::new(rx)))
        .await?;
    let agg_size = resp.into_inner().aggregated_payload_size();
    if agg_size != 74922 {
        return Err(format!("client_streaming: expected aggregated size 74922, got {agg_size}").into());
    }
    Ok(())
}

pub async fn server_streaming(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let expected = [31415, 9, 2653, 58979];
    let mut req = StreamingOutputCallRequest::new();
    for &n in &expected {
        let mut p = ResponseParameters::new();
        p.set_size(n);
        req.response_parameters_mut().push(p);
    }

    let resp = client.server_streaming(Request::new(req)).await?;
    let mut stream = resp.into_inner();
    let mut count = 0;
    while let Some(msg) = stream.next().await {
        let msg = msg?;
        if count >= expected.len() {
            return Err("server_streaming: extra messages received".into());
        }
        let len = msg.payload().body().len() as i32;
        if len != expected[count] {
            return Err(format!(
                "server_streaming msg {count}: expected {} bytes, got {len}",
                expected[count]
            )
            .into());
        }
        count += 1;
    }
    if count != expected.len() {
        return Err(format!(
            "server_streaming: expected {} messages, got {count}",
            expected.len()
        )
        .into());
    }
    Ok(())
}

pub async fn ping_pong(client: &mut TestServiceClient) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let resp = client
        .full_duplex_call(Request::new(ReceiverStream::new(rx)))
        .await?;
    let mut inbound = resp.into_inner();

    let steps = [
        (31415i32, 27182i32),
        (9, 8),
        (2653, 1828),
        (58979, 45904),
    ];

    for (resp_size, req_size) in steps {
        let mut req = StreamingOutputCallRequest::new();
        let mut p = ResponseParameters::new();
        p.set_size(resp_size);
        req.response_parameters_mut().push(p);
        let mut payload = Payload::new();
        payload.set_body(vec![0u8; req_size as usize]);
        req.set_payload(payload);

        tx.send(req).await?;
        let reply = inbound.next().await.ok_or("missing ping_pong reply")??;
        let got_len = reply.payload().body().len() as i32;
        if got_len != resp_size {
            return Err(format!("ping_pong: expected {resp_size} bytes, got {got_len}").into());
        }
    }
    drop(tx);
    if inbound.next().await.is_some() {
        return Err("ping_pong: received extra message after stream closed".into());
    }
    Ok(())
}

pub async fn empty_stream(client: &mut TestServiceClient) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamingOutputCallRequest>(1);
    drop(tx); // immediate half-close
    let resp = client
        .full_duplex_call(Request::new(ReceiverStream::new(rx)))
        .await?;
    let mut inbound = resp.into_inner();
    if inbound.next().await.is_some() {
        return Err("empty_stream: expected 0 messages".into());
    }
    Ok(())
}

pub async fn cancel_after_begin(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamingInputCallRequest>(1);
    let mut cl = client.clone();
    let handle = tokio::spawn(async move {
        cl.client_streaming(Request::new(ReceiverStream::new(rx))).await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    handle.abort();
    let res = handle.await;
    match res {
        Err(e) if e.is_cancelled() => {}
        Ok(Err(st)) if st.code() == Code::Cancelled => {}
        other => return Err(format!("expected cancelled, got {other:?}").into()),
    }
    drop(tx);
    Ok(())
}

pub async fn timeout_on_sleeping_server(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let mut req = Request::new(ReceiverStream::new(rx));
    req.set_timeout(Duration::from_millis(1));
    let mut m = StreamingOutputCallRequest::new();
    let mut p = Payload::new();
    p.set_body(vec![0u8; 27182]);
    m.set_payload(p);
    let _ = tx.send(m).await;

    match client.full_duplex_call(req).await {
        Err(status) => {
            if status.code() == Code::DeadlineExceeded || status.code() == Code::Cancelled {
                Ok(())
            } else {
                Err(format!(
                    "timeout_on_sleeping_server: expected DeadlineExceeded/Cancelled, got {status}"
                )
                .into())
            }
        }
        Ok(resp) => {
            let mut inbound = resp.into_inner();
            match inbound.next().await {
                Some(Err(status)) => {
                    if status.code() == Code::DeadlineExceeded || status.code() == Code::Cancelled {
                        Ok(())
                    } else {
                        Err(format!(
                            "timeout_on_sleeping_server: expected DeadlineExceeded/Cancelled, got {status}"
                        )
                        .into())
                    }
                }
                Some(Ok(_)) => Err("timeout_on_sleeping_server: received message from sleeping server".into()),
                None => Err("timeout_on_sleeping_server: stream ended without error".into()),
            }
        }
    }
}

pub async fn custom_metadata(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. UnaryCall
    let mut req = SimpleRequest::new();
    req.set_response_size(LARGE_RESP as i32);
    let mut p = Payload::new();
    p.set_body(vec![0u8; LARGE_REQ]);
    req.set_payload(p);

    let mut request = Request::new(req);
    request
        .metadata_mut()
        .insert(ECHO_INITIAL, ECHO_INITIAL_VAL.parse()?);
    request.metadata_mut().insert_bin(
        ECHO_TRAILING_BIN,
        tonic::metadata::MetadataValue::from_bytes(ECHO_TRAILING_BIN_VAL),
    );

    let resp = client.unary_call(request).await?;
    let initial_echo = resp
        .metadata()
        .get(ECHO_INITIAL)
        .ok_or("missing x-grpc-test-echo-initial")?;
    if initial_echo.to_str()? != ECHO_INITIAL_VAL {
        return Err(format!(
            "custom_metadata unary: expected {ECHO_INITIAL_VAL}, got {:?}",
            initial_echo
        )
        .into());
    }

    // 2. FullDuplexCall
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let mut fd_req = Request::new(ReceiverStream::new(rx));
    fd_req
        .metadata_mut()
        .insert(ECHO_INITIAL, ECHO_INITIAL_VAL.parse()?);
    fd_req.metadata_mut().insert_bin(
        ECHO_TRAILING_BIN,
        tonic::metadata::MetadataValue::from_bytes(ECHO_TRAILING_BIN_VAL),
    );

    let mut m = StreamingOutputCallRequest::new();
    let mut rp = ResponseParameters::new();
    rp.set_size(LARGE_RESP as i32);
    m.response_parameters_mut().push(rp);
    let mut pl = Payload::new();
    pl.set_body(vec![0u8; LARGE_REQ]);
    m.set_payload(pl);
    tx.send(m).await?;
    drop(tx);

    let resp = client.full_duplex_call(fd_req).await?;
    let initial_echo = resp
        .metadata()
        .get(ECHO_INITIAL)
        .ok_or("missing x-grpc-test-echo-initial")?;
    if initial_echo.to_str()? != ECHO_INITIAL_VAL {
        return Err(format!(
            "custom_metadata duplex: expected {ECHO_INITIAL_VAL}, got {:?}",
            initial_echo
        )
        .into());
    }
    let mut inbound = resp.into_inner();
    let mut count = 0;
    while let Some(msg) = inbound.next().await {
        let msg = msg?;
        if msg.payload().body().len() != LARGE_RESP {
            return Err("custom_metadata duplex payload size mismatch".into());
        }
        count += 1;
    }
    if count != 1 {
        return Err(format!("custom_metadata duplex: expected 1 message, got {count}").into());
    }
    let trailers = inbound.trailers().await?.ok_or("missing trailers")?;
    let trailing_echo = trailers
        .get_bin(ECHO_TRAILING_BIN)
        .ok_or("missing x-grpc-test-echo-trailing-bin")?;
    if trailing_echo.to_bytes()? != ECHO_TRAILING_BIN_VAL {
        return Err("custom_metadata duplex trailing-bin mismatch".into());
    }

    Ok(())
}

pub async fn status_code_and_message(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let want_code = Code::Unknown;
    let want_msg = "test status message";

    // 1. UnaryCall
    let mut sr = SimpleRequest::new();
    let mut es = EchoStatus::new();
    es.set_code(want_code as i32);
    es.set_message(want_msg);
    sr.set_response_status(es);

    match client.unary_call(Request::new(sr)).await {
        Err(status) => {
            if status.code() != want_code {
                return Err(format!(
                    "status_code_and_message unary: expected code {:?}, got {:?}",
                    want_code,
                    status.code()
                )
                .into());
            }
            if status.message() != want_msg {
                return Err(format!(
                    "status_code_and_message unary: expected msg {:?}, got {:?}",
                    want_msg,
                    status.message()
                )
                .into());
            }
        }
        Ok(_) => return Err("status_code_and_message unary: expected Err, got Ok".into()),
    }

    // 2. FullDuplexCall
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let mut m = StreamingOutputCallRequest::new();
    let mut es = EchoStatus::new();
    es.set_code(want_code as i32);
    es.set_message(want_msg);
    m.set_response_status(es);
    tx.send(m).await?;
    drop(tx);

    match client.full_duplex_call(Request::new(ReceiverStream::new(rx))).await {
        Err(status) => {
            if status.code() != want_code || status.message() != want_msg {
                return Err(format!(
                    "status_code_and_message duplex setup: expected {want_code:?}/{want_msg:?}, got {status:?}"
                )
                .into());
            }
        }
        Ok(resp) => {
            let mut inbound = resp.into_inner();
            match inbound.next().await {
                Some(Err(status)) => {
                    if status.code() != want_code {
                        return Err(format!(
                            "status_code_and_message duplex stream: expected code {:?}, got {:?}",
                            want_code,
                            status.code()
                        )
                        .into());
                    }
                    if status.message() != want_msg {
                        return Err(format!(
                            "status_code_and_message duplex stream: expected msg {:?}, got {:?}",
                            want_msg,
                            status.message()
                        )
                        .into());
                    }
                }
                Some(Ok(_)) => return Err("status_code_and_message duplex: expected error, got message".into()),
                None => return Err("status_code_and_message duplex: stream ended without error".into()),
            }
        }
    }

    Ok(())
}

pub async fn special_status_message(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let want_code = Code::Unknown;
    let want_msg = "\t\ntest with whitespace\r\nand Unicode BMP ☺ and non-BMP 😈\t\n";

    let mut sr = SimpleRequest::new();
    let mut es = EchoStatus::new();
    es.set_code(want_code as i32);
    es.set_message(want_msg);
    sr.set_response_status(es);

    match client.unary_call(Request::new(sr)).await {
        Err(status) => {
            if status.code() != want_code {
                return Err(format!("expected {want_code:?}, got {:?}", status.code()).into());
            }
            if status.message() != want_msg {
                return Err(format!("expected {want_msg:?}, got {:?}", status.message()).into());
            }
            Ok(())
        }
        Ok(_) => Err("expected status error, got Ok".into()),
    }
}

pub async fn unimplemented_method(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    match client.unimplemented_call(Request::new(Empty::new())).await {
        Err(status) if status.code() == Code::Unimplemented => Ok(()),
        Err(status) => Err(format!("expected Unimplemented, got {status}").into()),
        Ok(_) => Err("expected Unimplemented, got Ok".into()),
    }
}

pub async fn unimplemented_service(
    client: &mut TestServiceClient,
) -> Result<(), Box<dyn std::error::Error>> {
    match client.unimplemented_service_call(Request::new(Empty::new())).await {
        Err(status) if status.code() == Code::Unimplemented => Ok(()),
        Err(status) => Err(format!("expected Unimplemented, got {status}").into()),
        Ok(_) => Err("expected Unimplemented, got Ok".into()),
    }
}

#[derive(Debug)]
enum Role {
    Server {
        port: u16,
    },
    Client {
        server_host: String,
        server_port: u16,
        test_case: String,
    },
}

fn parse_cli_args() -> Result<Role, String> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let mut role: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut server_host = "127.0.0.1".to_string();
    let mut server_port: Option<u16> = None;
    let mut test_case: Option<String> = None;

    let mut i = 0;
    while i < raw_args.len() {
        let arg = &raw_args[i];
        if !arg.starts_with('-') {
            if role.is_none() && (arg == "server" || arg == "client") {
                role = Some(arg.clone());
            }
            i += 1;
            continue;
        }

        let (key, val_opt) = match arg.split_once('=') {
            Some((k, v)) => (k.trim_start_matches('-'), Some(v.to_string())),
            None => {
                let k = arg.trim_start_matches('-');
                let v = if i + 1 < raw_args.len() && !raw_args[i + 1].starts_with('-') {
                    i += 1;
                    Some(raw_args[i].clone())
                } else {
                    None
                };
                (k, v)
            }
        };

        match key {
            "role" => {
                if let Some(v) = val_opt {
                    role = Some(v);
                }
            }
            "port" => {
                if let Some(v) = val_opt {
                    port = Some(v.parse().map_err(|e| format!("invalid port: {e}"))?);
                }
            }
            "server_host" => {
                if let Some(v) = val_opt {
                    server_host = v;
                }
            }
            "server_port" => {
                if let Some(v) = val_opt {
                    server_port = Some(v.parse().map_err(|e| format!("invalid server_port: {e}"))?);
                }
            }
            "test_case" => {
                if let Some(v) = val_opt {
                    test_case = Some(v);
                }
            }
            "use_tls" => {}
            _ => {}
        }
        i += 1;
    }

    let resolved_role = role.as_deref().unwrap_or(if test_case.is_some() {
        "client"
    } else {
        "server"
    });

    if resolved_role == "server" {
        let p = port.or(server_port).unwrap_or(10000);
        Ok(Role::Server { port: p })
    } else {
        let p = server_port.or(port).unwrap_or(10000);
        let tc = test_case.unwrap_or_else(|| "empty_unary".to_string());
        Ok(Role::Client {
            server_host,
            server_port: p,
            test_case: tc,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = parse_cli_args().map_err(|e| {
        eprintln!("Error: {e}");
        e
    })?;

    match role {
        Role::Server { port } => {
            let addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
            eprintln!("Tonic interop server listening on {addr}");
            Server::builder()
                .add_service(TestServiceServer)
                .serve(addr)
                .await?;
            Ok(())
        }
        Role::Client {
            server_host,
            server_port,
            test_case,
        } => {
            let mut client = TestServiceClient::connect(&server_host, server_port).await?;
            match run_client_case(&mut client, &test_case).await {
                Ok(()) => {
                    println!("Passed");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("FAIL: case {test_case} failed: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
