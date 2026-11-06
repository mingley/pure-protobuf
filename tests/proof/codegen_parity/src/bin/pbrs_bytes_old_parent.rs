use pbrs::{Parse, Serialize};
use prost::Message;
include!(concat!(env!("OUT_DIR"), "/native-bytes/mod.rs"));
include!("../bytes_reference.rs");

mod old_parent {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/old_bytes_parent/bytes_old_parent.rs"
    ));
}
mod reference_old {
    include!(concat!(
        env!("OUT_DIR"),
        "/prost-bytes-old-parent/proof.bytes_old.rs"
    ));
}
mod current_parent {
    include!(concat!(
        env!("OUT_DIR"),
        "/native-bytes-old-parent/bytes_old_parent.rs"
    ));
}

fn main() {
    use bytes::Bytes;
    let child = reference::proof::bytes::Child {
        payload: Bytes::from_static(b"old-parent-copy"),
    };
    let reference = reference_old::OldEnvelope {
        children: vec![child.clone()],
        nodes: [(3, child.clone())].into(),
        singular: Some(child),
    };
    let frame = Bytes::from(reference.encode_to_vec());
    let old = old_parent::OldEnvelope::parse_bytes(frame.clone()).unwrap();
    let current = current_parent::OldEnvelope::parse_bytes(frame.clone()).unwrap();
    assert!(!within(
        old.children().get(0).unwrap().payload_shared().as_bytes(),
        &frame
    ));
    assert!(!within(
        old.nodes().get(3).unwrap().payload_shared().as_bytes(),
        &frame
    ));
    assert!(within(old.singular().payload_shared().as_bytes(), &frame));
    assert!(!within(
        current
            .children()
            .get(0)
            .unwrap()
            .payload_shared()
            .as_bytes(),
        &frame
    ));
    assert!(!within(
        current.nodes().get(3).unwrap().payload_shared().as_bytes(),
        &frame
    ));
    assert!(within(
        current.singular().payload_shared().as_bytes(),
        &frame
    ));
    assert_eq!(
        reference_old::OldEnvelope::decode(Serialize::serialize(&old).unwrap().as_slice()).unwrap(),
        reference
    );
    assert_eq!(
        reference_old::OldEnvelope::decode(Serialize::serialize(&current).unwrap().as_slice())
            .unwrap(),
        reference
    );
    println!("PASS bytes-old-parent eager-copy-lazy-share-and-correct-values");

    // Preserve this existing map gap as a counterexample, rather than imply
    // that the bytes representation option changes map mutation semantics.
    let duplicate_bytes = [
        0x22, 8, 0x0a, 3, b'k', b'e', b'y', 0x12, 1, 2, 0x22, 8, 0x0a, 3, b'k', b'e', b'y', 0x12,
        1, 19,
    ];
    let mut selected = proof::bytes::Flat::parse(&duplicate_bytes).unwrap();
    let mut peer = reference::proof::bytes::Flat::decode(duplicate_bytes.as_slice()).unwrap();
    assert_eq!(selected.lookup_shared("key").unwrap().as_bytes(), &[19]);
    assert!(selected.lookup_mut().remove("key"));
    assert_eq!(peer.lookup.remove("key").unwrap().as_ref(), &[19]);
    assert_eq!(selected.lookup_shared("key").unwrap().as_bytes(), &[2]);
    assert!(!peer.lookup.contains_key("key"));
    let duplicate_children = [
        0x12, 7, 0x08, 3, 0x12, 3, 0x0a, 1, 2, 0x12, 7, 0x08, 3, 0x12, 3, 0x0a, 1, 19,
    ];
    let mut old = old_parent::OldEnvelope::parse(&duplicate_children).unwrap();
    let mut current = current_parent::OldEnvelope::parse(&duplicate_children).unwrap();
    let mut peer = reference_old::OldEnvelope::decode(duplicate_children.as_slice()).unwrap();
    assert!(old.nodes_mut().remove(3));
    assert!(current.nodes_mut().remove(3));
    assert_eq!(peer.nodes.remove(&3).unwrap().payload.as_ref(), &[19]);
    assert_eq!(old.nodes().get(3).unwrap().payload(), &[2]);
    assert_eq!(current.nodes().get(3).unwrap().payload(), &[2]);
    assert!(!peer.nodes.contains_key(&3));
    println!(
        "KNOWN-GAP bytes-map-remove old-gencode=Some(2) current-gencode=Some(2) selected-bytes=Some(2) prost=None"
    );
}
