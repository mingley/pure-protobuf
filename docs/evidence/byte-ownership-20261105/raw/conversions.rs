//! Allocation-only comparison with the historical conversion expressions.
//! The old paths model source from 1c6ec8ca; both paths use one current pbrs rlib.
//! Payload creation, boxing, assertions and output are outside measured regions.
use pbrs::IntoProxied;
use pbrs::ProtoBytes;
use pbrs::rt::LazyBytes;
use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

// Same allocation accounting as tests/proof/codec_benchmark/src/main.rs.
struct Alloc;
unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ENABLED.load(Ordering::Relaxed) {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() && ENABLED.load(Ordering::Relaxed) {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, old, new) };
        if !result.is_null() && ENABLED.load(Ordering::Relaxed) {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(new as u64, Ordering::Relaxed);
        }
        result
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOC: Alloc = Alloc;

fn measure<T>(f: impl FnOnce() -> T) -> (T, (u64, u64)) {
    CALLS.store(0, Ordering::SeqCst);
    BYTES.store(0, Ordering::SeqCst);
    ENABLED.store(true, Ordering::SeqCst);
    let value = std::hint::black_box(f());
    ENABLED.store(false, Ordering::SeqCst);
    let counts = (CALLS.load(Ordering::SeqCst), BYTES.load(Ordering::SeqCst));
    (value, counts)
}

enum Input {
    Vec(Vec<u8>),
    CowOwned(Cow<'static, [u8]>),
    Box(Box<[u8]>),
    Borrowed(Vec<u8>),
}

impl Input {
    fn prepare(kind: &str, len: usize) -> (Self, *const u8, usize) {
        let capacity = if kind.ends_with("spare") {
            len.saturating_mul(2).max(4096)
        } else {
            len
        };
        let mut payload = Vec::with_capacity(capacity);
        payload.resize(len, 0x9d);
        let (input, ptr, capacity) = match kind {
            "vec_exact" | "vec_spare" => {
                let ptr = payload.as_ptr();
                let capacity = payload.capacity();
                (Self::Vec(payload), ptr, capacity)
            }
            "cow_owned_exact" | "cow_owned_spare" => {
                let ptr = payload.as_ptr();
                let capacity = payload.capacity();
                (Self::CowOwned(Cow::Owned(payload)), ptr, capacity)
            }
            "box_exact" => {
                let payload = payload.into_boxed_slice();
                let ptr = payload.as_ptr();
                let capacity = payload.len();
                (Self::Box(payload), ptr, capacity)
            }
            "cow_borrowed" => {
                let ptr = payload.as_ptr();
                let capacity = payload.capacity();
                (Self::Borrowed(payload), ptr, capacity)
            }
            _ => panic!("unknown input kind"),
        };
        (input, ptr, capacity)
    }

    fn convert(self, old: bool) -> LazyBytes {
        let owned = match self {
            Self::Vec(payload) => {
                return if old {
                    // Exact former LazyBytes::from(Vec<u8>) expression.
                    LazyBytes::owned(ProtoBytes::from(payload.as_slice()))
                } else {
                    LazyBytes::from(payload)
                };
            }
            Self::CowOwned(payload) => {
                if old {
                    // Exact former IntoProxied<ProtoBytes> for Cow expression.
                    ProtoBytes::from(payload.as_ref())
                } else {
                    payload.into_proxied(pbrs::__internal::Private)
                }
            }
            Self::Box(payload) => {
                if old {
                    // Exact former IntoProxied<ProtoBytes> for Box expression.
                    ProtoBytes::from(payload.as_ref())
                } else {
                    payload.into_proxied(pbrs::__internal::Private)
                }
            }
            Self::Borrowed(payload) => {
                let borrowed = Cow::Borrowed(payload.as_slice());
                if old {
                    ProtoBytes::from(borrowed.as_ref())
                } else {
                    borrowed.into_proxied(pbrs::__internal::Private)
                }
            }
        };
        LazyBytes::owned(owned)
    }
}

fn main() {
    println!("kind,model,len,input_capacity,payload_reused,conversion_calls,conversion_bytes,clone_calls,clone_bytes,share_calls,share_bytes");
    for len in [0, 1, 16, 4096, 1_048_576] {
        for kind in [
            "vec_exact", "vec_spare", "cow_owned_exact", "cow_owned_spare",
            "box_exact", "cow_borrowed",
        ] {
            for old in [true, false] {
                let (input, ptr, capacity) = Input::prepare(kind, len);
                let (value, conversion) = measure(|| input.convert(old));
                assert_eq!(value.as_bytes().len(), len);
                assert!(value.as_bytes().iter().all(|&b| b == 0x9d));
                let reused = if len == 0 {
                    "not_applicable"
                } else {
                    let reused = value.as_bytes().as_ptr() == ptr;
                    assert_eq!(reused, !old && kind != "cow_borrowed");
                    if reused { "true" } else { "false" }
                };
                let (cloned, clone_counts) = measure(|| value.clone());
                if len != 0 {
                    assert_eq!(cloned.as_bytes().as_ptr(), value.as_bytes().as_ptr());
                }
                let (shared, share_counts) = measure(|| value.as_shared());
                if len != 0 {
                    assert_eq!(shared.as_ptr(), value.as_bytes().as_ptr());
                }
                drop(value);
                drop(cloned);
                assert_eq!(shared.len(), len);
                assert!(shared.iter().all(|&b| b == 0x9d));
                println!(
                    "{kind},{},{len},{capacity},{reused},{},{},{},{},{},{}",
                    if old { "historical_expression" } else { "current_runtime" },
                    conversion.0, conversion.1, clone_counts.0, clone_counts.1,
                    share_counts.0, share_counts.1,
                );
            }
        }
    }
}
