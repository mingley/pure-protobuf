use self::proof::ordered::{Child, Control, Grandparent, Parent, PbValue as Value, Recursive};
use self::proof::ordered_required::{Envelope, RequiredMaps};
use crate::reference::proof::ordered as peer;
use pbrs::{AsView, CopyFrom, MergeFrom, Parse, Serialize};
use prost::Message as _;

fn assert_child(child: &Child, encoded: &[u8]) {
    let expected = peer::Child::decode(encoded).unwrap();
    let actual = child.serialize().unwrap();
    assert_eq!(child.serialized_len(), actual.len());
    assert_eq!(peer::Child::decode(actual.as_slice()).unwrap(), expected);
    assert_eq!(Child::parse(&actual).unwrap(), *child);
}

fn duplicate_storage(profile: &str) {
    let wire = crate::numbers(&[("z", 2), ("a", 1), ("z", 9), ("é", 4)]);
    let mut child = Child::parse(&wire).unwrap();
    assert_eq!(child.numbers().len(), 3);
    assert_eq!(child.numbers().get("z"), Some(9));
    assert_eq!(
        child
            .numbers()
            .keys()
            .map(|key| key.as_bytes().to_vec())
            .collect::<Vec<_>>(),
        [b"a".to_vec(), b"z".to_vec(), "é".as_bytes().to_vec()]
    );
    assert_child(&child, &wire);
    assert_eq!(
        child.serialize().unwrap(),
        peer::Child::decode(wire.as_slice())
            .unwrap()
            .encode_to_vec()
    );
    assert!(child.numbers_mut().remove("z"));
    assert_eq!(child.numbers().get("z"), None);
    assert_eq!(child.numbers().len(), 2);
    // Unselected Map retains its established duplicate-pop behavior.
    let mut ordinary = Control::parse(&wire).unwrap();
    assert_eq!(ordinary.ordinary().get("z"), Some(9));
    assert!(ordinary.ordinary_mut().remove("z"));
    assert_eq!(ordinary.ordinary().get("z"), Some(2));
    println!("PASS ordered-case {profile} unique-sorted-and-legacy-unchanged");
}

fn parents(profile: &str) {
    let wire = crate::numbers(&[("z", 2), ("a", 1), ("z", 9)]);
    let normalized = peer::Child::decode(wire.as_slice())
        .unwrap()
        .encode_to_vec();
    let mut parent_wire = Vec::new();
    for number in [1, 2, 4, 6] {
        parent_wire.extend(crate::field(number, &wire));
    }
    let mut entry = crate::field(1, b"key");
    entry.extend(crate::field(2, &wire));
    parent_wire.extend(crate::field(3, &entry));
    let parent = Parent::parse(&parent_wire).unwrap();
    assert_child(parent.child(), &wire);
    assert_child(parent.children().get(0).unwrap().0, &wire);
    assert_child(parent.by_name().get("key").unwrap().0, &wire);
    assert_child(parent.selected(), &wire);
    assert_child(parent.presence(), &wire);
    let encoded = parent.serialize().unwrap();
    for number in [1, 2, 4, 6] {
        assert_eq!(crate::payloads(&encoded, number), [normalized.as_slice()]);
    }
    let map = crate::payloads(&encoded, 3);
    assert_eq!(crate::payloads(map[0], 2), [normalized.as_slice()]);
    assert_eq!(
        peer::Parent::decode(encoded.as_slice()).unwrap(),
        peer::Parent::decode(parent_wire.as_slice()).unwrap()
    );
    let grand = Grandparent::parse(&crate::field(1, &parent_wire)).unwrap();
    assert_eq!(grand.serialize().unwrap(), crate::field(1, &encoded));
    // Distinct occurrences merge their maps into the same singular child.
    let mut split = crate::field(1, &crate::numbers(&[("z", 1)]));
    split.extend(crate::field(1, &crate::numbers(&[("a", 2), ("z", 3)])));
    let split = Parent::parse(&split).unwrap();
    assert_eq!(split.child().numbers().get("z"), Some(3));
    assert_eq!(split.child().numbers().len(), 2);
    let mut nested = Parent::new();
    nested
        .nested_mut()
        .entries_mut()
        .copy_from([("z", 2), ("a", 1), ("z", 3)]);
    let nested = Parent::parse(&nested.serialize().unwrap()).unwrap();
    assert_eq!(
        nested
            .nested()
            .entries()
            .keys()
            .map(|key| key.as_bytes().to_vec())
            .collect::<Vec<_>>(),
        [b"a".to_vec(), b"z".to_vec()]
    );
    println!("PASS ordered-case {profile} singular-repeated-map-oneof-nested-ancestors");
}

fn mutations(profile: &str) {
    let mut child = pbrs::proto!(Child {
        numbers: [("z", 2), ("a", 1), ("z", 3)]
    });
    let first_size = child.serialized_len();
    assert_eq!(child.numbers().len(), 2);
    assert!(!child.numbers_mut().insert("a", 4));
    assert!(child.numbers_mut().insert("longer-key", 5));
    assert!(child.serialized_len() > first_size);
    let mut value = Value::new();
    value.set_first(7);
    child.values_mut().insert(2u32, value);
    child.values_mut().get_mut(2u32).unwrap().set_second(8);
    assert_eq!(child.values().get(2u32).unwrap().second(), 8);
    let cloned = child.clone();
    let mut copied = Child::new();
    copied.copy_from(cloned.as_view());
    assert_eq!(copied, cloned);
    let mut view_copy = Child::new();
    view_copy.numbers_mut().copy_from(child.numbers());
    view_copy.values_mut().copy_from_view(child.values());
    assert_eq!(
        view_copy
            .numbers()
            .iter()
            .map(|(key, value)| (key.as_bytes().to_vec(), value))
            .collect::<Vec<_>>(),
        child
            .numbers()
            .iter()
            .map(|(key, value)| (key.as_bytes().to_vec(), value))
            .collect::<Vec<_>>()
    );
    assert_eq!(view_copy.values().get(2u32).unwrap().second(), 8);
    copied.numbers_mut().clear();
    copied.merge_from(child.as_view());
    assert_eq!(copied, child);
    let mut replacement = pbrs::TreeMap::new();
    replacement.insert("replacement", 1i32);
    copied.set_numbers(replacement);
    assert_eq!(copied.numbers().len(), 1);
    assert_eq!(copied.numbers().get("replacement"), Some(1));
    let current = copied.serialize().unwrap();
    assert_eq!(copied.serialized_len(), current.len());
    assert_eq!(Child::parse(&current).unwrap(), copied);
    println!("PASS ordered-case {profile} proto-copy-merge-mutation-size-cache");
}

fn all_keys_values(profile: &str) {
    let fixture = peer::Child {
        numbers: [("a".into(), 1), ("é".into(), 2)].into(),
        labels: [
            (3, "three".into()),
            (-2, "minus".into()),
            (0, "zero".into()),
        ]
        .into(),
        flags: [(true, 2), (false, 1)].into(),
        signed: [(-300, 9), (0, 8), (i64::MAX, 7)].into(),
        payloads: [
            (u64::MAX, prost::bytes::Bytes::from_static(b"last")),
            (0, prost::bytes::Bytes::new()),
        ]
        .into(),
        values: [
            (
                2,
                peer::Value {
                    first: 1,
                    second: 2,
                },
            ),
            (0, peer::Value::default()),
        ]
        .into(),
        states: [(9, 1), (0, 0)].into(),
    };
    let wire = fixture.encode_to_vec();
    let child = Child::parse(&wire).unwrap();
    assert_child(&child, &wire);
    assert_eq!(child.flags().keys().collect::<Vec<_>>(), [false, true]);
    assert_eq!(child.labels().keys().collect::<Vec<_>>(), [-2, 0, 3]);
    assert_eq!(
        child.signed().keys().collect::<Vec<_>>(),
        [-300, 0, i64::MAX]
    );
    assert_eq!(child.payloads().keys().collect::<Vec<_>>(), [0, u64::MAX]);
    println!("PASS ordered-case {profile} bool-signed-unsigned-string-enum-bytes-message");
}

fn codecs(profile: &str) {
    let child = Child::parse(&crate::numbers(&[("z", 2), ("a", 1), ("z", 9)])).unwrap();
    assert_eq!(Child::from_json(&child.to_json().unwrap()).unwrap(), child);
    assert_eq!(Child::from_text(&child.to_text().unwrap()).unwrap(), child);
    let pool = std::sync::Arc::new(
        pbrs::DescriptorPool::from_file_descriptor_set(proof::ordered::FILE_DESCRIPTOR_SET)
            .unwrap(),
    );
    let dynamic = pbrs::DynamicMessage::parse_with_pool(
        pool.get_message("proof.ordered.Child").unwrap(),
        Some(pool),
        &child.serialize().unwrap(),
    )
    .unwrap();
    assert_eq!(dynamic.get_map(1).unwrap().len(), 2);
    assert_eq!(Child::parse(&dynamic.serialize().unwrap()).unwrap(), child);
    let mut parent = Parent::new();
    parent.set_child(child);
    // The archived05a3 runtime failed this case. The accepted canonical
    // descriptor lookup fix must support both default and selected parents.
    assert_eq!(
        Parent::from_json(&parent.to_json().unwrap()).unwrap(),
        parent
    );
    println!("PASS ordered-parent-json {profile} canonical-descriptor-roundtrip");
    assert_eq!(
        Parent::from_text(&parent.to_text().unwrap()).unwrap(),
        parent
    );
    println!("PASS ordered-case {profile} json-text-descriptor-roundtrips");
}

fn shared_bytes(profile: &str) {
    use prost::bytes::Bytes;
    struct Segments {
        bytes: Vec<u8>,
        shared: Vec<Bytes>,
    }
    impl pbrs::WireOut for Segments {
        fn put_u8(&mut self, byte: u8) {
            self.bytes.push(byte);
        }
        fn put_slice(&mut self, bytes: &[u8]) {
            self.bytes.extend_from_slice(bytes);
        }
        fn supports_shared(&self) -> bool {
            true
        }
        fn put_shared(&mut self, bytes: &Bytes) {
            self.bytes.extend_from_slice(bytes);
            self.shared.push(bytes.clone());
        }
    }
    let reference = peer::Child {
        payloads: [(3, Bytes::from(vec![7; 65536]))].into(),
        ..Default::default()
    };
    let frame = Bytes::from(reference.encode_to_vec());
    let child = Child::parse_bytes(frame.clone()).unwrap();
    let retained = child.payloads_shared(3).unwrap();
    let mut view_copy = Child::new();
    view_copy.payloads_mut().copy_from(child.payloads());
    assert_eq!(view_copy.payloads().get(3).unwrap(), retained.as_bytes());
    // Converting the actual tree view clones ProtoBytes owners. Iterating
    // byte slices into copy_from intentionally creates owned payload copies.
    assert_ne!(
        view_copy.payloads_shared(3).unwrap().as_bytes().as_ptr(),
        retained.as_bytes().as_ptr()
    );
    view_copy.payloads_mut().copy_from_view(child.payloads());
    assert_eq!(
        view_copy.payloads_shared(3).unwrap().as_bytes().as_ptr(),
        retained.as_bytes().as_ptr()
    );
    view_copy.set_payloads(pbrs::TreeMap::from(child.payloads()));
    assert_eq!(
        view_copy.payloads_shared(3).unwrap().as_bytes().as_ptr(),
        retained.as_bytes().as_ptr()
    );
    let start = frame.as_ptr().addr();
    let pointer = retained.as_bytes().as_ptr().addr();
    assert!(pointer >= start && pointer + retained.as_bytes().len() <= start + frame.len());
    let mut sink = Segments {
        bytes: Vec::new(),
        shared: Vec::new(),
    };
    child.encode(&mut sink).unwrap();
    assert_eq!(sink.bytes, reference.encode_to_vec());
    assert_eq!(sink.shared.len(), 1);
    assert_eq!(sink.shared[0].as_ptr(), retained.as_bytes().as_ptr());
    drop(child);
    drop(frame);
    assert_eq!(retained.as_bytes(), vec![7; 65536]);
    assert_eq!(sink.shared[0].as_ref(), retained.as_bytes());
    println!("PASS ordered-case {profile} selected-bytes-retention-segmented-send");
}

fn required(profile: &str) {
    let key = [0x08, 1];
    let mut incomplete_entry = key.to_vec();
    incomplete_entry.extend(crate::field(2, &[]));
    let mut complete_entry = key.to_vec();
    complete_entry.extend(crate::field(2, &[0x08, 7]));
    let incomplete = crate::field(1, &incomplete_entry);
    let complete = crate::field(1, &complete_entry);
    let mut replaced = incomplete.clone();
    replaced.extend_from_slice(&complete);
    let map = RequiredMaps::parse(&replaced).unwrap();
    assert_eq!(map.values().len(), 1);
    assert_eq!(map.values().get(1).unwrap().id(), 7);
    let mut invalid_last = complete.clone();
    invalid_last.extend_from_slice(&incomplete);
    assert!(RequiredMaps::parse(&invalid_last).is_err());
    let mut merged_entry = complete_entry;
    merged_entry.extend(crate::field(2, &[]));
    let merged = RequiredMaps::parse(&crate::field(1, &merged_entry)).unwrap();
    assert_eq!(merged.values().get(1).unwrap().id(), 7);
    let mut envelope = crate::field(1, &replaced);
    envelope.extend(crate::field(2, &replaced));
    envelope.push(0x1b); // group field3
    envelope.extend(crate::field(4, &replaced));
    envelope.push(0x1c);
    let envelope = Envelope::parse(&envelope).unwrap();
    assert_eq!(envelope.child().values().get(1).unwrap().id(), 7);
    assert_eq!(
        envelope
            .children()
            .get(0)
            .unwrap()
            .values()
            .get(1)
            .unwrap()
            .id(),
        7
    );
    assert_eq!(envelope.group().child().values().get(1).unwrap().id(), 7);
    assert_eq!(
        RequiredMaps::parse_dont_enforce_required(&incomplete)
            .unwrap()
            .values()
            .len(),
        1
    );
    println!("PASS ordered-case {profile} required-live-winners-entry-merge-groups");
}

fn depth(profile: &str) {
    let mut shallow = Vec::new();
    for _ in 0..4 {
        shallow = crate::field(2, &shallow);
    }
    assert!(Recursive::parse(&shallow).is_ok());
    assert!(peer::Recursive::decode(shallow.as_slice()).is_ok());
    let mut deep = Vec::new();
    for _ in 0..=pbrs::RECURSION_LIMIT {
        deep = crate::field(2, &deep);
    }
    assert!(Recursive::parse(&deep).is_err());
    assert!(peer::Recursive::decode(deep.as_slice()).is_err());
    assert!(Recursive::parse(&[0x12, 1, 0x08]).is_err());
    println!("PASS ordered-case {profile} bounded-depth-malformed-descendants");
}

pub fn run(profile: &str) {
    duplicate_storage(profile);
    parents(profile);
    mutations(profile);
    all_keys_values(profile);
    codecs(profile);
    shared_bytes(profile);
    required(profile);
    depth(profile);
}
