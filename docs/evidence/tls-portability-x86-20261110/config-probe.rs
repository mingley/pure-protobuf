//! Scratch-only TLS configuration construction and drop probe.
use pbrs_grpc::{ClientTls, Identity, ServerTls};
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

#[cfg(feature = "allocation-probe")]
mod allocation {
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
#[cfg(feature = "allocation-probe")]
#[global_allocator]
static ALLOC: allocation::Counting = allocation::Counting;

fn cpu_ns() -> u128 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: ts is valid output for the process CPU clock.
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) }, 0);
    ts.tv_sec as u128 * 1_000_000_000 + ts.tv_nsec as u128
}

struct Inputs {
    ca: Vec<u8>,
    server_cert: Vec<u8>,
    server_key: Vec<u8>,
    client_cert: Vec<u8>,
    client_key: Vec<u8>,
}
impl Inputs {
    fn read(dir: &Path) -> Self {
        let get = |name: &str| std::fs::read(dir.join(name)).expect("fixture");
        Self { ca: get("ca.crt"), server_cert: get("server.crt"),
               server_key: get("server.key"), client_cert: get("client.crt"),
               client_key: get("client.key") }
    }
}
#[derive(Clone, Copy)]
enum Case { ClientCa, Server, ClientMtls, ServerMtls }
impl Case {
    fn parse(value: &str) -> Self {
        match value {
            "client-ca" => Self::ClientCa,
            "server" => Self::Server,
            "client-mtls" => Self::ClientMtls,
            "server-mtls" => Self::ServerMtls,
            _ => panic!("unknown case"),
        }
    }
    fn identities(self, inputs: &Inputs, count: usize) -> Vec<Identity> {
        match self {
            Self::ClientCa => Vec::new(),
            Self::ClientMtls => (0..count).map(|_| {
                Identity::from_pem(&inputs.client_cert, &inputs.client_key).expect("client identity")
            }).collect(),
            Self::Server | Self::ServerMtls => (0..count).map(|_| {
                Identity::from_pem(&inputs.server_cert, &inputs.server_key).expect("server identity")
            }).collect(),
        }
    }
    fn run_one(self, inputs: &Inputs, identity: Option<Identity>) {
        match self {
            Self::ClientCa => {
                assert!(identity.is_none());
                let cfg = ClientTls::ca(black_box("localhost"), black_box(inputs.ca.as_slice()))
                    .expect("client CA config");
                black_box(&cfg);
                drop(cfg);
            }
            Self::Server => {
                let cfg = ServerTls::new(black_box(identity.expect("server identity")))
                    .expect("server config");
                black_box(&cfg);
                drop(cfg);
            }
            Self::ClientMtls => {
                let cfg = ClientTls::ca_mtls(black_box("localhost"), black_box(inputs.ca.as_slice()),
                    black_box(identity.expect("client identity")))
                    .expect("client mTLS config");
                black_box(&cfg);
                drop(cfg);
            }
            Self::ServerMtls => {
                let cfg = ServerTls::mtls(black_box(identity.expect("server identity")),
                    black_box(inputs.ca.as_slice())).expect("server mTLS config");
                black_box(&cfg);
                drop(cfg);
            }
        }
    }
    fn batch(self, inputs: &Inputs, count: usize, measured: bool) -> (u128, u128, u64, u64) {
        // PEM parsing of identities is intentionally outside the measured interval.
        let mut identities = self.identities(inputs, count).into_iter();
        #[cfg(feature = "allocation-probe")]
        if measured { allocation::start(); }
        let start_cpu = cpu_ns();
        let start_wall = Instant::now();
        for _ in 0..count {
            self.run_one(inputs, identities.next());
        }
        let wall = start_wall.elapsed().as_nanos();
        let cpu = cpu_ns() - start_cpu;
        #[cfg(feature = "allocation-probe")]
        let (calls, bytes) = if measured { allocation::stop() } else { (0, 0) };
        #[cfg(not(feature = "allocation-probe"))]
        let (calls, bytes) = { let _ = measured; (0, 0) };
        assert!(identities.next().is_none());
        (cpu, wall, calls, bytes)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 4, "CASE ITERATIONS TLS_FIXTURE_DIR");
    let case = Case::parse(&args[1]);
    let count: usize = args[2].parse().expect("iterations");
    assert!(count > 0);
    let inputs = Inputs::read(Path::new(&args[3]));
    let parallelism = std::thread::available_parallelism().expect("parallelism").get();
    let _ = case.batch(&inputs, 8, false);
    let (cpu, wall, calls, bytes) = case.batch(&inputs, count, true);
    println!("TLS_CONSTRUCT_V1 case={} iters={count} cpu_ns={cpu} wall_ns={wall} alloc_calls={calls} requested_bytes={bytes} parallelism={parallelism}", args[1]);
}
