use pbrs::{Parse, Serialize};
use prost::Message;

mod reference {
    pub mod proof {
        pub mod boxed_external {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-boxed/proof.boxed_external.rs"
            ));
        }
        pub mod boxed_required {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-boxed/proof.boxed_required.rs"
            ));
        }
        pub mod boxed {
            include!(concat!(env!("OUT_DIR"), "/prost-boxed/proof.boxed.rs"));
        }
    }
}

use proof::boxed::{Leaf, Node, Parent};
use reference::proof::boxed::{
    Leaf as ReferenceLeaf, Node as ReferenceNode, Parent as ReferenceParent,
};

fn leaf(value: i32) -> Leaf {
    let mut leaf = Leaf::new();
    leaf.set_value(value);
    leaf.set_payload(vec![0x93; 4096]);
    leaf.set_label(format!("leaf-{value}"));
    leaf
}

fn reference_leaf(value: i32) -> ReferenceLeaf {
    ReferenceLeaf {
        value,
        payload: vec![0x93; 4096],
        label: format!("leaf-{value}"),
    }
}

fn fixtures() -> (Parent, ReferenceParent) {
    let mut native = Parent::new();
    native.set_chosen(leaf(1));
    native.set_deferred(leaf(2));
    native.many_mut().push(leaf(7));
    native.lookup_mut().insert(9, leaf(8));
    native.set_one(leaf(3));
    native.set_lone(leaf(4));
    native.set_optional_child(leaf(5));
    let mut imported = proof::boxed_external::Imported::new();
    imported.set_payload(vec![0x71; 4096]);
    imported.nested_mut().set_value(6);
    native.set_imported(imported);
    let mut required = proof::boxed_required::RequiredChild::new();
    required.set_value(0);
    required.set_payload(vec![0x59; 4096]);
    native.set_required_child(required);

    let reference = ReferenceParent {
        chosen: Some(Box::new(reference_leaf(1))),
        deferred: Some(reference_leaf(2)),
        many: vec![reference_leaf(7)],
        lookup: [(9, reference_leaf(8))].into(),
        choice: Some(reference::proof::boxed::parent::Choice::One(Box::new(
            reference_leaf(3),
        ))),
        single: Some(reference::proof::boxed::parent::Single::Lone(Box::new(
            reference_leaf(4),
        ))),
        optional_child: Some(Box::new(reference_leaf(5))),
        imported: Some(Box::new(reference::proof::boxed_external::Imported {
            payload: vec![0x71; 4096],
            nested: Some(reference::proof::boxed_external::imported::Nested { value: 6 }),
        })),
        required_child: Some(Box::new(reference::proof::boxed_required::RequiredChild {
            value: 0,
            payload: Some(vec![0x59; 4096]),
        })),
    };
    // Compile-time controls exercise the actual pinned Prost field types.
    let _: &Option<Box<ReferenceLeaf>> = &reference.chosen;
    let _: &Option<ReferenceLeaf> = &reference.deferred;
    let _: &Vec<ReferenceLeaf> = &reference.many;
    let _: &std::collections::HashMap<i32, ReferenceLeaf> = &reference.lookup;
    (native, reference)
}

fn assert_values(native: &Parent, reference: &ReferenceParent) {
    assert_eq!(
        ReferenceParent::decode(Serialize::serialize(native).unwrap().as_slice()).unwrap(),
        *reference
    );
}

fn delimited(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut result = vec![tag];
    let mut len = payload.len();
    while len >= 128 {
        result.push((len as u8 & 0x7f) | 0x80);
        len >>= 7;
    }
    result.push(len as u8);
    result.extend_from_slice(payload);
    result
}

fn main() {
    let (native, reference) = fixtures();
    assert_values(&native, &reference);
    assert_values(
        &Parent::parse(&reference.encode_to_vec()).unwrap(),
        &reference,
    );
    assert_eq!(Parent::default(), Parent::new());
    println!("PASS boxed-case roundtrip-setters-defaults");

    let wire = bytes::Bytes::from(reference.encode_to_vec());
    let start = wire.as_ptr() as usize;
    let end = start + wire.len();
    let parsed = Parent::parse_bytes(wire.clone()).unwrap();
    let pointer = parsed.chosen().payload().as_ptr() as usize;
    assert!(pointer >= start && pointer + 4096 <= end);
    let imported_pointer = parsed.imported().payload().as_ptr() as usize;
    assert!(imported_pointer >= start && imported_pointer + 4096 <= end);
    let mut cloned = parsed.clone();
    assert_eq!(cloned.chosen().payload().as_ptr() as usize, pointer);
    drop(parsed);
    drop(wire);
    assert_eq!(cloned.chosen().payload(), &[0x93; 4096]);
    cloned.chosen_mut().set_value(99);
    assert_eq!(
        ReferenceParent::decode(Serialize::serialize(&cloned).unwrap().as_slice())
            .unwrap()
            .chosen
            .unwrap()
            .value,
        99
    );
    assert_eq!(cloned.imported().nested().value(), 6);
    println!("PASS boxed-case sharing-clone-mutation-import");

    let mut merged = delimited(0x0a, &[0x08, 0x07]);
    merged.extend(delimited(0x0a, &[0x1a, 0x04, b'j', b'o', b'i', b'n']));
    let parent = Parent::parse(&merged).unwrap();
    assert_eq!(parent.chosen().value(), 7);
    assert_eq!(parent.chosen().label(), "join");
    assert_values(
        &parent,
        &ReferenceParent::decode(merged.as_slice()).unwrap(),
    );
    println!("PASS boxed-case duplicate-message-merge");

    let mut switched = delimited(0x2a, &[0x08, 0x01]);
    switched.extend(delimited(0x32, &[0x08, 0x02]));
    switched.extend(delimited(0x2a, &[0x1a, 0x01, b'x']));
    let parent = Parent::parse(&switched).unwrap();
    assert!(parent.has_one());
    assert!(!parent.has_two());
    assert_eq!(parent.one().value(), 0);
    assert_eq!(parent.one().label(), "x");
    assert_values(
        &parent,
        &ReferenceParent::decode(switched.as_slice()).unwrap(),
    );
    println!("PASS boxed-case oneof-reset");

    // Missing required fields are an intentional pbrs/Prost policy difference.
    assert!(Parent::parse(&delimited(0x52, &[])).is_err());
    let mut required_merge = delimited(0x52, &[0x12, 0x01, 0x3f]);
    required_merge.extend(delimited(0x52, &[0x08, 0x00]));
    let parent = Parent::parse(&required_merge).unwrap();
    assert!(parent.required_child().has_value());
    assert_eq!(parent.required_child().payload(), &[0x3f]);
    assert_values(
        &parent,
        &ReferenceParent::decode(required_merge.as_slice()).unwrap(),
    );
    println!("PASS boxed-case required-field-merge-policy");

    for malformed in [
        delimited(0x0a, &[0x1a, 0x01, 0xff]),
        vec![0x0a, 0x04, 0x08, 0x01],
        delimited(0x0a, &[0x1b, 0x14]),
    ] {
        assert!(Parent::parse(&malformed).is_err());
        assert!(ReferenceParent::decode(malformed.as_slice()).is_err());
    }
    println!("PASS boxed-case malformed-wire-utf8-groups");

    for depth in [0, 1, 99, 100, 101] {
        let mut wire = vec![0x10, 0x01];
        for _ in 0..depth {
            wire = delimited(0x0a, &wire);
        }
        let native = Node::parse(&wire);
        let prost = ReferenceNode::decode(wire.as_slice());
        assert_eq!(native.is_ok(), prost.is_ok(), "recursion depth {depth}");
        if let (Ok(node), Ok(reference)) = (native, prost) {
            assert_eq!(
                ReferenceNode::decode(Serialize::serialize(&node).unwrap().as_slice()).unwrap(),
                reference
            );
        }
    }
    println!("PASS boxed-case recursion-boundary");

    let json = native.to_json().unwrap();
    assert_values(&Parent::from_json(&json).unwrap(), &reference);
    let text = native.to_text().unwrap();
    assert_values(&Parent::from_text(&text).unwrap(), &reference);
    println!("PASS boxed-case json-text-roundtrips");
    if REPORT_SELECTORS {
        print!(
            "{}",
            include_str!(concat!(env!("OUT_DIR"), "/boxed-selectors.txt"))
        );
    }
}
