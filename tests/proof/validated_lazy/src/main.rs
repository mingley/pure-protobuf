//! Behavioral proof for checked parsing followed by lazy materialization.

use pbrs::{ClearAndParse, Parse, Serialize};
use prost::Message;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

static GENERATED_VALIDATIONS: AtomicUsize = AtomicUsize::new(0);

#[allow(dead_code, nonstandard_style, unused_imports)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/pbrs/proof.rs"));
}
#[allow(dead_code, nonstandard_style, unused_imports)]
mod strict {
    include!(concat!(env!("OUT_DIR"), "/pbrs/strict.rs"));
}
#[allow(dead_code)]
mod reference {
    include!(concat!(env!("OUT_DIR"), "/prost/linear.rs"));
}
#[allow(dead_code)]
mod strict_reference {
    include!(concat!(env!("OUT_DIR"), "/prost/strict.rs"));
}

struct Counting;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() && COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            REQUESTED.fetch_add(layout.size(), Ordering::Relaxed);
        }
        result
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() && COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            REQUESTED.fetch_add(layout.size(), Ordering::Relaxed);
        }
        result
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, layout, size) };
        if !result.is_null() && COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            REQUESTED.fetch_add(size, Ordering::Relaxed);
        }
        result
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocated<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    REQUESTED.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let result = operation();
    COUNTING.store(false, Ordering::Relaxed);
    (
        result,
        ALLOCATIONS.load(Ordering::Relaxed),
        REQUESTED.load(Ordering::Relaxed),
    )
}
fn varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        out.push(n as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn scalar(number: u32, value: u64) -> Vec<u8> {
    let mut wire = Vec::new();
    varint(&mut wire, u64::from(number) << 3);
    varint(&mut wire, value);
    wire
}
fn bytes(number: u32, body: &[u8]) -> Vec<u8> {
    let mut wire = Vec::new();
    varint(&mut wire, (u64::from(number) << 3) | 2);
    varint(&mut wire, body.len() as u64);
    wire.extend_from_slice(body);
    wire
}
fn chain(depth: u32) -> Vec<u8> {
    let mut wire = scalar(1, 1);
    for value in 2..=depth {
        wire = [scalar(1, u64::from(value)), bytes(2, &wire)].concat();
    }
    wire
}
fn traverse(mut current: &generated::Node) -> i32 {
    let mut sum = current.n();
    while current.has_child() {
        current = current.child();
        sum += current.n();
    }
    sum
}
fn reset_validations() {
    GENERATED_VALIDATIONS.store(0, Ordering::Relaxed);
}
fn assert_not_revalidated() {
    assert_eq!(GENERATED_VALIDATIONS.load(Ordering::Relaxed), 0);
}

fn default_layout() {
    let node = generated::Node::default();
    assert!(!node.has_child());
    assert_eq!(node.text().as_bytes(), b"");
    assert_eq!(node.serialize().unwrap(), Vec::<u8>::new());
    let strict = strict::StrictNode::default();
    assert!(!strict.has_child());
    assert_eq!(strict.text().as_bytes(), b"");
    assert_eq!(strict.serialize().unwrap(), Vec::<u8>::new());
    let blob = strict::Blob::default();
    assert!(blob.data().is_empty());
    assert_eq!(blob.serialize().unwrap(), Vec::<u8>::new());
    println!(
        "PASS generated all-zero/default field layout: Wire={}B CompactNode={}B",
        std::mem::size_of::<pbrs::rt::Wire>(),
        std::mem::size_of::<strict::CompactNode>()
    );
}

fn lazy_work() {
    let wire = chain(64);
    reset_validations();
    let (root, allocations, requested) = allocated(|| generated::Node::parse(&wire).unwrap());
    assert_eq!(GENERATED_VALIDATIONS.load(Ordering::Relaxed), 63);
    assert_eq!(allocations, 2, "parsing constructed descendants eagerly");
    assert_eq!(root.serialize().unwrap(), wire);
    reset_validations();
    let (first, first_allocations, first_bytes) = allocated(|| root.child());
    assert_eq!(first.n(), 63);
    assert_eq!(
        first_allocations, 1,
        "first getter constructed the full tree"
    );
    assert_not_revalidated();
    let (sum, later_allocations, _) = allocated(|| traverse(first));
    assert_eq!(sum, (1..64).sum::<i32>());
    assert_eq!(later_allocations, 61);
    assert_not_revalidated();
    assert_eq!(root.serialize().unwrap(), wire);
    let short = generated::Node::parse(&chain(4)).unwrap();
    reset_validations();
    let (_, short_allocations, short_bytes) = allocated(|| short.child());
    assert_eq!(short_allocations, first_allocations);
    assert_eq!(
        short_bytes, first_bytes,
        "partial allocation grew with subtree depth"
    );
    assert_not_revalidated();
    assert_eq!(
        reference::Node::decode(wire.as_slice()).unwrap(),
        reference::Node::decode(root.serialize().unwrap().as_slice()).unwrap()
    );
    println!(
        "PASS lazy work: initial_validations=63 getter_validations=0 parse_allocations={allocations} parse_bytes={requested} first_getter_allocations={first_allocations} first_getter_bytes={first_bytes} later_allocations={later_allocations}"
    );
}

fn invalid_before_access() {
    let malformed = [
        bytes(2, &[0x80]),
        bytes(2, &[0x00]),
        bytes(2, &[0x12, 0x04, 0x08]),
        chain(pbrs::RECURSION_LIMIT + 2),
    ];
    for (index, wire) in malformed.into_iter().enumerate() {
        eprintln!("invalid case {index} ordinary parse");
        assert!(generated::Node::parse(&wire).is_err());
        eprintln!("invalid case {index} shared parse");
        assert!(generated::Node::parse_bytes(pbrs::rt::Bytes::from(wire)).is_err());
    }
    eprintln!("invalid case UTF8");
    let invalid_utf8 = bytes(1, &bytes(1, &bytes(2, &[255])));
    assert!(strict::StrictNode::parse(&invalid_utf8).is_err());
    assert!(strict_reference::StrictNode::decode(invalid_utf8.as_slice()).is_err());
    // Proto2 strings remain byte-preserving when UTF-8 checking is disabled.
    eprintln!("invalid case proto2 NONE accepted");
    let legacy = bytes(2, &bytes(2, &bytes(3, &[255])));
    let parsed = generated::Node::parse(&legacy).unwrap();
    assert_eq!(parsed.child().child().text().as_bytes(), &[255]);
    println!("PASS public parse rejects malformed nested wire and UTF-8 before getters");
}

fn fresh_merge() {
    let original = generated::Node::parse(&chain(8)).unwrap();
    let mut merged = original.clone();
    assert_eq!(merged.child().n(), 7);
    merged.merge_from_bytes(&bytes(2, &scalar(1, 99))).unwrap();
    assert_eq!(merged.child().n(), 99);
    assert_eq!(merged.child().child().n(), 6);
    assert_eq!(original.child().n(), 7);
    let mut mutated = original.clone();
    mutated.child_mut().set_n(123);
    assert!(
        mutated
            .merge_from_bytes(&bytes(2, &[0x08, 0x09, 0x80]))
            .is_err()
    );
    assert_eq!(
        mutated.child().n(),
        9,
        "failed merge lost ordinary partial state"
    );
    let mut absent = generated::Node::default();
    assert!(absent.merge_from_bytes(&bytes(2, &[0x80])).is_err());
    assert!(!absent.has_child());
    let nested_bad = bytes(2, &bytes(2, &[0x80]));
    assert!(merged.merge_from_bytes(&nested_bad).is_err());
    let mut strict_node = strict::StrictNode::parse(&bytes(1, &bytes(2, b"ok"))).unwrap();
    assert_eq!(strict_node.child().text().as_bytes(), b"ok");
    assert!(
        strict_node
            .merge_from_bytes(&bytes(1, &bytes(1, &bytes(2, &[255]))))
            .is_err()
    );
    let duplicate = [bytes(2, &scalar(1, 12)), bytes(2, &bytes(2, &chain(4)))].concat();
    let parsed = generated::Node::parse(&duplicate).unwrap();
    assert_eq!(parsed.child().n(), 12);
    assert_eq!(parsed.child().child().n(), 4);
    assert_eq!(
        reference::Node::decode(duplicate.as_slice()).unwrap(),
        reference::Node::decode(parsed.serialize().unwrap().as_slice()).unwrap()
    );
    println!(
        "PASS fresh merges validate new bytes after materialization/mutation and preserve merge state"
    );
}

fn unknown_fields_and_names() {
    let unknown = [scalar(99, 1234), bytes(100, b"opaque")].concat();
    let body = [chain(4), unknown.clone()].concat();
    let wire = [bytes(2, &body), scalar(101, 44)].concat();
    let mut root = generated::Node::parse(&wire).unwrap();
    assert_eq!(root.serialize().unwrap(), wire);
    reset_validations();
    assert_eq!(traverse(root.child()), 10);
    assert_not_revalidated();
    assert_eq!(root.serialize().unwrap(), wire);
    let retained = root.clone();
    root.child_mut().set_n(77);
    let body = [scalar(1, 77), bytes(2, &chain(3)), unknown].concat();
    let expected = [bytes(2, &body), scalar(101, 44)].concat();
    assert_eq!(root.serialize().unwrap(), expected);
    assert_eq!(retained.serialize().unwrap(), wire);
    let names = [scalar(4, 11), scalar(5, 22), scalar(6, 33), scalar(7, 44)].concat();
    let names = generated::Node::parse(&names).unwrap();
    assert_eq!(names.merge_inner_mode(), 11);
    let decoded = reference::Node::decode(names.serialize().unwrap().as_slice()).unwrap();
    assert_eq!(decoded.merge_inner_mode, Some(11));
    // The other three fields intentionally collide with new private helper
    // names. Compiling and preserving their wire values tests uniquification.
    assert_eq!(
        names.serialize().unwrap(),
        [scalar(4, 11), scalar(5, 22), scalar(6, 33), scalar(7, 44)].concat()
    );
    println!("PASS cached unknown wire, mutation/clone isolation and helper-name collisions");
}

fn clone_and_threads() {
    let root = generated::Node::parse(&chain(32)).unwrap();
    let unread = root.clone();
    reset_validations();
    assert_eq!(traverse(&unread), (1..=32).sum::<i32>());
    assert_not_revalidated();
    let materialized = unread.clone();
    assert_eq!(materialized.serialize().unwrap(), root.serialize().unwrap());
    let root = Arc::new(root);
    let barrier = Arc::new(Barrier::new(8));
    reset_validations();
    let mut threads = Vec::new();
    for _ in 0..8 {
        let root = Arc::clone(&root);
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            assert_eq!(traverse(&root), (1..=32).sum::<i32>());
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_not_revalidated();
    assert_eq!(root.serialize().unwrap(), chain(32));
    println!("PASS clone before/after materialization and concurrent shared getters");
}

fn shared_input() {
    let payload = vec![7u8; 64 * 1024];
    let shared = pbrs::rt::Bytes::from(bytes(1, &bytes(1, &payload)));
    let expected_pointer = shared.as_ptr().wrapping_add(shared.len() - payload.len());
    let root = strict::BlobRoot::parse_bytes(shared.clone()).unwrap();
    drop(shared);
    reset_validations();
    assert_eq!(root.child().data().as_ptr(), expected_pointer);
    assert_eq!(root.child().data(), payload.as_slice());
    assert_not_revalidated();
    let mut copy = root.clone();
    copy.child_mut().set_data(vec![9u8; payload.len()]);
    assert_eq!(root.child().data(), payload.as_slice());
    assert!(copy.child().data().iter().all(|byte| *byte == 9));
    assert_eq!(root.child().data().as_ptr(), expected_pointer);
    println!("PASS shared input lifetime, cached wire and independent mutation");
}

fn collections_and_required() {
    let node = chain(8);
    let entry = [scalar(1, 7), bytes(2, &node)].concat();
    let group = [vec![(4 << 3) | 3], bytes(5, &node), vec![(4 << 3) | 4]].concat();
    let body = [bytes(2, &node), bytes(3, &entry), group].concat();
    let wire = bytes(1, &body);
    let root = generated::Mixed::parse(&wire).unwrap();
    reset_validations();
    let child = root.child();
    assert_eq!(traverse(&child.nodes().get(0).unwrap()), 36);
    assert_eq!(traverse(&child.values().get(7).unwrap()), 36);
    assert_eq!(traverse(child.group().node()), 36);
    let collection_fallback_validations = GENERATED_VALIDATIONS.load(Ordering::Relaxed);
    assert_eq!(
        reference::Mixed::decode(wire.as_slice()).unwrap(),
        reference::Mixed::decode(root.serialize().unwrap().as_slice()).unwrap()
    );
    let split = [bytes(6, &scalar(1, 11)), bytes(6, &scalar(2, 22))].concat();
    let split_wire = bytes(1, &split);
    let parsed = generated::WithRequired::parse(&split_wire).unwrap();
    assert_eq!(parsed.child().required().left(), 11);
    assert_eq!(parsed.child().required().right(), 22);
    assert_eq!(
        reference::WithRequired::decode(split_wire.as_slice()).unwrap(),
        reference::WithRequired::decode(parsed.serialize().unwrap().as_slice()).unwrap()
    );
    let replaced = bytes(1, &[bytes(6, &scalar(1, 11)), scalar(7, 5)].concat());
    assert_eq!(
        generated::WithRequired::parse(&replaced)
            .unwrap()
            .child()
            .replacement(),
        5
    );
    assert!(generated::WithRequired::parse(&bytes(1, &bytes(6, &scalar(1, 11)))).is_err());
    let split_value = [
        scalar(1, 7),
        bytes(2, &scalar(1, 11)),
        bytes(2, &scalar(2, 22)),
    ]
    .concat();
    let collection_wire = bytes(1, &split_value);
    let collection = generated::RequiredCollections::parse(&collection_wire).unwrap();
    assert_eq!(collection.values().get(7).unwrap().left(), 11);
    assert_eq!(collection.values().get(7).unwrap().right(), 22);
    let complete = [scalar(1, 11), scalar(2, 22)].concat();
    let missing = [scalar(1, 7), bytes(2, &scalar(1, 11))].concat();
    let valid = [scalar(1, 7), bytes(2, &complete)].concat();
    assert!(
        generated::RequiredCollections::parse(&[bytes(1, &missing), bytes(1, &valid)].concat())
            .is_ok()
    );
    assert!(
        generated::RequiredCollections::parse(&[bytes(1, &valid), bytes(1, &missing)].concat())
            .is_err()
    );
    let reused = [bytes(2, &scalar(1, 11)), bytes(2, &bytes(2, &chain(4)))].concat();
    let choice_wire = bytes(1, &reused);
    let choice = generated::FreeChoice::parse(&choice_wire).unwrap();
    assert_eq!(choice.child().node().n(), 11);
    assert_eq!(choice.child().node().child().n(), 4);
    let switched = [bytes(2, &chain(4)), scalar(3, 99), bytes(2, &scalar(1, 7))].concat();
    let choice = generated::FreeChoice::parse(&bytes(1, &switched)).unwrap();
    assert_eq!(choice.child().node().n(), 7);
    assert!(!choice.child().node().has_child());
    println!(
        "PASS repeated/map/group child values, required final-tree checks and oneof replacement; safe fallback validations={collection_fallback_validations}"
    );
}

fn main() {
    default_layout();
    lazy_work();
    invalid_before_access();
    fresh_merge();
    unknown_fields_and_names();
    clone_and_threads();
    shared_input();
    collections_and_required();
    runtime_fallback::verify();
}

mod runtime_fallback;
