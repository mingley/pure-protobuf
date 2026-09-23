//! Retry commitment boundaries and execution counting tests.
//!
//! Pinned reference: gRFC A6 (Client-side retry support in gRPC) at
//! grpc/proposal@6342be729b96478a2897ceb208a8cddcd832a17b (see docs/plan/README.md).
//!
//! Retry safety verification for RT-02:
//! - Scenario A: failure before headers (connection refused / failed connect / REFUSED_STREAM)
//!   -> verify transparent retry is safe and works (execution counter = 1).
//! - Scenario B: server receives request, increments counter, connection drops before response headers
//!   -> assert post-dispatch connection drops DO NOT transparently retry (counter stays 1).
//! - Scenario C: response headers committed
//!   -> assert no retry on subsequent stream error (counter = 1).

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

use common::{name_of, name_of_request, reply, req, reserve_loopback, serve, serve_on};
use http::header::CONTENT_TYPE;
use http::{HeaderValue, StatusCode};
use pbrs::Serialize;
use pbrs_grpc::hello::{Greeter, GreeterClient, HelloReply, HelloRequest};
use pbrs_grpc::timeout::parse_timeout;
use pbrs_grpc::{
    codec, Channel, ChannelConfig, Code, Request, Response, ServerConfig, Status, Streaming,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

/// Greeter implementation that tracks invocations with an atomic counter.
struct CountingGreeter {
    executions: Arc<AtomicUsize>,
}

impl CountingGreeter {
    fn new(executions: Arc<AtomicUsize>) -> Self {
        Self { executions }
    }
}

impl Greeter for CountingGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(Response::new(reply(name_of_request(request.get_ref()))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        let name = name_of_request(request.get_ref());
        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            let _ = tx.send(reply(name)).await;
        }));
        Ok(Response::new(stream))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trailers_only_rejection_survives_early_request_body_reset() {
    for shape in ["unary", "server_stream"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept connection");
            let mut conn = h2::server::handshake(socket).await.expect("handshake");
            let (request, mut respond) = conn
                .accept()
                .await
                .expect("request")
                .expect("valid request");
            let response = http::Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/grpc")
                .header("grpc-status", "8")
                .header("grpc-message", "too%20many%20concurrent%20RPCs")
                .body(())
                .expect("trailers-only response");
            respond.send_response(response, true).expect("reject");
            drop(respond);
            drop(request);
            while let Some(Ok(_)) = conn.accept().await {}
        });

        let channel = Channel::connect_with(
            addr,
            ChannelConfig::new()
                .connections(1)
                .max_send_buffer_size(1024),
        )
        .await
        .expect("connect");
        let client = GreeterClient::new(channel.byte_budget(256 * 1024));
        let mut request = Request::new(req(&"x".repeat(128 * 1024)));
        request.set_timeout(Duration::from_secs(3));
        let result = if shape == "unary" {
            tokio::time::timeout(Duration::from_secs(5), client.say_hello(request))
                .await
                .expect("unary rejection timed out")
                .map(|_| ())
        } else {
            tokio::time::timeout(Duration::from_secs(5), client.server_hello(request))
                .await
                .expect("server-stream rejection timed out")
                .map(|_| ())
        };
        let status = result.expect_err("server refused the RPC");
        assert_eq!(
            status.code(),
            Code::ResourceExhausted,
            "{shape}: server status lost to an HTTP/2 send failure: {status}"
        );
        assert_eq!(status.message(), "too many concurrent RPCs");
        server.abort();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn request_send_window_stall_obeys_deadline_for_both_single_request_shapes() {
    for shape in ["unary", "server_stream"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept connection");
            let mut conn = h2::server::handshake(socket).await.expect("handshake");
            let (request, _respond) = conn
                .accept()
                .await
                .expect("request")
                .expect("valid request");
            let _body = request.into_body();
            while let Some(Ok(_)) = conn.accept().await {}
        });
        let channel = Channel::connect_with(
            addr,
            ChannelConfig::new()
                .connections(1)
                .max_send_buffer_size(1024),
        )
        .await
        .expect("connect");
        let client = GreeterClient::new(channel.byte_budget(256 * 1024));
        let mut request = Request::new(req(&"x".repeat(128 * 1024)));
        request.set_timeout(Duration::from_millis(120));
        let started = std::time::Instant::now();
        let result = if shape == "unary" {
            tokio::time::timeout(Duration::from_secs(2), client.say_hello(request))
                .await
                .expect("unary must not hang behind send flow control")
                .map(|_| ())
        } else {
            tokio::time::timeout(Duration::from_secs(2), client.server_hello(request))
                .await
                .expect("server stream must not hang behind send flow control")
                .map(|_| ())
        };
        let status = result.expect_err("peer withheld request send credit");
        assert_eq!(status.code(), Code::DeadlineExceeded, "{shape}: {status}");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{shape}: request send exceeded the deadline"
        );
        server.abort();
    }
}

// ============================================================================
// Scenario A: Failure before headers -> transparent retry is safe and works
// ============================================================================

/// Scenario A1: Connection refused on initial dial. With wait-for-ready, the client
/// retries connection until the listener starts, and executes the RPC exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_a_connection_refused_wait_for_ready_retries_safely() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();
    let executions = Arc::new(AtomicUsize::new(0));

    let channel = Channel::connect_lazy(addr).expect("connect_lazy");
    let client = GreeterClient::new(channel);

    let client_task = tokio::spawn(async move {
        let mut request = Request::new(req("scenario_a1"));
        request.set_wait_for_ready(true);
        client.say_hello(request).await
    });

    // Client connection attempts initially fail before headers (ECONNREFUSED).
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Server starts listening on the reserved port.
    let listener = reserved.listen();
    let _guard = serve_on(
        listener,
        CountingGreeter::new(Arc::clone(&executions)),
        ServerConfig::default(),
    );

    let response = client_task.await.expect("client task").expect("say_hello");
    assert_eq!(name_of(response.get_ref()), "scenario_a1");
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "Scenario A1: Failure before headers transparently retried and executed exactly once"
    );
}

/// Scenario A2: Server sends HTTP/2 REFUSED_STREAM before processing.
/// gRFC A6 explicitly allows transparent retry on REFUSED_STREAM because the remote
/// peer refused the stream prior to performing any application processing (RFC 7540 §8.1.4).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_a_refused_stream_retries_safely() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let executions = Arc::new(AtomicUsize::new(0));
    let executions_clone = Arc::clone(&executions);

    let server_task = tokio::spawn(async move {
        // Attempt 1: Server accepts connection and stream, but immediately sends REFUSED_STREAM.
        let (socket, _) = listener.accept().await.expect("accept conn 1");
        let mut conn = h2::server::handshake(socket).await.expect("handshake 1");
        if let Some(Ok((_req, respond))) = conn.accept().await {
            // Refuse stream before any application logic runs:
            let mut send = respond;
            send.send_reset(h2::Reason::REFUSED_STREAM);
        }
        // Drive conn 1 until closed:
        drop(tokio::spawn(async move {
            while let Some(Ok(_)) = conn.accept().await {}
        }));

        // Attempt 2: Client transparently retries on a new connection.
        let (socket2, _) = listener.accept().await.expect("accept conn 2");
        let mut conn2 = h2::server::handshake(socket2).await.expect("handshake 2");
        if let Some(Ok((req, mut respond2))) = conn2.accept().await {
            let mut body = req.into_body();
            while let Some(chunk) = body.data().await {
                chunk.expect("data chunk");
            }
            executions_clone.fetch_add(1, Ordering::SeqCst);

            let reply_msg = reply("refused_retry_success");
            let reply_bytes = reply_msg.serialize().expect("serialize");
            let frame = codec::encode(&reply_bytes, false).expect("encode frame");
            let response = http::Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/grpc")
                .body(())
                .expect("response");
            let mut send_stream = respond2.send_response(response, false).expect("headers");
            send_stream.send_data(frame, false).expect("send data");
            let mut trailers = http::HeaderMap::new();
            trailers.insert("grpc-status", HeaderValue::from_static("0"));
            send_stream.send_trailers(trailers).expect("trailers");
        }
        while let Some(Ok(_)) = conn2.accept().await {}
    });

    let client = GreeterClient::connect(addr).await.expect("connect");
    let response = client
        .say_hello(Request::new(req("test")))
        .await
        .expect("say_hello after transparent retry");

    assert_eq!(name_of(response.get_ref()), "refused_retry_success");
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "Scenario A2: REFUSED_STREAM transparently retried and executed exactly once on attempt 2"
    );
    server_task.abort();
}

// ============================================================================
// Scenario B: Request dispatched to server -> connection drops -> baseline retries
// ============================================================================

/// Scenario B (Unary):
/// Server receives request, increments execution counter to 1, then connection drops
/// before sending response headers.
///
/// Post-dispatch connection drop must NOT transparently retry (gRFC A6).
/// Counter must remain 1 and client receives an Unavailable error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_b_unary_drops_after_request_dispatched_baseline_retries() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let executions = Arc::new(AtomicUsize::new(0));
    let executions_clone = Arc::clone(&executions);

    let server_task = tokio::spawn(async move {
        // Attempt 1: Server receives request, increments counter, then drops connection
        // before response headers are sent.
        let (socket, _) = listener.accept().await.expect("accept 1");
        let mut conn = h2::server::handshake(socket).await.expect("handshake 1");
        if let Some(Ok((req, respond))) = conn.accept().await {
            let mut body = req.into_body();
            while let Some(chunk) = body.data().await {
                chunk.expect("data chunk");
            }
            // Application logic started: increment execution counter
            executions_clone.fetch_add(1, Ordering::SeqCst);
            // Abruptly terminate connection: drop respond and conn without headers
            drop(respond);
            drop(body);
            drop(conn);
        }

        // If client wrongfully attempts a second execution, record it:
        if let Ok((socket2, _)) = listener.accept().await {
            if let Ok(mut conn2) = h2::server::handshake(socket2).await {
                if let Some(Ok((_req, _respond2))) = conn2.accept().await {
                    executions_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    });

    let client = GreeterClient::connect(addr).await.expect("connect");
    let result = client.say_hello(Request::new(req("test"))).await;

    assert!(result.is_err(), "Expected error after connection drop");
    let status = result.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);

    // CRITICAL ASSERTION:
    // Post-dispatch connection drop must NOT transparently retry.
    // Execution counter must remain 1.
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "Scenario B: Client must not retry after request dispatch; executions must stay 1"
    );
    server_task.abort();
}

/// Scenario B (Server Streaming):
/// Server receives request, increments execution counter to 1, then connection drops
/// before sending response headers.
///
/// Post-dispatch connection drop must NOT transparently retry (gRFC A6).
/// Counter must remain 1 and client receives an Unavailable error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_b_server_streaming_drops_after_request_dispatched_baseline_retries() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let executions = Arc::new(AtomicUsize::new(0));
    let executions_clone = Arc::clone(&executions);

    let server_task = tokio::spawn(async move {
        // Attempt 1: Server receives request, increments counter, drops connection
        let (socket, _) = listener.accept().await.expect("accept 1");
        let mut conn = h2::server::handshake(socket).await.expect("handshake 1");
        if let Some(Ok((req, respond))) = conn.accept().await {
            let mut body = req.into_body();
            while let Some(chunk) = body.data().await {
                chunk.expect("data chunk");
            }
            executions_clone.fetch_add(1, Ordering::SeqCst);
            drop(respond);
            drop(body);
            drop(conn);
        }

        // If client wrongfully attempts a second execution, record it:
        if let Ok((socket2, _)) = listener.accept().await {
            if let Ok(mut conn2) = h2::server::handshake(socket2).await {
                if let Some(Ok((_req, _respond2))) = conn2.accept().await {
                    executions_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    });

    let client = GreeterClient::connect(addr).await.expect("connect");
    let result = client.server_hello(Request::new(req("test"))).await;

    assert!(result.is_err(), "Expected error after connection drop");
    let status = result.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);

    // CRITICAL ASSERTION:
    // Post-dispatch connection drop must NOT transparently retry.
    // Execution counter must remain 1.
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "Scenario B: Server streaming must not retry after request dispatch; executions must stay 1"
    );
    server_task.abort();
}

// ============================================================================
// Scenario C: Response headers committed -> no retry on subsequent stream error
// ============================================================================

/// Scenario C (Unary):
/// Server receives request, increments execution counter to 1, sends response headers
/// (committing the response). Then a stream error occurs (RST_STREAM).
///
/// Acceptance criteria: Client must NOT retry once response headers have arrived.
/// Execution counter must remain 1.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_c_unary_response_headers_committed_no_retry_on_stream_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let executions = Arc::new(AtomicUsize::new(0));
    let executions_clone = Arc::clone(&executions);

    let server_task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        let mut conn = h2::server::handshake(socket).await.expect("handshake");
        if let Some(Ok((req, mut respond))) = conn.accept().await {
            let mut body = req.into_body();
            while let Some(chunk) = body.data().await {
                chunk.expect("data chunk");
            }
            executions_clone.fetch_add(1, Ordering::SeqCst);

            // Send response HEADERS: commits the response to the client!
            let response = http::Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/grpc")
                .body(())
                .expect("response");
            let mut send_stream = respond.send_response(response, false).expect("headers");

            // Reset stream after response headers have been committed:
            send_stream.send_reset(h2::Reason::CANCEL);
        }
        while let Some(Ok(_)) = conn.accept().await {}
    });

    let client = GreeterClient::connect(addr).await.expect("connect");
    let result = client.say_hello(Request::new(req("test"))).await;

    assert!(result.is_err(), "Expected error after stream reset");
    let status = result.unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);

    // CRITICAL ASSERTION: No transparent retry because response was committed!
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "Scenario C: Response committed; client must NOT retry on subsequent stream error"
    );
    server_task.abort();
}

/// Scenario C (Server Streaming):
/// Server receives request, increments counter to 1, sends response headers and one data item.
/// Client's Call resolves with `Response<Streaming<T>>`.
/// When subsequent stream error occurs (RST_STREAM) during streaming, client cannot retry.
/// Execution counter must remain 1.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_c_server_streaming_response_headers_committed_no_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let executions = Arc::new(AtomicUsize::new(0));
    let executions_clone = Arc::clone(&executions);
    let (reset_tx, reset_rx) = tokio::sync::oneshot::channel::<()>();

    let server_task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        let mut conn = h2::server::handshake(socket).await.expect("handshake");
        if let Some(Ok((req, mut respond))) = conn.accept().await {
            let mut body = req.into_body();
            while let Some(chunk) = body.data().await {
                chunk.expect("data chunk");
            }
            executions_clone.fetch_add(1, Ordering::SeqCst);

            // Send response HEADERS: commits the response to the client
            let response = http::Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/grpc")
                .body(())
                .expect("response");
            let mut send_stream = respond.send_response(response, false).expect("headers");

            // Send first stream message
            let reply_msg = reply("first_stream_item");
            let reply_bytes = reply_msg.serialize().expect("serialize");
            let frame = codec::encode(&reply_bytes, false).expect("encode frame");
            send_stream.send_data(frame, false).expect("send data");

            // Handle stream reset in background while conn is driven:
            tokio::spawn(async move {
                let _ = reset_rx.await;
                send_stream.send_reset(h2::Reason::CANCEL);
            });
        }
        while let Some(Ok(_)) = conn.accept().await {}
    });

    let client = GreeterClient::connect(addr).await.expect("connect");
    let response = client
        .server_hello(Request::new(req("test")))
        .await
        .expect("server_hello call succeeds on response headers");

    let mut stream = response.into_inner();
    let first_msg = stream
        .message()
        .await
        .expect("first message")
        .expect("item");
    assert_eq!(name_of(&first_msg), "first_stream_item");

    // Signal server to send stream reset now that response is committed and stream is active:
    reset_tx.send(()).expect("signal reset");

    let msg_err = stream.message().await.expect_err("expected stream error");
    assert_eq!(msg_err.code(), Code::Cancelled);

    // CRITICAL ASSERTION: No transparent retry because response was committed!
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1,
        "Scenario C: Server streaming response committed; client must not retry"
    );
    server_task.abort();
}

// ============================================================================
// Scenario D: Absolute deadline propagation across retries, queueing, and slots
// ============================================================================

/// Scenario D1: Transparent retry preserves original deadline and transmits
/// remaining timeout instead of restarting the initial duration.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_d_deadline_preserved_and_remaining_timeout_decreases_across_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let attempt1_timeout = Arc::new(tokio::sync::Mutex::new(None::<Duration>));
    let attempt2_timeout = Arc::new(tokio::sync::Mutex::new(None::<Duration>));
    let t1_clone = Arc::clone(&attempt1_timeout);
    let t2_clone = Arc::clone(&attempt2_timeout);

    let server_task = tokio::spawn(async move {
        // Attempt 1: Server accepts connection and stream.
        let (socket, _) = listener.accept().await.expect("accept conn 1");
        let mut conn = h2::server::handshake(socket).await.expect("handshake 1");
        if let Some(Ok((req, respond))) = conn.accept().await {
            if let Some(val) = req.headers().get("grpc-timeout") {
                if let Ok(s) = val.to_str() {
                    *t1_clone.lock().await = parse_timeout(s);
                }
            }
            // Delay before refusing stream so time clearly elapses:
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut send = respond;
            send.send_reset(h2::Reason::REFUSED_STREAM);
        }
        drop(tokio::spawn(async move {
            while let Some(Ok(_)) = conn.accept().await {}
        }));

        // Attempt 2: Client transparently retries with remaining deadline budget.
        let (socket2, _) = listener.accept().await.expect("accept conn 2");
        let mut conn2 = h2::server::handshake(socket2).await.expect("handshake 2");
        if let Some(Ok((req, mut respond2))) = conn2.accept().await {
            if let Some(val) = req.headers().get("grpc-timeout") {
                if let Ok(s) = val.to_str() {
                    *t2_clone.lock().await = parse_timeout(s);
                }
            }
            let mut body = req.into_body();
            while let Some(chunk) = body.data().await {
                chunk.expect("data chunk");
            }
            let reply_msg = reply("retry_budget_success");
            let reply_bytes = reply_msg.serialize().expect("serialize");
            let frame = codec::encode(&reply_bytes, false).expect("encode frame");
            let response = http::Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/grpc")
                .body(())
                .expect("response");
            let mut send_stream = respond2.send_response(response, false).expect("headers");
            send_stream.send_data(frame, false).expect("send data");
            let mut trailers = http::HeaderMap::new();
            trailers.insert("grpc-status", HeaderValue::from_static("0"));
            send_stream.send_trailers(trailers).expect("trailers");
        }
        while let Some(Ok(_)) = conn2.accept().await {}
    });

    let client = GreeterClient::connect(addr).await.expect("connect");
    let mut request = Request::new(req("test"));
    request.set_timeout(Duration::from_millis(1500));
    let response = client.say_hello(request).await.expect("say_hello succeeds");

    assert_eq!(name_of(response.get_ref()), "retry_budget_success");

    let t1 = attempt1_timeout
        .lock()
        .await
        .expect("attempt 1 timeout recorded");
    let t2 = attempt2_timeout
        .lock()
        .await
        .expect("attempt 2 timeout recorded");

    // CRITICAL ASSERTIONS:
    // 1. Initial attempt was close to 1500ms.
    assert!(
        t1 <= Duration::from_millis(1500) && t1 >= Duration::from_millis(1400),
        "t1 was {:?}",
        t1
    );
    // 2. Retry attempt timeout must strictly decrease (deadline preserved across retry).
    assert!(
        t2 < t1,
        "t2 ({:?}) must be strictly less than t1 ({:?})",
        t2,
        t1
    );
    // 3. Difference should reflect the ~50ms elapsed sleep.
    assert!(
        t1 - t2 >= Duration::from_millis(35),
        "t1 - t2 was {:?}",
        t1 - t2
    );

    server_task.abort();
}

/// Scenario D2: Deadline expires while waiting for connection (wait-for-ready).
/// Verifies call aborts with DEADLINE_EXCEEDED without extra attempts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_d_deadline_expires_waiting_for_connection_returns_deadline_exceeded() {
    let reserved = reserve_loopback();
    let addr = reserved.addr();
    let executions = Arc::new(AtomicUsize::new(0));

    let channel = Channel::connect_lazy(addr).expect("connect_lazy");
    let client = GreeterClient::new(channel);

    let mut request = Request::new(req("scenario_d2"));
    request.set_wait_for_ready(true);
    request.set_timeout(Duration::from_millis(60));

    let result = client.say_hello(request).await;
    assert!(result.is_err(), "Expected deadline exceeded");
    let status = result.unwrap_err();
    assert_eq!(status.code(), Code::DeadlineExceeded);

    // Now start the listener. The expired client call must NOT make extra attempts.
    let listener = reserved.listen();
    let _guard = serve_on(
        listener,
        CountingGreeter::new(Arc::clone(&executions)),
        ServerConfig::default(),
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "No attempts should have reached the server after deadline expired"
    );
}

/// Scenario D3: Deadline expires while waiting for an HTTP/2 stream slot.
/// Verifies call aborts with DEADLINE_EXCEEDED without extra attempts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_d_deadline_expires_waiting_for_slot_returns_deadline_exceeded() {
    struct SlotBlockingGreeter {
        started_tx: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
        release_rx: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
        call2_executions: Arc<AtomicUsize>,
    }

    impl Greeter for SlotBlockingGreeter {
        async fn say_hello(
            &self,
            _request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            self.call2_executions.fetch_add(1, Ordering::SeqCst);
            Ok(Response::new(reply("call2_done")))
        }

        async fn client_hello(
            &self,
            request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<HelloReply>, Status> {
            let mut inbound = request.into_inner();
            if let Some(_msg) = inbound.message().await? {
                if let Some(tx) = self.started_tx.lock().await.take() {
                    let _ = tx.send(());
                }
                if let Some(rx) = self.release_rx.lock().await.take() {
                    let _ = rx.await;
                }
            }
            Ok(Response::new(reply("call1_done")))
        }

        async fn server_hello(
            &self,
            _request: Request<HelloRequest>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            Err(Status::unimplemented("unused"))
        }
    }

    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let call2_executions = Arc::new(AtomicUsize::new(0));

    let greeter = SlotBlockingGreeter {
        started_tx: Arc::new(tokio::sync::Mutex::new(Some(started_tx))),
        release_rx: Arc::new(tokio::sync::Mutex::new(Some(release_rx))),
        call2_executions: Arc::clone(&call2_executions),
    };

    // Server limits concurrent streams per connection to 1:
    let (addr, _guard) = serve(greeter, ServerConfig::new().max_concurrent_streams(1))
        .await
        .expect("serve");

    let channel = Channel::connect_with(addr, ChannelConfig::new().connections(1))
        .await
        .expect("connect");
    let client = GreeterClient::new(channel);

    // Call 1: occupies the sole stream slot by sending on an active stream without half-closing
    let (tx1, call1) = client.client_hello(Request::new(()));
    let call1_task = tokio::spawn(async move { call1.await });
    tx1.send(req("call1_blocking")).await.expect("send msg 1");

    // Wait until Call 1 is active on the server
    started_rx.await.expect("call 1 started");

    // Call 2: issued on the same connection, needs a stream slot, but cap is 1 and stream 1 is active
    let mut req2 = Request::new(req("call2"));
    req2.set_timeout(Duration::from_millis(50));
    let call2_result = client.say_hello(req2).await;

    // Call 2 must fail with DeadlineExceeded while waiting for a stream slot
    assert!(
        call2_result.is_err(),
        "Expected call 2 to fail with deadline exceeded"
    );
    let status2 = call2_result.unwrap_err();
    assert_eq!(status2.code(), Code::DeadlineExceeded);

    // Release Call 1 to complete cleanly
    let _ = release_tx.send(());
    tx1.close();
    let call1_result = call1_task.await.expect("call 1 task").expect("call 1 ok");
    assert_eq!(name_of(call1_result.get_ref()), "call1_done");

    // Call 2 was never executed on the server
    assert_eq!(
        call2_executions.load(Ordering::SeqCst),
        0,
        "Call 2 must not have executed on server after slot wait timed out"
    );
}
