//! Tower server adapter for [`Router`].
//!
//! The adapter is feature-gated behind `tower` and routes each HTTP/2 request
//! through the existing native server path, so health, reflection, interceptors,
//! limits, and every RPC shape keep the same behavior as `serve_connection`.

use crate::{Router, Status};
use bytes::{Buf, Bytes, BytesMut};
use http::header::{CONTENT_TYPE, HeaderValue};
use http::{HeaderMap, Request, Response, Uri};
use http_body::{Body, Frame};
use std::convert::Infallible;
use std::future::Future;
#[cfg(feature = "tonic")]
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::Service;

/// A [`Router`] exposed as `tower::Service<http::Request<B>>`.
#[derive(Clone, Debug)]
pub struct RouterService {
    router: Router,
    duplex_buffer: usize,
}

impl RouterService {
    /// Build a tower service from a native router.
    #[must_use]
    pub fn new(router: Router) -> Self {
        Self {
            router,
            duplex_buffer: 1024 * 1024,
        }
    }

    /// Size of the in-memory HTTP/2 bridge used per request.
    #[must_use]
    pub fn duplex_buffer(mut self, bytes: usize) -> Self {
        self.duplex_buffer = bytes.max(64 * 1024);
        self
    }

    /// Recover the native router.
    #[must_use]
    pub fn into_inner(self) -> Router {
        self.router
    }
}

impl Router {
    /// Expose this router as a tower service.
    #[must_use]
    pub fn into_tower_service(self) -> RouterService {
        RouterService::new(self)
    }
}

/// A native generated service exposed with tonic's server type contract.
///
/// `N` supplies the generated service name at compile time. The request and
/// response bodies are tonic's own [`tonic::body::Body`], while payload chunks
/// remain `Bytes` throughout the bridge.
#[cfg(feature = "tonic")]
#[derive(Clone, Debug)]
pub struct TonicService<N> {
    inner: RouterService,
    service: PhantomData<fn() -> N>,
}

#[cfg(feature = "tonic")]
impl<N> TonicService<N> {
    fn new(inner: RouterService) -> Self {
        Self {
            inner,
            service: PhantomData,
        }
    }
}

/// Convert a generated native service into a service accepted by
/// `tonic::transport::Server::add_service`.
#[cfg(feature = "tonic")]
pub trait TonicServiceExt: crate::Service + Sized {
    /// Mount this service in a tonic transport stack.
    fn into_tonic_service(self) -> TonicService<Self> {
        TonicService::new(Router::new().add_service(self).into_tower_service())
    }
}

#[cfg(feature = "tonic")]
impl<S: crate::Service> TonicServiceExt for S {}

#[cfg(feature = "tonic")]
impl<N: crate::Service> tonic::server::NamedService for TonicService<N> {
    const NAME: &'static str = N::NAME;
}

#[cfg(feature = "tonic")]
impl<N> Service<Request<tonic::body::Body>> for TonicService<N>
where
    N: crate::Service,
{
    type Response = Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <RouterService as Service<Request<tonic::body::Body>>>::poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, request: Request<tonic::body::Body>) -> Self::Future {
        let future = self.inner.call(request);
        Box::pin(async move {
            future.await.map(|response| {
                let (parts, body) = response.into_parts();
                Response::from_parts(parts, tonic::body::Body::new(body))
            })
        })
    }
}

impl<B> Service<Request<B>> for RouterService
where
    B: Body + Send + Unpin + 'static,
    B::Data: Buf,
    B::Error: std::fmt::Display,
{
    type Response = Response<TowerBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let router = self.router.clone();
        let duplex_buffer = self.duplex_buffer;
        Box::pin(async move {
            let response = match call_router(router, request, duplex_buffer).await {
                Ok(response) => response,
                Err(status) => status_response(status),
            };
            Ok(response)
        })
    }
}

async fn call_router<B>(
    router: Router,
    request: Request<B>,
    duplex_buffer: usize,
) -> Result<Response<TowerBody>, Status>
where
    B: Body + Send + Unpin + 'static,
    B::Data: Buf,
    B::Error: std::fmt::Display,
{
    let (mut parts, body) = request.into_parts();
    parts.uri = bridge_uri(&parts.uri)?;
    let payload = collect_body(body).await?;

    let (client_io, server_io) = tokio::io::duplex(duplex_buffer);
    let server = tokio::spawn(async move {
        router.serve_connection(server_io).await.ok();
    });

    let (mut send_request, connection) = h2::client::handshake(client_io)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    let driver = tokio::spawn(async move {
        connection.await.ok();
    });

    let end_stream = payload.is_empty();
    let request = Request::from_parts(parts, ());
    let (response, mut send_stream) = send_request
        .send_request(request, end_stream)
        .map_err(|e| Status::internal(e.to_string()))?;
    if !end_stream {
        send_stream
            .send_data(payload, true)
            .map_err(|e| Status::internal(e.to_string()))?;
    }

    let response = response
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    let (parts, mut body) = response.into_parts();
    let mut data = BytesMut::new();
    while let Some(chunk) = body
        .data()
        .await
        .transpose()
        .map_err(|e| Status::internal(e.to_string()))?
    {
        data.extend_from_slice(&chunk);
    }
    let trailers = body
        .trailers()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    driver.abort();
    server.abort();
    Ok(Response::from_parts(
        parts,
        TowerBody::new(data.freeze(), trailers),
    ))
}

async fn collect_body<B>(mut body: B) -> Result<Bytes, Status>
where
    B: Body + Unpin,
    B::Data: Buf,
    B::Error: std::fmt::Display,
{
    let mut out = BytesMut::new();
    while let Some(frame) = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let frame = frame.map_err(|e| Status::internal(e.to_string()))?;
        if let Ok(mut data) = frame.into_data() {
            while data.has_remaining() {
                let chunk = data.chunk();
                out.extend_from_slice(chunk);
                let len = chunk.len();
                data.advance(len);
            }
        }
    }
    Ok(out.freeze())
}

fn bridge_uri(uri: &Uri) -> Result<Uri, Status> {
    if uri.scheme().is_some() && uri.authority().is_some() {
        return Ok(uri.clone());
    }
    let path = uri
        .path_and_query()
        .map_or("/", http::uri::PathAndQuery::as_str);
    Uri::builder()
        .scheme("http")
        .authority("tower.local")
        .path_and_query(path)
        .build()
        .map_err(|e| Status::internal(e.to_string()))
}

fn status_response(status: Status) -> Response<TowerBody> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
    if let Ok(value) = HeaderValue::from_str(&status.code().to_i32().to_string()) {
        headers.insert("grpc-status", value);
    }
    if !status.message().is_empty() {
        if let Ok(value) = HeaderValue::from_str(status.message()) {
            headers.insert("grpc-message", value);
        }
    }
    Response::builder()
        .status(http::StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/grpc"))
        .body(TowerBody::new(Bytes::new(), Some(headers)))
        .unwrap_or_else(|_| Response::new(TowerBody::new(Bytes::new(), None)))
}

/// Response body returned by [`RouterService`].
#[derive(Debug)]
pub struct TowerBody {
    data: Option<Bytes>,
    trailers: Option<HeaderMap>,
}

impl TowerBody {
    fn new(data: Bytes, trailers: Option<HeaderMap>) -> Self {
        Self {
            data: (!data.is_empty()).then_some(data),
            trailers,
        }
    }
}

impl Body for TowerBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if let Some(data) = this.data.take() {
            return Poll::Ready(Some(Ok(Frame::data(data))));
        }
        if let Some(trailers) = this.trailers.take() {
            return Poll::Ready(Some(Ok(Frame::trailers(trailers))));
        }
        Poll::Ready(None)
    }

    fn is_end_stream(&self) -> bool {
        self.data.is_none() && self.trailers.is_none()
    }
}
