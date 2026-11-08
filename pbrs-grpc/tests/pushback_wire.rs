//! A6 pushback carried by HTTP/2 gRPC status blocks.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "local HTTP/2 peer and regression assertions"
)]

use pbrs_grpc::{Channel, Code, HelloReply, HelloRequest, Pushback, Request};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const POLICY: &str = r#"{"methodConfig":[{"name":[{}],"retryPolicy":{
    "maxAttempts":2,"initialBackoff":"0.001s","maxBackoff":"0.001s",
    "backoffMultiplier":1,"retryableStatusCodes":["UNAVAILABLE"]}}]}"#;

async fn peer_response(values: &[&str], trailers_only: bool) -> (pbrs_grpc::Status, usize) {
    let (client, peer) = tokio::io::duplex(64 * 1024);
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    let values: Vec<http::HeaderValue> = values
        .iter()
        .map(|value| http::HeaderValue::from_bytes(value.as_bytes()).expect("header value"))
        .collect();
    let server = tokio::spawn(async move {
        let mut connection = h2::server::handshake(peer).await.expect("handshake");
        let mut handlers = tokio::task::JoinSet::new();
        while let Some(incoming) = connection.accept().await {
            let (request, mut respond) = incoming.expect("request");
            seen.fetch_add(1, Ordering::SeqCst);
            let values = values.clone();
            handlers.spawn(async move {
                let mut body = request.into_body();
                while let Some(chunk) = body.data().await {
                    let chunk = chunk.expect("request DATA");
                    body.flow_control()
                        .release_capacity(chunk.len())
                        .expect("release credit");
                }
                let mut status_block = http::HeaderMap::new();
                status_block.insert("grpc-status", http::HeaderValue::from_static("14"));
                for value in values {
                    status_block.append("grpc-retry-pushback-ms", value);
                }
                let mut response = http::Response::builder()
                    .status(200)
                    .header("content-type", "application/grpc")
                    .body(())
                    .expect("response");
                if trailers_only {
                    response.headers_mut().extend(status_block);
                    respond
                        .send_response(response, true)
                        .expect("trailers-only");
                } else {
                    let mut send = respond.send_response(response, false).expect("headers");
                    send.send_trailers(status_block).expect("trailers");
                }
            });
        }
    });
    let channel = tokio::time::timeout(
        Duration::from_secs(5),
        Channel::from_io(client, "localhost"),
    )
    .await
    .expect("channel timeout")
    .expect("channel")
    .service_config(POLICY)
    .expect("retry policy");
    let mut request = Request::new(HelloRequest::new());
    request.set_timeout(Duration::from_secs(5));
    let status = tokio::time::timeout(
        Duration::from_secs(7),
        channel.unary::<HelloRequest, HelloReply>("/test.Service/Pushback", request),
    )
    .await
    .expect("call timeout")
    .expect_err("peer sends UNAVAILABLE");
    server.abort();
    (status, attempts.load(Ordering::SeqCst))
}

#[tokio::test]
async fn pushback_on_wire_controls_policy_retries() {
    let cases: &[(&[&str], Option<Pushback>, usize)] = &[
        (&["-1"], Some(Pushback::DoNotRetry), 1),
        (&["-2"], Some(Pushback::DoNotRetry), 1),
        (&["bad"], Some(Pushback::DoNotRetry), 1),
        (&["é"], Some(Pushback::DoNotRetry), 1),
        (&["2147483648"], Some(Pushback::DoNotRetry), 1),
        (&["0", "1"], Some(Pushback::DoNotRetry), 1),
        (&["-2", "0"], Some(Pushback::DoNotRetry), 1),
        (&["0", "-2"], Some(Pushback::DoNotRetry), 1),
        (&["0"], Some(Pushback::Delay(Duration::ZERO)), 2),
        (&["+1"], Some(Pushback::Delay(Duration::from_millis(1))), 2),
        (&[], None, 2),
    ];

    for &(values, expected, attempts) in cases {
        let (status, actual_attempts) = peer_response(values, true).await;
        assert_eq!(status.code(), Code::Unavailable, "values={values:?}");
        assert_eq!(status.retry_pushback(), expected, "values={values:?}");
        assert_eq!(actual_attempts, attempts, "values={values:?}");
    }
}

#[tokio::test]
async fn pushback_in_trailers_remains_visible_after_response_commitment() {
    let cases: &[(&[&str], Option<Pushback>)] = &[
        (&["-2"], Some(Pushback::DoNotRetry)),
        (&["bad"], Some(Pushback::DoNotRetry)),
        (&["é"], Some(Pushback::DoNotRetry)),
        (&["2147483648"], Some(Pushback::DoNotRetry)),
        (&["0", "1"], Some(Pushback::DoNotRetry)),
        (&["0"], Some(Pushback::Delay(Duration::ZERO))),
        (
            &["2147483647"],
            Some(Pushback::Delay(Duration::from_millis(2_147_483_647))),
        ),
        (&[], None),
    ];

    for &(values, expected) in cases {
        let (status, attempts) = peer_response(values, false).await;
        assert_eq!(status.code(), Code::Unavailable, "values={values:?}");
        assert_eq!(status.retry_pushback(), expected, "values={values:?}");
        assert_eq!(attempts, 1, "headers commit the call; values={values:?}");
    }
}
