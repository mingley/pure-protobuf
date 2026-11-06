use crate::reference::proof::{embedded as reference, embedded_legacy as old_reference};
use alloc::{boxed::Box, string::ToString, vec};
use pbrs::{AsView as _, Parse, Serialize};
use proof::{
    embedded::{Fields, FieldsView},
    embedded_legacy::Legacy,
};
use prost::Message as _;

fn reference_child(id: i32) -> reference::Child {
    reference::Child {
        id,
        label: "child".to_string(),
        payload: vec![0x81; 96],
        next: None,
    }
}

fn fixture() -> reference::Fields {
    reference::Fields {
        double_value: 1.25,
        float_value: -2.5,
        int32_value: -123,
        int64_value: i64::MIN + 5,
        uint32_value: u32::MAX,
        uint64_value: u64::MAX,
        sint32_value: -456,
        sint64_value: -9_876_543_210,
        fixed32_value: 0xfedc_ba98,
        fixed64_value: 0xfedc_ba98_7654_3210,
        sfixed32_value: i32::MIN + 7,
        sfixed64_value: i64::MIN + 7,
        bool_value: true,
        label: "native alloc λ".to_string(),
        payload: vec![0xff; 256],
        state: 123,
        maybe: Some(0),
        maybe_label: Some("present".to_string()),
        maybe_payload: Some(vec![0; 128]),
        maybe_bool: Some(false),
        child: Some(reference_child(1)),
        children: vec![reference_child(2)],
        labels: vec!["first".to_string(), "second".to_string()],
        payloads: vec![vec![3; 80]],
        counts: (0..32)
            .map(|index| (alloc::format!("key-{index:02}"), index))
            .collect(),
        child_map: [(3, reference_child(3))].into(),
        byte_map: [("key".to_string(), vec![4; 128])].into(),
        choice: Some(reference::fields::Choice::Picked(reference_child(4))),
        p_double: vec![1.25, -2.5],
        p_float: vec![3.5, -4.25],
        p_int32: vec![-1, 0, i32::MAX],
        p_int64: vec![i64::MIN, 0, i64::MAX],
        p_uint32: vec![0, 128, u32::MAX],
        p_uint64: vec![0, 128, u64::MAX],
        p_sint32: vec![i32::MIN, 0, i32::MAX],
        p_sint64: vec![i64::MIN, 0, i64::MAX],
        p_fixed32: vec![0, u32::MAX],
        p_fixed64: vec![0, u64::MAX],
        p_sfixed32: vec![i32::MIN, i32::MAX],
        p_sfixed64: vec![i64::MIN, i64::MAX],
        p_bool: vec![false, true, false],
        p_state: vec![0, 1, 123],
        unpacked: vec![-1, 0, 128],
        unpacked_bool: vec![true, false, true],
    }
}

fn agree(message: &Fields, expected: &reference::Fields) {
    let wire = Serialize::serialize(message).unwrap();
    assert_eq!(
        reference::Fields::decode(wire.as_slice()).unwrap(),
        *expected
    );
    let parsed = Fields::parse(&wire).unwrap();
    assert_eq!(Serialize::serialize(&parsed).unwrap(), wire);
}

pub fn run() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Fields>();
    send_sync::<Child>();
    let empty = Fields::new();
    assert_eq!(empty.child().id(), 0);
    assert_eq!(empty.child().next().id(), 0);
    assert_eq!(FieldsView::default().child().id(), 0);
    let mut expected = fixture();
    let wire = expected.encode_to_vec();
    let mut message = Fields::parse(&wire).unwrap();
    assert_eq!(message.int32_value(), -123);
    assert!(message.has_maybe() && message.has_maybe_bool());
    assert_eq!(message.child().payload(), &[0x81; 96]);
    assert_eq!(message.counts().get("key-17"), Some(17));
    assert_eq!(message.counts().get("key-17"), Some(17));
    assert_eq!(message.as_view().child().id(), 1);
    agree(&message, &expected);

    message.set_label("mutated");
    expected.label = "mutated".to_string();
    message.set_payload(vec![9; 144]);
    expected.payload = vec![9; 144];
    message.child_mut().set_id(99);
    expected.child.as_mut().unwrap().id = 99;
    message.p_int32_mut().push(-7);
    expected.p_int32.push(-7);
    message.p_fixed64_mut().push(999);
    expected.p_fixed64.push(999);
    message.p_double_mut().push(-99.25);
    expected.p_double.push(-99.25);
    message.p_bool_mut().push(true);
    expected.p_bool.push(true);
    message.unpacked_mut().push(8);
    expected.unpacked.push(8);
    message.labels_mut().push("third");
    expected.labels.push("third".to_string());
    message.payloads_mut().push(&b"bytes"[..]);
    expected.payloads.push(b"bytes".to_vec());
    message.counts_mut().insert("new", 45);
    expected.counts.insert("new".to_string(), 45);
    let mut child = Child::new();
    child.set_id(77);
    message.child_map_mut().insert(7, child.clone());
    expected.child_map.insert(
        7,
        reference::Child {
            id: 77,
            ..Default::default()
        },
    );
    message.children_mut().push(child);
    expected.children.push(reference::Child {
        id: 77,
        ..Default::default()
    });
    message.byte_map_mut().insert("new", &b"map-bytes"[..]);
    expected
        .byte_map
        .insert("new".to_string(), b"map-bytes".to_vec());
    message.set_note("oneof changed");
    expected.choice = Some(reference::fields::Choice::Note("oneof changed".to_string()));
    message.clear_maybe();
    expected.maybe = None;
    assert!(!message.has_maybe());
    agree(&message, &expected);

    let mut legacy = Legacy::new();
    assert!(legacy.check_required().is_err());
    assert_eq!(legacy.label(), "legacy");
    assert_eq!(legacy.payload(), &[0xff]);
    legacy.set_required_value(0);
    legacy.nested_mut().set_signed_value(-7);
    let old = old_reference::Legacy {
        required_value: 0,
        label: None,
        payload: None,
        nested: Some(old_reference::legacy::Nested {
            signed_value: -7,
            active: None,
        }),
        items: vec![],
        next: Some(Box::new(old_reference::Legacy {
            required_value: 1,
            ..Default::default()
        })),
        state: None,
        words: vec![0, u64::MAX],
    };
    let old_wire = old.encode_to_vec();
    let mut parsed = Legacy::parse(&old_wire).unwrap();
    assert_eq!(parsed.nested().signed_value(), -7);
    assert!(parsed.nested().active());
    assert_eq!(parsed.next().required_value(), 1);
    parsed.next_mut().set_required_value(2);
    parsed.items_mut().push_default().set_id(42);
    let mut expected_old = old;
    expected_old.next.as_mut().unwrap().required_value = 2;
    expected_old
        .items
        .push(old_reference::legacy::Items { id: Some(42) });
    assert_eq!(
        old_reference::Legacy::decode(Serialize::serialize(&parsed).unwrap().as_slice()).unwrap(),
        expected_old
    );
    assert!(Legacy::parse(&[]).is_err());
}
