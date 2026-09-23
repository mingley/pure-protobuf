//! Integration tests for low-cardinality lifecycle telemetry hooks.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
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

use common::{Echo, name_of, reply, req};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::telemetry::{
    AttemptLabels, CallLabels, CallRole, CancellationEvent, CancellationReason, DiagnosticConfig,
    LifecycleObserver, MetadataMap, ReconnectEvent, RejectionEvent, RejectionReason,
    TelemetryContext,
};
use pbrs_grpc::{Channel, Code, Metadata, Request, Response, Router, Server, Status};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Notify;

/// Concrete recording event for asserting exact event counts and ordering.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RecordedEvent {
    CallStart {
        service: String,
        method: String,
        role: CallRole,
    },
    CallEnd {
        service: String,
        method: String,
        code: Code,
        role: CallRole,
    },
    AttemptStart {
        attempt: u32,
    },
    AttemptEnd {
        attempt: u32,
        code: Code,
    },
    ServerCallStart {
        service: String,
        method: String,
    },
    ServerCallEnd {
        service: String,
        method: String,
        code: Code,
    },
    QueueWait {
        role: CallRole,
    },
    BytesSent {
        role: CallRole,
        bytes: usize,
    },
    BytesReceived {
        role: CallRole,
        bytes: usize,
    },
    Reconnect {
        target: String,
        attempt: u32,
        status: Option<Code>,
    },
    Rejection {
        reason: RejectionReason,
        code: Code,
    },
    Cancellation {
        reason: CancellationReason,
    },
}

#[derive(Default, Clone)]
struct RecordingObserver {
    events: Arc<Mutex<Vec<RecordedEvent>>>,
}

impl RecordingObserver {
    fn new() -> Self {
        Self::default()
    }

    fn events(&self) -> Vec<RecordedEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl LifecycleObserver for RecordingObserver {
    fn on_call_start(&self, call: &CallLabels<'_>) {
        self.events.lock().unwrap().push(RecordedEvent::CallStart {
            service: call.service().to_string(),
            method: call.method().to_string(),
            role: call.role(),
        });
    }

    fn on_call_end(&self, call: &CallLabels<'_>, status: &Status, _latency: Duration) {
        self.events.lock().unwrap().push(RecordedEvent::CallEnd {
            service: call.service().to_string(),
            method: call.method().to_string(),
            code: status.code(),
            role: call.role(),
        });
    }

    fn on_attempt_start(&self, attempt: &AttemptLabels<'_>) {
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::AttemptStart {
                attempt: attempt.attempt(),
            });
    }

    fn on_attempt_end(&self, attempt: &AttemptLabels<'_>, status: &Status, _latency: Duration) {
        self.events.lock().unwrap().push(RecordedEvent::AttemptEnd {
            attempt: attempt.attempt(),
            code: status.code(),
        });
    }

    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::ServerCallStart {
                service: call.service().to_string(),
                method: call.method().to_string(),
            });
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, _latency: Duration) {
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::ServerCallEnd {
                service: call.service().to_string(),
                method: call.method().to_string(),
                code: status.code(),
            });
    }

    fn on_queue_wait(&self, call: &CallLabels<'_>, _wait: Duration) {
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::QueueWait { role: call.role() });
    }

    fn on_server_queue_wait(&self, call: &CallLabels<'_>, _wait: Duration) {
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::QueueWait { role: call.role() });
    }

    fn on_bytes_sent(&self, call: &CallLabels<'_>, bytes: usize) {
        self.events.lock().unwrap().push(RecordedEvent::BytesSent {
            role: call.role(),
            bytes,
        });
    }

    fn on_bytes_received(&self, call: &CallLabels<'_>, bytes: usize) {
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::BytesReceived {
                role: call.role(),
                bytes,
            });
    }

    fn on_reconnect(&self, event: &ReconnectEvent<'_>) {
        self.events.lock().unwrap().push(RecordedEvent::Reconnect {
            target: event.target.to_string(),
            attempt: event.attempt,
            status: event.status,
        });
    }

    fn on_rejection(&self, event: &RejectionEvent<'_>) {
        self.events.lock().unwrap().push(RecordedEvent::Rejection {
            reason: event.reason,
            code: event.code,
        });
    }

    fn on_cancellation(&self, event: &CancellationEvent<'_>) {
        self.events
            .lock()
            .unwrap()
            .push(RecordedEvent::Cancellation {
                reason: event.reason,
            });
    }
}

struct EchoService;

impl Greeter for EchoService {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request.get_ref().name().to_string();
        Ok(Response::new(reply(&name)))
    }
}

#[tokio::test]
async fn test_unary_success_exact_events_and_counts() {
    let client_observer = RecordingObserver::new();
    let server_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let srv_obs = server_observer.clone();
    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(EchoService))
            .observer(srv_obs)
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(client_observer.clone());
    let client = GreeterClient::new(channel);

    let reply = client
        .say_hello(Request::new(req("telemetry")))
        .await
        .expect("say_hello");
    assert_eq!(name_of(reply.get_ref()), "telemetry");

    let client_events = client_observer.events();
    let server_events = server_observer.events();

    // Verify exact client event counts
    let call_starts = client_events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallStart { .. }))
        .count();
    let call_ends = client_events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallEnd { .. }))
        .count();
    let attempt_starts = client_events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptStart { .. }))
        .count();
    let attempt_ends = client_events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptEnd { .. }))
        .count();

    assert_eq!(call_starts, 1, "Expected exactly 1 call_start on client");
    assert_eq!(call_ends, 1, "Expected exactly 1 call_end on client");
    assert_eq!(
        attempt_starts, 1,
        "Expected exactly 1 attempt_start on client"
    );
    assert_eq!(attempt_ends, 1, "Expected exactly 1 attempt_end on client");

    // Verify client event order
    assert_eq!(
        client_events[0],
        RecordedEvent::CallStart {
            service: "helloworld.Greeter".to_string(),
            method: "SayHello".to_string(),
            role: CallRole::Client,
        }
    );
    assert_eq!(client_events[1], RecordedEvent::AttemptStart { attempt: 1 });
    assert!(matches!(
        client_events[2],
        RecordedEvent::QueueWait {
            role: CallRole::Client
        }
    ));
    assert!(
        matches!(client_events[3], RecordedEvent::BytesSent { role: CallRole::Client, bytes } if bytes > 0)
    );
    assert_eq!(
        client_events[client_events.len() - 2],
        RecordedEvent::AttemptEnd {
            attempt: 1,
            code: Code::Ok
        }
    );
    assert_eq!(
        client_events[client_events.len() - 1],
        RecordedEvent::CallEnd {
            service: "helloworld.Greeter".to_string(),
            method: "SayHello".to_string(),
            code: Code::Ok,
            role: CallRole::Client,
        }
    );

    // Verify server events
    let srv_starts = server_events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::ServerCallStart { .. }))
        .count();
    let srv_ends = server_events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::ServerCallEnd { .. }))
        .count();
    assert_eq!(srv_starts, 1, "Expected exactly 1 server_call_start");
    assert_eq!(srv_ends, 1, "Expected exactly 1 server_call_end");

    assert_eq!(
        server_events[0],
        RecordedEvent::ServerCallStart {
            service: "helloworld.Greeter".to_string(),
            method: "SayHello".to_string(),
        }
    );
    assert_eq!(
        server_events[server_events.len() - 1],
        RecordedEvent::ServerCallEnd {
            service: "helloworld.Greeter".to_string(),
            method: "SayHello".to_string(),
            code: Code::Ok,
        }
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_failed_setup_client_interceptor_exact_events() {
    let client_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(EchoService))
            .serve_listener(listener)
            .await
            .ok();
    });

    fn reject_token(_call: &mut pbrs_grpc::Outgoing<'_>) -> Result<(), Status> {
        Err(Status::unauthenticated("missing token"))
    }

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(client_observer.clone())
        .intercept(reject_token);
    let client = GreeterClient::new(channel);

    let err = client
        .say_hello(Request::new(req("test")))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::Unauthenticated);

    let events = client_observer.events();

    // Verify exact event counts on failed setup
    let call_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallStart { .. }))
        .count();
    let rejections = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::Rejection { .. }))
        .count();
    let call_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallEnd { .. }))
        .count();
    let attempt_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptStart { .. }))
        .count();
    let attempt_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptEnd { .. }))
        .count();

    assert_eq!(call_starts, 1);
    assert_eq!(rejections, 1);
    assert_eq!(call_ends, 1);
    assert_eq!(attempt_starts, 0, "No attempt should start on failed setup");
    assert_eq!(attempt_ends, 0, "No attempt should end on failed setup");

    assert_eq!(
        events[0],
        RecordedEvent::CallStart {
            service: "helloworld.Greeter".to_string(),
            method: "SayHello".to_string(),
            role: CallRole::Client,
        }
    );
    assert_eq!(
        events[1],
        RecordedEvent::Rejection {
            reason: RejectionReason::ClientInterceptor,
            code: Code::Unauthenticated,
        }
    );
    assert_eq!(
        events[2],
        RecordedEvent::CallEnd {
            service: "helloworld.Greeter".to_string(),
            method: "SayHello".to_string(),
            code: Code::Unauthenticated,
            role: CallRole::Client,
        }
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_transparent_retry_exact_event_counts_and_order() {
    let client_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let executions = Arc::new(AtomicUsize::new(0));
    let executions_clone = Arc::clone(&executions);

    struct CountingService(Arc<AtomicUsize>);
    impl Greeter for CountingService {
        async fn say_hello(
            &self,
            req: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let name = req.get_ref().name().to_string();
            Ok(Response::new(reply(&name)))
        }
    }

    let server_task = tokio::spawn(async move {
        // Connection 1: Sends REFUSED_STREAM before processing
        let (socket, _) = listener.accept().await.expect("accept conn 1");
        let mut conn = h2::server::handshake(socket).await.expect("handshake 1");
        if let Some(Ok((_req, respond))) = conn.accept().await {
            let mut send = respond;
            send.send_reset(h2::Reason::REFUSED_STREAM);
        }
        drop(tokio::spawn(async move {
            while let Some(Ok(_)) = conn.accept().await {}
        }));

        // Connection 2: Regular Greeter server answering OK
        let (socket2, _) = listener.accept().await.expect("accept conn 2");
        Server::new(GreeterServer::new(CountingService(executions_clone)))
            .serve_connection(socket2)
            .await
            .expect("serve conn 2");
    });

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(client_observer.clone());
    let client = GreeterClient::new(channel);

    let reply = client
        .say_hello(Request::new(req("retry_test")))
        .await
        .expect("say_hello");
    assert_eq!(name_of(reply.get_ref()), "retry_test");
    assert_eq!(executions.load(Ordering::SeqCst), 1);

    drop(client);
    server_task.await.expect("server task");

    let events = client_observer.events();

    // Verify exact event counts during transparent retry:
    // Exactly 1 call start, exactly 2 attempt starts, exactly 2 attempt ends, exactly 1 call end!
    let call_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallStart { .. }))
        .count();
    let call_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallEnd { .. }))
        .count();
    let attempt_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptStart { .. }))
        .count();
    let attempt_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptEnd { .. }))
        .count();
    let reconnects = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::Reconnect { .. }))
        .count();

    assert_eq!(call_starts, 1, "Must not double count call_start on retry");
    assert_eq!(call_ends, 1, "Must not double count call_end on retry");
    assert_eq!(attempt_starts, 2, "Expected exactly 2 attempts");
    assert_eq!(attempt_ends, 2, "Expected exactly 2 attempt completions");
    assert!(reconnects >= 1, "Expected reconnect event for retry redial");

    // Verify ordering of attempts
    let attempt_1_start_idx = events
        .iter()
        .position(|e| *e == RecordedEvent::AttemptStart { attempt: 1 })
        .expect("attempt 1 start");
    let attempt_1_end_idx = events
        .iter()
        .position(|e| {
            *e == RecordedEvent::AttemptEnd {
                attempt: 1,
                code: Code::Unavailable,
            }
        })
        .expect("attempt 1 end");
    let attempt_2_start_idx = events
        .iter()
        .position(|e| *e == RecordedEvent::AttemptStart { attempt: 2 })
        .expect("attempt 2 start");
    let attempt_2_end_idx = events
        .iter()
        .position(|e| {
            *e == RecordedEvent::AttemptEnd {
                attempt: 2,
                code: Code::Ok,
            }
        })
        .expect("attempt 2 end");

    assert!(attempt_1_start_idx < attempt_1_end_idx);
    assert!(attempt_1_end_idx < attempt_2_start_idx);
    assert!(attempt_2_start_idx < attempt_2_end_idx);
}

#[tokio::test]
async fn test_server_rejection_concurrency_limit() {
    let server_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());

    struct BlockingService {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    impl Greeter for BlockingService {
        async fn say_hello(
            &self,
            _req: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(Response::new(reply("done")))
        }
    }

    let svc = BlockingService {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    };

    let srv_obs = server_observer.clone();
    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(svc))
            .max_concurrent_rpcs(1)
            .observer(srv_obs)
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr).await.expect("connect");
    let client1 = GreeterClient::new(channel.clone());
    let client2 = GreeterClient::new(channel);

    let t1 = tokio::spawn(async move { client1.say_hello(Request::new(req("first"))).await });

    started.notified().await;

    // Second RPC arrives while first holds the only slot:
    let err2 = client2
        .say_hello(Request::new(req("second")))
        .await
        .unwrap_err();
    assert_eq!(err2.code(), Code::ResourceExhausted);

    release.notify_one();
    let res1 = t1.await.expect("t1").expect("say_hello 1");
    assert_eq!(name_of(res1.get_ref()), "done");

    let events = server_observer.events();
    let rejections: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            RecordedEvent::Rejection { reason, code } => Some((*reason, *code)),
            _ => None,
        })
        .collect();

    assert!(
        rejections.contains(&(RejectionReason::ConcurrencyLimit, Code::ResourceExhausted)),
        "Server observer must record ConcurrencyLimit rejection"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_server_rejection_unimplemented() {
    let server_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let srv_obs = server_observer.clone();
    let server_handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(EchoService))
            .observer(srv_obs)
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr).await.expect("connect");
    // Call an unmounted service path
    let call = channel.unary::<HelloRequest, HelloReply>(
        "/unknown.Service/Method",
        Request::new(HelloRequest::new()),
    );
    let err = call.await.unwrap_err();
    assert_eq!(err.code(), Code::Unimplemented);

    let events = server_observer.events();
    let has_unimplemented_rejection = events.iter().any(|e| {
        matches!(
            e,
            RecordedEvent::Rejection {
                reason: RejectionReason::Unimplemented,
                code: Code::Unimplemented
            }
        )
    });
    assert!(
        has_unimplemented_rejection,
        "Must record Unimplemented rejection on router"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_caller_cancellation_lifecycle() {
    let client_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    struct SlowService;
    impl Greeter for SlowService {
        async fn say_hello(
            &self,
            _req: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok(Response::new(reply("ok")))
        }
    }

    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(SlowService))
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(client_observer.clone());

    let call = channel.unary::<HelloRequest, HelloReply>(
        "/helloworld.Greeter/SayHello",
        Request::new(HelloRequest::new()),
    );
    let handle = call.handle();
    let call_task = tokio::spawn(call);

    tokio::time::sleep(Duration::from_millis(20)).await;
    handle.cancel();

    let err = call_task.await.expect("task").unwrap_err();
    assert_eq!(err.code(), Code::Cancelled);

    let events = client_observer.events();
    let has_cancellation = events.iter().any(|e| {
        matches!(
            e,
            RecordedEvent::Cancellation {
                reason: CancellationReason::CallerCancelled
            }
        )
    });
    assert!(has_cancellation, "Must record CallerCancelled");

    let call_ends: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            RecordedEvent::CallEnd { code, .. } => Some(*code),
            _ => None,
        })
        .collect();
    assert_eq!(call_ends, vec![Code::Cancelled]);

    server_handle.abort();
}

#[tokio::test]
async fn test_deadline_exceeded_lifecycle() {
    let client_observer = RecordingObserver::new();
    let server_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    struct SlowService;
    impl Greeter for SlowService {
        async fn say_hello(
            &self,
            _req: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok(Response::new(reply("ok")))
        }
    }

    let srv_obs = server_observer.clone();
    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(SlowService))
            .observer(srv_obs)
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(client_observer.clone());
    let client = GreeterClient::new(channel);

    let mut req_msg = Request::new(req("timeout_test"));
    req_msg.set_timeout(Duration::from_millis(20));

    let err = client.say_hello(req_msg).await.unwrap_err();
    assert_eq!(err.code(), Code::DeadlineExceeded);

    tokio::time::sleep(Duration::from_millis(50)).await;

    let client_events = client_observer.events();
    let has_client_deadline = client_events.iter().any(|e| {
        matches!(
            e,
            RecordedEvent::Cancellation {
                reason: CancellationReason::DeadlineExceeded
            }
        )
    });
    assert!(
        has_client_deadline,
        "Client must record DeadlineExceeded cancellation"
    );

    let client_ends: Vec<_> = client_events
        .iter()
        .filter_map(|e| match e {
            RecordedEvent::CallEnd { code, .. } => Some(*code),
            _ => None,
        })
        .collect();
    assert_eq!(client_ends, vec![Code::DeadlineExceeded]);

    server_handle.abort();
}

#[tokio::test]
async fn test_observer_chaining() {
    let obs1 = RecordingObserver::new();
    let obs2 = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(EchoService))
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(obs1.clone())
        .observer(obs2.clone());
    let client = GreeterClient::new(channel);

    let reply = client
        .say_hello(Request::new(req("chaining")))
        .await
        .expect("say_hello");
    assert_eq!(name_of(reply.get_ref()), "chaining");

    let events1 = obs1.events();
    let events2 = obs2.events();

    assert!(!events1.is_empty(), "Observer 1 must receive events");
    assert!(!events2.is_empty(), "Observer 2 must receive events");
    assert_eq!(
        events1, events2,
        "Both chained observers must receive identical events"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_server_streaming_telemetry() {
    let client_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(Echo))
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(client_observer.clone());
    let client = GreeterClient::new(channel);

    let resp = client
        .server_hello(Request::new(req("alpha,beta,gamma")))
        .await
        .expect("server-stream");
    let mut inbound = resp.into_inner();
    let mut parts = Vec::new();
    while let Some(msg) = inbound.message().await.expect("msg") {
        parts.push(name_of(&msg));
    }
    assert_eq!(parts, ["alpha", "beta", "gamma"]);

    let events = client_observer.events();
    let call_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallStart { .. }))
        .count();
    let call_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallEnd { .. }))
        .count();
    let attempt_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptStart { .. }))
        .count();
    let attempt_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptEnd { .. }))
        .count();

    assert_eq!(call_starts, 1);
    assert_eq!(call_ends, 1);
    assert_eq!(attempt_starts, 1);
    assert_eq!(attempt_ends, 1);

    server_handle.abort();
}

#[tokio::test]
async fn test_client_streaming_telemetry() {
    let client_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(Echo))
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(client_observer.clone());
    let client = GreeterClient::new(channel);

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("hello")).await.expect("send 1");
    tx.send(req("world")).await.expect("send 2");
    tx.close();
    let resp = call.await.expect("client-stream");
    assert_eq!(name_of(&resp.into_inner()), "hello,world");

    let events = client_observer.events();
    let call_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallStart { .. }))
        .count();
    let call_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallEnd { .. }))
        .count();
    let attempt_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptStart { .. }))
        .count();
    let attempt_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::AttemptEnd { .. }))
        .count();

    assert_eq!(call_starts, 1);
    assert_eq!(call_ends, 1);
    assert_eq!(attempt_starts, 1);
    assert_eq!(attempt_ends, 1);

    server_handle.abort();
}

#[tokio::test]
async fn test_bidi_streaming_telemetry() {
    let client_observer = RecordingObserver::new();

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(Echo))
            .serve_listener(listener)
            .await
            .ok();
    });

    let channel = Channel::connect(addr)
        .await
        .expect("connect")
        .observer(client_observer.clone());
    let client = GreeterClient::new(channel);

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("ping")).await.expect("send");
    tx.close();
    let resp = call.await.expect("bidi");
    let mut inbound = resp.into_inner();
    let first = inbound
        .message()
        .await
        .expect("msg")
        .expect("at least one bidi reply");
    assert_eq!(name_of(&first), "ping");

    let events = client_observer.events();
    let call_starts = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallStart { .. }))
        .count();
    let call_ends = events
        .iter()
        .filter(|e| matches!(e, RecordedEvent::CallEnd { .. }))
        .count();

    assert_eq!(call_starts, 1);
    assert_eq!(call_ends, 1);

    server_handle.abort();
}

#[tokio::test]
async fn test_disabled_instrumentation_zero_overhead() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let server_handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(Echo))
            .serve_listener(listener)
            .await
            .ok();
    });

    // Default channel has observer == None (zero allocation, disabled)
    let channel = Channel::connect(addr).await.expect("connect");
    let client = GreeterClient::new(channel);

    let reply = client
        .say_hello(Request::new(req("disabled_test")))
        .await
        .expect("say_hello");
    assert_eq!(name_of(reply.get_ref()), "disabled_test");

    server_handle.abort();
}

#[test]
fn test_bounded_labels_safety() {
    // Verify that labels contain bounded static strings and do NOT store payloads or credentials.
    let labels = CallLabels::new(
        "/helloworld.Greeter/SayHello",
        Some("127.0.0.1:50051"),
        CallRole::Client,
    );
    assert_eq!(labels.service(), "helloworld.Greeter");
    assert_eq!(labels.method(), "SayHello");
    assert_eq!(labels.path(), "/helloworld.Greeter/SayHello");
    assert_eq!(labels.authority(), Some("127.0.0.1:50051"));
    assert_eq!(labels.role(), CallRole::Client);

    let owned = labels.to_owned();
    assert_eq!(owned.as_borrowed(), labels);

    let attempt = AttemptLabels::new(labels, 1);
    assert_eq!(attempt.attempt(), 1);
    assert_eq!(attempt.service(), "helloworld.Greeter");
    assert_eq!(attempt.method(), "SayHello");
}

#[test]
fn test_metadata_map_default_debug_redacts_credentials_and_binary() {
    let mut md = MetadataMap::new();

    // Standard credential headers
    md.insert("authorization", "Bearer top-secret-jwt-token-value")
        .expect("insert auth");
    md.insert("cookie", "session_id=super_sensitive_cookie_val")
        .expect("insert cookie");
    md.insert("set-cookie", "refresh_token=sensitive_refresh")
        .expect("insert set-cookie");
    md.insert("proxy-authorization", "Basic dXNlcjpwYXNz")
        .expect("insert proxy-auth");

    // Binary metadata headers
    md.insert_bin("x-trace-bin", [0xde, 0xad, 0xbe, 0xef])
        .expect("insert bin");
    md.insert_bin("credential-bin", [0x01, 0x02, 0x03])
        .expect("insert bin");

    // Custom sensitive headers (substring matches)
    md.insert("x-api-key", "secret-api-key-999")
        .expect("insert api-key");
    md.insert("session-token", "tok-xyz-888")
        .expect("insert token");
    md.insert("my-secret-header", "classified-data")
        .expect("insert secret");
    md.insert("user-password", "p@ssw0rd123")
        .expect("insert password");
    md.insert("custom-auth-hdr", "custom-auth-val")
        .expect("insert auth");

    // Explicitly marked custom sensitive header
    md.insert("x-tenant-private-id", "tenant-private-42")
        .expect("insert tenant");
    md.mark_sensitive("x-tenant-private-id");

    // Safe, non-sensitive headers
    md.insert("x-request-id", "req-safe-uuid-12345")
        .expect("insert req-id");
    md.insert(
        "traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
    )
    .expect("insert traceparent");

    let formatted = format!("{md:?}");

    // Credential values must NEVER appear in the formatted output
    assert!(
        !formatted.contains("top-secret-jwt-token-value"),
        "leaked authorization token: {formatted}"
    );
    assert!(
        !formatted.contains("super_sensitive_cookie_val"),
        "leaked cookie: {formatted}"
    );
    assert!(
        !formatted.contains("sensitive_refresh"),
        "leaked set-cookie: {formatted}"
    );
    assert!(
        !formatted.contains("dXNlcjpwYXNz"),
        "leaked proxy-authorization: {formatted}"
    );
    assert!(
        !formatted.contains("secret-api-key-999"),
        "leaked api-key: {formatted}"
    );
    assert!(
        !formatted.contains("tok-xyz-888"),
        "leaked session-token: {formatted}"
    );
    assert!(
        !formatted.contains("classified-data"),
        "leaked secret: {formatted}"
    );
    assert!(
        !formatted.contains("p@ssw0rd123"),
        "leaked password: {formatted}"
    );
    assert!(
        !formatted.contains("custom-auth-val"),
        "leaked custom auth: {formatted}"
    );
    assert!(
        !formatted.contains("tenant-private-42"),
        "leaked marked sensitive: {formatted}"
    );

    // Redacted placeholders MUST appear for each sensitive entry
    assert!(
        formatted.contains("\"authorization\": \"[REDACTED]\""),
        "missing authorization redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"cookie\": \"[REDACTED]\""),
        "missing cookie redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"set-cookie\": \"[REDACTED]\""),
        "missing set-cookie redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"proxy-authorization\": \"[REDACTED]\""),
        "missing proxy-authorization redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"x-trace-bin\": \"[REDACTED]\""),
        "missing binary metadata redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"credential-bin\": \"[REDACTED]\""),
        "missing binary metadata redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"x-api-key\": \"[REDACTED]\""),
        "missing api-key redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"session-token\": \"[REDACTED]\""),
        "missing token redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"my-secret-header\": \"[REDACTED]\""),
        "missing secret redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"user-password\": \"[REDACTED]\""),
        "missing password redaction: {formatted}"
    );
    assert!(
        formatted.contains("\"x-tenant-private-id\": \"[REDACTED]\""),
        "missing marked sensitive redaction: {formatted}"
    );

    // Safe headers MUST remain observable with their real values
    assert!(
        formatted.contains("\"x-request-id\": \"req-safe-uuid-12345\""),
        "safe header should be preserved: {formatted}"
    );
    assert!(
        formatted.contains(
            "\"traceparent\": \"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01\""
        ),
        "safe header should be preserved: {formatted}"
    );

    // Underlying values must remain intact for application logic
    assert_eq!(
        md.get("authorization"),
        Some("Bearer top-secret-jwt-token-value")
    );
    assert_eq!(
        md.get("cookie"),
        Some("session_id=super_sensitive_cookie_val")
    );
    assert_eq!(
        md.get_bin("x-trace-bin").as_deref(),
        Some(&[0xde, 0xad, 0xbe, 0xef][..])
    );
}

#[test]
fn test_request_and_response_default_debug_redaction() {
    // Test Request
    let mut req = Request::new("sensitive-request-payload-xyz");
    req.metadata_mut()
        .insert("authorization", "Bearer super-secret-req-token")
        .expect("insert");
    req.metadata_mut()
        .insert("cookie", "session=req-secret")
        .expect("insert");
    req.metadata_mut()
        .insert("x-request-id", "req-123")
        .expect("insert");

    let formatted_req = format!("{req:?}");

    // Payload and credentials must be redacted by default
    assert!(
        !formatted_req.contains("sensitive-request-payload-xyz"),
        "payload leaked in default Request Debug: {formatted_req}"
    );
    assert!(
        !formatted_req.contains("super-secret-req-token"),
        "authorization leaked in Request Debug: {formatted_req}"
    );
    assert!(
        formatted_req.contains("message: \"[REDACTED]\""),
        "expected redacted payload in Request Debug: {formatted_req}"
    );
    assert!(
        formatted_req.contains("\"authorization\": \"[REDACTED]\""),
        "expected redacted authorization in Request Debug: {formatted_req}"
    );
    assert!(
        formatted_req.contains("\"x-request-id\": \"req-123\""),
        "safe header missing in Request Debug: {formatted_req}"
    );

    // Explicit consent/permission unlocks diagnostic payload
    req.allow_diagnostic_payload(true);
    let allowed_req = format!("{req:?}");
    assert!(
        allowed_req.contains("sensitive-request-payload-xyz"),
        "payload should appear with diagnostic consent: {allowed_req}"
    );
    // Even when payload is allowed, sensitive headers remain redacted!
    assert!(
        allowed_req.contains("\"authorization\": \"[REDACTED]\""),
        "authorization must remain redacted: {allowed_req}"
    );

    // Test Response
    let mut resp = Response::new("sensitive-response-payload-abc");
    resp.metadata_mut()
        .insert("set-cookie", "token=sensitive-cookie")
        .expect("insert");
    resp.trailers_mut()
        .insert("x-trace-bin", "deadbeef")
        .expect_err("must end in -bin");
    resp.trailers_mut()
        .insert_bin("x-trace-bin", [0xde, 0xad])
        .expect("insert bin");

    let formatted_resp = format!("{resp:?}");
    assert!(
        !formatted_resp.contains("sensitive-response-payload-abc"),
        "payload leaked in default Response Debug: {formatted_resp}"
    );
    assert!(
        !formatted_resp.contains("sensitive-cookie"),
        "set-cookie leaked in Response Debug: {formatted_resp}"
    );
    assert!(
        formatted_resp.contains("message: \"[REDACTED]\""),
        "expected redacted payload in Response Debug: {formatted_resp}"
    );
    assert!(
        formatted_resp.contains("\"set-cookie\": \"[REDACTED]\""),
        "expected redacted set-cookie in Response Debug: {formatted_resp}"
    );
    assert!(
        formatted_resp.contains("\"x-trace-bin\": \"[REDACTED]\""),
        "expected redacted binary trailer in Response Debug: {formatted_resp}"
    );

    resp.allow_diagnostic_payload(true);
    let allowed_resp = format!("{resp:?}");
    assert!(
        allowed_resp.contains("sensitive-response-payload-abc"),
        "response payload should appear with diagnostic consent: {allowed_resp}"
    );
    assert!(
        allowed_resp.contains("\"set-cookie\": \"[REDACTED]\""),
        "set-cookie must remain redacted: {allowed_resp}"
    );
}

#[test]
fn test_diagnostic_config_consent_and_cardinality_limits() {
    // 1. Without consent, options are inactive
    let no_consent_config = DiagnosticConfig::new()
        .with_payload(true)
        .with_sensitive_headers(true)
        .with_binary_metadata(true);
    assert!(!no_consent_config.has_consent());
    assert!(!no_consent_config.is_payload_allowed());
    assert!(!no_consent_config.are_sensitive_headers_allowed());
    assert!(!no_consent_config.is_binary_metadata_allowed());

    // 2. With explicit consent, options become active
    let consent_config = DiagnosticConfig::new()
        .with_consent(true)
        .with_payload(true)
        .with_sensitive_headers(true)
        .with_binary_metadata(true);
    assert!(consent_config.has_consent());
    assert!(consent_config.is_payload_allowed());
    assert!(consent_config.are_sensitive_headers_allowed());
    assert!(consent_config.is_binary_metadata_allowed());

    // 3. Cardinality limits on metadata entries
    let mut md = Metadata::new();
    for i in 0..20 {
        md.insert(format!("x-header-{i}"), format!("val-{i}"))
            .expect("insert");
    }

    let limited_config = DiagnosticConfig::new().with_max_metadata_entries(5);

    let formatted_limited = format!("{:?}", md.safe_debug(&limited_config));
    assert!(
        formatted_limited.contains("[TRUNCATED: cardinality limit exceeded]"),
        "expected truncation marker: {formatted_limited}"
    );

    // 4. Value length limits
    let mut md_long = Metadata::new();
    let long_val = "A".repeat(500);
    md_long.insert("x-long-header", &long_val).expect("insert");

    let len_limited_config = DiagnosticConfig::new().with_max_value_length(20);

    let formatted_len = format!("{:?}", md_long.safe_debug(&len_limited_config));
    assert!(
        formatted_len.contains("[TRUNCATED]"),
        "expected value truncation: {formatted_len}"
    );
    assert!(
        !formatted_len.contains(&long_val),
        "full 500-char value should not appear: {formatted_len}"
    );

    // 5. Custom sensitive header registered in DiagnosticConfig
    let mut md_custom = Metadata::new();
    md_custom
        .insert("x-custom-tenant-uuid", "uuid-secret-999")
        .expect("insert");

    let custom_sensitive_config =
        DiagnosticConfig::new().with_sensitive_header("x-custom-tenant-uuid");

    let formatted_custom = format!("{:?}", md_custom.safe_debug(&custom_sensitive_config));
    assert!(
        !formatted_custom.contains("uuid-secret-999"),
        "custom sensitive header value leaked: {formatted_custom}"
    );
    assert!(
        formatted_custom.contains("\"x-custom-tenant-uuid\": \"[REDACTED]\""),
        "custom sensitive header should be redacted: {formatted_custom}"
    );
}

#[test]
fn test_status_error_paths_remain_observable() {
    // Status error codes and messages must remain actionable and observable
    let status_unauth = Status::unauthenticated("missing token");
    assert_eq!(status_unauth.code(), Code::Unauthenticated);
    assert_eq!(status_unauth.message(), "missing token");
    let display_unauth = format!("{status_unauth}");
    assert!(
        display_unauth.contains("UNAUTHENTICATED"),
        "status code must be observable in Display: {display_unauth}"
    );
    assert!(
        display_unauth.contains("missing token"),
        "status message must be observable in Display: {display_unauth}"
    );

    let status_invalid = Status::invalid_argument("malformed parameter foo");
    let debug_invalid = format!("{status_invalid:?}");
    assert!(
        debug_invalid.contains("InvalidArgument"),
        "error code must be observable in Debug: {debug_invalid}"
    );
    assert!(
        debug_invalid.contains("malformed parameter foo"),
        "error message must be observable in Debug: {debug_invalid}"
    );

    // Attaching metadata with credentials to a Status still redacts the credentials in Debug
    let mut status_with_md = Status::permission_denied("access denied");
    status_with_md
        .metadata_mut()
        .insert("authorization", "Bearer leaking-token-attempt")
        .expect("insert");
    status_with_md
        .metadata_mut()
        .insert("x-request-id", "req-err-456")
        .expect("insert");

    let debug_status_md = format!("{status_with_md:?}");
    assert!(
        debug_status_md.contains("PermissionDenied"),
        "code must be observable: {debug_status_md}"
    );
    assert!(
        debug_status_md.contains("access denied"),
        "message must be observable: {debug_status_md}"
    );
    assert!(
        !debug_status_md.contains("leaking-token-attempt"),
        "sensitive header leaked in Status Debug: {debug_status_md}"
    );
    assert!(
        debug_status_md.contains("\"authorization\": \"[REDACTED]\""),
        "expected redacted header in Status Debug: {debug_status_md}"
    );
    assert!(
        debug_status_md.contains("\"x-request-id\": \"req-err-456\""),
        "safe header should remain in Status Debug: {debug_status_md}"
    );
}

#[test]
fn test_telemetry_context_safe_debug_and_observability() {
    let mut req = Request::new(req("test-user"));
    req.metadata_mut()
        .insert("authorization", "Bearer obs-secret-token")
        .expect("insert");
    req.metadata_mut()
        .insert("cookie", "sess=obs-cookie")
        .expect("insert");
    req.metadata_mut()
        .insert("x-request-id", "req-obs-789")
        .expect("insert");

    let ctx = req.telemetry_context();
    let status = Status::not_found("user not found");
    let ctx_with_status = ctx.with_status(&status);

    assert_eq!(ctx_with_status.status_code(), Some(Code::NotFound));

    let formatted_ctx = format!("{ctx_with_status:?}");

    // Observability: status code and message are clearly visible
    assert!(
        formatted_ctx.contains("status_code: NotFound"),
        "status code must be observable in TelemetryContext: {formatted_ctx}"
    );
    assert!(
        formatted_ctx.contains("status_message: \"user not found\""),
        "status message must be observable in TelemetryContext: {formatted_ctx}"
    );

    // Safety: credentials are redacted
    assert!(
        !formatted_ctx.contains("obs-secret-token"),
        "authorization leaked in TelemetryContext: {formatted_ctx}"
    );
    assert!(
        !formatted_ctx.contains("sess=obs-cookie"),
        "cookie leaked in TelemetryContext: {formatted_ctx}"
    );
    assert!(
        formatted_ctx.contains("\"authorization\": \"[REDACTED]\""),
        "expected redacted authorization in TelemetryContext: {formatted_ctx}"
    );
    assert!(
        formatted_ctx.contains("\"cookie\": \"[REDACTED]\""),
        "expected redacted cookie in TelemetryContext: {formatted_ctx}"
    );
    assert!(
        formatted_ctx.contains("\"x-request-id\": \"req-obs-789\""),
        "safe header should be preserved in TelemetryContext: {formatted_ctx}"
    );

    // Direct construction using TelemetryContext::new
    let direct_labels = CallLabels::new(
        "/helloworld.Greeter/SayHello",
        Some("localhost:50051"),
        CallRole::Client,
    );
    let direct_ctx = TelemetryContext::new(direct_labels)
        .with_metadata(req.metadata())
        .with_status(&status);
    let direct_formatted = format!("{direct_ctx:?}");
    assert!(direct_formatted.contains("service: \"helloworld.Greeter\""));
    assert!(direct_formatted.contains("status_code: NotFound"));
    assert!(direct_formatted.contains("\"authorization\": \"[REDACTED]\""));
    assert!(!direct_formatted.contains("obs-secret-token"));
}
