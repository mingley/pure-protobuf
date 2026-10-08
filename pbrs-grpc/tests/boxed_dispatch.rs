//! Router dispatch hooks and generated full-path routing.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "integration test assertions"
)]

use pbrs_grpc::{
    Channel, Code, Greeter, GreeterServer, HelloReply, HelloRequest, Request, Response, Router,
    Rpc, Server, Service, Status,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message(request.get_ref().name().to_owned());
        Ok(Response::new(reply))
    }
}

struct DispatchProbe {
    inner: GreeterServer<Echo>,
    direct: Arc<AtomicUsize>,
    boxed: Arc<AtomicUsize>,
}

impl Service for DispatchProbe {
    const NAME: &'static str = <GreeterServer<Echo> as Service>::NAME;

    async fn call(&self, rpc: Rpc) {
        self.direct.fetch_add(1, Ordering::Relaxed);
        self.inner.call(rpc).await;
    }

    fn call_boxed(&self, rpc: Rpc) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.boxed.fetch_add(1, Ordering::Relaxed);
        self.inner.call_boxed(rpc)
    }
}

async fn check_dispatch(router: bool) {
    let direct = Arc::new(AtomicUsize::new(0));
    let boxed = Arc::new(AtomicUsize::new(0));
    let service = DispatchProbe {
        inner: GreeterServer::new(Echo),
        direct: Arc::clone(&direct),
        boxed: Arc::clone(&boxed),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        if router {
            Router::new()
                .add_service(service)
                .serve_listener(listener)
                .await
        } else {
            Server::new(service).serve_listener(listener).await
        }
    });
    let channel = Channel::connect(address).await.unwrap();
    let mut request = HelloRequest::new();
    request.set_name("selected method");
    let mut request = Request::new(request);
    request.set_timeout(Duration::from_secs(2));
    let response = channel
        .unary::<_, HelloReply>("/helloworld.Greeter/SayHello", request)
        .await
        .unwrap();
    assert_eq!(
        response.get_ref().message().to_str().unwrap(),
        "selected method"
    );
    let mut request = Request::new(HelloRequest::new());
    request.set_timeout(Duration::from_secs(2));
    let status = channel
        .unary::<_, HelloReply>("/helloworld.Greeter/Unknown", request)
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);
    assert_eq!(direct.load(Ordering::Relaxed), if router { 0 } else { 2 });
    assert_eq!(boxed.load(Ordering::Relaxed), if router { 2 } else { 0 });
    server.abort();
}

#[tokio::test]
async fn router_uses_selected_boxed_dispatch_and_preserves_unknown_methods() {
    check_dispatch(true).await;
}

#[tokio::test]
async fn monomorphic_server_keeps_direct_dispatch() {
    check_dispatch(false).await;
}
