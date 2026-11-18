//! Isolated loopback measurement for server RPC paths.
#![allow(unsafe_code, reason = "wrap System for the isolated allocation probe")]

use pbrs_grpc::{
    CallLabels, Channel, HelloReply, HelloRequest, LifecycleObserver, Request, Response,
    ResponseParts, Rpc, Server, Service, Status, Streaming,
};
use std::time::Duration;

#[cfg(allocation_counts)]
mod allocation {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

    static ARMED: AtomicBool = AtomicBool::new(false);
    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static BYTES: AtomicUsize = AtomicUsize::new(0);

    struct Counter;

    #[global_allocator]
    static ALLOCATOR: Counter = Counter;

    fn count(size: usize) {
        if ARMED.load(Relaxed) {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(size, Relaxed);
        }
    }

    unsafe impl GlobalAlloc for Counter {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count(layout.size());
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            count(layout.size());
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            count(size);
            unsafe { System.realloc(ptr, layout, size) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    pub fn begin() {
        ARMED.store(true, Relaxed);
    }

    pub fn finish() -> (usize, usize) {
        ARMED.store(false, Relaxed);
        (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
    }
}

#[cfg(not(allocation_counts))]
mod allocation {
    pub fn begin() {}

    pub fn finish() -> (usize, usize) {
        (0, 0)
    }
}

const CACHED: &str = "/probe.Echo/CacheFirstPath";
const OWNED: &str = "/probe.Echo/SecondPathWithOwnedBytes";
const STREAM: &str = "/probe.Echo/ThirdPathWithOwnedBytes";
const REJECTED: &str = "/probe.Echo/Rejected";

struct Echo;

fn reply<T>(message: T) -> Response<T> {
    let mut response = Response::new(message);
    response.set_compress(false);
    response
}

fn check_request(req: &Request<HelloRequest>, path: &str) {
    assert_eq!(req.path(), Some(path));
    assert_eq!(req.service(), Some("probe.Echo"));
    assert_eq!(req.scheme(), Some("http"));
    assert_eq!(req.authority(), Some("localhost"));
    assert_eq!(req.remote_addr(), None);
    assert_eq!(req.local_addr(), None);
    assert_eq!(req.get_ref().name(), "probe");
}

impl Service for Echo {
    const NAME: &'static str = "probe.Echo";

    async fn call(&self, rpc: Rpc) {
        match rpc.path() {
            "/probe.Echo/ClientStreaming" => {
                rpc.client_streaming(|_req: Request<Streaming<HelloRequest>>| async move {
                    Ok(reply(HelloReply::new()))
                })
                .await;
            }
            "/probe.Echo/BidiStreaming" => {
                rpc.bidi_streaming(|_req: Request<Streaming<HelloRequest>>| async move {
                    Ok(reply(Streaming::<HelloReply>::empty()))
                })
                .await;
            }
            STREAM => {
                rpc.server_streaming(|req| async move {
                    check_request(&req, STREAM);
                    let (sender, stream) = Streaming::channel(2);
                    sender.send(HelloReply::new()).await?;
                    sender.send(HelloReply::new()).await?;
                    drop(sender);
                    Ok(reply(stream))
                })
                .await;
            }
            REJECTED => {
                rpc.unary(|req| async move {
                    check_request(&req, REJECTED);
                    Err::<Response<HelloReply>, _>(Status::failed_precondition("handler rejected"))
                })
                .await;
            }
            other => {
                let path = match other {
                    CACHED => CACHED,
                    OWNED => OWNED,
                    _ => panic!("unexpected path {other}"),
                };
                rpc.unary(move |req| async move {
                    check_request(&req, path);
                    Ok(reply(HelloReply::new()))
                })
                .await;
            }
        }
    }
}

struct Observed;

impl LifecycleObserver for Observed {
    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        assert!(matches!(call.path(), CACHED | OWNED));
    }

    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, _: Duration) {
        assert!(matches!(call.path(), CACHED | OWNED));
        assert!(status.is_ok());
    }
}

async fn invoke(channel: &Channel, path: &'static str, hooked: bool) {
    let mut request = HelloRequest::new();
    request.set_name("probe");
    if path == STREAM {
        let mut stream = channel
            .server_streaming::<_, HelloReply>(path, Request::new(request))
            .await
            .expect("stream response")
            .into_inner();
        for _ in 0..2 {
            let message = stream.next_framed().await.expect("stream message").expect("present");
            assert!(!message.compressed);
            assert_eq!(message.message.message(), "");
        }
        assert!(stream.message().await.expect("terminal status").is_none());
        assert!(stream.trailers().await.expect("trailers").is_empty());
    } else {
        let response = channel
            .unary::<_, HelloReply>(path, Request::new(request))
            .await
            .expect("unary response");
        assert_eq!(response.get_ref().message(), "");
        assert_eq!(response.metadata().get("x-probe-phase"), hooked.then_some("second"));
        assert_eq!(response.trailers().get("x-probe-trailer"), hooked.then_some("ok"));
        assert!(!response.compressed());
    }
}

async fn run(case: &str, count: usize) -> (usize, usize) {
    let (client_io, server_io) = tokio::io::duplex(256 * 1024);
    let hooked = case == "hooked";
    let server = match case {
        "hooked" => Server::new(Echo)
            .send_compressed()
            .gzip_compression_level(4)
            .on_response(|parts: &mut ResponseParts| {
                assert!(matches!(parts.path(), Some(CACHED | OWNED)));
                assert_eq!(parts.gzip_level(), 4);
                assert!(parts.compresses_outbound());
                assert!(parts.accepts_compressed());
                assert!(parts.compress_is_set());
                assert!(!parts.compress());
                assert!(parts.limits().is_some());
                assert!(parts.send_buffer_size().is_some());
                assert!(parts.deadline().is_none());
                assert!(parts.timeout().is_none());
                assert!(parts.peer_timeout().is_none());
                assert!(parts.rpc_timeout().is_none());
                parts.metadata_mut().insert("x-probe-phase", "first")
            })
            .on_response(|parts: &mut ResponseParts| {
                assert_eq!(parts.metadata().get("x-probe-phase"), Some("first"));
                parts.metadata_mut().insert("x-probe-phase", "second")?;
                parts.trailers_mut().insert("x-probe-trailer", "ok")
            }),
        "observed" => Server::new(Echo).observer(Observed),
        "cached" | "owned" | "stream" => Server::new(Echo),
        _ => panic!("unknown case {case}"),
    };
    let server_task = tokio::spawn(async move {
        server.serve_connection(server_io).await.expect("server");
    });
    let channel = Channel::from_io(client_io, "localhost").await.expect("channel");
    // In a new process this fills the stock h2 path cache before the second
    // method is exercised. The measured path is stable in all cases.
    for _ in 0..64 {
        invoke(&channel, CACHED, hooked).await;
    }
    if hooked {
        let mut request = HelloRequest::new();
        request.set_name("probe");
        let status = channel
            .unary::<_, HelloReply>(REJECTED, Request::new(request))
            .await
            .expect_err("handler error bypasses both response hooks");
        assert_eq!(status.code(), pbrs_grpc::Code::FailedPrecondition);
        assert_eq!(status.message(), "handler rejected");
    }
    let path = match case {
        "cached" => CACHED,
        "stream" => STREAM,
        _ => OWNED,
    };
    for _ in 0..128 {
        invoke(&channel, path, hooked).await;
    }

    allocation::begin();
    for _ in 0..count {
        invoke(&channel, path, hooked).await;
    }
    let result = allocation::finish();
    drop(channel);
    server_task.abort();
    result
}

fn main() {
    let mut args = std::env::args().skip(1);
    let case = args.next().expect("case");
    let count: usize = args.next().expect("RPC count").parse().expect("numeric count");
    assert!(args.next().is_none());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let (allocations, bytes) = runtime.block_on(run(&case, count));
    println!(
        "{{\"case\":\"{case}\",\"rpcs\":{count},\"allocations\":{allocations},\"bytes\":{bytes}}}"
    );
}
