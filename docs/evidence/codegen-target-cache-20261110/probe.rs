//! Scratch-only direct CodeGeneratorRequest capacity and CPU probe.
use pbrs::codegen::generate_from_code_generator_request;
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
    // SAFETY: ts is a valid output and this clock reads process CPU time.
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut ts) }, 0);
    ts.tv_sec as u128 * 1_000_000_000 + ts.tv_nsec as u128
}

fn generate(input: &[u8]) -> Vec<(String, String)> {
    generate_from_code_generator_request(black_box(input)).expect("generate")
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(args.len() == 4 || args.len() == 5, "REQUEST ITERATIONS bench|out|result [DIR]");
    let input = std::fs::read(&args[1]).expect("request");
    let parallelism = std::thread::available_parallelism().unwrap().get();
    let iterations: u64 = args[2].parse().expect("iterations");
    assert!(iterations > 0);
    if args[3] == "result" {
        match generate_from_code_generator_request(&input) {
            Ok(_) => println!("RESULT_OK_V1"),
            Err(error) => println!("RESULT_ERR_V1 {error:?}"),
        }
        return;
    }
    if args[3] == "out" {
        assert_eq!(args.len(), 5);
        let out = generate(&input);
        let mut total = 0usize;
        for (index, (name, content)) in out.iter().enumerate() {
            let prefix = format!("{index:06}");
            std::fs::write(Path::new(&args[4]).join(format!("{prefix}.name")), name).unwrap();
            std::fs::write(Path::new(&args[4]).join(format!("{prefix}.rs")), content).unwrap();
            total += content.len();
        }
        println!("OUTPUT_V1 files={} bytes={} parallelism={parallelism}", out.len(), total);
        return;
    }
    if args[3] == "dump" {
        assert_eq!(args.len(), 5);
        let out = generate(&input);
        let mut total_len = 0usize;
        let mut total_capacity = 0usize;
        for (name, text) in &out {
            assert!(!Path::new(name).is_absolute() && !name.split('/').any(|x| x == ".."));
            let path = Path::new(&args[4]).join(name);
            if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).unwrap(); }
            std::fs::write(path, text).unwrap();
            total_len += text.len();
            total_capacity += text.capacity();
        }
        println!("CAPACITY_V1 files={} bytes={} capacity={}", out.len(), total_len, total_capacity);
        return;
    }
    assert_eq!(args[3], "bench");
    for _ in 0..2 {
        let output = generate(&input);
        black_box(&output);
    }
    #[cfg(feature = "allocation-probe")]
    allocation::start();
    let before_cpu = cpu_ns();
    let before_wall = Instant::now();
    for _ in 0..iterations {
        let output = generate(&input);
        black_box(&output);
    }
    let elapsed = before_wall.elapsed().as_nanos();
    let cpu = cpu_ns() - before_cpu;
    #[cfg(feature = "allocation-probe")]
    let (calls, bytes) = allocation::stop();
    #[cfg(not(feature = "allocation-probe"))]
    let (calls, bytes) = (0u64, 0u64);
    println!("PROBE_V1 iters={iterations} cpu_ns={cpu} wall_ns={elapsed} alloc_calls={calls} requested_bytes={bytes} parallelism={parallelism}");
}
