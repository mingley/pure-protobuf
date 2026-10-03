//! Bounded 100/101 recursion oracles for the actual linked rust_out arena kernel.
//! This baseline is intentionally red until QG-19; it never builds a crash workload.
#![forbid(unsafe_code)]

use pbrs::runtime::AssociatedMiniTable;
use pbrs::{ClearAndParse, MergeFrom, Parse, ParseError, RECURSION_LIMIT, Serialize};
use rust_out_shared::map_unittest_rust_proto::MessageContainingEnumCalledType as RecursiveMap;
use rust_out_shared::unittest_rust_proto::{NestedTestAllTypes as Node, TestRequired, TestRequiredForeign};

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push(u8::try_from(value & 127).expect("seven bits") | 128);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("last seven bits"));
}

fn len_field(number: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    varint(&mut out, u64::from(number) * 8 + 2);
    varint(&mut out, u64::try_from(payload.len()).expect("bounded fixture length"));
    out.extend_from_slice(payload);
    out
}

fn chain(number: u32, edges: u32, mut leaf: Vec<u8>) -> Vec<u8> {
    assert!(edges <= RECURSION_LIMIT + 1, "fixtures stop at 101 known edges");
    for _ in 0..edges {
        leaf = len_field(number, &leaf);
    }
    leaf
}

fn groups(count: u32) -> Vec<u8> {
    assert!(count <= 3, "only small mixed-group fixtures");
    let mut out = Vec::new();
    for _ in 0..count {
        varint(&mut out, 127 * 8 + 3);
    }
    out.extend_from_slice(&[0x08, 7]);
    for _ in 0..count {
        varint(&mut out, 127 * 8 + 4);
    }
    out
}

fn maps(pairs: u32, mut leaf: Vec<u8>) -> Vec<u8> {
    assert!(pairs <= 51, "map entry plus value uses two frames");
    for _ in 0..pairs {
        let mut entry = len_field(1, b"k");
        entry.extend(len_field(2, &leaf));
        leaf = len_field(1, &entry);
    }
    leaf
}

fn assert_linked<T: AssociatedMiniTable>() {
    assert!(!T::mini_table().0.is_null(), "actual linked rust_out MiniTable");
    assert_eq!(RECURSION_LIMIT, 100, "existing policy, not a new limit");
}

#[derive(Clone, Copy, Debug)]
enum Entry {
    Clear,
    ClearNoRequired,
    Merge,
    MergeNoRequired,
}

impl Entry {
    fn apply(self, msg: &mut Node, bytes: &[u8], mutable_proxy: bool) -> Result<(), ParseError> {
        if mutable_proxy {
            let mut msg = msg.as_mut();
            match self {
                Self::Clear => msg.clear_and_parse(bytes),
                Self::ClearNoRequired => msg.clear_and_parse_dont_enforce_required(bytes),
                Self::Merge => msg.merge_from_bytes(bytes),
                Self::MergeNoRequired => msg.merge_from_bytes_dont_enforce_required(bytes),
            }
        } else {
            match self {
                Self::Clear => msg.clear_and_parse(bytes),
                Self::ClearNoRequired => msg.clear_and_parse_dont_enforce_required(bytes),
                Self::Merge => msg.merge_from_bytes(bytes),
                Self::MergeNoRequired => msg.merge_from_bytes_dont_enforce_required(bytes),
            }
        }
    }
}

fn public_entry(entry: Entry) {
    assert_linked::<Node>();
    let at_limit = chain(1, 100, Vec::new());
    let over_limit = chain(1, 101, Vec::new());
    assert!(over_limit.len() < 1024, "bounded raw oracle");
    for mutable_proxy in [false, true] {
        let mut msg = Node::new();
        msg.payload_mut().set_optional_int32(7);
        entry.apply(&mut msg, &at_limit, mutable_proxy).expect("100 known frames");
        let mut expected = at_limit.clone();
        if matches!(entry, Entry::Merge | Entry::MergeNoRequired) {
            expected.extend(len_field(2, &[8, 7]));
            assert_eq!(msg.payload().optional_int32(), 7);
        } else {
            assert!(!msg.has_payload(), "clear entry actually cleared the old payload");
        }
        assert_eq!(msg.serialize().expect("serialize bounded tree"), expected);
        let mut rejected = Node::new();
        assert!(entry.apply(&mut rejected, &over_limit, mutable_proxy).is_err(),
                "101 known frames must be rejected by {entry:?}, mutable_proxy={mutable_proxy}");
        assert!(!rejected.has_child(), "failed child must not be installed");
    }
}

#[test]
fn arena_clear_and_parse_known_depth_100_101() { public_entry(Entry::Clear); }

#[test]
fn arena_clear_no_required_known_depth_100_101() { public_entry(Entry::ClearNoRequired); }

#[test]
fn arena_merge_bytes_known_depth_100_101() { public_entry(Entry::Merge); }

#[test]
fn arena_merge_bytes_no_required_known_depth_100_101() { public_entry(Entry::MergeNoRequired); }

#[test]
fn arena_shared_parse_has_same_known_depth_budget() {
    assert_linked::<Node>();
    Node::parse_bytes(chain(1, 100, Vec::new()).into()).expect("shared 100 frames");
    assert!(Node::parse_bytes(chain(1, 101, Vec::new()).into()).is_err(),
            "shared 101 frames must not escape the same kernel budget");
}

#[test]
fn arena_repeated_message_depth_100_101() {
    assert_linked::<Node>();
    let accepted = chain(3, 100, Vec::new());
    let msg = Node::parse(&accepted).expect("100 repeated-message frames");
    assert_eq!(msg.serialize().expect("repeat roundtrip"), accepted);
    assert!(Node::parse(&chain(3, 101, Vec::new())).is_err(), "101 repeated-message frames");
}

#[test]
fn arena_lazy_declared_message_depth_100_101() {
    assert_linked::<Node>();
    let accepted = chain(4, 100, Vec::new());
    let msg = Node::parse(&accepted).expect("100 lazy-declared messages in arena decoder");
    assert_eq!(msg.serialize().expect("lazy-declared roundtrip"), accepted);
    assert!(Node::parse(&chain(4, 101, Vec::new())).is_err(), "101 lazy-declared frames");
}

#[test]
fn arena_recursive_map_entry_depth_100_101() {
    assert_linked::<RecursiveMap>();
    let accepted = maps(50, Vec::new()); // entry1,value2,...,entry99,value100.
    let msg = RecursiveMap::parse(&accepted).expect("50 entry/value pairs = 100 frames");
    assert_eq!(msg.serialize().expect("map roundtrip"), accepted);
    let entry_at_101 = len_field(1, &len_field(1, b"last")); // key only, no value frame.
    let rejected = maps(50, entry_at_101);
    assert!(rejected.len() < 1024);
    assert!(RecursiveMap::parse(&rejected).is_err(), "map entry at 101 must be counted even without a value");
}

#[test]
fn arena_recursive_map_value_depth_100_102() {
    assert_linked::<RecursiveMap>();
    // This upstream schema has even-depth message values. The distinct 101
    // entry oracle above pins the odd boundary; do not call this a 101 value.
    RecursiveMap::parse(&maps(50, Vec::new())).expect("message value at 100");
    assert!(RecursiveMap::parse(&maps(51, Vec::new())).is_err(), "entry101/value102 pair");
}

#[test]
fn arena_known_messages_compose_unknown_group_depth() {
    assert_linked::<Node>();
    let accepted = chain(1, 99, groups(1));
    let msg = Node::parse(&accepted).expect("99 messages + one group = 100");
    assert_eq!(msg.serialize().expect("unknown groups drop"), chain(1, 99, Vec::new()));
    assert!(Node::parse(&chain(1, 99, groups(2))).is_err(), "99 messages + two groups = 101");
}

#[test]
fn arena_map_values_compose_unknown_group_depth() {
    assert_linked::<RecursiveMap>();
    let accepted = maps(49, groups(2)); // value98 + groups99,100.
    let msg = RecursiveMap::parse(&accepted).expect("98 map frames + two groups");
    assert_eq!(msg.serialize().expect("unknown groups drop"), maps(49, Vec::new()));
    assert!(RecursiveMap::parse(&maps(49, groups(3))).is_err(), "98 map frames + three groups = 101");
}

#[test]
fn arena_map_entry_unknowns_use_entry_depth() {
    assert_linked::<RecursiveMap>();
    let entry = |count| {
        let mut payload = len_field(1, b"last");
        payload.extend(groups(count));
        len_field(1, &payload)
    };
    RecursiveMap::parse(&maps(49, entry(1))).expect("98 outer + entry99 + group100");
    assert!(RecursiveMap::parse(&maps(49, entry(2))).is_err(), "entry99 + two groups = 101");
}

fn owned_chain(edges: u32) -> Node {
    let mut node = Node::new();
    for _ in 0..edges {
        let mut parent = Node::new();
        parent.set_child(node);
        node = parent;
    }
    node
}

#[test]
fn arena_programmatic_merge_from_has_bounded_decode_side() {
    assert_linked::<Node>();
    let src = owned_chain(100);
    assert_eq!(
        src.serialize().expect("constructed source100"),
        chain(1, 100, Vec::new()),
        "safe setters must construct the exact source before MergeFrom"
    );
    let mut dst = Node::new();
    dst.merge_from(src.as_view());
    assert_eq!(dst.serialize().expect("merge100"), chain(1, 100, Vec::new()));
    let oversized = owned_chain(101);
    assert_eq!(
        oversized.serialize().expect("constructed source101"),
        chain(1, 101, Vec::new()),
        "safe setters must not silently collapse the oversized source"
    );
    let mut rejected = Node::new();
    rejected.payload_mut().set_optional_int32(7);
    // The existing public MergeFrom returns (), so rejection stays an ignored
    // parse error. The first failed child must not replace/install a root slot.
    rejected.merge_from(oversized.as_view());
    assert!(!rejected.has_child(), "MergeFrom decode must reject child101");
    assert_eq!(rejected.payload().optional_int32(), 7);
}

#[test]
fn arena_sibling_branches_reuse_the_depth_budget() {
    assert_linked::<Node>();
    let branch = len_field(3, &chain(1, 99, Vec::new()));
    let mut bytes = Vec::new();
    for _ in 0..8 { bytes.extend_from_slice(&branch); }
    assert!(bytes.len() < 8192, "eight small branches, never a stress workload");
    let msg = Node::parse(&bytes).expect("each sibling path is exactly 100 frames");
    assert_eq!(msg.repeated_child().len(), 8);
    assert_eq!(msg.serialize().expect("siblings roundtrip"), bytes);
}

#[test]
fn arena_required_checks_and_opaque_unknowns_are_preserved() {
    assert_linked::<Node>();
    assert_linked::<TestRequired>();
    assert!(TestRequired::parse(&[]).is_err());
    let mut required = TestRequired::new();
    required.clear_and_parse_dont_enforce_required(&[]).expect("existing root no-required path");
    assert!(TestRequiredForeign::parse(&len_field(1, &[])).is_err(), "singular required child stays enforced");
    let mut parent = TestRequiredForeign::new();
    assert!(parent.clear_and_parse_dont_enforce_required(&len_field(1, &[])).is_err(),
            "existing singular child enforcement is not silently changed");
    // Unknown length-delimited data is opaque. Do not count or validate it as a message.
    let opaque = Node::parse(&len_field(127, &[0xff])).expect("opaque unknown LEN");
    assert!(opaque.serialize().expect("dropped unknown LEN").is_empty());
    let mut truncated = Vec::new();
    varint(&mut truncated, 127 * 8 + 3);
    assert!(Node::parse(&truncated).is_err());
}
