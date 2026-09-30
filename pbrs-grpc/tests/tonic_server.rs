//! Mounting a native generated service in tonic's transport stack.

#![cfg(feature = "tonic")]
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test")]

mod common;

use common::{Echo, name_of, req};
use http::{Request as HttpRequest, Response as HttpResponse};
use pbrs_grpc::Request;
use pbrs_grpc::hello::{GreeterClient, GreeterServer};
use pbrs_grpc::tower_server::{TonicService, TonicServiceExt};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::body::Body;
use tonic::server::NamedService;
use tower::Service;

#[derive(Clone)]
struct ConnectInfoProbe<S> {
    inner: S,
    observed: Arc<AtomicBool>,
}

impl<S: NamedService> NamedService for ConnectInfoProbe<S> {
    const NAME: &'static str = S::NAME;
}

impl<S> Service<HttpRequest<Body>> for ConnectInfoProbe<S>
where
    S: Service<HttpRequest<Body>, Response = HttpResponse<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = HttpResponse<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: HttpRequest<Body>) -> Self::Future {
        if let Some(info) = request
            .extensions()
            .get::<tonic::transport::server::TcpConnectInfo>()
        {
            self.observed.store(
                info.local_addr().is_some() && info.remote_addr().is_some(),
                Ordering::Release,
            );
        }
        Box::pin(self.inner.call(request))
    }
}

#[tokio::test]
async fn generated_service_mounts_in_tonic_and_exposes_tonic_connect_info() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let observed = Arc::new(AtomicBool::new(false));
    let service: TonicService<GreeterServer<Echo>> = GreeterServer::new(Echo).into_tonic_service();
    let service = ConnectInfoProbe {
        inner: service,
        observed: Arc::clone(&observed),
    };

    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .expect("tonic server");
    });

    let client = GreeterClient::connect(addr).await.expect("native client");
    let response = client
        .say_hello(Request::new(req("tonic mount")))
        .await
        .expect("say hello");
    assert_eq!(name_of(response.get_ref()), "tonic mount");
    assert!(observed.load(Ordering::Acquire));
    server.abort();
}

#[test]
fn tonic_tls_connect_info_keeps_peer_certificate_api_available() {
    fn assert_peer_certs_api(
        info: &tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>,
    ) {
        let _ = info.peer_certs();
    }
    let _ = assert_peer_certs_api;
}
