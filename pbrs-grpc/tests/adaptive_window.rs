//! Opt-in adaptive HTTP/2 receive-window coverage.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    unreachable_pub,
    reason = "integration tests"
)]

use pbrs_grpc::{
    Channel, ChannelConfig, InteropTestService, Payload, Request, ResponseParameters, ServerConfig,
    SimpleRequest, SizedInteropTestService, StreamingInputCallRequest, StreamingOutputCallRequest,
    TestServiceClient, TestServiceServer,
};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

const ONE_MIB: usize = 1024 * 1024;

struct ServerGuard(tokio::task::JoinHandle<()>);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn spawn(config: ServerConfig) -> (SocketAddr, ServerGuard) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(async move {
        TestServiceServer::new(SizedInteropTestService::new(2 * ONE_MIB))
            .config(config)
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

async fn connect(addr: SocketAddr, config: ChannelConfig) -> TestServiceClient {
    let mut last = None;
    for _ in 0..80 {
        match Channel::connect_with(addr, config).await {
            Ok(channel) => return TestServiceClient::new(channel),
            Err(status) => {
                last = Some(status);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("connect failed: {last:?}");
}

fn adaptive_server_config() -> ServerConfig {
    ServerConfig::new()
        .adaptive_window(true)
        .adaptive_window_max_size(ONE_MIB as u32)
        .keep_alive_interval(Duration::from_millis(20))
        .keep_alive_timeout(Duration::from_secs(2))
}

fn adaptive_channel_config() -> ChannelConfig {
    ChannelConfig::new()
        .adaptive_window(true)
        .adaptive_window_max_size(ONE_MIB as u32)
        .keep_alive_interval(Duration::from_millis(20))
        .keep_alive_timeout(Duration::from_secs(2))
}

fn unary_request(req_bytes: usize, resp_bytes: usize) -> SimpleRequest {
    let mut request = SimpleRequest::new();
    request.set_response_size(resp_bytes as i32);
    if req_bytes > 0 {
        let mut payload = Payload::new();
        payload.set_body(vec![0u8; req_bytes]);
        request.set_payload(payload);
    }
    request
}

fn output_request(msgs: usize, resp_bytes: usize) -> StreamingOutputCallRequest {
    let mut request = StreamingOutputCallRequest::new();
    for _ in 0..msgs {
        let mut param = ResponseParameters::new();
        param.set_size(resp_bytes as i32);
        request.response_parameters_mut().push(param);
    }
    request
}

fn input_request(req_bytes: usize) -> StreamingInputCallRequest {
    let mut request = StreamingInputCallRequest::new();
    let mut payload = Payload::new();
    payload.set_body(vec![0u8; req_bytes]);
    request.set_payload(payload);
    request
}

#[tokio::test]
async fn adaptive_client_and_server_handle_large_unary_with_keepalive() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (addr, _guard) = spawn(adaptive_server_config()).await;
        let client = connect(addr, adaptive_channel_config()).await;
        let response = client
            .unary_call(Request::new(unary_request(ONE_MIB, ONE_MIB)))
            .await
            .expect("large unary");
        assert_eq!(response.get_ref().payload().body().len(), ONE_MIB);
    })
    .await
    .expect("adaptive large unary timed out");
}

#[tokio::test]
async fn adaptive_client_and_server_handle_large_streaming_with_keepalive() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (addr, _guard) = spawn(adaptive_server_config()).await;
        let client = connect(addr, adaptive_channel_config()).await;

        let mut inbound = client
            .streaming_output_call(Request::new(output_request(4, ONE_MIB / 4)))
            .await
            .expect("server stream")
            .into_inner();
        let mut received = 0usize;
        while let Some(message) = inbound.message().await.expect("stream item") {
            received += message.payload().body().len();
        }
        assert_eq!(received, ONE_MIB);

        let (tx, call) = client.streaming_input_call(Request::new(()));
        for _ in 0..4 {
            tx.send(input_request(ONE_MIB / 4)).await.expect("send");
        }
        tx.close();
        let response = call.await.expect("client stream");
        assert_eq!(response.get_ref().aggregated_payload_size(), ONE_MIB as i32);
    })
    .await
    .expect("adaptive streaming timed out");
}

#[tokio::test]
async fn adaptive_window_is_opt_in() {
    let fixed = ServerConfig::new();
    let adaptive = ServerConfig::new().adaptive_window(true);
    assert!(!fixed.adaptive_window_enabled());
    assert!(adaptive.adaptive_window_enabled());

    let fixed = ChannelConfig::new();
    let adaptive = ChannelConfig::new().adaptive_window(true);
    assert!(!fixed.adaptive_window_enabled());
    assert!(adaptive.adaptive_window_enabled());
}

#[tokio::test]
async fn default_fixed_window_still_serves_small_rpc() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let _guard = ServerGuard(tokio::spawn(async move {
            TestServiceServer::new(InteropTestService)
                .serve_listener(listener)
                .await
                .ok();
        }));
        let client = connect(addr, ChannelConfig::new()).await;
        let response = client
            .unary_call(Request::new(unary_request(0, 0)))
            .await
            .expect("small unary");
        assert_eq!(response.get_ref().payload().body().len(), 0);
    })
    .await
    .expect("default fixed small unary timed out");
}
