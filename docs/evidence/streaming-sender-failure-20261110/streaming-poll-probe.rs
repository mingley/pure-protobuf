#![allow(unsafe_code, reason = "diagnostic reads the Linux process CPU clock")]
//! Counter-free loopback streaming call comparison.

use pbrs_grpc::binlog::{BinaryLogFilter, BinaryLogger, LogRecord, Sink, VecSink};
use pbrs_grpc::testing::{
    InteropTestService, Payload, ResponseParameters, StreamingInputCallRequest,
    StreamingOutputCallRequest, TestServiceClient, TestServiceServer,
};
use pbrs_grpc::{Channel, Request};
use std::sync::Arc;

#[derive(Debug)]
struct NoopSink;
impl Sink for NoopSink {
    fn emit(&self, _record: &LogRecord) {}
}

fn process_cpu_ns() -> u128 {
    let mut value = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: clock_gettime initializes the timespec on success.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, value.as_mut_ptr()) };
    assert_eq!(rc, 0, "process CPU clock unavailable");
    // SAFETY: clock_gettime succeeded above.
    let value = unsafe { value.assume_init() };
    u128::try_from(value.tv_sec).expect("seconds") * 1_000_000_000
        + u128::try_from(value.tv_nsec).expect("nanoseconds")
}

fn payload() -> Payload {
    let mut payload = Payload::new();
    payload.set_body(vec![0; 32]);
    payload
}

fn output_request(messages: usize) -> StreamingOutputCallRequest {
    let mut request = StreamingOutputCallRequest::new();
    for _ in 0..messages {
        let mut parameter = ResponseParameters::new();
        parameter.set_size(32);
        request.response_parameters_mut().push(parameter);
    }
    request
}

async fn stream_once(client: &TestServiceClient, shape: &str, messages: usize) {
    match shape {
        "client" => {
            let (sender, call) = client.streaming_input_call(Request::new(()));
            // Poll the response while sending so streams larger than the
            // sender buffer cannot deadlock.
            let task = tokio::spawn(call);
            for _ in 0..messages {
                let mut message = StreamingInputCallRequest::new();
                message.set_payload(payload());
                sender.send(message).await.expect("client send");
            }
            sender.close();
            let response = task.await.expect("client task").expect("client stream");
            assert_eq!(
                response.into_inner().aggregated_payload_size(),
                i32::try_from(messages * 32).expect("size")
            );
        }
        "server" => {
            let mut inbound = client
                .streaming_output_call(Request::new(output_request(messages)))
                .await
                .expect("server stream")
                .into_inner();
            for _ in 0..messages {
                let message = inbound.message().await.expect("server recv").expect("message");
                assert_eq!(message.payload().body().len(), 32);
            }
            assert!(inbound.message().await.expect("server end").is_none());
        }
        "bidi" => {
            let (sender, call) = client.full_duplex_call(Request::new(()));
            sender.send(output_request(1)).await.expect("first bidi send");
            let mut inbound = call.await.expect("bidi stream").into_inner();
            let first = inbound.message().await.expect("bidi recv").expect("first");
            assert_eq!(first.payload().body().len(), 32);
            for _ in 1..messages {
                sender.send(output_request(1)).await.expect("bidi send");
                let message = inbound.message().await.expect("bidi recv").expect("message");
                assert_eq!(message.payload().body().len(), 32);
            }
            sender.close();
            assert!(inbound.message().await.expect("bidi end").is_none());
        }
        _ => panic!("unknown stream shape"),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let shape = args.next().expect("shape");
    let mode = args.next().expect("logger mode");
    let messages: usize = args.next().expect("messages").parse().expect("messages integer");
    let calls: usize = args.next().expect("calls").parse().expect("calls integer");
    assert!(messages > 0);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        TestServiceServer::new(InteropTestService)
            .serve_listener(listener)
            .await
            .expect("serve");
    });
    let channel = Channel::connect(address).await.expect("connect");
    let verify = std::env::var_os("PBRS_PROBE_VERIFY").is_some();
    let sink = Arc::new(VecSink::new());
    let channel = match mode.as_str() {
        "none" => channel,
        "excluded" | "enabled" => {
            let filter = if mode == "excluded" { "" } else { "*" };
            let chosen: Arc<dyn Sink> = if verify || mode == "excluded" {
                sink.clone()
            } else {
                Arc::new(NoopSink)
            };
            channel.binary_logger(BinaryLogger::new(
                BinaryLogFilter::parse(filter).expect("filter"),
                chosen,
            ))
        }
        _ => panic!("unknown mode"),
    };
    let client = TestServiceClient::new(channel);
    let warmups = if messages <= 4 { 10 } else { 2 };
    for _ in 0..warmups {
        stream_once(&client, &shape, messages).await;
    }
    let wall = std::time::Instant::now();
    let start = process_cpu_ns();
    for _ in 0..calls {
        stream_once(&client, &shape, messages).await;
    }
    let cpu = process_cpu_ns() - start;
    let wall = wall.elapsed().as_nanos();
    if mode == "excluded" {
        assert!(sink.is_empty(), "excluded method emitted a binary-log record");
    } else if mode == "enabled" && verify {
        assert!(!sink.is_empty(), "enabled method emitted no binary-log records");
    }
    println!("PBRS_STREAM_PROBE {shape} {mode} {messages} {calls} {cpu} {wall}");
    server.abort();
}
