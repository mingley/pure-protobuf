use crate::{native, native_external, prost_closed, prost_external, prost_symbols as p};
use native::proof::{closed as c, symbols as n};
use pbrs::prelude::*;
use prost::Message as _;

fn populated() -> (n::Record, p::Record) {
    let mut value = n::Record::new();
    let mut control = p::Record::default();
    value.set_count(0);
    control.count = Some(0);
    value.set_label("namespace");
    control.label = Some("namespace".into());
    value.set_blob(vec![1, 2, 3]);
    control.blob = vec![1, 2, 3];
    let mut external = native_external::Result::new();
    external.set_value(777);
    value.set_external(external);
    control.external = Some(prost_external::Result { value: 777 });
    macro_rules! child {
        ($native:ident, $prost:ident, $setter:ident, $field:ident, $number:literal) => {
            let mut child = n::$native::new();
            child.set_value($number);
            value.$setter(child);
            control.$field = Some(p::$prost { value: $number });
        };
    }
    child!(PbResult, Result, set_f_result_value, f_result_value, 1);
    child!(PbOption, Option, set_f_option_value, f_option_value, 2);
    child!(PbVec, Vec, set_f_vec_value, f_vec_value, 3);
    child!(PbBox, Box, set_f_box_value, f_box_value, 4);
    child!(PbString, String, set_f_string_value, f_string_value, 5);
    child!(Clone, Clone, set_f_clone_value, f_clone_value, 6);
    child!(Eq, Eq, set_f_eq_value, f_eq_value, 7);
    child!(
        PartialEq,
        PartialEq,
        set_f_partial_eq_value,
        f_partial_eq_value,
        8
    );
    child!(From, From, set_f_from_value, f_from_value, 9);
    child!(TryFrom, TryFrom, set_f_try_from_value, f_try_from_value, 10);
    child!(AsRef, AsRef, set_f_as_ref_value, f_as_ref_value, 11);
    child!(AsView, AsView, set_f_as_view_value, f_as_view_value, 12);
    child!(
        ParseError,
        ParseError,
        set_f_parse_error_value,
        f_parse_error_value,
        13
    );
    child!(
        SerializeError,
        SerializeError,
        set_f_serialize_error_value,
        f_serialize_error_value,
        14
    );
    child!(
        ProtoBytes,
        ProtoBytes,
        set_f_proto_bytes_value,
        f_proto_bytes_value,
        15
    );
    child!(
        ProtoString,
        ProtoString,
        set_f_proto_string_value,
        f_proto_string_value,
        16
    );
    child!(
        Repeated,
        Repeated,
        set_f_repeated_value,
        f_repeated_value,
        17
    );
    child!(
        RepeatedView_,
        RepeatedView,
        set_f_repeated_view_value,
        f_repeated_view_value,
        18
    );
    child!(
        RepeatedMut_,
        RepeatedMut,
        set_f_repeated_mut_value,
        f_repeated_mut_value,
        19
    );
    child!(
        UnknownEnumValue,
        UnknownEnumValue,
        set_f_unknown_enum_value_value,
        f_unknown_enum_value_value,
        20
    );
    child!(
        UnknownFields,
        UnknownFields,
        set_f_unknown_fields_value,
        f_unknown_fields_value,
        21
    );
    child!(Default, Default, set_f_default_value, f_default_value, 22);
    child!(Map, Map, set_f_map_value, f_map_value, 23);
    child!(Enum, Enum, set_f_enum_value, f_enum_value, 24);
    child!(Hash, Hash, set_f_hash_value, f_hash_value, 25);
    child!(Ord, Ord, set_f_ord_value, f_ord_value, 26);
    child!(
        PartialOrd,
        PartialOrd,
        set_f_partial_ord_value,
        f_partial_ord_value,
        27
    );
    child!(Into, Into, set_f_into_value, f_into_value, 28);
    child!(Copy, Copy, set_f_copy_value, f_copy_value, 29);
    child!(Debug, Debug, set_f_debug_value, f_debug_value, 30);
    child!(pbrs, Pbrs, set_f_pbrs_value, f_pbrs_value, 32);
    child!(std, Std, set_f_std_value, f_std_value, 33);
    child!(core, Core, set_f_core_value, f_core_value, 34);
    child!(
        IntoIterator,
        IntoIterator,
        set_f_into_iterator_value,
        f_into_iterator_value,
        31
    );
    let mut child = n::Clone::new();
    child.set_value(123);
    value.children_mut().push(child.clone());
    control.children.push(p::Clone { value: 123 });
    value.entries_mut().insert("key", child.clone());
    control
        .entries
        .insert("key".into(), p::Clone { value: 123 });
    value.set_some_value(n::Some::One);
    control.some_value = 1;
    value.set_none_value(n::None::One);
    control.none_value = 1;
    value.set_ok_value(n::Ok::One);
    control.ok_value = 1;
    value.set_err_value(n::Err::One);
    control.err_value = 1;
    value.enum_values_mut().push(1);
    control.enum_values.push(1);
    value.set_chosen_flag(false);
    assert!(value.has_chosen_flag());
    value.set_chosen_text("replace");
    assert!(!value.has_chosen_flag());
    value.set_picked(child);
    assert!(!value.has_chosen_text() && value.has_picked());
    control.choice = Some(p::record::Choice::Picked(p::Clone { value: 123 }));
    value.numbers_mut().push(-7);
    control.numbers.push(-7);
    value.flags_mut().push(true);
    control.flags.push(true);
    value.weights_mut().push(1.25);
    control.weights.push(1.25);
    value.samples_mut().push(2.5);
    control.samples.push(2.5);
    value.offsets_mut().push(-11);
    control.offsets.push(-11);
    value.ports_mut().push(443);
    control.ports.push(443);
    (value, control)
}

fn closed() {
    assert!(c::Record::parse(&[]).is_err());
    let mut value = c::Record::new();
    assert_eq!(value.some_value(), c::Some::One);
    assert!(!value.has_some_value());
    assert_eq!(c::Some::try_from(1).unwrap(), c::Some::One);
    assert!(c::Some::try_from(99).is_err());
    assert_eq!(c::None::try_from(1).unwrap(), c::None::One);
    assert_eq!(c::Ok::try_from(1).unwrap(), c::Ok::One);
    assert_eq!(c::Err::try_from(1).unwrap(), c::Err::One);
    value.set_required_value(7);
    value.set_some_value(c::Some::One);
    value.set_none_value(c::None::One);
    value.set_ok_value(c::Ok::One);
    value.set_err_value(c::Err::One);
    value.values_mut().push(1);
    value.entries_mut().insert("entry", 1);
    value.from_value_mut().set_value(11);
    value.try_from_value_mut().set_value(13);
    value.error_value_mut().set_value(17);
    value.parse_value_mut().set_value(19);
    let control = prost_closed::Record {
        required_value: 7,
        some_value: Some(1),
        none_value: Some(1),
        ok_value: Some(1),
        err_value: Some(1),
        values: vec![1],
        entries: [("entry".into(), 1)].into(),
        from_value: Some(prost_closed::From { value: Some(11) }),
        try_from_value: Some(prost_closed::TryFrom { value: Some(13) }),
        error_value: Some(prost_closed::UnknownEnumValue { value: Some(17) }),
        parse_value: Some(prost_closed::ParseError { value: Some(19) }),
    };
    let wire = value.serialize().unwrap();
    assert_eq!(
        prost_closed::Record::decode(wire.as_slice()).unwrap(),
        control
    );
    assert_eq!(c::Record::parse(&control.encode_to_vec()).unwrap(), value);
    assert_eq!(
        c::Record::from_json(&value.to_json().unwrap()).unwrap(),
        value
    );
    assert_eq!(
        c::Record::from_text(&value.to_text().unwrap()).unwrap(),
        value
    );
    // Proto2 numeric storage retains an unknown enum value in both frameworks.
    // TryFrom's rejection above describes the known-value API, not wire parsing.
    let unknown_wire = [8, 7, 16, 99];
    let unknown = c::Record::parse(&unknown_wire).unwrap();
    let prost_unknown = prost_closed::Record::decode(unknown_wire.as_slice()).unwrap();
    assert_eq!(unknown.has_some_value(), prost_unknown.some_value.is_some());
    assert_eq!(
        i32::from(unknown.some_value()),
        prost_unknown.some_value.unwrap()
    );
    assert_eq!(
        prost_closed::Record::decode(unknown.serialize().unwrap().as_slice()).unwrap(),
        prost_unknown
    );
    println!(
        "OBS proto2 unknown enum: native_present={} native_value={} prost_value={:?}",
        unknown.has_some_value(),
        i32::from(unknown.some_value()),
        prost_unknown.some_value
    );
    println!(
        "PASS proto2 closed enum constructors, defaults, required fields and Prost wire controls"
    );
}

pub fn run() {
    let (value, control) = populated();
    let wire = value.serialize().unwrap();
    assert_eq!(p::Record::decode(wire.as_slice()).unwrap(), control);
    assert_eq!(n::Record::parse(&control.encode_to_vec()).unwrap(), value);
    assert_eq!(
        <n::Record as pbrs::Name>::type_url(),
        "/proof.symbols.Record"
    );
    println!(
        "PASS legal schema names, established Pb names, extern Result alias and Prost wire controls"
    );
    let mut copied = n::Record::new();
    pbrs::CopyFrom::copy_from(&mut copied, value.as_view());
    assert_eq!(copied, value);
    assert_eq!(n::RecordView::default().count(), 0);
    assert!(!n::RecordView::default().has_count());
    let mut shared = n::Record::new();
    shared
        .merge_from_bytes_shared(pbrs::rt::Bytes::from(wire.clone()))
        .unwrap();
    assert_eq!(shared, value);
    assert_eq!(
        n::Record::from_json(&value.to_json().unwrap()).unwrap(),
        value
    );
    assert_eq!(
        n::Record::from_text(&value.to_text().unwrap()).unwrap(),
        value
    );
    let mut unknown = wire.clone();
    unknown.extend_from_slice(&[0xf8, 0x07, 0x09]);
    assert!(
        n::Record::parse(&unknown)
            .unwrap()
            .serialize()
            .unwrap()
            .ends_with(&[0xf8, 0x07, 0x09])
    );
    assert!(n::Record::parse(&[0x80]).is_err());
    assert!(p::Record::decode([0x80].as_slice()).is_err());
    copied.as_mut().clear();
    assert_eq!(copied, n::Record::default());
    println!("PASS typed views, CopyFrom, Clear, shared parse, JSON/text and error paths");
    closed();
    super::dynamic_controls::run();
}
