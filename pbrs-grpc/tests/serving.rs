//! Serving surface exercised the way a third party would: a hand-written
//! [`Service`], a [`Router`] hosting two services, and graceful drain.
//!
//! This file deliberately avoids the generated `GreeterServer` wrapper for the
//! hand-written cases, because the point is that the public API is enough.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests"
)]

mod common;

use common::{greeter_client, name_of, req, Echo};
use pbrs_grpc::hello::{GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{
    Channel, Code, Empty, InteropTestService, Request, Response, Router, Rpc, Server, ServerConfig,
    Service, Status, TestServiceClient, TestServiceServer,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

/// A service written without any generated code, mounted on the public API.
struct Reverser {
    seen: Arc<AtomicUsize>,
}

impl Service for Reverser {
    const NAME: &'static str = "demo.Reverser";

    async fn call(&self, rpc: Rpc) {
        let seen = Arc::clone(&self.seen);
        match rpc.method() {
            "Reverse" => {
                let peer = rpc.remote_addr();
                rpc.unary(move |request: Request<HelloRequest>| async move {
                    seen.fetch_add(1, Ordering::Relaxed);
                    if peer.is_none() {
                        return Err(Status::internal("expected a peer address"));
                    }
                    let name: String = request
                        .get_ref()
                        .name()
                        .to_str()
                        .unwrap_or_default()
                        .chars()
                        .rev()
                        .collect();
                    let mut reply = HelloReply::new();
                    reply.set_message(name);
                    Ok(Response::new(reply))
                })
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

async fn bind() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    (addr, listener)
}

async fn channel(addr: SocketAddr) -> Channel {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(channel) => return channel,
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect: {last:?}");
}

#[tokio::test]
async fn a_hand_written_service_serves_without_generated_code() {
    let (addr, listener) = bind().await;
    let seen = Arc::new(AtomicUsize::new(0));
    let service = Reverser {
        seen: Arc::clone(&seen),
    };
    let task = tokio::spawn(async move {
        Server::new(service).serve_listener(listener).await.ok();
    });

    let channel = channel(addr).await;
    let reply: HelloReply = channel
        .unary("/demo.Reverser/Reverse", Request::new(req("stressed")))
        .await
        .expect("unary")
        .into_inner();
    assert_eq!(name_of(&reply), "desserts");
    assert_eq!(seen.load(Ordering::Relaxed), 1);

    let missing = channel
        .unary::<HelloRequest, HelloReply>("/demo.Reverser/Nope", Request::new(req("x")))
        .await
        .expect_err("unknown method");
    assert_eq!(missing.code(), Code::Unimplemented);

    task.abort();
}

#[tokio::test]
async fn a_router_dispatches_between_two_services() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Echo))
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = channel(addr).await;

    let greeting = GreeterClient::new(channel.clone())
        .say_hello(Request::new(req("ada")))
        .await
        .expect("greeter");
    assert_eq!(name_of(greeting.get_ref()), "ada");

    TestServiceClient::new(channel.clone())
        .empty_call(Request::new(Empty::new()))
        .await
        .expect("test service");

    let missing = channel
        .unary::<HelloRequest, HelloReply>("/nope.Absent/Method", Request::new(req("x")))
        .await
        .expect_err("unmounted service");
    assert_eq!(missing.code(), Code::Unimplemented);

    task.abort();
}

#[tokio::test]
async fn router_reports_its_mounted_services() {
    let router = Router::new()
        .add_service(GreeterServer::new(Echo))
        .add_service(TestServiceServer::new(InteropTestService));
    let mut names: Vec<&str> = router.service_names().collect();
    names.sort_unstable();
    assert_eq!(names, ["grpc.testing.TestService", "helloworld.Greeter"]);
}

#[tokio::test]
async fn mounting_the_same_service_twice_keeps_the_last() {
    let router = Router::new()
        .add_service(GreeterServer::new(Echo))
        .add_service(GreeterServer::new(Echo));
    assert_eq!(router.service_names().count(), 1);
}

/// A handler slow enough that the drain has to wait for it.
struct Slow;

impl pbrs_grpc::Greeter for Slow {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut reply = HelloReply::new();
        reply.set_message(request.get_ref().name());
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("slow"))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("slow"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("slow"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_finishes_in_flight_rpcs() {
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Slow)
            .serve_with_shutdown(listener, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let call = client.say_hello(Request::new(req("ada")));

    // Signal shutdown while the 200 ms handler is still running.
    tokio::time::sleep(Duration::from_millis(30)).await;
    shutdown_tx.send(()).expect("signal");

    let reply = call.await.expect("in-flight RPC must complete");
    assert_eq!(name_of(reply.get_ref()), "ada");

    // Drain finishes on its own once the connection closes.
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_stops_accepting_new_connections() {
    let (addr, listener) = bind().await;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let served = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .serve_with_shutdown(listener, async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });

    // Prove the server is up, then shut it down and let the drain complete.
    let client = GreeterClient::new(channel(addr).await);
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("before shutdown");
    drop(client);
    shutdown_tx.send(()).expect("signal");
    tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("drain must finish")
        .expect("join");

    // A fresh connection now finds nothing listening, or finds a socket that
    // refuses to carry an RPC.
    let refused = match Channel::connect(addr).await {
        Err(_) => true,
        Ok(channel) => GreeterClient::new(channel)
            .say_hello(Request::new(req("late")))
            .await
            .is_err(),
    };
    assert!(refused, "the listener must be closed after drain");
}

/// Answers without ever reading the request stream. Inbound messages are
/// decoded on the handler's task, so a handler that ignores them must still
/// terminate the RPC rather than leaving the client blocked on the window.
struct Deaf;

impl pbrs_grpc::Greeter for Deaf {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("deaf"))
    }

    async fn client_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut reply = HelloReply::new();
        reply.set_message("ignored your stream");
        Ok(Response::new(reply))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("deaf"))
    }

    async fn stream_hello(
        &self,
        _request: Request<pbrs_grpc::Streaming<HelloRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("deaf"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handler_that_ignores_its_request_stream_still_answers() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Deaf).serve_listener(listener).await.ok();
    });

    let client = GreeterClient::new(channel(addr).await);
    let (tx, call) = client.client_hello(Request::new(()));

    // Push more than fits in any buffer, from another task, so a hang here
    // would show up as the test timing out rather than as a deadlock.
    let sender = tokio::spawn(async move {
        for i in 0..512 {
            if tx.send(req(&format!("n{i}"))).await.is_err() {
                break;
            }
        }
    });

    let reply = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .expect("must not hang")
        .expect("must answer");
    assert_eq!(name_of(reply.get_ref()), "ignored your stream");
    sender.abort();
    task.abort();
}

#[tokio::test]
async fn config_flows_from_the_generated_server_to_the_router() {
    let (addr, listener) = bind().await;
    let task = tokio::spawn(async move {
        GreeterServer::new(Echo)
            .config(ServerConfig::new().max_decoding_message_size(16))
            .add_service(TestServiceServer::new(InteropTestService))
            .serve_listener(listener)
            .await
            .ok();
    });

    let client = greeter_client(addr).await;
    let err = client
        .say_hello(Request::new(req(&"x".repeat(64))))
        .await
        .expect_err("over the server cap");
    assert_eq!(err.code(), Code::ResourceExhausted);

    task.abort();
}
