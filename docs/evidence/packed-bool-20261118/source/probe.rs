//! Scratch-only component screen. Timing binaries contain no allocator instrumentation.
use pbrs::rt::{FixedU32, FixedU64, Ieee64, Packed, PackedCodec, VarintU32, Wire};
use pbrs::gencode::TestAllTypesEdition2023;
use pbrs::rt::{Bools, encode_len_field};
use pbrs::{ParseError, Repeated, RepeatedView};
use std::hint::black_box;
#[cfg(feature = "inline-count")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    pub static ACTIVE: AtomicBool = AtomicBool::new(false);
    pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static REALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);
    pub struct Counted;
    unsafe impl GlobalAlloc for Counted {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if ACTIVE.load(Ordering::Relaxed) {
                ALLOCS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
            }
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
            if ACTIVE.load(Ordering::Relaxed) {
                REALLOCS.fetch_add(1, Ordering::Relaxed);
                BYTES.fetch_add(size as u64, Ordering::Relaxed);
            }
            unsafe { System.realloc(ptr, old, size) }
        }
    }
    pub fn begin() {
        ALLOCS.store(0, Ordering::SeqCst);
        REALLOCS.store(0, Ordering::SeqCst);
        BYTES.store(0, Ordering::SeqCst);
        ACTIVE.store(true, Ordering::SeqCst);
    }
    pub fn end() -> (u64, u64, u64) {
        ACTIVE.store(false, Ordering::SeqCst);
        (ALLOCS.load(Ordering::SeqCst), REALLOCS.load(Ordering::SeqCst),
         BYTES.load(Ordering::SeqCst))
    }
}
#[cfg(feature = "inline-count")]
#[global_allocator]
static ALLOCATOR: counting::Counted = counting::Counted;
#[cfg(not(feature = "inline-count"))]
mod counting {
    pub fn end() -> (u64, u64, u64) { (0, 0, 0) }
}

#[cfg(not(feature = "inline-count"))]
mod cpu {
    #[repr(C)]
    struct Timespec { sec: i64, nsec: i64 }
    unsafe extern "C" { fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32; }
    pub fn thread_cpu_ns() -> u64 {
        let mut ts = Timespec { sec: 0, nsec: 0 };
        assert_eq!(unsafe { clock_gettime(3, &mut ts) }, 0);
        (ts.sec as u64) * 1_000_000_000 + (ts.nsec as u64)
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
#[repr(C)]
struct Padded { small: u8, wide: u32 }
enum Custom {}
impl PackedCodec for Custom {
    type Elem = Padded;
    const MEMCPY_SAFE: bool = true;
    fn validate(buf: &[u8]) -> Result<(), ParseError> {
        if buf.len() % 5 == 0 { Ok(()) } else { Err(ParseError::new("custom width")) }
    }
    fn decode(buf: &[u8], out: &mut Vec<Padded>) -> Result<(), ParseError> {
        Self::validate(buf)?;
        for c in buf.chunks_exact(5) {
            out.push(Padded { small:c[0], wide:u32::from_le_bytes(c[1..].try_into().unwrap()) });
        }
        Ok(())
    }
    fn encode(elems: &[Padded], out: &mut Vec<u8>) {
        for e in elems { out.push(e.small); out.extend_from_slice(&e.wide.to_le_bytes()); }
    }
}
fn packed<C:PackedCodec>(values: Vec<C::Elem>) -> Packed<C> {
    Packed::from_repeated(Repeated::from_vec(values))
}
fn consume(bytes: Option<&[u8]>, out: &mut Vec<u8>, copy:bool) -> u64 {
    if let Some(bytes) = bytes {
        let bytes=black_box(bytes);
        if copy { out.clear(); out.extend_from_slice(bytes); black_box(&*out); }
        bytes.len() as u64 + u64::from(bytes[0]) + u64::from(bytes[bytes.len()-1])
    } else { 0 }
}
#[inline(never)]
fn run<C:PackedCodec>(mode:&str, n:usize, values:&[C::Elem], warm:&Packed<C>, wire:&Packed<C>, out:&mut Vec<u8>) -> u64 {
    let mut total=0u64;
    for _ in 0..n {
        let result = match mode {
            "cold" | "coldcopy" => {
                let value=packed::<C>(black_box(values).to_vec());
                consume(value.packed_bytes(),out,mode=="coldcopy")
            }
            "warm" | "warmcopy" => consume(black_box(warm).packed_bytes(),out,mode=="warmcopy"),
            "wire" => consume(black_box(wire).packed_bytes(),out,false),
            "clone" => {
                let value=black_box(warm).clone();
                consume(value.packed_bytes(),out,true)
            }
            "mutate" => {
                let mut value=black_box(warm).clone();
                value.push(values[0]);
                consume(value.packed_bytes(),out,true)
            }
            "empty" => consume(black_box(&Packed::<C>::new()).packed_bytes(),out,false),
            _=> panic!("unknown mode"),
        };
        total=total.wrapping_add(black_box(result));
    }
    black_box(total)
}
fn screen<C:PackedCodec>(case:&str, mode:&str, n:usize, values:Vec<C::Elem>) {
    let warm=packed::<C>(values.clone());
    let mut expected=Vec::new(); C::encode(&values,&mut expected);
    assert_eq!(warm.packed_bytes(),Some(expected.as_slice()));
    let mut wire=Packed::<C>::new();
    wire.append_wire(Wire::from_slice(&expected)).unwrap();
    assert_eq!(wire.packed_bytes(),Some(expected.as_slice()));
    let mut out=Vec::with_capacity(expected.len()+16);
    measure(case, n, expected.len(), |n| run::<C>(mode,n,&values,&warm,&wire,&mut out));
}

fn measure(case: &str, n: usize, wire_len: usize, mut run: impl FnMut(usize) -> u64) {
    run(2);
    #[cfg(not(feature = "inline-count"))]
    let (checksum, cpu_ns, wall_ns) = {
        let start = cpu::thread_cpu_ns();
        let wall = std::time::Instant::now();
        let checksum = run(n);
        (checksum, cpu::thread_cpu_ns() - start, wall.elapsed().as_nanos())
    };
    #[cfg(feature = "inline-count")]
    let (checksum, cpu_ns, wall_ns) = {
        counting::begin();
        let checksum = run(n);
        (checksum, 0, 0)
    };
    let (allocs, reallocs, bytes) = counting::end();
    println!("{{\"case\":\"{}\",\"n\":{},\"cpu_ns\":{},\"wall_ns\":{},\"allocs\":{},\"reallocs\":{},\"bytes\":{},\"checksum\":{},\"wire_len\":{}}}",
        case, n, cpu_ns, wall_ns, allocs, reallocs, bytes, checksum, wire_len);
}

fn bool_fixture(count: usize, overlong: bool) -> (Vec<bool>, Vec<u8>, Vec<u8>) {
    let values: Vec<_> = (0..count).map(|i| i % 3 != 0).collect();
    let canonical: Vec<_> = (0..count).map(|i| if i % 3 == 0 { 0 } else { 1 }).collect();
    let wire = if overlong {
        // Both value shapes are valid two-byte overlong varints. The nonzero
        // shape also checks bool's normalization of input values other than 1.
        values.iter().flat_map(|&v| [if v { 0x85 } else { 0x80 }, 0]).collect()
    } else {
        canonical.clone()
    };
    (values, canonical, wire)
}

fn observe_values(values: RepeatedView<'_, bool>) -> u64 {
    let values = black_box(values);
    let len = values.len();
    let first = values.get(0).expect("nonempty fixture");
    let last = values.get(len - 1).expect("nonempty fixture");
    len as u64 + u64::from(first) + u64::from(last)
}

fn bool_component(case: &str, mode: &str, n: usize, count: usize, overlong: bool) {
    let (values, canonical, input) = bool_fixture(count, overlong);
    let wire = Wire::from_slice(&input);
    let fresh = || {
        let mut value = Packed::<Bools>::new();
        value.append_wire(black_box(&wire).clone()).expect("valid packed input");
        value
    };
    let owned = || packed::<Bools>(black_box(&values).to_vec());
    let warm = fresh();
    assert_eq!(warm.as_view().iter().collect::<Vec<_>>(), values);
    assert_eq!(warm.packed_bytes(), Some(canonical.as_slice()));
    assert_eq!(fresh().packed_bytes(), Some(canonical.as_slice()));
    assert_eq!(owned().packed_bytes(), Some(canonical.as_slice()));
    let mut out = Vec::with_capacity(canonical.len());
    measure(case, n, input.len(), |iterations| {
        let mut total = 0u64;
        for _ in 0..iterations {
            let observation = match mode {
                "cold" => consume(owned().packed_bytes(), &mut out, false),
                "wirecold" => consume(fresh().packed_bytes(), &mut out, false),
                "wirewarm" => consume(black_box(&warm).packed_bytes(), &mut out, false),
                "wirewarmcopy" => consume(black_box(&warm).packed_bytes(), &mut out, true),
                "wirecoldget" => observe_values(fresh().as_view()),
                "wirewarmget" => observe_values(black_box(&warm).as_view()),
                _ => panic!("unknown bool mode"),
            };
            total = total.wrapping_add(black_box(observation));
        }
        black_box(total)
    });
}

fn generated_bool(case: &str, mode: &str, n: usize, count: usize, overlong: bool) {
    let (values, canonical, input) = bool_fixture(count, overlong);
    let mut wire = Vec::new();
    encode_len_field(&mut wire, 43, &input);
    let mut expected = Vec::new();
    encode_len_field(&mut expected, 43, &canonical);
    let fresh = || {
        let mut message = TestAllTypesEdition2023::new();
        message.merge_bytes(black_box(&wire), 0).expect("valid generated input");
        message
    };
    let warm = fresh();
    let mut out = Vec::with_capacity(expected.len());
    warm.write_to(&mut out);
    assert_eq!(out, expected);
    assert_eq!(warm.repeated_bool().iter().collect::<Vec<_>>(), values);
    measure(case, n, wire.len(), |iterations| {
        let mut total = 0u64;
        for _ in 0..iterations {
            out.clear();
            match mode {
                "cold" => fresh().write_to(&mut out),
                "warm" => black_box(&warm).write_to(&mut out),
                _ => panic!("unknown generated mode"),
            }
            total = total.wrapping_add(black_box(out.as_slice()).len() as u64);
        }
        black_box(total)
    });
}

fn main() {
    let args:Vec<String>=std::env::args().collect();
    let case=&args[1];let n:usize=args[2].parse().unwrap();assert!(n>0);
    let parts:Vec<_>=case.split('.').collect();let count:usize=parts[1].parse().unwrap();
    match parts[0] {
        "boolc" | "boolo" => bool_component(case, parts[2], n, count, parts[0] == "boolo"),
        "genc" | "geno" => generated_bool(case, parts[2], n, count, parts[0] == "geno"),
        "fx32"=>screen::<FixedU32>(case,parts[2],n,(0..count).map(|i|(i as u32).wrapping_mul(2_654_435_761)).collect()),
        "fx64"=>screen::<FixedU64>(case,parts[2],n,(0..count).map(|i|(i as u64).wrapping_mul(11_400_714_819_323_198_485)).collect()),
        "f64"=>screen::<Ieee64>(case,parts[2],n,(0..count).map(|i|f64::from_bits((i as u64).wrapping_mul(11_400_714_819_323_198_485))).collect()),
        "varint"=>screen::<VarintU32>(case,parts[2],n,(0..count).map(|i|(i as u32).wrapping_mul(2_654_435_761)).collect()),
        "custom"=>screen::<Custom>(case,parts[2],n,(0..count).map(|i|Padded{small:i as u8,wide:(i as u32).wrapping_mul(2_654_435_761)}).collect()),
        _=>panic!("unknown codec"),
    }
}
