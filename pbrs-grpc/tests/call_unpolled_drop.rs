//! Unpolled calls must still cancel detached handles and release senders.

use pbrs_grpc::binlog::{BinaryLogFilter, BinaryLogger, VecSink};
use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{Call, Channel, Request};
use std::sync::{Arc, Barrier};

const PATH: &str = "/helloworld.Greeter/SayHello";

fn assert_drop_signals<T>(call: Call<T>) {
    let handle = call.handle();
    assert!(!handle.is_cancelled());
    drop(call);
    assert!(
        handle.is_cancelled(),
        "unpolled drop must cancel detached handle"
    );
}

#[tokio::test]
async fn all_call_shapes_signal_detached_handles_before_poll() {
    let sink = Arc::new(VecSink::new());
    let logger = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
    let channel = Channel::connect_lazy("127.0.0.1:1")
        .expect("lazy channel")
        .binary_logger(logger);

    assert_drop_signals(
        channel.unary::<HelloRequest, HelloReply>(PATH, Request::new(HelloRequest::new())),
    );
    assert_drop_signals(
        channel
            .server_streaming::<HelloRequest, HelloReply>(PATH, Request::new(HelloRequest::new())),
    );
    let (client_sender, client_call) =
        channel.client_streaming::<HelloRequest, HelloReply>(PATH, Request::new(()));
    assert_drop_signals(client_call);
    assert!(
        client_sender.is_closed(),
        "client stream receiver must be dropped"
    );
    let (bidi_sender, bidi_call) = channel.bidi::<HelloRequest, HelloReply>(PATH, Request::new(()));
    assert_drop_signals(bidi_call);
    assert!(
        bidi_sender.is_closed(),
        "bidi stream receiver must be dropped"
    );
    assert!(
        sink.is_empty(),
        "unpolled calls must not create log entries"
    );
}

#[tokio::test]
async fn unpolled_drop_races_detached_and_early_explicit_cancel() {
    let sink = Arc::new(VecSink::new());
    let logger = BinaryLogger::new(BinaryLogFilter::parse("*").expect("filter"), sink.clone());
    let channel = Channel::connect_lazy("127.0.0.1:1")
        .expect("lazy channel")
        .binary_logger(logger);

    let early = channel.unary::<HelloRequest, HelloReply>(PATH, Request::new(HelloRequest::new()));
    let handle = early.handle();
    early.cancel();
    drop(early);
    assert!(handle.is_cancelled());

    for _ in 0..64 {
        let call =
            channel.unary::<HelloRequest, HelloReply>(PATH, Request::new(HelloRequest::new()));
        let handle = call.handle();
        let worker_handle = handle.clone();
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            worker_handle.cancel();
        });
        barrier.wait();
        drop(call);
        worker.join().expect("cancel worker");
        assert!(handle.is_cancelled());
    }
    assert!(
        sink.is_empty(),
        "unpolled calls must not create log entries"
    );
}
