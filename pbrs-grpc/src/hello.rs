//! helloworld messages (generated) and kernel Greeter client/server.

#![allow(
    missing_docs,
    reason = "generated messages plus handwritten kernel stubs"
)]

include!(concat!(env!("OUT_DIR"), "/hello.rs"));

use crate::client::Channel;
use crate::request::{Call, Request, Response};
use crate::server::{
    dispatch_bidi, dispatch_client_stream, dispatch_server_stream, dispatch_unary, reject_unknown,
    Http2Handler, Server,
};
use crate::status::Status;
use crate::stream::{Inbound, StreamingSender};
use bytes::Bytes;
use h2::RecvStream;
use std::future::Future;
use std::sync::Arc;
use tokio::net::TcpListener;

const SAY_HELLO: &str = "/helloworld.Greeter/SayHello";
const CLIENT_HELLO: &str = "/helloworld.Greeter/ClientHello";
const SERVER_HELLO: &str = "/helloworld.Greeter/ServerHello";
const STREAM_HELLO: &str = "/helloworld.Greeter/StreamHello";

/// Service implemented by a kernel server.
pub trait Greeter: Send + Sync + 'static {
    /// Unary echo.
    fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> impl Future<Output = Result<Response<HelloReply>, Status>> + Send;

    /// Client-stream: handler reads [`Inbound`] until half-close.
    fn client_hello(
        &self,
        request: Request<Inbound<HelloRequest>>,
    ) -> impl Future<Output = Result<Response<HelloReply>, Status>> + Send;

    /// Server-stream: handler returns an [`Inbound`] of replies.
    fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> impl Future<Output = Result<Response<Inbound<HelloReply>>, Status>> + Send;

    /// Bidi stream.
    fn stream_hello(
        &self,
        request: Request<Inbound<HelloRequest>>,
    ) -> impl Future<Output = Result<Response<Inbound<HelloReply>>, Status>> + Send;
}

/// Serve [`Greeter`] over HTTP/2.
pub struct GreeterServer<T> {
    inner: Arc<T>,
}

impl<T: Greeter> GreeterServer<T> {
    /// Wrap an implementation.
    pub fn new(inner: T) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Accept on `listener` until it fails.
    pub async fn serve_listener(self, listener: TcpListener) -> Result<(), Status> {
        Server::new(self).serve_listener(listener).await
    }
}

impl<T: Greeter> Http2Handler for GreeterServer<T> {
    fn handle(
        &self,
        request: http::Request<RecvStream>,
        respond: h2::server::SendResponse<Bytes>,
    ) -> impl Future<Output = ()> + Send {
        let inner = Arc::clone(&self.inner);
        async move {
            match request.uri().path() {
                SAY_HELLO => {
                    dispatch_unary(
                        request,
                        respond,
                        |req| async move { inner.say_hello(req).await },
                    )
                    .await;
                }
                CLIENT_HELLO => {
                    dispatch_client_stream(request, respond, |req| async move {
                        inner.client_hello(req).await
                    })
                    .await;
                }
                SERVER_HELLO => {
                    dispatch_server_stream(request, respond, |req| async move {
                        inner.server_hello(req).await
                    })
                    .await;
                }
                STREAM_HELLO => {
                    dispatch_bidi(request, respond, |req| async move {
                        inner.stream_hello(req).await
                    })
                    .await;
                }
                other => reject_unknown(respond, other),
            }
        }
    }
}

/// Client for `helloworld.Greeter`.
#[derive(Clone)]
pub struct GreeterClient {
    channel: Channel,
}

impl GreeterClient {
    /// Wrap a connected channel.
    #[must_use]
    pub fn new(channel: Channel) -> Self {
        Self { channel }
    }

    /// Unary `SayHello`.
    pub fn say_hello(&self, req: Request<HelloRequest>) -> Call<Response<HelloReply>> {
        self.channel.unary(SAY_HELLO, req)
    }

    /// Client-stream `ClientHello`.
    pub fn client_hello(
        &self,
        req: Request<()>,
    ) -> (StreamingSender<HelloRequest>, Call<Response<HelloReply>>) {
        self.channel.client_streaming(CLIENT_HELLO, req)
    }

    /// Server-stream `ServerHello`.
    pub fn server_hello(&self, req: Request<HelloRequest>) -> Call<Response<Inbound<HelloReply>>> {
        self.channel.server_streaming(SERVER_HELLO, req)
    }

    /// Bidi `StreamHello`.
    pub fn stream_hello(
        &self,
        req: Request<()>,
    ) -> (
        StreamingSender<HelloRequest>,
        Call<Response<Inbound<HelloReply>>>,
    ) {
        self.channel.bidi(STREAM_HELLO, req)
    }
}
