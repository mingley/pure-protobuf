//! Hedged transparent redial must retain an attempt's response commitment.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "integration tests"
)]

mod common;

use bytes::Bytes;
use common::{name_of, reply, req};
use http::header::CONTENT_TYPE;
use http::{HeaderValue, StatusCode};
use pbrs::Serialize;
use pbrs_grpc::hello::GreeterClient;
use pbrs_grpc::{Channel, ChannelConfig, Code, Request, codec};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

async fn read_request_body(
    conn: &mut h2::server::Connection<tokio::net::TcpStream, Bytes>,
    body: &mut h2::RecvStream,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            tokio::select! {
                chunk = body.data() => match chunk {
                    Some(Ok(_)) => {}
                    Some(Err(error)) => panic!("request body failed: {error}"),
                    None => break,
                },
                accepted = conn.accept() => panic!("unexpected stream while reading body: {accepted:?}"),
            }
        }
    })
    .await
    .expect("request body");
}

async fn exercise_hedge_transparent_redial(after_headers: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let redials = Arc::new(AtomicUsize::new(0));
    let server_redials = Arc::clone(&redials);
    let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("first connection");
        let mut conn = h2::server::Builder::new()
            .initial_window_size(1_024)
            .handshake::<_, Bytes>(socket)
            .await
            .expect("first handshake");
        let (request, mut respond) = conn
            .accept()
            .await
            .expect("first stream")
            .expect("valid first stream");
        let mut response = if after_headers {
            let mut body = request.into_body();
            read_request_body(&mut conn, &mut body).await;
            let headers = http::Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/grpc")
                .body(())
                .expect("response");
            Some(respond.send_response(headers, false).expect("headers"))
        } else {
            respond.send_reset(h2::Reason::REFUSED_STREAM);
            drop(request);
            None
        };
        // Keep polling the connection while the send stream waits for credit.
        let mut drivers = tokio::task::JoinSet::new();
        drivers.spawn(async move { while let Some(Ok(_)) = conn.accept().await {} });
        if let Some(send) = response.as_mut() {
            send.reserve_capacity(1_024);
            let initial = tokio::time::timeout(
                Duration::from_secs(5),
                std::future::poll_fn(|cx| send.poll_capacity(cx)),
            )
            .await
            .expect("initial credit timeout")
            .expect("stream closed before initial credit")
            .expect("initial credit");
            assert_eq!(initial, 1_024);
            let mut incomplete = vec![0_u8; 1_024];
            incomplete[1..5].copy_from_slice(&4_096_u32.to_be_bytes());
            send.send_data(Bytes::from(incomplete), false)
                .expect("DATA");
            assert_eq!(send.capacity(), 0, "the first DATA must consume the window");
            send.reserve_capacity(1_024);
            let replenished = tokio::time::timeout(
                Duration::from_secs(5),
                std::future::poll_fn(|cx| send.poll_capacity(cx)),
            )
            .await
            .expect("client did not return response credit")
            .expect("stream closed before credit was returned")
            .expect("returned credit");
            assert!(replenished > 0);
            // Only consuming DATA returns this credit. The client therefore
            // processed the ordinary response headers before the reset.
            observed_tx.send(()).expect("header witness");
            send.send_reset(h2::Reason::REFUSED_STREAM);
        }

        // A wrongly sent transparent retry is answered successfully so the
        // regression fails instead of being hidden by a connection timeout.
        let (socket, _) = listener.accept().await.expect("redial connection");
        let mut conn = h2::server::handshake(socket)
            .await
            .expect("redial handshake");
        let (request, mut respond) = conn
            .accept()
            .await
            .expect("redial stream")
            .expect("valid redial stream");
        let mut body = request.into_body();
        read_request_body(&mut conn, &mut body).await;
        server_redials.fetch_add(1, Ordering::SeqCst);
        let response = http::Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/grpc")
            .body(())
            .expect("response");
        let mut send = respond.send_response(response, false).expect("headers");
        let bytes = reply("redial-success").serialize().expect("serialize");
        send.send_data(codec::encode(&bytes, false).expect("frame"), false)
            .expect("data");
        let mut trailers = http::HeaderMap::new();
        trailers.insert("grpc-status", HeaderValue::from_static("0"));
        send.send_trailers(trailers).expect("trailers");
        while let Some(Ok(_)) = conn.accept().await {}
    });

    let channel = Channel::connect_with(
        addr,
        ChannelConfig::new().initial_stream_window_size(1_024),
    )
    .await
    .expect("connect")
    .service_config(
        r#"{"methodConfig":[{"name":[{}],"hedgingPolicy":{"maxAttempts":2,"hedgingDelay":"30s"}}]}"#,
    )
    .expect("hedging policy");
    let client = GreeterClient::new(channel.clone());
    let message = if after_headers {
        "committed".to_owned()
    } else {
        "x".repeat(32 * 1_024)
    };
    let mut request = Request::new(req(&message));
    request.set_timeout(Duration::from_secs(5));
    let result = tokio::time::timeout(Duration::from_secs(7), client.say_hello(request))
        .await
        .expect("call timed out");
    if after_headers {
        tokio::time::timeout(Duration::from_secs(2), observed_rx)
            .await
            .expect("header witness timed out")
            .expect("no response consumption witness");
        let status = result.expect_err("committed reset is terminal");
        assert_eq!(status.code(), Code::Unavailable);
        assert_eq!(channel.retry_stats().transparent_retries, 0);
        assert_eq!(redials.load(Ordering::SeqCst), 0);
    } else {
        let response = result.expect("uncommitted refusal is transparently retried");
        assert_eq!(name_of(response.get_ref()), "redial-success");
        assert_eq!(channel.retry_stats().transparent_retries, 1);
        assert_eq!(redials.load(Ordering::SeqCst), 1);
    }
    assert_eq!(channel.retry_stats().hedged_sends, 0);
    assert_eq!(channel.byte_budget_allocated(), 0);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hedge_transparent_redial_after_response_headers() {
    exercise_hedge_transparent_redial(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hedge_transparent_redial_before_response_headers() {
    exercise_hedge_transparent_redial(false).await;
}
