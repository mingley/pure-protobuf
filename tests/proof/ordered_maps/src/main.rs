use pbrs::{Parse, Serialize};
use prost::Message as _;

mod reference {
    pub mod proof {
        pub mod ordered {
            include!(concat!(env!("OUT_DIR"), "/reference/proof.ordered.rs"));
        }
        pub mod ordered_required {
            include!(concat!(
                env!("OUT_DIR"),
                "/reference/proof.ordered_required.rs"
            ));
        }
    }
}
mod selected {
    include!(concat!(env!("OUT_DIR"), "/selected/mod.rs"));
    include!("checks.rs");
}
mod owned {
    include!(concat!(env!("OUT_DIR"), "/owned/mod.rs"));
    include!("checks.rs");
}
#[forbid(unsafe_code)]
mod safe {
    include!(concat!(env!("OUT_DIR"), "/safe/mod.rs"));
    include!("checks.rs");
}
mod old_parent {
    include!("../old_parent/old_parent.rs");
}
mod current_parent {
    include!(concat!(env!("OUT_DIR"), "/current-parent/old_parent.rs"));
}
mod unselected {
    include!(concat!(env!("OUT_DIR"), "/default/mod.rs"));
}

fn field(number: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, number, payload);
    out
}

fn numbers(entries: &[(&str, i32)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in entries {
        let child = reference::proof::ordered::Child {
            numbers: [(key.to_string(), *value)].into(),
            ..Default::default()
        };
        out.extend(child.encode_to_vec());
    }
    out
}

fn payloads(data: &[u8], number: u32) -> Vec<&[u8]> {
    let mut pos = 0;
    let mut values = Vec::new();
    while pos < data.len() {
        let (field, wire) = pbrs::rt::decode_tag(data, &mut pos).unwrap();
        if wire == pbrs::rt::WIRE_LEN {
            let (start, end) = pbrs::rt::read_len_span(data, &mut pos).unwrap();
            if field == number {
                values.push(&data[start..end]);
            }
        } else {
            pbrs::rt::skip_field_with_number_and_depth(data, &mut pos, field, wire, 0).unwrap();
        }
    }
    values
}

fn legacy_observations() {
    let input = numbers(&[("z", 2), ("a", 1), ("z", 9)]);
    let normalized = reference::proof::ordered::Child::decode(input.as_slice())
        .unwrap()
        .encode_to_vec();
    assert_ne!(input, normalized);
    let old_wire = field(1, &input);
    let grand_wire = field(1, &old_wire);
    let old = old_parent::OldGrandparent::parse(&grand_wire).unwrap();
    assert_eq!(old.parent().child().numbers().get("z"), Some(9));
    assert_eq!(Serialize::serialize(&old).unwrap(), grand_wire);
    // Even an eager child inside a read-only ancestor retains the ancestor's
    // explicit cached original. The unchanged public API keeps exposing it.
    let parent = current_parent::OldParent::parse(&old_wire).unwrap();
    assert_eq!(
        Serialize::serialize(&parent).unwrap(),
        field(1, &normalized)
    );
    let lazy = pbrs::rt::LazyMsg::from_parsed(parent, pbrs::rt::Wire::from_slice(&old_wire));
    assert_eq!(lazy.as_deref().unwrap().child().numbers().len(), 2);
    assert_eq!(lazy.wire_bytes(), Some(old_wire.as_slice()));
    for prevalidated in [false, true] {
        let wire = pbrs::rt::Wire::from_slice(&old_wire);
        let mut lazy = if prevalidated {
            pbrs::rt::LazyMsg::<current_parent::OldParent>::from_validated_wire(wire)
        } else {
            pbrs::rt::LazyMsg::<current_parent::OldParent>::from_wire(wire)
        };
        assert_eq!(lazy.as_deref().unwrap().child().numbers().len(), 2);
        assert_eq!(lazy.wire_bytes(), Some(old_wire.as_slice()));
        assert_eq!(
            Serialize::serialize(lazy.get_or_insert()).unwrap(),
            field(1, &normalized)
        );
        assert!(lazy.wire_bytes().is_none());
    }
    let current = current_parent::OldGrandparent::parse(&grand_wire).unwrap();
    assert_eq!(
        Serialize::serialize(&current).unwrap(),
        field(1, &field(1, &normalized))
    );
    // Mutation has always cleared originals in ordinary LazyMsg. The owned
    // selected child is reached after those ordinary invalidation points.
    let mut old = old;
    old.parent_mut().child_mut().numbers_mut().insert("z", 10);
    let reparsed = reference::proof::ordered::Child::decode(
        payloads(payloads(&Serialize::serialize(&old).unwrap(), 1)[0], 1)[0],
    )
    .unwrap();
    assert_eq!(reparsed.numbers.get("z"), Some(&10));
    println!("PASS ordered-legacy raw-cache-and-regenerated-parent");
}

fn main() {
    let mut child = unselected::proof::ordered::Child::new();
    child.numbers_mut().insert("z", 9);
    let mut parent = unselected::proof::ordered::Parent::new();
    parent.set_child(child);
    assert_eq!(
        unselected::proof::ordered::Parent::from_json(&parent.to_json().unwrap()).unwrap(),
        parent
    );
    assert_eq!(
        unselected::proof::ordered::Parent::from_text(&parent.to_text().unwrap()).unwrap(),
        parent
    );
    println!("PASS ordered-parent-json unselected frozen-default-source-hash-control");
    selected::run("selected");
    owned::run("owned");
    safe::run("safe");
    legacy_observations();
    print!(
        "{}",
        std::fs::read_to_string(env!("SELECTOR_REPORT")).unwrap()
    );
    println!("DEFAULT_ROOT={}", env!("DEFAULT_ROOT"));
}
