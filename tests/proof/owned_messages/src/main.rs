mod map_spans;

use pbrs::{ClearAndParse, Parse, Serialize};
use prost::Message;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

mod owned {
    pub mod types {
        include!(concat!(env!("OUT_DIR"), "/owned/proof.rs"));
    }
    pub mod strict_types {
        include!(concat!(env!("OUT_DIR"), "/owned/strict.rs"));
    }
}
mod lazy {
    pub mod types {
        include!(concat!(env!("OUT_DIR"), "/lazy/proof.rs"));
    }
    pub mod strict_types {
        include!(concat!(env!("OUT_DIR"), "/lazy/strict.rs"));
    }
}
mod defaulted {
    pub mod types {
        include!(concat!(env!("OUT_DIR"), "/default/proof.rs"));
    }
    pub mod strict_types {
        include!(concat!(env!("OUT_DIR"), "/default/strict.rs"));
    }
}
mod reference {
    include!(concat!(env!("OUT_DIR"), "/reference/linear.rs"));
}
mod strict_reference {
    include!(concat!(env!("OUT_DIR"), "/reference/strict.rs"));
}

struct Counting;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc(layout) };
        if !result.is_null() && COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let result = unsafe { System.alloc_zeroed(layout) };
        if !result.is_null() && COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, layout, size) };
        if !result.is_null() && COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        result
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

fn varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        out.push(n as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn scalar(number: u32, n: u64) -> Vec<u8> {
    let mut wire = vec![];
    varint(&mut wire, u64::from(number) << 3);
    varint(&mut wire, n);
    wire
}
fn bytes(number: u32, data: &[u8]) -> Vec<u8> {
    let mut wire = vec![];
    varint(&mut wire, (u64::from(number) << 3) | 2);
    varint(&mut wire, data.len() as u64);
    wire.extend_from_slice(data);
    wire
}
fn chain(depth: u32) -> Vec<u8> {
    let mut wire = scalar(1, 1);
    for n in 2..=depth {
        wire = [scalar(1, u64::from(n)), bytes(2, &wire)].concat();
    }
    wire
}
fn allocated(operation: impl FnOnce()) -> usize {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    operation();
    COUNTING.store(false, Ordering::Relaxed);
    ALLOCATIONS.load(Ordering::Relaxed)
}
macro_rules! verify_profile {
    ($module:ident, $owned:expr) => {{
        use $module::strict_types as strict_generated;
        use $module::types as generated;

        let wire = chain(64);
        let root = generated::Node::parse(&wire).unwrap();
        let original = root.serialize().unwrap();
        assert_eq!(
            reference::Node::decode(wire.as_slice()).unwrap(),
            reference::Node::decode(original.as_slice()).unwrap()
        );
        let mut checksum = 0;
        let first = root.child();
        // The owned profile has already constructed every child during parsing.
        // Default/lazy traversal allocations are recorded separately; neither
        // profile uses a timing threshold in this correctness check.
        let later_allocations = allocated(|| {
            let mut cursor = first;
            checksum += cursor.n();
            while cursor.has_child() {
                cursor = cursor.child();
                checksum += cursor.n();
            }
        });
        assert_eq!(checksum, (1..64).sum::<i32>());
        if $owned {
            assert_eq!(later_allocations, 0);
        }
        assert_eq!(root.serialize().unwrap(), original);

        let mut merged = root.clone();
        merged.merge_from_bytes(&bytes(2, &scalar(1, 99))).unwrap();
        assert_eq!(merged.child().n(), 99);
        assert_eq!(merged.child().child().n(), 62);
        assert_eq!(root.child().n(), 63);

        // The first unread descendant still has to be valid during ordinary parse.
        let legacy_bytes = bytes(2, &bytes(2, &bytes(3, &[255])));
        assert!(generated::Node::parse(&legacy_bytes).is_ok());
        let invalid = bytes(1, &bytes(1, &bytes(2, &[255])));
        assert!(strict_generated::StrictNode::parse(&invalid).is_err());
        assert!(strict_reference::StrictNode::decode(invalid.as_slice()).is_err());
        assert!(generated::Node::parse(&bytes(2, &[0x80])).is_err());
        assert!(generated::Node::parse(&chain(pbrs::RECURSION_LIMIT + 2)).is_err());

        // Owned parsing still retains unknown fields through mutation/serialization.
        let unknown = [chain(3), scalar(99, 1234)].concat();
        let parsed = generated::Node::parse(&unknown).unwrap();
        let roundtrip = parsed.serialize().unwrap();
        assert!(roundtrip.ends_with(&scalar(99, 1234)));
        let mut updated = parsed.clone();
        updated.merge_from_bytes(&scalar(1, 77)).unwrap();
        assert!(updated.serialize().unwrap().ends_with(&scalar(99, 1234)));
        assert_eq!(updated.n(), 77);

        // Failed first occurrences stay absent; existing children retain the
        // ordinary partial-merge state after a later malformed fragment.
        let mut absent = generated::Node::default();
        assert!(absent.merge_from_bytes(&bytes(2, &[0x80])).is_err());
        assert!(!absent.has_child());
        let mut existing = generated::Node::parse(&bytes(2, &scalar(1, 7))).unwrap();
        assert!(
            existing
                .merge_from_bytes(&bytes(2, &[0x08, 0x09, 0x80]))
                .is_err()
        );
        assert!(existing.has_child());
        assert_eq!(existing.child().n(), 9);
        let bad_group = [vec![(4 << 3) | 3], bytes(5, &[0x80]), vec![(4 << 3) | 4]].concat();
        let mut absent_group = generated::Mixed::default();
        assert!(absent_group.merge_from_bytes(&bad_group).is_err());
        assert!(!absent_group.has_group());
        let initial_group = [
            vec![(4 << 3) | 3],
            bytes(5, &scalar(1, 7)),
            vec![(4 << 3) | 4],
        ]
        .concat();
        let mut existing_group = generated::Mixed::parse(&initial_group).unwrap();
        let truncated_group = [vec![(4 << 3) | 3], bytes(5, &scalar(1, 9))].concat();
        assert!(existing_group.merge_from_bytes(&truncated_group).is_err());
        assert!(existing_group.has_group());
        assert_eq!(existing_group.group().node().n(), 9);

        // Required descendants are checked on the final merged tree.
        let split = [bytes(6, &scalar(1, 11)), bytes(6, &scalar(2, 22))].concat();
        let mixed = generated::WithRequired::parse(&bytes(1, &split)).unwrap();
        assert_eq!(
            reference::WithRequired::decode(bytes(1, &split).as_slice()).unwrap(),
            reference::WithRequired::decode(mixed.serialize().unwrap().as_slice()).unwrap()
        );
        assert_eq!(mixed.child().required().left(), 11);
        assert_eq!(mixed.child().required().right(), 22);
        let replaced = [bytes(6, &scalar(1, 11)), scalar(7, 5)].concat();
        let mixed = generated::WithRequired::parse(&bytes(1, &replaced)).unwrap();
        assert_eq!(
            reference::WithRequired::decode(bytes(1, &replaced).as_slice()).unwrap(),
            reference::WithRequired::decode(mixed.serialize().unwrap().as_slice()).unwrap()
        );
        assert_eq!(mixed.child().replacement(), 5);

        // Reused free oneof cases merge; switching away and back replaces the tree.
        let reused = [bytes(2, &scalar(1, 11)), bytes(2, &bytes(2, &chain(4)))].concat();
        let choice = generated::FreeChoice::parse(&bytes(1, &reused)).unwrap();
        assert_eq!(
            reference::FreeChoice::decode(bytes(1, &reused).as_slice()).unwrap(),
            reference::FreeChoice::decode(choice.serialize().unwrap().as_slice()).unwrap()
        );
        assert_eq!(choice.child().node().n(), 11);
        assert_eq!(choice.child().node().child().n(), 4);
        let switched = [bytes(2, &chain(4)), scalar(3, 99), bytes(2, &scalar(1, 7))].concat();
        let choice = generated::FreeChoice::parse(&bytes(1, &switched)).unwrap();
        assert_eq!(
            reference::FreeChoice::decode(bytes(1, &switched).as_slice()).unwrap(),
            reference::FreeChoice::decode(choice.serialize().unwrap().as_slice()).unwrap()
        );
        assert_eq!(choice.child().node().n(), 7);
        assert!(!choice.child().node().has_child());

        // All storage profiles preserve repeated, map and group descendant values.
        let node = chain(8);
        let entry = [scalar(1, 7), bytes(2, &node)].concat();
        let group = [vec![(4 << 3) | 3], bytes(5, &node), vec![(4 << 3) | 4]].concat();
        let body = [bytes(2, &node), bytes(3, &entry), group].concat();
        let mixed = generated::Mixed::parse(&bytes(1, &body)).unwrap();
        assert_eq!(
            reference::Mixed::decode(bytes(1, &body).as_slice()).unwrap(),
            reference::Mixed::decode(mixed.serialize().unwrap().as_slice()).unwrap()
        );
        let child = mixed.child();
        assert_eq!(child.nodes().get(0).unwrap().child().n(), 7);
        assert_eq!(child.values().get(7).unwrap().child().n(), 7);
        assert_eq!(child.group().node().child().n(), 7);

        // Shared input keeps large singular bytes shared. The owned profile
        // propagates the same source through repeated children and map values.
        let payload = vec![7u8; 64 * 1024];
        let blob = bytes(1, &payload);
        let shared = pbrs::rt::Bytes::from(bytes(1, &blob));
        let mut parsed = strict_generated::BlobRoot::parse_bytes(shared.clone()).unwrap();
        let expected_pointer = shared.as_ptr().wrapping_add(shared.len() - payload.len());
        assert_eq!(parsed.child().data().as_ptr(), expected_pointer);
        assert_eq!(
            strict_reference::BlobRoot::decode(shared.as_ref()).unwrap(),
            strict_reference::BlobRoot::decode(parsed.serialize().unwrap().as_slice()).unwrap()
        );
        let untouched = parsed.clone();
        parsed.child_mut().set_data(vec![9u8; payload.len()]);
        assert_eq!(untouched.child().data(), payload.as_slice());
        assert_eq!(&shared[shared.len() - payload.len()..], payload.as_slice());
        assert!(parsed.child().data().iter().all(|byte| *byte == 9));
        assert_eq!(
            strict_reference::BlobRoot::decode(parsed.serialize().unwrap().as_slice())
                .unwrap()
                .child
                .unwrap()
                .data,
            vec![9u8; payload.len()]
        );
        let shared_repeated = pbrs::rt::Bytes::from(bytes(2, &blob));
        let repeated = strict_generated::BlobRoot::parse_bytes(shared_repeated.clone()).unwrap();
        let repeated_pointer = shared_repeated
            .as_ptr()
            .wrapping_add(shared_repeated.len() - payload.len());
        let repeated_shared =
            repeated.children().get(0).unwrap().data().as_ptr() == repeated_pointer;
        let entry = [scalar(1, 7), bytes(2, &blob)].concat();
        let shared_map = pbrs::rt::Bytes::from(bytes(3, &entry));
        let mapped = strict_generated::BlobRoot::parse_bytes(shared_map.clone()).unwrap();
        let map_pointer = shared_map
            .as_ptr()
            .wrapping_add(shared_map.len() - payload.len());
        let map_shared = mapped.values().get(7).unwrap().data().as_ptr() == map_pointer;
        assert_eq!(
            repeated.children().get(0).unwrap().data(),
            payload.as_slice()
        );
        assert_eq!(mapped.values().get(7).unwrap().data(), payload.as_slice());
        if $owned {
            assert!(repeated_shared, "owned repeated child copied shared bytes");
            assert!(map_shared, "owned map value copied shared bytes");
        }
        println!(
            "PASS {} large-byte sharing: singular=true repeated={} map={}",
            stringify!($module),
            repeated_shared,
            map_shared
        );
        println!(
            "PASS {} semantics against Prost, later getter allocations={}",
            stringify!($module),
            later_allocations
        );
    }};
}

fn reference_compact(wire: &[u8]) -> strict_reference::CompactNode {
    strict_reference::CompactNode::decode(wire).unwrap()
}

fn main() {
    map_spans::verify();
    verify_profile!(owned, true);
    verify_profile!(lazy, false);
    verify_profile!(defaulted, false);
    let wire = chain(64);
    let compact = owned::strict_types::CompactNode::parse(&wire).unwrap();
    assert_eq!(
        reference_compact(&wire),
        reference_compact(&compact.serialize().unwrap())
    );
    let owned_node_size = std::mem::size_of::<owned::strict_types::CompactNode>();
    let lazy_node_size = std::mem::size_of::<lazy::strict_types::CompactNode>();
    assert!(
        owned_node_size < lazy_node_size,
        "owned profile did not reduce compact node storage"
    );
    println!("PASS compact node layout: owned={owned_node_size} lazy={lazy_node_size}");
    let mut owned_root = None;
    let owned_allocations =
        allocated(|| owned_root = Some(owned::types::Node::parse(&wire).unwrap()));
    let mut lazy_root = None;
    let lazy_allocations = allocated(|| lazy_root = Some(lazy::types::Node::parse(&wire).unwrap()));
    let mut default_root = None;
    let default_allocations =
        allocated(|| default_root = Some(defaulted::types::Node::parse(&wire).unwrap()));
    assert_eq!(owned_allocations, 63, "one box for each actual child");
    assert_eq!(
        lazy_allocations, default_allocations,
        "owned setting leaked into default"
    );
    assert!(lazy_allocations < owned_allocations);
    println!(
        "PASS profile work distinction: owned parse {owned_allocations} allocations, lazy/default {lazy_allocations}/{default_allocations}"
    );
}
