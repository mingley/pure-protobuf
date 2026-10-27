//! Custom connection-driver scheduling, independent of the private runtime seam.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration assertions and intentionally rejecting executor fixtures"
)]

use pbrs_grpc::{
    Channel, ChannelConfig, Code, Endpoint, Executor, ExecutorFuture, Greeter, GreeterClient,
    GreeterServer, HelloReply, HelloRequest, Incoming, IncomingAccept, Request, Response, Status,
    Streaming,
};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

const CLIENT_TAG: usize = 41;
const SERVER_TAG: usize = 99;
const BOUND: Duration = Duration::from_secs(5);

tokio::task_local! {
    static EXECUTOR_TAG: usize;
}

#[derive(Default)]
struct Counts {
    scheduled: AtomicUsize,
    finished: AtomicUsize,
}

struct Finished(Arc<Counts>);
impl Drop for Finished {
    fn drop(&mut self) {
        self.0.finished.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct Recording {
    tag: usize,
    counts: Arc<Counts>,
}
impl Recording {
    fn new(tag: usize) -> Self {
        Self {
            tag,
            counts: Arc::new(Counts::default()),
        }
    }
}
impl Executor for Recording {
    fn execute(&self, future: ExecutorFuture) {
        self.counts.scheduled.fetch_add(1, Ordering::SeqCst);
        let finished = Finished(Arc::clone(&self.counts));
        let tag = self.tag;
        drop(tokio::spawn(async move {
            let _finished = finished;
            EXECUTOR_TAG.scope(tag, future).await;
        }));
    }
}

#[derive(Clone, Copy)]
enum Place {
    Tag(usize),
    Thread(&'static str),
}

struct Echo {
    expected: Place,
    calls: Arc<AtomicUsize>,
}
impl Echo {
    fn checked(&self) {
        match self.expected {
            Place::Tag(tag) => assert_eq!(EXECUTOR_TAG.try_with(|active| *active).ok(), Some(tag)),
            Place::Thread(name) => assert_eq!(std::thread::current().name(), Some(name)),
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

fn request(name: &str) -> HelloRequest {
    let mut message = HelloRequest::new();
    message.set_name(name);
    message
}
fn reply(name: impl Into<String>) -> HelloReply {
    let mut message = HelloReply::new();
    message.set_message(name.into());
    message
}
fn text(message: &HelloReply) -> &str {
    message.message().to_str().unwrap()
}

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.checked();
        Ok(Response::new(reply(
            request.get_ref().name().to_str().unwrap(),
        )))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        self.checked();
        let mut input = request.into_inner();
        let mut names = Vec::new();
        while let Some(message) = input.message().await? {
            names.push(message.name().to_str().unwrap().to_owned());
        }
        Ok(Response::new(reply(names.join(","))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        self.checked();
        let (sender, stream) = Streaming::channel(2);
        for name in request.get_ref().name().to_str().unwrap().split(',') {
            sender.send(reply(name)).await?;
        }
        sender.close();
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        self.checked();
        let mut input = request.into_inner();
        let (sender, stream) = Streaming::channel(2);
        while let Some(message) = input.message().await? {
            sender.send(reply(message.name().to_str().unwrap())).await?;
        }
        sender.close();
        Ok(Response::new(stream))
    }
}

async fn all_shapes(channel: Channel) {
    let client = GreeterClient::new(channel);
    let response = client
        .say_hello(Request::new(request("unary")))
        .await
        .unwrap();
    assert_eq!(text(response.get_ref()), "unary");
    let mut stream = client
        .server_hello(Request::new(request("a,b")))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(text(&stream.message().await.unwrap().unwrap()), "a");
    assert_eq!(text(&stream.message().await.unwrap().unwrap()), "b");
    assert!(stream.message().await.unwrap().is_none());
    let (sender, call) = client.client_hello(Request::new(()));
    sender.send(request("c")).await.unwrap();
    sender.send(request("d")).await.unwrap();
    sender.close();
    assert_eq!(text(call.await.unwrap().get_ref()), "c,d");
    let (sender, call) = client.stream_hello(Request::new(()));
    sender.send(request("e")).await.unwrap();
    sender.send(request("f")).await.unwrap();
    sender.close();
    let mut stream = call.await.unwrap().into_inner();
    assert_eq!(text(&stream.message().await.unwrap().unwrap()), "e");
    assert_eq!(text(&stream.message().await.unwrap().unwrap()), "f");
    assert!(stream.message().await.unwrap().is_none());
}

async fn wait_count(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(BOUND, async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn direct_and_redial(lazy: bool) {
    let server_executor = Recording::new(SERVER_TAG);
    let client_executor = Recording::new(CLIENT_TAG);
    let calls = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = GreeterServer::new(Echo {
        expected: Place::Tag(SERVER_TAG),
        calls: Arc::clone(&calls),
    })
    .executor(server_executor.clone());
    let (stop, stopped) = oneshot::channel();
    // Cloning and conversion must preserve the configured executor.
    let router = server.clone().into_router();
    let serve = tokio::spawn(router.serve_with_shutdown(listener, async {
        stopped.await.ok();
    }));
    let endpoint = Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .with_config(ChannelConfig::new().max_connection_idle(Duration::from_millis(10)))
        .executor(client_executor.clone());
    let channel = if lazy {
        endpoint.connect_lazy().unwrap()
    } else {
        endpoint.connect().await.unwrap()
    };
    tokio::time::timeout(BOUND, all_shapes(channel.clone()))
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    wait_count(&client_executor.counts.finished, 1).await;
    let client = GreeterClient::new(channel.clone());
    assert_eq!(
        text(
            client
                .say_hello(Request::new(request("again")))
                .await
                .unwrap()
                .get_ref()
        ),
        "again"
    );
    assert!(client_executor.counts.scheduled.load(Ordering::SeqCst) >= 2);
    assert!(server_executor.counts.scheduled.load(Ordering::SeqCst) >= 2);
    drop(client);
    drop(channel);
    stop.send(()).unwrap();
    tokio::time::timeout(BOUND, serve)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let scheduled = client_executor.counts.scheduled.load(Ordering::SeqCst);
    wait_count(&client_executor.counts.finished, scheduled).await;
    let scheduled = server_executor.counts.scheduled.load(Ordering::SeqCst);
    wait_count(&server_executor.counts.finished, scheduled).await;
}

#[tokio::test]
async fn eager_executor_polls_all_shapes_and_survives_idle_redial() {
    direct_and_redial(false).await;
}

#[tokio::test]
async fn lazy_executor_polls_all_shapes_and_survives_idle_redial() {
    direct_and_redial(true).await;
}

struct One(Option<tokio::io::DuplexStream>);
impl Incoming for One {
    type Io = tokio::io::DuplexStream;
    fn accept(&mut self) -> impl Future<Output = IncomingAccept<Self::Io>> + Send {
        let io = self.0.take();
        async move {
            match io {
                Some(io) => Some(Ok((io, None))),
                None => std::future::pending().await,
            }
        }
    }
}

async fn connected_io(use_endpoint: bool) {
    let server_executor = Recording::new(SERVER_TAG);
    let client_executor = Recording::new(CLIENT_TAG);
    let calls = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = tokio::io::duplex(8192);
    let server = GreeterServer::new(Echo {
        expected: Place::Tag(SERVER_TAG),
        calls: Arc::clone(&calls),
    })
    .executor(server_executor.clone());
    let (stop, stopped) = oneshot::channel();
    let serve = tokio::spawn(
        server.serve_with_incoming_shutdown(One(Some(server_io)), async {
            stopped.await.ok();
        }),
    );
    let channel = if use_endpoint {
        Endpoint::from_static("http://connector.invalid")
            .unwrap()
            .executor(client_executor.clone())
            .connect_with_connector(|uri| async move {
                assert_eq!(uri.host(), Some("connector.invalid"));
                Ok(client_io)
            })
            .await
            .unwrap()
    } else {
        Channel::from_io_with_executor(
            client_io,
            "connector.invalid",
            ChannelConfig::new(),
            client_executor.clone(),
        )
        .await
        .unwrap()
    };
    tokio::time::timeout(BOUND, all_shapes(channel.clone()))
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(client_executor.counts.scheduled.load(Ordering::SeqCst), 1);
    assert_eq!(server_executor.counts.scheduled.load(Ordering::SeqCst), 1);
    drop(channel);
    stop.send(()).unwrap();
    tokio::time::timeout(BOUND, serve)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    wait_count(&client_executor.counts.finished, 1).await;
    wait_count(&server_executor.counts.finished, 1).await;
}

#[tokio::test]
async fn custom_connector_preserves_executor_and_drains() {
    connected_io(true).await;
}

#[tokio::test]
async fn connected_io_uses_executor_and_drains() {
    connected_io(false).await;
}

struct Discard;
impl Executor for Discard {
    fn execute(&self, future: ExecutorFuture) {
        drop(future);
    }
}

#[tokio::test]
async fn dropping_scheduled_driver_closes_connection_and_releases_server_drain() {
    let server_executor = Recording::new(SERVER_TAG);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = GreeterServer::new(Echo {
        expected: Place::Tag(SERVER_TAG),
        calls: Arc::new(AtomicUsize::new(0)),
    })
    .executor(server_executor.clone());
    let (stop, stopped) = oneshot::channel();
    let serve = tokio::spawn(server.serve_with_shutdown(listener, async {
        stopped.await.ok();
    }));
    let channel = Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .executor(Discard)
        .connect()
        .await
        .unwrap();
    let client = GreeterClient::new(channel);
    let error = tokio::time::timeout(BOUND, client.say_hello(Request::new(request("rejected"))))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code(), Code::Unavailable);
    drop(client);
    stop.send(()).unwrap();
    tokio::time::timeout(BOUND, serve)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    wait_count(&server_executor.counts.finished, 1).await;
}

#[tokio::test]
async fn balance_helpers_reject_executor_override_instead_of_ignoring_it() {
    let endpoint = Endpoint::from_static("http://127.0.0.1:1")
        .unwrap()
        .executor(Discard);
    let error = Endpoint::balance_list([endpoint], ChannelConfig::new())
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(error.message().contains("per-endpoint executors"));
}

struct PanicExecutor;
impl Executor for PanicExecutor {
    fn execute(&self, _future: ExecutorFuture) {
        panic!("executor rejects scheduling");
    }
}

#[tokio::test]
async fn executor_panic_unwinds_driver_and_releases_server_drain() {
    let server_executor = Recording::new(SERVER_TAG);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = GreeterServer::new(Echo {
        expected: Place::Tag(SERVER_TAG),
        calls: Arc::new(AtomicUsize::new(0)),
    })
    .executor(server_executor.clone());
    let (stop, stopped) = oneshot::channel();
    let serve = tokio::spawn(server.serve_with_shutdown(listener, async {
        stopped.await.ok();
    }));
    let endpoint = Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .executor(PanicExecutor);
    let attempted = tokio::spawn(async move { endpoint.connect().await });
    assert!(
        tokio::time::timeout(BOUND, attempted)
            .await
            .unwrap()
            .unwrap_err()
            .is_panic()
    );
    stop.send(()).unwrap();
    tokio::time::timeout(BOUND, serve)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    wait_count(&server_executor.counts.finished, 1).await;
}

#[tokio::test]
async fn tokio_handle_moves_transport_polling_to_a_dedicated_runtime() {
    let (handle_sender, handle_receiver) = oneshot::channel();
    let (stop_runtime, stopped_runtime) = oneshot::channel();
    let thread = std::thread::Builder::new()
        .name("executor-transport".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            handle_sender.send(runtime.handle().clone()).unwrap();
            runtime.block_on(async {
                stopped_runtime.await.ok();
            });
        })
        .unwrap();
    let executor = handle_receiver.await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = GreeterServer::new(Echo {
        expected: Place::Thread("executor-transport"),
        calls: Arc::clone(&calls),
    })
    .executor(executor.clone());
    let (stop, stopped) = oneshot::channel();
    let serve = tokio::spawn(server.serve_with_shutdown(listener, async {
        stopped.await.ok();
    }));
    let channel = Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .executor(executor.clone())
        .connect()
        .await
        .unwrap();
    tokio::time::timeout(BOUND, all_shapes(channel.clone()))
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    drop(channel);
    stop.send(()).unwrap();
    tokio::time::timeout(BOUND, serve)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    drop(executor);
    stop_runtime.send(()).unwrap();
    tokio::task::spawn_blocking(move || thread.join().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn driver_keeps_executor_alive_until_last_response_stream_drops() {
    let server_executor = Recording::new(SERVER_TAG);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = GreeterServer::new(Echo {
        expected: Place::Tag(SERVER_TAG),
        calls: Arc::new(AtomicUsize::new(0)),
    })
    .executor(server_executor.clone());
    let (stop, stopped) = oneshot::channel();
    let serve = tokio::spawn(server.serve_with_shutdown(listener, async {
        stopped.await.ok();
    }));
    let executor = Arc::new(Recording::new(CLIENT_TAG));
    let weak = Arc::downgrade(&executor);
    let counts = Arc::clone(&executor.counts);
    let channel = Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .executor(Arc::clone(&executor))
        .connect()
        .await
        .unwrap();
    drop(executor);
    let client = GreeterClient::new(channel);
    let response = client
        .server_hello(Request::new(request("a,b")))
        .await
        .unwrap()
        .into_inner();
    drop(client);
    assert!(
        weak.upgrade().is_some(),
        "live response retains its driver scheduler"
    );
    drop(response);
    wait_count(&counts.finished, 1).await;
    tokio::time::timeout(BOUND, async {
        while weak.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(BOUND, serve)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
