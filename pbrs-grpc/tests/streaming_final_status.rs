//! Final gRPC status is mandatory on responses, but not request half-closes.
//!
//! The peer is the independently locked registry h2 implementation. Message
//! bytes below are fixed protobuf/framing fixtures, not the native encoder.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "bounded transport integration fixtures"
)]

use bytes::Bytes;
use http::{HeaderMap, Method, Response as HttpResponse};
use pbrs_grpc::hello::{Greeter, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Channel, Code, Request, Response, Server, Status, Streaming};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};

const UNARY: &str = "/helloworld.Greeter/SayHello";
const CLIENT_STREAM: &str = "/helloworld.Greeter/ClientHello";
const SERVER_STREAM: &str = "/helloworld.Greeter/ServerHello";
const BIDI: &str = "/helloworld.Greeter/StreamHello";
const REPLY: &[u8] = b"\0\0\0\0\x07\n\x05reply";
const FIRST: &[u8] = b"\0\0\0\0\x07\n\x05first";
const SECOND: &[u8] = b"\0\0\0\0\x08\n\x06second";

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("fixture operation completed within five seconds")
}

fn request() -> HelloRequest {
    let mut request = HelloRequest::new();
    request.set_name("request");
    request
}

fn named_reply(value: String) -> HelloReply {
    let mut reply = HelloReply::new();
    reply.set_message(value);
    reply
}

fn assert_reply(reply: HelloReply, value: &str) {
    assert_eq!(reply.message().to_str().expect("UTF-8 reply"), value);
}

fn assert_missing(status: &Status) {
    assert_eq!(status.code(), Code::Unknown);
    assert_eq!(status.message(), "missing grpc-status");
}

#[derive(Clone, Copy)]
enum Terminal {
    Missing,
    Ok,
    RichError,
    DeadlineExceeded,
    Malformed,
    HeadersOnlyOk,
    HeadersOnlyMissing,
}

struct Reply {
    path: &'static str,
    chunks: Vec<Bytes>,
    terminal: Terminal,
    // Deliver initial headers before the application permits the empty DATA
    // half-close. Otherwise h2 may already classify it as headers-only, which
    // is an existing, distinct rejection path.
    hold_data: bool,
}

impl Reply {
    fn missing(path: &'static str) -> Self {
        Self {
            path,
            chunks: vec![Bytes::from_static(REPLY)],
            terminal: Terminal::Missing,
            hold_data: false,
        }
    }

    fn ok(path: &'static str) -> Self {
        Self {
            terminal: Terminal::Ok,
            ..Self::missing(path)
        }
    }
}

#[derive(Default)]
struct Observed {
    calls: AtomicUsize,
    half_closes: AtomicUsize,
    responses: AtomicUsize,
    permit_data: Notify,
}

struct Peer {
    task: Option<JoinHandle<()>>,
    observed: Arc<Observed>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            // Also covers assertion failures in the test. Dropping the
            // driver's JoinSet aborts all of its owned request handlers.
            task.abort();
        }
    }
}

impl Peer {
    async fn start(replies: Vec<Reply>) -> (Channel, Self) {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let observed = Arc::new(Observed::default());
        let state = observed.clone();
        let task = tokio::spawn(async move {
            let mut connection = h2::server::handshake(server).await.expect("raw handshake");
            let mut replies = replies.into_iter();
            let mut handlers = JoinSet::new();
            loop {
                tokio::select! {
                    completed = handlers.join_next(), if !handlers.is_empty() => {
                        completed.expect("handler result").expect("raw handler did not panic");
                    }
                    incoming = connection.accept() => {
                        let Some(incoming) = incoming else {
                            break;
                        };
                        let (request, mut respond) = incoming.expect("raw incoming request");
                        let reply = replies.next().expect("only the expected actual requests");
                        assert_eq!(request.method(), Method::POST);
                        assert_eq!(request.uri().path(), reply.path);
                        assert_eq!(request.headers().get("content-type").expect("gRPC type"), "application/grpc");
                        state.calls.fetch_add(1, Ordering::SeqCst);
                        let state = state.clone();
                        handlers.spawn(async move {
                            let mut recv = request.into_body();
                            while let Some(chunk) = recv.data().await {
                                let chunk = chunk.expect("request DATA");
                                recv.flow_control().release_capacity(chunk.len()).expect("request credit");
                            }
                            assert!(recv.trailers().await.expect("request trailers").is_none());
                            state.half_closes.fetch_add(1, Ordering::SeqCst);
                            let headers_only = matches!(reply.terminal, Terminal::HeadersOnlyOk | Terminal::HeadersOnlyMissing);
                            let mut response = HttpResponse::builder()
                                .header("content-type", "application/grpc")
                                .header("x-initial", "kept");
                            if matches!(reply.terminal, Terminal::HeadersOnlyOk) {
                                response = response.header("grpc-status", "0");
                            }
                            let mut send = respond.send_response(response.body(()).expect("headers"), headers_only).expect("send headers");
                            if !headers_only {
                                if reply.hold_data {
                                    state.permit_data.notified().await;
                                }
                                let missing = matches!(reply.terminal, Terminal::Missing);
                                let count = reply.chunks.len();
                                assert!(count != 0, "body reply contains at least an empty DATA chunk");
                                for (index, chunk) in reply.chunks.into_iter().enumerate() {
                                    send.send_data(chunk, missing && index + 1 == count).expect("send response DATA");
                                }
                                if !missing {
                                    let mut trailers = HeaderMap::new();
                                    trailers.insert("x-terminal", "kept".parse().expect("metadata"));
                                    trailers.insert("x-terminal-bin", "AAH/".parse().expect("binary metadata"));
                                    if matches!(reply.terminal, Terminal::RichError) {
                                        trailers.insert("grpc-status", "3".parse().expect("status"));
                                        trailers.insert("grpc-message", "bad%20input".parse().expect("message"));
                                        trailers.insert("grpc-status-details-bin", "CAE=".parse().expect("details"));
                                    } else if matches!(reply.terminal, Terminal::DeadlineExceeded) {
                                        trailers.insert("grpc-status", "4".parse().expect("status"));
                                        trailers.insert("grpc-message", "deadline%20mid-frame".parse().expect("message"));
                                    } else if matches!(reply.terminal, Terminal::Malformed) {
                                        trailers.insert("grpc-status", "invalid".parse().expect("status"));
                                    } else {
                                        trailers.insert("grpc-status", "0".parse().expect("status"));
                                    }
                                    send.send_trailers(trailers).expect("send trailers");
                                }
                            }
                            state.responses.fetch_add(1, Ordering::SeqCst);
                        });
                    }
                }
            }
            while let Some(result) = handlers.join_next().await {
                result.expect("raw handler did not panic");
            }
        });
        let peer = Self {
            task: Some(task),
            observed,
        };
        let channel = bounded(Channel::from_io(client, "localhost"))
            .await
            .expect("native handshake with independent raw peer");
        (channel, peer)
    }

    async fn finish(mut self, expected: usize) {
        assert_eq!(self.observed.calls.load(Ordering::SeqCst), expected);
        assert_eq!(self.observed.half_closes.load(Ordering::SeqCst), expected);
        assert_eq!(self.observed.responses.load(Ordering::SeqCst), expected);
        let task = self.task.take().expect("owned raw driver");
        task.abort();
        match bounded(task).await {
            Ok(()) => {}
            Err(error) => assert!(error.is_cancelled(), "raw driver failed: {error}"),
        }
    }
}

async fn server_stream(channel: &Channel) -> Streaming<HelloReply> {
    bounded(
        channel
            .server_streaming::<HelloRequest, HelloReply>(SERVER_STREAM, Request::new(request())),
    )
    .await
    .expect("response headers")
    .into_inner()
}

#[tokio::test]
async fn server_streaming_missing_status_is_unknown() {
    let (channel, peer) = Peer::start(vec![Reply::missing(SERVER_STREAM)]).await;
    let mut stream = server_stream(&channel).await;
    assert_reply(
        bounded(stream.message())
            .await
            .expect("message")
            .expect("reply"),
        "reply",
    );
    assert_missing(
        &bounded(stream.message())
            .await
            .expect_err("response EOF needs status"),
    );
    assert!(
        bounded(stream.message())
            .await
            .expect("fused stream")
            .is_none()
    );
    peer.finish(1).await;
}

#[tokio::test]
async fn bidi_missing_status_is_unknown() {
    let (channel, peer) = Peer::start(vec![Reply::missing(BIDI)]).await;
    let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(BIDI, Request::new(()));
    bounded(sender.send(request())).await.expect("send request");
    sender.close();
    let mut stream = bounded(call).await.expect("response headers").into_inner();
    assert_reply(
        bounded(stream.message())
            .await
            .expect("message")
            .expect("reply"),
        "reply",
    );
    assert_missing(
        &bounded(stream.message())
            .await
            .expect_err("response EOF needs status"),
    );
    assert!(
        bounded(stream.message())
            .await
            .expect("fused stream")
            .is_none()
    );
    peer.finish(1).await;
}

#[tokio::test]
async fn unary_and_client_streaming_already_reject_missing_status() {
    let (channel, peer) =
        Peer::start(vec![Reply::missing(UNARY), Reply::missing(CLIENT_STREAM)]).await;
    let result =
        bounded(channel.unary::<HelloRequest, HelloReply>(UNARY, Request::new(request()))).await;
    assert_missing(&result.expect_err("unary requires status"));
    let (sender, call) =
        channel.client_streaming::<HelloRequest, HelloReply>(CLIENT_STREAM, Request::new(()));
    bounded(sender.send(request())).await.expect("send request");
    sender.close();
    assert_missing(
        &bounded(call)
            .await
            .expect_err("client-streaming requires status"),
    );
    peer.finish(2).await;
}

#[tokio::test]
async fn empty_data_response_eof_requires_status() {
    let mut reply = Reply::missing(SERVER_STREAM);
    reply.chunks = vec![Bytes::new()];
    reply.hold_data = true;
    let (channel, peer) = Peer::start(vec![reply]).await;
    let mut stream = server_stream(&channel).await;
    peer.observed.permit_data.notify_one();
    assert_missing(
        &bounded(stream.message())
            .await
            .expect_err("empty DATA EOF needs status"),
    );
    assert!(
        bounded(stream.message())
            .await
            .expect("fused stream")
            .is_none()
    );
    peer.finish(1).await;
}

#[tokio::test]
async fn trailers_drain_requires_response_status() {
    let (channel, peer) = Peer::start(vec![Reply::missing(SERVER_STREAM)]).await;
    let mut stream = server_stream(&channel).await;
    assert_missing(
        &bounded(stream.trailers())
            .await
            .expect_err("drained response needs status"),
    );
    assert!(
        bounded(stream.message())
            .await
            .expect("fused stream")
            .is_none()
    );
    peer.finish(1).await;
}

#[tokio::test]
async fn explicit_ok_preserves_fragmented_messages_and_metadata() {
    let mut reply = Reply::ok(SERVER_STREAM);
    reply.chunks = vec![
        Bytes::from_static(b"\0\0"),
        Bytes::from_static(b"\0\0\x07\n"),
        Bytes::from_static(b"\x05first"),
        Bytes::from_static(SECOND),
    ];
    let (channel, peer) = Peer::start(vec![reply]).await;
    let response = bounded(
        channel
            .server_streaming::<HelloRequest, HelloReply>(SERVER_STREAM, Request::new(request())),
    )
    .await
    .expect("headers");
    assert_eq!(response.metadata().get("x-initial"), Some("kept"));
    let mut stream = response.into_inner();
    for expected in ["first", "second"] {
        assert_reply(
            bounded(stream.message())
                .await
                .expect("message")
                .expect("reply"),
            expected,
        );
    }
    assert!(
        bounded(stream.message())
            .await
            .expect("explicit OK")
            .is_none()
    );
    let metadata = bounded(stream.trailers()).await.expect("trailing metadata");
    assert_eq!(metadata.get("x-terminal"), Some("kept"));
    assert_eq!(
        metadata.get_bin("x-terminal-bin").as_deref(),
        Some([0, 1, 255].as_slice())
    );
    peer.finish(1).await;
}

#[tokio::test]
async fn explicit_error_preserves_prior_messages_details_and_metadata() {
    let mut reply = Reply::ok(SERVER_STREAM);
    reply.chunks = vec![Bytes::from_static(FIRST)];
    reply.terminal = Terminal::RichError;
    let (channel, peer) = Peer::start(vec![reply]).await;
    let mut stream = server_stream(&channel).await;
    assert_reply(
        bounded(stream.message())
            .await
            .expect("message")
            .expect("reply"),
        "first",
    );
    let status = bounded(stream.message())
        .await
        .expect_err("explicit peer error");
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(status.message(), "bad input");
    assert_eq!(status.details(), [8, 1]);
    assert_eq!(status.metadata().get("x-terminal"), Some("kept"));
    assert_eq!(
        status.metadata().get_bin("x-terminal-bin").as_deref(),
        Some([0, 1, 255].as_slice())
    );
    assert!(
        bounded(stream.message())
            .await
            .expect("fused error")
            .is_none()
    );
    peer.finish(1).await;
}

#[tokio::test]
async fn valid_trailers_only_ok_remains_clean_for_both_streaming_shapes() {
    let replies = [SERVER_STREAM, BIDI]
        .into_iter()
        .map(|path| Reply {
            terminal: Terminal::HeadersOnlyOk,
            ..Reply::missing(path)
        })
        .collect();
    let (channel, peer) = Peer::start(replies).await;
    let mut stream = server_stream(&channel).await;
    assert!(
        bounded(stream.message())
            .await
            .expect("trailers-only OK")
            .is_none()
    );
    assert!(
        bounded(stream.trailers())
            .await
            .expect("trailers-only metadata")
            .is_empty()
    );
    let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(BIDI, Request::new(()));
    sender.close();
    let mut stream = bounded(call)
        .await
        .expect("trailers-only headers")
        .into_inner();
    assert!(
        bounded(stream.message())
            .await
            .expect("trailers-only OK")
            .is_none()
    );
    peer.finish(2).await;
}

#[tokio::test]
async fn header_only_missing_status_is_rejected_at_call_completion() {
    let reply = Reply {
        terminal: Terminal::HeadersOnlyMissing,
        ..Reply::missing(SERVER_STREAM)
    };
    let (channel, peer) = Peer::start(vec![reply]).await;
    let result = bounded(
        channel
            .server_streaming::<HelloRequest, HelloReply>(SERVER_STREAM, Request::new(request())),
    )
    .await;
    assert_missing(&result.expect_err("headers-only needs status"));
    peer.finish(1).await;
}

#[tokio::test]
async fn malformed_frame_and_message_cap_precede_missing_status() {
    for bytes in [b"\0\0".as_slice(), b"\0\0\0\0\x07\n\x05rep".as_slice()] {
        let mut reply = Reply::missing(SERVER_STREAM);
        reply.chunks = vec![Bytes::copy_from_slice(bytes)];
        let (channel, peer) = Peer::start(vec![reply]).await;
        let mut stream = server_stream(&channel).await;
        assert_eq!(
            bounded(stream.message())
                .await
                .expect_err("incomplete frame")
                .code(),
            Code::Internal
        );
        peer.finish(1).await;
    }
    let (channel, peer) = Peer::start(vec![Reply::missing(SERVER_STREAM)]).await;
    let channel = channel.max_decoding_message_size(1);
    let mut stream = server_stream(&channel).await;
    assert_eq!(
        bounded(stream.message())
            .await
            .expect_err("native message cap")
            .code(),
        Code::ResourceExhausted
    );
    peer.finish(1).await;
}

struct HalfCloseEcho {
    completed: Arc<AtomicUsize>,
}

impl HalfCloseEcho {
    async fn drain(&self, mut stream: Streaming<HelloRequest>) -> HelloReply {
        let mut count = 0;
        while let Some(request) = stream
            .message()
            .await
            .expect("request message or clean half-close")
        {
            assert_eq!(request.name().to_str().expect("request UTF-8"), "request");
            count += 1;
        }
        self.completed.fetch_add(1, Ordering::SeqCst);
        named_reply(count.to_string())
    }
}

impl Greeter for HalfCloseEcho {
    async fn say_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Err(Status::unimplemented("unused fixture shape"))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(self.drain(request.into_inner()).await))
    }

    async fn server_hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        Err(Status::unimplemented("unused fixture shape"))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let reply = self.drain(request.into_inner()).await;
        let (sender, stream) = Streaming::channel(1);
        sender.send(reply).await?;
        sender.close();
        Ok(Response::new(stream))
    }
}

#[tokio::test]
async fn native_request_half_closes_remain_accepted() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let completed = Arc::new(AtomicUsize::new(0));
    let observed = completed.clone();
    let task = tokio::spawn(async move {
        Server::new(GreeterServer::new(HalfCloseEcho { completed }))
            .serve_connection(server)
            .await
            .expect("native fixture server");
    });
    let peer = Peer {
        task: Some(task),
        observed: Arc::new(Observed::default()),
    };
    let channel = bounded(Channel::from_io(client, "localhost"))
        .await
        .expect("native handshake");
    for count in [0, 2] {
        let (sender, call) =
            channel.client_streaming::<HelloRequest, HelloReply>(CLIENT_STREAM, Request::new(()));
        for _ in 0..count {
            bounded(sender.send(request())).await.expect("request");
        }
        sender.close();
        assert_reply(
            bounded(call)
                .await
                .expect("client-streaming half-close")
                .into_inner(),
            &count.to_string(),
        );
        let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(BIDI, Request::new(()));
        for _ in 0..count {
            bounded(sender.send(request())).await.expect("request");
        }
        sender.close();
        let mut stream = bounded(call).await.expect("bidi half-close").into_inner();
        assert_reply(
            bounded(stream.message())
                .await
                .expect("message")
                .expect("reply"),
            &count.to_string(),
        );
        assert!(
            bounded(stream.message())
                .await
                .expect("server final status")
                .is_none()
        );
    }
    assert_eq!(observed.load(Ordering::SeqCst), 4);
    peer.finish(0).await;
}

#[tokio::test]
async fn terminal_error_drop_returns_rpc_and_byte_permits_for_recovery() {
    let (channel, peer) = Peer::start(vec![Reply::missing(SERVER_STREAM), Reply::ok(UNARY)]).await;
    let channel = channel.max_concurrent_rpcs(1).byte_budget(256);
    let tracker = channel.byte_budget_tracker().clone();
    let mut stream = server_stream(&channel).await;
    assert_reply(
        bounded(stream.message())
            .await
            .expect("message")
            .expect("reply"),
        "reply",
    );
    assert_missing(
        &bounded(stream.message())
            .await
            .expect_err("missing status is terminal error"),
    );
    // The documented RPC slot belongs to the received stream until Drop.
    let rejected =
        bounded(channel.unary::<HelloRequest, HelloReply>(UNARY, Request::new(request()))).await;
    assert_eq!(
        rejected
            .expect_err("live stream still owns its RPC slot")
            .code(),
        Code::ResourceExhausted
    );
    drop(stream);
    assert_eq!(tracker.allocated(), 0);
    assert_eq!(tracker.active_byte_permit_tokens(), 0);
    assert!(
        tracker.peak_allocated() > 0,
        "actual request bytes were accounted"
    );
    let response =
        bounded(channel.unary::<HelloRequest, HelloReply>(UNARY, Request::new(request())))
            .await
            .expect("same-connection recovery after Drop");
    assert_reply(response.into_inner(), "reply");
    assert_eq!(tracker.allocated(), 0);
    assert_eq!(tracker.active_byte_permit_tokens(), 0);
    peer.finish(2).await;
}

async fn terminal_for_shape(channel: &Channel, path: &'static str) -> Status {
    if path == UNARY {
        return bounded(channel.unary::<HelloRequest, HelloReply>(path, Request::new(request())))
            .await
            .expect_err("partial unary response must fail");
    }
    if path == CLIENT_STREAM {
        let (sender, call) =
            channel.client_streaming::<HelloRequest, HelloReply>(path, Request::new(()));
        bounded(sender.send(request())).await.expect("send request");
        sender.close();
        return bounded(call)
            .await
            .expect_err("partial upload response must fail");
    }
    let mut stream = if path == SERVER_STREAM {
        server_stream(channel).await
    } else {
        let (sender, call) = channel.bidi::<HelloRequest, HelloReply>(path, Request::new(()));
        bounded(sender.send(request())).await.expect("send request");
        sender.close();
        bounded(call).await.expect("bidi headers").into_inner()
    };
    let status = bounded(stream.message())
        .await
        .expect_err("partial response must fail");
    assert!(
        bounded(stream.message())
            .await
            .expect("fused error")
            .is_none()
    );
    status
}

#[tokio::test]
async fn partial_response_preserves_explicit_peer_errors_for_all_shapes() {
    for partial in [b"\0\0".as_slice(), b"\0\0\0\0\x40\n".as_slice()] {
        for (terminal, code, message) in [
            (Terminal::RichError, Code::InvalidArgument, "bad input"),
            (
                Terminal::DeadlineExceeded,
                Code::DeadlineExceeded,
                "deadline mid-frame",
            ),
        ] {
            for path in [UNARY, CLIENT_STREAM, SERVER_STREAM, BIDI] {
                let reply = Reply {
                    path,
                    chunks: vec![Bytes::copy_from_slice(partial)],
                    terminal,
                    hold_data: false,
                };
                let (channel, peer) = Peer::start(vec![reply]).await;
                let status = terminal_for_shape(&channel, path).await;
                assert_eq!(status.code(), code, "path {path}");
                assert_eq!(status.message(), message);
                assert_eq!(status.metadata().get("x-terminal"), Some("kept"));
                if matches!(terminal, Terminal::RichError) {
                    assert_eq!(status.details(), [8, 1]);
                }
                peer.finish(1).await;
            }
        }
    }
}

#[tokio::test]
async fn partial_response_with_ok_missing_or_malformed_status_is_still_invalid() {
    for partial in [b"\0\0".as_slice(), b"\0\0\0\0\x40\n".as_slice()] {
        for terminal in [Terminal::Ok, Terminal::Missing, Terminal::Malformed] {
            for path in [UNARY, CLIENT_STREAM, SERVER_STREAM, BIDI] {
                let reply = Reply {
                    path,
                    chunks: vec![Bytes::copy_from_slice(partial)],
                    terminal,
                    hold_data: false,
                };
                let (channel, peer) = Peer::start(vec![reply]).await;
                let status = terminal_for_shape(&channel, path).await;
                assert_eq!(status.code(), Code::Internal, "path {path}");
                assert_eq!(status.message(), "truncated gRPC frame");
                peer.finish(1).await;
            }
        }
    }
}
