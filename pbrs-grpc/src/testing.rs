//! Official `grpc.testing` messages plus TestService client/server.

#![allow(
    missing_docs,
    reason = "generated messages plus handwritten kernel stubs"
)]

include!(concat!(env!("OUT_DIR"), "/test.rs"));

use crate::client::Channel;
use crate::request::{Call, Request, Response};
use crate::server::{
    dispatch_bidi, dispatch_client_stream, dispatch_server_stream, dispatch_unary, reject_unknown,
    Http2Handler, Server,
};
use crate::status::{Code, Status};
use crate::stream::{InItem, Inbound, StreamingSender};
use bytes::Bytes;
use h2::RecvStream;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

const EMPTY_CALL: &str = "/grpc.testing.TestService/EmptyCall";
const UNARY_CALL: &str = "/grpc.testing.TestService/UnaryCall";
const STREAMING_OUTPUT: &str = "/grpc.testing.TestService/StreamingOutputCall";
const STREAMING_INPUT: &str = "/grpc.testing.TestService/StreamingInputCall";
const FULL_DUPLEX: &str = "/grpc.testing.TestService/FullDuplexCall";
const HALF_DUPLEX: &str = "/grpc.testing.TestService/HalfDuplexCall";

/// Official interop `TestService`. `UnimplementedCall` is intentionally absent.
pub trait TestService: Send + Sync + 'static {
    /// EmptyCall.
    fn empty_call(
        &self,
        request: Request<Empty>,
    ) -> impl Future<Output = Result<Response<Empty>, Status>> + Send;
    /// UnaryCall.
    fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> impl Future<Output = Result<Response<SimpleResponse>, Status>> + Send;
    /// StreamingOutputCall.
    fn streaming_output_call(
        &self,
        request: Request<StreamingOutputCallRequest>,
    ) -> impl Future<Output = Result<Response<Inbound<StreamingOutputCallResponse>>, Status>> + Send;
    /// StreamingInputCall.
    fn streaming_input_call(
        &self,
        request: Request<Inbound<StreamingInputCallRequest>>,
    ) -> impl Future<Output = Result<Response<StreamingInputCallResponse>, Status>> + Send;
    /// FullDuplexCall.
    fn full_duplex_call(
        &self,
        request: Request<Inbound<StreamingOutputCallRequest>>,
    ) -> impl Future<Output = Result<Response<Inbound<StreamingOutputCallResponse>>, Status>> + Send;
    /// HalfDuplexCall.
    fn half_duplex_call(
        &self,
        request: Request<Inbound<StreamingOutputCallRequest>>,
    ) -> impl Future<Output = Result<Response<Inbound<StreamingOutputCallResponse>>, Status>> + Send;
}

/// Serve [`TestService`].
pub struct TestServiceServer<T> {
    inner: Arc<T>,
}

impl<T: TestService> TestServiceServer<T> {
    /// Wrap an implementation.
    pub fn new(inner: T) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Accept on `listener`.
    pub async fn serve_listener(self, listener: TcpListener) -> Result<(), Status> {
        Server::new(self).serve_listener(listener).await
    }
}

impl<T: TestService> Http2Handler for TestServiceServer<T> {
    fn handle(
        &self,
        request: http::Request<RecvStream>,
        respond: h2::server::SendResponse<Bytes>,
    ) -> impl Future<Output = ()> + Send {
        let inner = Arc::clone(&self.inner);
        async move {
            match request.uri().path() {
                EMPTY_CALL => {
                    dispatch_unary(request, respond, |req| async move {
                        inner.empty_call(req).await
                    })
                    .await;
                }
                UNARY_CALL => {
                    dispatch_unary(request, respond, |req| async move {
                        inner.unary_call(req).await
                    })
                    .await;
                }
                STREAMING_OUTPUT => {
                    dispatch_server_stream(request, respond, |req| async move {
                        inner.streaming_output_call(req).await
                    })
                    .await;
                }
                STREAMING_INPUT => {
                    dispatch_client_stream(request, respond, |req| async move {
                        inner.streaming_input_call(req).await
                    })
                    .await;
                }
                FULL_DUPLEX => {
                    dispatch_bidi(request, respond, |req| async move {
                        inner.full_duplex_call(req).await
                    })
                    .await;
                }
                HALF_DUPLEX => {
                    dispatch_bidi(request, respond, |req| async move {
                        inner.half_duplex_call(req).await
                    })
                    .await;
                }
                other => reject_unknown(respond, other),
            }
        }
    }
}

/// Client for `grpc.testing.TestService`.
#[derive(Clone)]
pub struct TestServiceClient {
    channel: Channel,
}

impl TestServiceClient {
    /// Wrap a connected channel.
    #[must_use]
    pub fn new(channel: Channel) -> Self {
        Self { channel }
    }

    /// EmptyCall.
    pub fn empty_call(&self, req: Request<Empty>) -> Call<Response<Empty>> {
        self.channel.unary(EMPTY_CALL, req)
    }

    /// UnaryCall.
    pub fn unary_call(&self, req: Request<SimpleRequest>) -> Call<Response<SimpleResponse>> {
        self.channel.unary(UNARY_CALL, req)
    }

    /// StreamingOutputCall.
    pub fn streaming_output_call(
        &self,
        req: Request<StreamingOutputCallRequest>,
    ) -> Call<Response<Inbound<StreamingOutputCallResponse>>> {
        self.channel.server_streaming(STREAMING_OUTPUT, req)
    }

    /// StreamingInputCall.
    pub fn streaming_input_call(
        &self,
        req: Request<()>,
    ) -> (
        StreamingSender<StreamingInputCallRequest>,
        Call<Response<StreamingInputCallResponse>>,
    ) {
        self.channel.client_streaming(STREAMING_INPUT, req)
    }

    /// FullDuplexCall.
    pub fn full_duplex_call(
        &self,
        req: Request<()>,
    ) -> (
        StreamingSender<StreamingOutputCallRequest>,
        Call<Response<Inbound<StreamingOutputCallResponse>>>,
    ) {
        self.channel.bidi(FULL_DUPLEX, req)
    }

    /// Call a method that does not exist on TestService.
    pub fn unimplemented_method(&self, req: Request<Empty>) -> Call<Response<Empty>> {
        self.channel
            .unary("/grpc.testing.TestService/UnimplementedCall", req)
    }

    /// Call UnimplementedService.
    pub fn unimplemented_service(&self, req: Request<Empty>) -> Call<Response<Empty>> {
        self.channel
            .unary("/grpc.testing.UnimplementedService/UnimplementedCall", req)
    }
}

/// Official interop TestService implementation.
#[derive(Default)]
pub struct InteropTestService;

const ECHO_INITIAL: &str = "x-grpc-test-echo-initial";
const ECHO_TRAILING: &str = "x-grpc-test-echo-trailing-bin";

fn echo_into<T, R>(req: &Request<T>, resp: &mut Response<R>) {
    if let Some(v) = req.metadata().get(ECHO_INITIAL) {
        resp.metadata_mut().insert(ECHO_INITIAL, v).ok();
    }
    if let Some(v) = req.metadata().get_bin(ECHO_TRAILING) {
        resp.trailers_mut()
            .insert_bin(ECHO_TRAILING, v.to_vec())
            .ok();
    }
}

fn zeros_payload(n: i32) -> Payload {
    let n = usize::try_from(n.max(0)).unwrap_or(0);
    let mut p = Payload::new();
    p.set_body(vec![0u8; n]);
    p
}

fn status_from_echo(inner_code: i32, message: impl Into<String>) -> Status {
    Status::new(Code::from_i32(inner_code), message)
}

impl TestService for InteropTestService {
    async fn empty_call(&self, request: Request<Empty>) -> Result<Response<Empty>, Status> {
        let mut resp = Response::new(Empty::new());
        echo_into(&request, &mut resp);
        Ok(resp)
    }

    async fn unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let compressed = request.compressed();
        let mut resp = Response::new(SimpleResponse::new());
        echo_into(&request, &mut resp);
        let inner = request.into_inner();
        if inner.has_expect_compressed() && inner.expect_compressed().value() && !compressed {
            return Err(Status::invalid_argument("request not compressed"));
        }
        if inner.has_response_status() {
            let st = inner.response_status();
            return Err(status_from_echo(st.code(), st.message().to_string()));
        }
        let mut msg = SimpleResponse::new();
        msg.set_payload(zeros_payload(inner.response_size()));
        let mut out = Response::new(msg);
        echo_into_copy(&resp, &mut out);
        if inner.has_response_compressed() && inner.response_compressed().value() {
            out.set_compress(true);
        }
        Ok(out)
    }

    async fn streaming_output_call(
        &self,
        request: Request<StreamingOutputCallRequest>,
    ) -> Result<Response<Inbound<StreamingOutputCallResponse>>, Status> {
        let mut resp = Response::new({
            let (_tx, rx) = Inbound::<StreamingOutputCallResponse>::channel(1);
            rx
        });
        echo_into(&request, &mut resp);
        let inner = request.into_inner();
        if inner.has_response_status() {
            let st = inner.response_status();
            return Err(status_from_echo(st.code(), st.message().to_string()));
        }
        let (tx, rx) = Inbound::channel(8);
        let want_gzip = inner
            .response_parameters()
            .iter()
            .any(|p| p.has_compressed() && p.compressed().value());
        let sizes: Vec<(i32, i32, bool)> = inner
            .response_parameters()
            .iter()
            .map(|p| {
                (
                    p.size(),
                    p.interval_us(),
                    p.has_compressed() && p.compressed().value(),
                )
            })
            .collect();
        drop(tokio::spawn(async move {
            for (size, interval_us, compress) in sizes {
                if interval_us > 0 {
                    let us = u64::try_from(interval_us).unwrap_or(0);
                    tokio::time::sleep(Duration::from_micros(us)).await;
                }
                let mut msg = StreamingOutputCallResponse::new();
                msg.set_payload(zeros_payload(size));
                if tx
                    .send(Ok(InItem {
                        message: msg,
                        compressed: compress,
                    }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        let mut out = Response::new(rx);
        echo_into_copy(&resp, &mut out);
        if want_gzip {
            out.set_compress(true);
        }
        Ok(out)
    }

    async fn streaming_input_call(
        &self,
        request: Request<Inbound<StreamingInputCallRequest>>,
    ) -> Result<Response<StreamingInputCallResponse>, Status> {
        let mut resp = Response::new(StreamingInputCallResponse::new());
        echo_into(&request, &mut resp);
        let mut inbound = request.into_inner();
        let mut total: i32 = 0;
        while let Some(item) = inbound.next_item().await? {
            if item.message.has_expect_compressed()
                && item.message.expect_compressed().value()
                && !item.compressed
            {
                return Err(Status::invalid_argument("request not compressed"));
            }
            let n = item.message.payload().body().len();
            total = total.saturating_add(i32::try_from(n).unwrap_or(i32::MAX));
        }
        let mut msg = StreamingInputCallResponse::new();
        msg.set_aggregated_payload_size(total);
        let mut out = Response::new(msg);
        echo_into_copy(&resp, &mut out);
        Ok(out)
    }

    async fn full_duplex_call(
        &self,
        request: Request<Inbound<StreamingOutputCallRequest>>,
    ) -> Result<Response<Inbound<StreamingOutputCallResponse>>, Status> {
        let mut resp = Response::new({
            let (_tx, rx) = Inbound::<StreamingOutputCallResponse>::channel(1);
            rx
        });
        echo_into(&request, &mut resp);
        let mut inbound = request.into_inner();
        let (tx, rx) = Inbound::channel(8);
        drop(tokio::spawn(async move {
            while let Ok(Some(item)) = inbound.next_item().await {
                let req = item.message;
                if req.has_response_status() {
                    let st = req.response_status();
                    tx.send(Err(status_from_echo(st.code(), st.message().to_string())))
                        .await
                        .ok();
                    return;
                }
                let sizes: Vec<(i32, i32, bool)> = req
                    .response_parameters()
                    .iter()
                    .map(|p| {
                        (
                            p.size(),
                            p.interval_us(),
                            p.has_compressed() && p.compressed().value(),
                        )
                    })
                    .collect();
                for (size, interval_us, compress) in sizes {
                    if interval_us > 0 {
                        let us = u64::try_from(interval_us).unwrap_or(0);
                        tokio::time::sleep(Duration::from_micros(us)).await;
                    }
                    let mut msg = StreamingOutputCallResponse::new();
                    msg.set_payload(zeros_payload(size));
                    if tx
                        .send(Ok(InItem {
                            message: msg,
                            compressed: compress,
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }));
        let mut out = Response::new(rx);
        echo_into_copy(&resp, &mut out);
        Ok(out)
    }

    async fn half_duplex_call(
        &self,
        request: Request<Inbound<StreamingOutputCallRequest>>,
    ) -> Result<Response<Inbound<StreamingOutputCallResponse>>, Status> {
        self.full_duplex_call(request).await
    }
}

fn echo_into_copy<A, B>(from: &Response<A>, to: &mut Response<B>) {
    if let Some(v) = from.metadata().get(ECHO_INITIAL) {
        to.metadata_mut().insert(ECHO_INITIAL, v).ok();
    }
    if let Some(v) = from.trailers().get_bin(ECHO_TRAILING) {
        to.trailers_mut().insert_bin(ECHO_TRAILING, v.to_vec()).ok();
    }
}
