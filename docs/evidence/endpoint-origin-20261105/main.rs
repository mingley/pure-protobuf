use pbrs_grpc::{Channel, Endpoint, HelloReply, HelloRequest, Request, Response, Rpc, Server, Service, Status};
use std::hint::black_box;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use std::time::{Duration, Instant};

const PATH: &str = "/origin.Bench/Call";
const PAYLOAD: &str = "payload1";

struct Echo { calls: Arc<AtomicU64>, authority: &'static str }
impl Service for Echo {
    const NAME: &'static str = "origin.Bench";
    async fn call(&self, rpc: Rpc) {
        assert_eq!(rpc.path(), PATH);
        assert_eq!(rpc.scheme(), Some("http"));
        assert_eq!(rpc.authority(), Some(self.authority));
        let calls = Arc::clone(&self.calls);
        rpc.unary(move |request: Request<HelloRequest>| async move {
            calls.fetch_add(1, Ordering::Relaxed);
            let mut reply = HelloReply::new();
            reply.set_message(request.get_ref().name());
            Ok::<_, Status>(Response::new(reply))
        }).await;
    }
}

fn cpu() -> u64 {
    let mut value = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // Benchmark-only observer; successful clock_gettime initializes timespec.
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, value.as_mut_ptr()) }, 0);
    let value = unsafe { value.assume_init() };
    u64::try_from(value.tv_sec).unwrap() * 1_000_000_000 + u64::try_from(value.tv_nsec).unwrap()
}

#[cfg(feature = "allocations")]
mod counts {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    pub struct Observer;
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static CALLS: AtomicU64 = AtomicU64::new(0);
    static BYTES: AtomicU64 = AtomicU64::new(0);
    fn note(size: usize) {
        if ACTIVE.load(Ordering::Relaxed) {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(size as u64, Ordering::Relaxed);
        }
    }
    unsafe impl GlobalAlloc for Observer {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() { note(layout.size()); }
            ptr
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if !ptr.is_null() { note(layout.size()); }
            ptr
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let ptr = unsafe { System.realloc(ptr, layout, size) };
            if !ptr.is_null() { note(size); }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    pub fn start() {
        CALLS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        ACTIVE.store(true, Ordering::SeqCst);
    }
    pub fn stop() -> (Option<u64>, Option<u64>) {
        ACTIVE.store(false, Ordering::SeqCst);
        (Some(CALLS.load(Ordering::Relaxed)), Some(BYTES.load(Ordering::Relaxed)))
    }
}

#[cfg(feature = "allocations")]
#[global_allocator]
static ALLOCATOR: counts::Observer = counts::Observer;

async fn invoke(channel: &Channel, message: &HelloRequest, deadline: bool) -> u64 {
    let mut request = Request::new(black_box(message).clone());
    if deadline { request.set_timeout(Duration::from_secs(5)); }
    let reply = channel.unary::<HelloRequest, HelloReply>(PATH, request).await.unwrap().into_inner();
    assert_eq!(reply.message().to_str().unwrap(), PAYLOAD);
    black_box(reply.message().as_bytes()).iter().map(|value| u64::from(*value)).sum()
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "MODE ITERATIONS WARMUP");
    let mode = args[1].clone();
    assert!(["stock", "deadline", "origin-http"].contains(&mode.as_str()));
    let iterations: u64 = args[2].parse().unwrap();
    let warmup: u64 = args[3].parse().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let report = runtime.block_on(async move {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let successful = Arc::new(AtomicU64::new(0));
        let authority = if mode=="origin-http" { "benchmark.local" } else { "benchmark.local:80" };
        let server = tokio::spawn(Server::new(Echo { calls: Arc::clone(&successful), authority }).serve_connection(server_io));
        let endpoint = Endpoint::from_static("http://benchmark.local").unwrap();
        #[cfg(feature = "candidate")]
        let endpoint = if mode == "origin-http" {
            endpoint.origin("http://benchmark.local/ignored?discard=1".parse().unwrap())
        } else { endpoint };
        let channel = endpoint.connect_with_connector(move |_| async move { Ok(client_io) }).await.unwrap();
        #[cfg(not(feature = "candidate"))]
        let channel = if mode == "origin-http" { channel.origin("benchmark.local").unwrap() } else { channel };
        assert_eq!(channel.authority(), authority);
        assert_eq!(channel.scheme(), "http");
        let layout = pbrs_grpc::__origin_layout_probe();
        let mut message = HelloRequest::new();
        message.set_name(PAYLOAD);
        let deadline = mode == "deadline";
        for _ in 0..warmup { black_box(invoke(&channel, &message, deadline).await); }
        #[cfg(feature = "allocations")]
        counts::start();
        let cpu_start = cpu();
        let start = Instant::now();
        let mut checksum = 0_u64;
        for _ in 0..iterations { checksum = checksum.wrapping_add(invoke(&channel, &message, deadline).await); }
        let wall_ns = start.elapsed().as_nanos();
        let cpu_ns = cpu() - cpu_start;
        #[cfg(feature = "allocations")]
        let allocations = counts::stop();
        #[cfg(not(feature = "allocations"))]
        let allocations = (None::<u64>, None::<u64>);
        assert_eq!(successful.load(Ordering::Relaxed), warmup + iterations);
        let expected: u64 = PAYLOAD.bytes().map(u64::from).sum();
        assert_eq!(checksum, expected * iterations);
        drop(channel);
        server.abort();
        let _ = server.await;
        serde_json::json!({
            "schema": "pbrs.origin-default-diagnostic.v1", "mode": mode,
            "iterations": iterations, "warmup": warmup, "successful": iterations,
            "wire_payload_bytes": 10, "framed_bytes_per_direction": 15, "checksum": checksum,
            "request_scheme":"http", "request_authority":authority, "request_path":PATH,
            "process_cpu_ns": cpu_ns, "wall_ns": wall_ns,
            "allocation_instrumentation": cfg!(feature="allocations"),
            "allocation_calls": allocations.0, "requested_bytes": allocations.1,
            "channel_local_bytes": layout.0, "channel_handle_bytes": layout.1,
            "scope": "in-memory current-thread client+server unary; setup, warmup and teardown excluded"
        })
    });
    println!("{report}");
}
