//! Scratch-only layout and allocation witness for upload item proposals.
#![allow(unsafe_code, dead_code, reason = "scratch-only layout instrumentation")]

use pbrs_grpc::{Code, Framed, Status};
use pbrs_grpc::binlog::{Address, BinaryLogFilter, BinaryLogger, Logger, Rule, Sink, VecSink};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::mem::{align_of, size_of};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
enum Origin2 { User, Internal }
#[derive(Clone, Copy)]
enum Origin3 { ExplicitSender, TowerInput, InternalWire }
struct TaggedFailure2 { status: Status, origin: Origin2 }
struct TaggedFailure3 { status: Status, origin: Origin3 }
enum Direct3<T> { Message(Framed<T>), User(Status), Internal(Status) }
enum Direct4<T> { Message(Framed<T>), ExplicitSender(Status), TowerInput(Status), InternalWire(Status) }
enum ErrorEnum2 { User(Status), Internal(Status) }
enum ErrorEnum3 { ExplicitSender(Status), TowerInput(Status), InternalWire(Status) }
type Existing<T> = Result<Framed<T>, Status>;
type Tagged2<T> = Result<Framed<T>, TaggedFailure2>;
type Tagged3<T> = Result<Framed<T>, TaggedFailure3>;
type Boxed2<T> = Result<Framed<T>, Box<TaggedFailure2>>;
type Boxed3<T> = Result<Framed<T>, Box<TaggedFailure3>>;
type EnumError2<T> = Result<Framed<T>, ErrorEnum2>;
type EnumError3<T> = Result<Framed<T>, ErrorEnum3>;

#[repr(align(32))]
struct Aligned32([u8; 64]);

// These reproduce the source field types, not the private type's actual layout.
// repr(Rust) permits nominally distinct structs to be laid out differently.
struct MirrorCallState {
    sink: Arc<dyn Sink>,
    rule: Rule,
    logger: Logger,
    call_id: u64,
    sequence: AtomicU64,
    peer: Mutex<Option<Address>>,
    peer_logged: AtomicBool,
    mask_sensitive: bool,
}
struct MirrorCallStateWithDedup {
    sink: Arc<dyn Sink>,
    rule: Rule,
    logger: Logger,
    call_id: u64,
    sequence: AtomicU64,
    peer: Mutex<Option<Address>>,
    peer_logged: AtomicBool,
    mask_sensitive: bool,
    cancel_logged: AtomicBool,
}

static TRACK: AtomicBool = AtomicBool::new(false);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static LAST_SIZE: AtomicUsize = AtomicUsize::new(0);
static LAST_ALIGN: AtomicUsize = AtomicUsize::new(0);

struct Instrumented;
#[global_allocator]
static ALLOCATOR: Instrumented = Instrumented;

fn record(p: *mut u8, layout: Layout) {
    if !p.is_null() && TRACK.load(Ordering::Relaxed) {
        COUNT.fetch_add(1, Ordering::Relaxed);
        LAST_SIZE.store(layout.size(), Ordering::Relaxed);
        LAST_ALIGN.store(layout.align(), Ordering::Relaxed);
    }
}
unsafe impl GlobalAlloc for Instrumented {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        record(p, layout);
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        record(p, layout);
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) };
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, layout, size) };
        let new_layout = Layout::from_size_align(size, layout.align()).expect("valid realloc layout");
        record(q, new_layout);
        q
    }
}

fn sizes<T>() -> String {
    format!("{}/{}", size_of::<T>(), align_of::<T>())
}
fn show<T>(label: &str) {
    println!(
        "ITEM_LAYOUT type={} value={} framed={} existing={} direct3={} direct4={} tagged2={} tagged3={} boxed2={} boxed3={} enumerror2={} enumerror3={} option_existing={} option_direct3={} option_tagged3={} option_boxed3={}",
        label, sizes::<T>(), sizes::<Framed<T>>(), sizes::<Existing<T>>(),
        sizes::<Direct3<T>>(), sizes::<Direct4<T>>(), sizes::<Tagged2<T>>(),
        sizes::<Tagged3<T>>(), sizes::<Boxed2<T>>(), sizes::<Boxed3<T>>(),
        sizes::<EnumError2<T>>(), sizes::<EnumError3<T>>(),
        sizes::<Option<Existing<T>>>(), sizes::<Option<Direct3<T>>>(),
        sizes::<Option<Tagged3<T>>>(), sizes::<Option<Boxed3<T>>>(),
    );
    println!(
        "VEC64_PAYLOAD type={} existing={} direct3={} direct4={} tagged2={} tagged3={} boxed2={} boxed3={} enumerror2={} enumerror3={}",
        label, size_of::<Existing<T>>() * 64, size_of::<Direct3<T>>() * 64,
        size_of::<Direct4<T>>() * 64, size_of::<Tagged2<T>>() * 64,
        size_of::<Tagged3<T>>() * 64, size_of::<Boxed2<T>>() * 64,
        size_of::<Boxed3<T>>() * 64, size_of::<EnumError2<T>>() * 64,
        size_of::<EnumError3<T>>() * 64,
    );
}

fn begin_count() {
    COUNT.store(0, Ordering::Relaxed);
    LAST_SIZE.store(0, Ordering::Relaxed);
    LAST_ALIGN.store(0, Ordering::Relaxed);
    TRACK.store(true, Ordering::SeqCst);
}
fn end_count() -> (usize, usize, usize) {
    TRACK.store(false, Ordering::SeqCst);
    (COUNT.load(Ordering::Relaxed), LAST_SIZE.load(Ordering::Relaxed), LAST_ALIGN.load(Ordering::Relaxed))
}

#[test]
fn show_upload_item_layouts_and_cold_allocations() {
    println!(
        "ERROR_LAYOUT status={} origin2={} origin3={} tagged2={} tagged3={} boxed_tagged2={} boxed_tagged3={} error_enum2={} error_enum3={}",
        sizes::<Status>(), sizes::<Origin2>(), sizes::<Origin3>(), sizes::<TaggedFailure2>(),
        sizes::<TaggedFailure3>(), sizes::<Box<TaggedFailure2>>(), sizes::<Box<TaggedFailure3>>(),
        sizes::<ErrorEnum2>(), sizes::<ErrorEnum3>(),
    );
    show::<()>("unit");
    show::<pbrs_grpc::Empty>("generated_Empty");
    show::<pbrs_grpc::HelloRequest>("generated_HelloRequest");
    show::<pbrs_grpc::HelloReply>("generated_HelloReply");
    show::<pbrs_grpc::Payload>("generated_Payload");
    show::<pbrs_grpc::SimpleRequest>("generated_SimpleRequest");
    show::<pbrs_grpc::SimpleResponse>("generated_SimpleResponse");
    show::<pbrs_grpc::StreamingInputCallRequest>("generated_StreamingInputCallRequest");
    show::<pbrs_grpc::StreamingOutputCallRequest>("generated_StreamingOutputCallRequest");
    show::<pbrs_grpc::StreamingOutputCallResponse>("generated_StreamingOutputCallResponse");
    show::<Vec<u8>>("heap_Vec_u8");
    show::<bytes::Bytes>("heap_Bytes");
    show::<[u8; 1]>("inline_u8_1");
    show::<[u8; 33]>("inline_u8_33");
    show::<[u8; 4096]>("inline_u8_4096");
    show::<Aligned32>("synthetic_aligned32_64");

    let sink: Arc<dyn Sink> = Arc::new(VecSink::new());
    let filter = BinaryLogFilter::parse("*").expect("filter");
    let logger = BinaryLogger::new(filter, sink);
    println!("LOGGER_MIRROR baseline={} with_dedup={}", sizes::<MirrorCallState>(), sizes::<MirrorCallStateWithDedup>());
    for sample in 0..5 {
        begin_count();
        let call = black_box(logger.start_call("/probe.Service/Call", Logger::Client).expect("enabled"));
        let (call_count, call_size, call_align) = end_count();
        black_box(&call);
        println!("START_CALL_ALLOCATION sample={} count={} last_bytes={} last_align={}", sample, call_count, call_size, call_align);
        drop(call);
    }
    for sample in 0..5 {
        let status = Status::from_code(Code::Cancelled);
        begin_count();
        let failure = black_box(Box::new(TaggedFailure3 { status, origin: Origin3::ExplicitSender }));
        let (error_count, error_size, error_align) = end_count();
        black_box(&failure);
        println!(
            "BOXED_ERROR_ALLOCATION sample={} count={} last_bytes={} last_align={} payload_size={} payload_align={}",
            sample, error_count, error_size, error_align, size_of::<TaggedFailure3>(), align_of::<TaggedFailure3>(),
        );
        if error_count == 1 {
            assert_eq!((error_size, error_align), (size_of::<TaggedFailure3>(), align_of::<TaggedFailure3>()));
        }
        drop(failure);
    }
}
