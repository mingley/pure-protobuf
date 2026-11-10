//! Source-pinned warmed loopback unary RPC probe for TLS portability.
use futures_util::{stream::FuturesUnordered, StreamExt};
use pbrs_grpc::{Channel, ClientTls, Code, HelloReply, HelloRequest, Identity, Request,
                Response, Rpc, Server, ServerTls, Service, Status};
use std::hint::black_box;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

const CA: &str = include_str!("../certs/ca.crt");
const OTHER_CA: &str = include_str!("../certs/other.crt");
const SERVER_CERT: &str = include_str!("../certs/server.crt");
const SERVER_KEY: &str = include_str!("../certs/server.key");
const METHOD: &str = "/bench.Echo/Unary";
const PAYLOAD: &str = "warm-tls-control";

#[cfg(feature = "alloc-probe")]
mod alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    pub struct Counting;
    static ARMED: AtomicBool = AtomicBool::new(false);
    static CALLS: AtomicU64 = AtomicU64::new(0);
    static BYTES: AtomicU64 = AtomicU64::new(0);
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc(layout) };
            if !p.is_null() && ARMED.load(Ordering::Relaxed) {
                CALLS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
            }
            p
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc_zeroed(layout) };
            if !p.is_null() && ARMED.load(Ordering::Relaxed) {
                CALLS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
            }
            p
        }
        unsafe fn realloc(&self, p: *mut u8, old: Layout, size: usize) -> *mut u8 {
            let q = unsafe { System.realloc(p, old, size) };
            if !q.is_null() && ARMED.load(Ordering::Relaxed) {
                CALLS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(size as u64, Ordering::Relaxed);
            }
            q
        }
        unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
            unsafe { System.dealloc(p, layout) }
        }
    }
    pub fn start() {
        CALLS.store(0, Ordering::SeqCst);
        BYTES.store(0, Ordering::SeqCst);
        ARMED.store(true, Ordering::SeqCst);
    }
    pub fn stop() -> (u64, u64) {
        ARMED.store(false, Ordering::SeqCst);
        (CALLS.load(Ordering::SeqCst), BYTES.load(Ordering::SeqCst))
    }
}
#[cfg(feature = "alloc-probe")]
#[global_allocator]
static ALLOC: alloc_probe::Counting = alloc_probe::Counting;

fn cpu_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) }, 0);
    (ts.tv_sec as u64) * 1_000_000_000 + ts.tv_nsec as u64
}
struct Echo {
    #[cfg(feature = "witness")]
    tls: bool,
}
impl Service for Echo {
    const NAME: &'static str = "bench.Echo";
    async fn call(&self, rpc: Rpc) {
        #[cfg(feature = "witness")]
        {
            assert_eq!(rpc.scheme(), Some(if self.tls { "https" } else { "http" }));
            assert!(rpc.local_addr().is_some());
        }
        if rpc.method() != "Unary" {
            rpc.unimplemented();
            return;
        }
        rpc.unary(|request: Request<HelloRequest>| async move {
            let mut reply = HelloReply::new();
            reply.set_message(request.get_ref().name().to_str().expect("valid UTF-8"));
            Ok::<_, Status>(Response::new(reply))
        }).await;
    }
}
fn message() -> HelloRequest {
    let mut request = HelloRequest::new();
    request.set_name(black_box(PAYLOAD));
    request
}
async fn load(channel: &Channel, count: usize, width: usize) {
    let mut done = 0;
    while done < count {
        let n = width.min(count - done);
        let mut pending = FuturesUnordered::new();
        for _ in 0..n {
            pending.push(channel.unary::<HelloRequest, HelloReply>(METHOD, Request::new(message())));
        }
        while let Some(result) = pending.next().await {
            let reply = result.expect("successful unary response").into_inner();
            assert_eq!(black_box(reply.message().to_str().expect("valid reply")), PAYLOAD);
            done += 1;
        }
    }
    assert_eq!(done, count);
}
async fn run(mode: &str, count: usize, width: usize) {
    let tls = match mode { "tls" => true, "plain" => false, _ => panic!("mode") };
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let service = Echo {
        #[cfg(feature = "witness")]
        tls,
    };
    let server = Server::new(service);
    let task = if tls {
        let identity = Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("identity");
        let config = ServerTls::new(identity).expect("server config");
        tokio::spawn(async move {
            server.serve_tls_with_shutdown(listener, std::future::pending(), config).await
        })
    } else {
        tokio::spawn(async move { server.serve_listener(listener).await })
    };
    #[cfg(feature = "witness")]
    if tls {
        let wrong = ClientTls::ca("localhost", OTHER_CA).expect("other CA parses");
        let result = tokio::time::timeout(Duration::from_secs(10), Channel::connect_tls(addr, wrong))
            .await.expect("negative TLS bounded");
        let status = match result { Ok(_) => panic!("wrong CA unexpectedly succeeded"), Err(e) => e };
        assert_eq!(status.code(), Code::Unauthenticated);
    }
    let channel = if tls {
        let config = ClientTls::ca("localhost", CA).expect("client config");
        Channel::connect_tls(addr, config).await.expect("TLS connect")
    } else {
        Channel::connect(addr).await.expect("plaintext connect")
    };
    tokio::time::timeout(Duration::from_secs(60), load(&channel, 256, width))
        .await.expect("warmup deadline");
    #[cfg(feature = "alloc-probe")]
    alloc_probe::start();
    let before = cpu_ns();
    let wall = Instant::now();
    tokio::time::timeout(Duration::from_secs(120), load(&channel, count, width))
        .await.expect("measurement deadline");
    let wall_ns = wall.elapsed().as_nanos();
    let elapsed = cpu_ns() - before;
    #[cfg(feature = "alloc-probe")]
    let (allocations, requested) = alloc_probe::stop();
    #[cfg(not(feature = "alloc-probe"))]
    let (allocations, requested) = (0u64, 0u64);
    assert!(elapsed > 0);
    println!("{}", serde_json::json!({
        "schema":"tls-rpc-control/1", "mode":mode, "requests":count, "width":width,
        "warmup":256, "cpu_ns":elapsed, "wall_ns":wall_ns,
        "allocations":allocations, "requested_bytes":requested,
        "tls_verified":tls, "response_validated":true,
        "scheme_witness":cfg!(feature="witness"),
        "allocation_instrumented":cfg!(feature="alloc-probe")
    }));
    task.abort();
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 4, "mode count width");
    let count: usize = args[2].parse().expect("count");
    let width: usize = args[3].parse().expect("width");
    assert!(count > 0 && width > 0);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2).enable_all().build().expect("runtime");
    runtime.block_on(run(&args[1], count, width));
}
