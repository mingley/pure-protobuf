//! Tower client adapter coverage.

#![cfg(feature = "tower")]
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests"
)]

mod common;

use common::{Echo, name_of, req, serve, until_ok};
use pbrs_grpc::hello::{Greeter, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Request, Response, ServerConfig, Status, Streaming};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tower::{Service, ServiceBuilder, ServiceExt};

#[tokio::test]
async fn tower_layers_wrap_unary_channel_without_buffering_default_path() {
    let (addr, _guard) = serve(Echo, ServerConfig::default()).await.expect("server");
    let channel = until_ok("connect", || async { Channel::connect(addr).await }).await;
    let service = channel.tower_unary::<HelloRequest, HelloReply>("/helloworld.Greeter/SayHello");
    let layered = ServiceBuilder::new()
        .timeout(Duration::from_secs(5))
        .concurrency_limit(1)
        .rate_limit(10, Duration::from_secs(1))
        .buffer(1)
        .load_shed()
        .service(service);

    let response = layered
        .oneshot(Request::new(req("tower")))
        .await
        .expect("tower call")
        .into_inner();
    assert_eq!(name_of(&response), "tower");
}

// Live request streams cannot be replayed. This policy explicitly declines
// cloning instead of silently retrying messages already consumed by a server.
#[derive(Clone)]
struct NeverReplay;

impl<Req, Resp, E> tower::retry::Policy<Req, Resp, E> for NeverReplay {
    type Future = std::future::Ready<()>;

    fn retry(&mut self, _: &mut Req, _: &mut Result<Resp, E>) -> Option<Self::Future> {
        None
    }

    fn clone_request(&mut self, _: &Req) -> Option<Req> {
        None
    }
}

fn layered<S, Req>(service: S) -> impl Service<Req, Response = S::Response, Error = tower::BoxError>
where
    S: Service<Req> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Into<tower::BoxError>,
    Req: Send + 'static,
{
    let endpoints = tower::discover::ServiceList::new(vec![
        tower::load::Constant::new(service.clone(), 0usize),
        tower::load::Constant::new(service, 0usize),
    ]);
    let balance = tower::balance::p2c::Balance::new(endpoints);
    tower::retry::Retry::new(NeverReplay, tower::buffer::Buffer::new(balance, 1))
}

fn tagged<T>(mut response: Response<T>) -> Response<T> {
    response
        .metadata_mut()
        .insert("x-header", "initial")
        .unwrap();
    response
        .trailers_mut()
        .insert("x-trailer", "terminal")
        .unwrap();
    response
}

#[derive(Clone, Default)]
struct TaggedEcho {
    cancelled: Arc<AtomicUsize>,
}

impl Greeter for TaggedEcho {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Echo.say_hello(request).await
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        assert_eq!(request.metadata().get("x-tower"), Some("preserved"));
        if request.metadata().get("x-early").is_some() {
            return Ok(tagged(Response::new(common::reply("early"))));
        }
        Echo.client_hello(request).await.map(tagged)
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        assert_eq!(request.metadata().get("x-tower"), Some("preserved"));
        if request.metadata().get("x-fail").is_some() {
            Ok(tagged(Response::new(failed_replies())))
        } else if request.get_ref().name().to_str().unwrap() == "stall" {
            let cancelled = request.cancelled();
            let count = self.cancelled.clone();
            let (tx, stream) = Streaming::channel(1);
            drop(tokio::spawn(async move {
                cancelled.await;
                count.fetch_add(1, Ordering::SeqCst);
                drop(tx);
            }));
            Ok(tagged(Response::new(stream)))
        } else {
            Echo.server_hello(request).await.map(tagged)
        }
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        assert_eq!(request.metadata().get("x-tower"), Some("preserved"));
        if request.metadata().get("x-fail").is_some() {
            return Ok(tagged(Response::new(failed_replies())));
        }
        Echo.stream_hello(request).await.map(tagged)
    }
}

fn failed_replies() -> Streaming<HelloReply> {
    let (tx, stream) = Streaming::channel(1);
    drop(tokio::spawn(async move {
        tx.send(common::reply("first")).await.unwrap();
        let mut status = Status::failed_precondition("terminal failure");
        status.metadata_mut().insert("x-error", "retained").unwrap();
        tx.fail(status).await;
    }));
    stream
}

fn request<T>(message: T) -> Request<T> {
    let mut request = Request::new(message);
    request
        .metadata_mut()
        .insert("x-tower", "preserved")
        .unwrap();
    request
}

async fn channel(service: TaggedEcho) -> (Channel, common::ServerGuard) {
    let (addr, guard) = serve(service, ServerConfig::default()).await.unwrap();
    (
        until_ok("connect", || async { Channel::connect(addr).await }).await,
        guard,
    )
}

#[tokio::test]
async fn streaming_shapes_through_balance_and_retry_preserve_envelopes() {
    let (channel, _guard) = channel(TaggedEcho::default()).await;

    let response = layered(
        channel
            .tower_server_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ServerHello"),
    )
    .oneshot(request(req("ada,grace")))
    .await
    .unwrap();
    assert_eq!(response.metadata().get("x-header"), Some("initial"));
    let mut replies = response.into_inner();
    assert_eq!(name_of(&replies.message().await.unwrap().unwrap()), "ada");
    assert_eq!(name_of(&replies.message().await.unwrap().unwrap()), "grace");
    assert!(replies.message().await.unwrap().is_none());
    assert_eq!(
        replies.trailers().await.unwrap().get("x-trailer"),
        Some("terminal")
    );

    let (tx, input) = Streaming::channel(1);
    let send = async move {
        tx.send(req("ada")).await.unwrap();
        tx.send(req("grace")).await.unwrap();
        tx.close();
    };
    let call = layered(
        channel
            .tower_client_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ClientHello"),
    )
    .oneshot(request(input));
    let ((), response) = tokio::join!(send, call);
    let response = response.unwrap();
    assert_eq!(response.metadata().get("x-header"), Some("initial"));
    assert_eq!(response.trailers().get("x-trailer"), Some("terminal"));
    assert_eq!(name_of(response.get_ref()), "ada,grace");

    let (tx, input) = Streaming::channel(1);
    let response =
        layered(channel.tower_bidi::<HelloRequest, HelloReply>("/helloworld.Greeter/StreamHello"))
            .oneshot(request(input))
            .await
            .unwrap();
    assert_eq!(response.metadata().get("x-header"), Some("initial"));
    let mut replies = response.into_inner();
    // Request production must keep running after the Tower future resolves.
    for name in ["ada", "grace"] {
        tx.send(req(name)).await.unwrap();
        assert_eq!(name_of(&replies.message().await.unwrap().unwrap()), name);
    }
    tx.close();
    assert!(replies.message().await.unwrap().is_none());
    assert_eq!(
        replies.trailers().await.unwrap().get("x-trailer"),
        Some("terminal")
    );
}

#[tokio::test]
async fn streaming_adapters_keep_deadlines_after_headers_and_with_idle_producers() {
    let (channel, _guard) = channel(TaggedEcho::default()).await;
    let mut request = request(req("stall"));
    request.set_timeout(Duration::from_millis(100));
    let mut replies = layered(
        channel
            .tower_server_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ServerHello"),
    )
    .oneshot(request)
    .await
    .unwrap()
    .into_inner();
    assert_eq!(
        replies.message().await.unwrap_err().code(),
        Code::DeadlineExceeded
    );

    let (tx, input) = Streaming::channel(1);
    let mut request = self::request(input);
    request.set_timeout(Duration::from_millis(100));
    let error = layered(
        channel
            .tower_client_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ClientHello"),
    )
    .oneshot(request)
    .await
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<Status>().unwrap().code(),
        Code::DeadlineExceeded
    );
    tokio::time::timeout(Duration::from_secs(2), tx.closed())
        .await
        .unwrap();

    let (tx, input) = Streaming::channel(1);
    let mut request = self::request(input);
    request.set_timeout(Duration::from_millis(100));
    let mut replies =
        layered(channel.tower_bidi::<HelloRequest, HelloReply>("/helloworld.Greeter/StreamHello"))
            .oneshot(request)
            .await
            .unwrap()
            .into_inner();
    assert_eq!(
        replies.message().await.unwrap_err().code(),
        Code::DeadlineExceeded
    );
    tokio::time::timeout(Duration::from_secs(2), tx.closed())
        .await
        .unwrap();
}

#[tokio::test]
async fn dropping_calls_and_received_streams_stops_idle_request_producers() {
    let echo = TaggedEcho::default();
    let cancelled = echo.cancelled.clone();
    let (channel, _guard) = channel(echo).await;
    let response = layered(
        channel
            .tower_server_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ServerHello"),
    )
    .oneshot(request(req("stall")))
    .await
    .unwrap();
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), async {
        while cancelled.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let (tx, input) = Streaming::channel(1);
    let mut service = channel
        .tower_client_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ClientHello");
    let call = service.call(request(input));
    let handle = call.handle();
    let running = tokio::spawn(call);
    tx.send(req("first")).await.unwrap();
    handle.cancel();
    assert_eq!(running.await.unwrap().unwrap_err().code(), Code::Cancelled);
    tokio::time::timeout(Duration::from_secs(2), tx.closed())
        .await
        .unwrap();

    let (tx, input) = Streaming::channel(1);
    let response =
        layered(channel.tower_bidi::<HelloRequest, HelloReply>("/helloworld.Greeter/StreamHello"))
            .oneshot(request(input))
            .await
            .unwrap();
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), tx.closed())
        .await
        .unwrap();

    // Even a call dropped without ever being polled must release its input.
    let (tx, input) = Streaming::channel(1);
    drop(service.call(request(input)));
    tokio::time::timeout(Duration::from_secs(2), tx.closed())
        .await
        .unwrap();
}

#[tokio::test]
async fn request_stream_errors_reach_the_native_call_without_replay() {
    let (channel, _guard) = channel(TaggedEcho::default()).await;
    let (tx, input) = Streaming::channel(1);
    let send = tx.fail(Status::invalid_argument("producer failed"));
    let call = layered(
        channel
            .tower_client_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ClientHello"),
    )
    .oneshot(request(input));
    let ((), result) = tokio::join!(send, call);
    let error = result.unwrap_err();
    let status = error.downcast_ref::<Status>().unwrap();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(status.message(), "producer failed");
}

#[tokio::test]
async fn streaming_trailing_errors_survive_balance_and_retry() {
    let (channel, _guard) = channel(TaggedEcho::default()).await;
    let mut request = request(req("ignored"));
    request.metadata_mut().insert("x-fail", "true").unwrap();
    let server_stream = layered(
        channel
            .tower_server_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ServerHello"),
    )
    .oneshot(request)
    .await
    .unwrap()
    .into_inner();
    let (tx, input) = Streaming::channel(1);
    let mut request = self::request(input);
    request.metadata_mut().insert("x-fail", "true").unwrap();
    let bidi_stream =
        layered(channel.tower_bidi::<HelloRequest, HelloReply>("/helloworld.Greeter/StreamHello"))
            .oneshot(request)
            .await
            .unwrap()
            .into_inner();

    for mut stream in [server_stream, bidi_stream] {
        assert_eq!(name_of(&stream.message().await.unwrap().unwrap()), "first");
        let error = stream.message().await.unwrap_err();
        assert_eq!(error.code(), Code::FailedPrecondition);
        assert_eq!(error.message(), "terminal failure");
        assert_eq!(error.metadata().get("x-error"), Some("retained"));
    }
    tokio::time::timeout(Duration::from_secs(2), tx.closed())
        .await
        .unwrap();
}

#[tokio::test]
async fn arbitrary_input_streams_work_and_early_replies_release_idle_inputs() {
    let (channel, _guard) = channel(TaggedEcho::default()).await;
    let response = layered(
        channel
            .tower_client_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ClientHello"),
    )
    .oneshot(request(tokio_stream::iter([
        Ok(req("ada")),
        Ok(req("grace")),
    ])))
    .await
    .unwrap();
    assert_eq!(name_of(response.get_ref()), "ada,grace");

    let (tx, input) = Streaming::<HelloRequest>::channel(1);
    let mut request = request(input);
    request.metadata_mut().insert("x-early", "true").unwrap();
    let response = layered(
        channel
            .tower_client_streaming::<HelloRequest, HelloReply>("/helloworld.Greeter/ClientHello"),
    )
    .oneshot(request)
    .await
    .unwrap();
    assert_eq!(name_of(response.get_ref()), "early");
    tokio::time::timeout(Duration::from_secs(2), tx.closed())
        .await
        .unwrap();
}
