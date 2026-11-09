//! A failed gRPC-Web terminal write must not count as a successful RPC.

#![cfg(feature = "grpc-web")]
#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "end-to-end local HTTP/2 assertions"
)]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use pbrs_grpc::channelz::{Channelz, ChannelzService, GetServersRequest};
use pbrs_grpc::telemetry::{CallLabels, LifecycleObserver};
use pbrs_grpc::{
    Code, HelloReply, HelloRequest, Request, Response, Rpc, Server, ServerConfig, Service, Status,
    Streaming,
};
use std::time::Duration;
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(15);
struct EmptyService<const ZERO: bool>;

impl<const ZERO: bool> Service for EmptyService<ZERO> {
    const NAME: &'static str = if ZERO {
        "test.terminal.Zero"
    } else {
        "test.terminal.Positive"
    };

    async fn call(&self, rpc: Rpc) {
        rpc.server_streaming(|_: Request<HelloRequest>| async move {
            let (sender, stream) = Streaming::<HelloReply>::channel(1);
            drop(sender);
            Ok::<_, Status>(Response::new(stream))
        })
        .await;
    }
}

#[derive(Clone)]
struct EndObserver(mpsc::UnboundedSender<Code>);

impl LifecycleObserver for EndObserver {
    fn on_server_call_end(&self, _: &CallLabels<'_>, status: &Status, _: Duration) {
        let _sent = self.0.send(status.code());
    }
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn verify_response(response: http::Response<h2::RecvStream>, content_type: &str) {
    assert_eq!(response.status(), http::StatusCode::OK);
    let mut body = response.into_body();
    let mut data = Vec::new();
    while let Some(chunk) = tokio::time::timeout(WAIT, body.data())
        .await
        .expect("response body timeout")
    {
        let chunk = chunk.expect("response data");
        data.extend_from_slice(&chunk);
        body.flow_control()
            .release_capacity(chunk.len())
            .expect("release response credit");
    }
    let trailers = tokio::time::timeout(WAIT, body.trailers())
        .await
        .expect("response trailers timeout")
        .expect("response trailers");
    if content_type == "application/grpc" {
        assert!(data.is_empty(), "empty native stream");
        assert_eq!(
            trailers
                .expect("native trailers")
                .get("grpc-status")
                .and_then(|value| value.to_str().ok()),
            Some("0")
        );
    } else {
        assert!(trailers.is_none(), "Web trailers are in the body");
        let bytes = if content_type == "application/grpc-web-text" {
            STANDARD.decode(data).expect("Web text base64")
        } else {
            data
        };
        assert!(bytes.len() >= 5, "Web trailer frame");
        assert_eq!(bytes[0], 0x80, "Web trailer flag");
        let size = u32::from_be_bytes(bytes[1..5].try_into().expect("frame length")) as usize;
        assert_eq!(bytes.len(), 5 + size, "one complete Web trailer frame");
        assert!(
            std::str::from_utf8(&bytes[5..])
                .expect("trailer text")
                .contains("grpc-status: 0\r\n"),
            "successful Web trailer"
        );
    }
}

async fn exercise_server(buffer: usize) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("address");
    let (end_tx, mut end_rx) = mpsc::unbounded_channel();
    let service_name = if buffer == 0 {
        "test.terminal.Zero"
    } else {
        "test.terminal.Positive"
    };
    let task = if buffer == 0 {
        tokio::spawn(async move {
            Server::new(EmptyService::<true>)
                .config(ServerConfig::new().max_send_buffer_size(buffer))
                .observer(EndObserver(end_tx))
                .serve_listener(listener)
                .await
                .expect("server");
        })
    } else {
        tokio::spawn(async move {
            Server::new(EmptyService::<false>)
                .config(ServerConfig::new().max_send_buffer_size(buffer))
                .observer(EndObserver(end_tx))
                .serve_listener(listener)
                .await
                .expect("server");
        })
    };
    let mut server = AbortOnDrop(task);

    let socket = tokio::time::timeout(WAIT, tokio::net::TcpStream::connect(addr))
        .await
        .expect("connect timeout")
        .expect("connect");
    let (mut client, connection) = tokio::time::timeout(WAIT, h2::client::handshake(socket))
        .await
        .expect("handshake timeout")
        .expect("handshake");
    let mut driver = AbortOnDrop(tokio::spawn(async move {
        let _result = connection.await;
    }));

    // Native HEADERS trailers work with zero DATA buffer. Web body trailers
    // do not; with a positive buffer, the same Web responses succeed.
    let cases: &[(&str, &[u8], Code)] = if buffer == 0 {
        &[
            ("application/grpc", b"\0\0\0\0\0", Code::Ok),
            ("application/grpc-web", b"\0\0\0\0\0", Code::InvalidArgument),
            (
                "application/grpc-web-text",
                b"AAAAAAA=",
                Code::InvalidArgument,
            ),
        ]
    } else {
        &[
            ("application/grpc-web", b"\0\0\0\0\0", Code::Ok),
            ("application/grpc-web-text", b"AAAAAAA=", Code::Ok),
        ]
    };
    for &(content_type, payload, expected) in cases {
        client = tokio::time::timeout(WAIT, client.ready())
            .await
            .expect("ready timeout")
            .expect("ready");
        let request = http::Request::builder()
            .method(http::Method::POST)
            .uri(format!("http://{addr}/{service_name}/Stream"))
            .header(http::header::CONTENT_TYPE, content_type)
            .header(http::header::TE, "trailers")
            .body(())
            .expect("request");
        let (response, mut upload) = client
            .send_request(request, false)
            .expect("request headers");
        upload
            .send_data(Bytes::copy_from_slice(payload), true)
            .expect("request body");
        let actual = tokio::time::timeout(WAIT, end_rx.recv())
            .await
            .expect("call end timeout")
            .expect("observer closed");
        assert_eq!(
            actual, expected,
            "content type {content_type}, buffer {buffer}"
        );
        // The response future and send half stay alive until accounting ends,
        // so dropping the client cannot create the terminal failure.
        if expected == Code::Ok {
            let response = tokio::time::timeout(WAIT, response)
                .await
                .expect("response timeout")
                .expect("response");
            verify_response(response, content_type).await;
        } else {
            drop(response);
        }
        drop(upload);
    }
    assert!(end_rx.try_recv().is_err(), "exactly one end per call");

    // Query the registry directly so the query adds no RPC to the counts.
    let mut query = GetServersRequest::new();
    query.set_max_results(1000);
    let servers = ChannelzService::shared_global()
        .get_servers(Request::new(query))
        .await
        .expect("servers");
    let count = i64::try_from(cases.len()).expect("count");
    let mut listed = servers
        .get_ref()
        .server()
        .into_iter()
        .filter(|entry| entry.r#ref().name().to_str().ok() == Some(service_name));
    let entry = listed.next().expect("registered server");
    assert!(listed.next().is_none(), "one matching server");
    let data = entry.data();
    assert_eq!(data.calls_started(), count);
    assert_eq!(data.calls_succeeded(), if buffer == 0 { 1 } else { 2 });
    assert_eq!(data.calls_failed(), if buffer == 0 { 2 } else { 0 });

    // Close the client and listener before the next case. The Channelz query
    // above selected this server by its service name.
    drop(client);
    driver.0.abort();
    let _driver_aborted = (&mut driver.0).await;
    server.0.abort();
    let _server_aborted = (&mut server.0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_write_failure_is_not_counted_as_success() {
    tokio::time::timeout(Duration::from_secs(120), async {
        exercise_server(0).await;
        exercise_server(1024).await;
    })
    .await
    .expect("terminal accounting scenarios timed out");
}
