//! Tower server adapter for [`Router`].
//!
//! The adapter is feature-gated behind `tower` and routes each HTTP/2 request
//! through the existing native server path, so health, reflection, interceptors,
//! limits, and every RPC shape keep the same behavior as `serve_connection`.
//! The two bodies progress independently under HTTP/2 flow control.

use crate::{Router, Status};
use bytes::{Buf, Bytes};
use http::header::{CONTENT_TYPE, HeaderValue};
use http::{HeaderMap, Request, Response, Uri};
use http_body::{Body, Frame};
use std::convert::Infallible;
use std::future::{Future, pending, poll_fn};
#[cfg(feature = "tonic")]
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::task::JoinHandle;
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
#[derive(Debug)]
pub struct TonicService<N> {
    inner: RouterService,
    service: PhantomData<fn() -> N>,
}

#[cfg(feature = "tonic")]
impl<N> Clone for TonicService<N> {
    fn clone(&self) -> Self {
        Self::new(self.inner.clone())
    }
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

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> std::fmt::Debug for AbortOnDrop<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AbortOnDrop").field(&self.0.id()).finish()
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct UploadFailure {
    status: Status,
    // Keep the paused connection alive until the caller reads the local
    // failure. A transport EOF could otherwise hide that status.
    _connection: h2::client::Connection<tokio::io::DuplexStream>,
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

    let (client_io, server_io) = tokio::io::duplex(duplex_buffer);
    let server = AbortOnDrop(tokio::spawn(router.serve_connection(server_io)));
    let (mut send_request, connection) = h2::client::handshake(client_io)
        .await
        .map_err(bridge_error)?;

    let end_stream = body.is_end_stream();
    let (mut response, send) = send_request
        .send_request(Request::from_parts(parts, ()), end_stream)
        .map_err(bridge_error)?;
    drop(send_request);

    // Drive h2 even while the upstream body waits for the next reply.
    let mut driver = AbortOnDrop(tokio::spawn(async move {
        let _server = server;
        let mut connection = connection;
        let upload = async move {
            if !end_stream {
                forward_body(body, send).await?;
            }
            pending::<Result<(), Status>>().await
        };
        tokio::select! {
            biased;
            result = upload => result.err().map(|status| UploadFailure {
                status,
                _connection: connection,
            }),
            // The response future and RecvStream carry h2 errors.
            _ = &mut connection => None,
        }
    }));

    let (response, driver) = tokio::select! {
        biased;
        result = &mut driver.0 => {
            if let Some(failure) = result.map_err(bridge_error)? {
                return Err(failure.status);
            }
            (response.await, None)
        }
        result = &mut response => (result, Some(driver)),
    };
    let (parts, recv) = response.map_err(bridge_error)?.into_parts();
    let body = (!recv.is_end_stream()).then_some(BridgeBody { recv, driver });
    Ok(Response::from_parts(parts, TowerBody(body)))
}

async fn forward_body<B>(mut body: B, mut send: h2::SendStream<Bytes>) -> Result<(), Status>
where
    B: Body + Unpin,
    B::Data: Buf,
    B::Error: std::fmt::Display,
{
    // Convert while polling so generic source buffers need not be Send.
    while let Some(frame) = poll_fn(|cx| {
        let frame = match Pin::new(&mut body).poll_frame(cx) {
            Poll::Ready(frame) => frame,
            Poll::Pending if send.poll_reset(cx).is_ready() => return Poll::Ready(Ok(None)),
            Poll::Pending => return Poll::Pending,
        };
        let frame = frame.transpose().map_err(bridge_error)?;
        Poll::Ready(Ok::<_, Status>(frame.map(|frame| {
            frame.map_data(|mut data| {
                let len = data.remaining();
                data.copy_to_bytes(len)
            })
        })))
    })
    .await?
    {
        match frame.into_data() {
            Ok(mut data) => {
                let end_stream = body.is_end_stream();
                if data.is_empty() && end_stream {
                    break;
                }
                while !data.is_empty() {
                    // Reserve a bounded chunk even when the source frame is large.
                    let wanted = data.len().min(64 * 1024);
                    send.reserve_capacity(wanted);
                    if send.capacity() == 0
                        && !matches!(poll_fn(|cx| send.poll_capacity(cx)).await, Some(Ok(_)))
                    {
                        // A reset can accompany an early terminal response.
                        // Let the receive side report the peer's outcome.
                        return Ok(());
                    }
                    let size = wanted.min(send.capacity());
                    let end = end_stream && size == data.len();
                    if send.send_data(data.split_to(size), end).is_err() {
                        return Ok(());
                    }
                }
                if end_stream {
                    return Ok(());
                }
            }
            Err(frame) => {
                if let Ok(trailers) = frame.into_trailers() {
                    send.send_trailers(trailers).ok();
                    return Ok(());
                }
            }
        }
        tokio::task::consume_budget().await;
    }
    send.send_data(Bytes::new(), true).ok();
    Ok(())
}

fn bridge_error(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
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

fn status_headers(status: &Status) -> HeaderMap {
    crate::wire::grpc_trailers(status).unwrap_or_else(|_| {
        let mut headers = HeaderMap::new();
        headers.insert("grpc-status", HeaderValue::from_static("13"));
        headers
    })
}

fn status_response(status: Status) -> Response<TowerBody> {
    let mut response = Response::new(TowerBody(None));
    *response.headers_mut() = status_headers(&status);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
    response
}

/// Response body returned by [`RouterService`].
#[derive(Debug)]
pub struct TowerBody(Option<BridgeBody>);

#[derive(Debug)]
struct BridgeBody {
    recv: h2::RecvStream,
    driver: Option<AbortOnDrop<Option<UploadFailure>>>,
}

impl BridgeBody {
    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<Frame<Bytes>>, Status>> {
        if let Some(driver) = &mut self.driver {
            if let Poll::Ready(result) = Pin::new(&mut driver.0).poll(cx) {
                self.driver = None;
                if let Some(failure) = result.map_err(bridge_error)? {
                    return Poll::Ready(Err(failure.status));
                }
            }
        }
        match std::task::ready!(self.recv.poll_data(cx)) {
            Some(data) => {
                let data = data.map_err(bridge_error)?;
                self.recv
                    .flow_control()
                    .release_capacity(data.len())
                    .map_err(bridge_error)?;
                Poll::Ready(Ok(Some(Frame::data(data))))
            }
            None => {
                let trailers =
                    std::task::ready!(self.recv.poll_trailers(cx)).map_err(bridge_error)?;
                Poll::Ready(Ok(trailers.map(Frame::trailers)))
            }
        }
    }
}

impl Body for TowerBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let Some(body) = &mut this.0 else {
            return Poll::Ready(None);
        };
        match std::task::ready!(body.poll_frame(cx)) {
            Ok(frame) => {
                if body.recv.is_end_stream() {
                    this.0 = None;
                }
                Poll::Ready(frame.map(Ok))
            }
            Err(status) => {
                this.0 = None;
                Poll::Ready(Some(Ok(Frame::trailers(status_headers(&status)))))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_none()
    }
}
