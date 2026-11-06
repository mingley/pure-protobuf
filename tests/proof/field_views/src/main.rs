#![cfg_attr(feature = "safe", forbid(unsafe_code))]

use pbrs::{Parse, Serialize};

mod external_native {
    include!(concat!(env!("OUT_DIR"), "/native-external/external.rs"));
}
mod external_control {
    include!(concat!(
        env!("OUT_DIR"),
        "/prost-external/proof.external.rs"
    ));
}
include!(concat!(env!("OUT_DIR"), "/native/mod.rs"));
mod control {
    include!(concat!(env!("OUT_DIR"), "/prost/mod.rs"));
}

fn agree<N, P>(value: &N) -> P
where
    N: Parse + Serialize + PartialEq + std::fmt::Debug,
    P: prost::Message + Default + PartialEq + std::fmt::Debug,
{
    let wire = value.serialize().unwrap();
    let control = P::decode(wire.as_slice()).unwrap();
    let parsed = N::parse(&control.encode_to_vec()).unwrap();
    assert_eq!(&parsed, value);
    assert_eq!(
        P::decode(parsed.serialize().unwrap().as_slice()).unwrap(),
        control
    );
    control
}

macro_rules! agree_view {
    ($value:expr, $name:ident) => {
        agree::<_, control::proof::views::$name>(&$value)
    };
}

fn main() {
    use proof::views::*;
    let mut scalar = ScalarNear::new();
    scalar.set_child(11);
    scalar.set_child_view(23);
    assert_eq!((scalar.child(), scalar.child_view()), (11, 23));
    assert_eq!(agree_view!(scalar, ScalarNear).child_view, 23);

    let mut child = proof::shared::Child::new();
    child.set_value(17);
    let mut repeated = RepeatedNear::new();
    repeated.child_mut().push(child.clone());
    repeated.set_child_view(23);
    assert_eq!(repeated.child().len(), 1);
    assert_eq!(repeated.child_view(), 23);
    assert_eq!(agree_view!(repeated, RepeatedNear).child[0].value, 17);

    let mut map = MapNear::new();
    map.child_mut().insert("entry", child.clone());
    map.set_child_view(23);
    assert_eq!(map.child_view(), 23);
    assert_eq!(agree_view!(map, MapNear).child["entry"].value, 17);

    let mut renamed = RenamedNear::new();
    renamed.set_clone_field(child.clone());
    renamed.set_clone_view(23);
    assert_eq!(
        (renamed.clone_field_view().value(), renamed.clone_view()),
        (17, 23)
    );
    assert_eq!(agree_view!(renamed, RenamedNear).clone_view, 23);

    let mut keyword = KeywordNear::new();
    keyword.r#type_mut().push(child.clone());
    keyword.set_type_view(23);
    assert_eq!(keyword.type_view(), 23);
    assert_eq!(agree_view!(keyword, KeywordNear).r#type.len(), 1);

    let mut presence_near = PresenceNear::new();
    presence_near.set_has_thing(child.clone());
    presence_near.set_thing_view(23);
    assert_eq!(
        (
            presence_near.has_thing_view().value(),
            presence_near.thing_view()
        ),
        (17, 23)
    );
    assert_eq!(agree_view!(presence_near, PresenceNear).thing_view, 23);

    let mut clear_near = ClearNear::new();
    clear_near.set_clear_thing(child.clone());
    clear_near.set_thing_view(23);
    assert_eq!(
        (
            clear_near.clear_thing_view().value(),
            clear_near.thing_view()
        ),
        (17, 23)
    );
    agree_view!(clear_near, ClearNear);

    let mut map_near = MapPresenceNear::new();
    map_near.set_has_thing(child.clone());
    map_near.thing_view_mut().insert("entry", 23);
    assert_eq!(map_near.has_thing_view().value(), 17);
    assert_eq!(
        agree_view!(map_near, MapPresenceNear).thing_view["entry"],
        23
    );

    let mut repeated_near = RepeatedClearNear::new();
    repeated_near.set_clear_thing(child.clone());
    repeated_near.thing_view_mut().push(23);
    assert_eq!(repeated_near.clear_thing_view().value(), 17);
    assert_eq!(
        agree_view!(repeated_near, RepeatedClearNear).thing_view,
        [23]
    );
    println!(
        "PASS 9 previously valid scalar/repeated/map/raw-keyword/renamed/presence near misses"
    );

    let mut getter = GetterClash::new();
    getter.set_child(child.clone());
    getter.set_child_view_2(23);
    assert_eq!(
        (getter.child_view().value(), getter.child_view_2()),
        (17, 23)
    );
    assert_eq!(agree_view!(getter, GetterClash).child_view, 23);

    let mut raw = RawKeywordClash::new();
    raw.set_type(child.clone());
    raw.set_type_view_2(23);
    assert_eq!((raw.r#type_view().value(), raw.type_view_2()), (17, 23));
    assert_eq!(agree_view!(raw, RawKeywordClash).type_view, 23);

    let mut base = RenamedBaseClash::new();
    base.set_clone_field(child.clone());
    base.set_clone_field_view_2(23);
    base.set_clone_view(29);
    assert_eq!(
        (
            base.clone_field_view().value(),
            base.clone_field_view_2(),
            base.clone_view()
        ),
        (17, 23, 29)
    );
    agree_view!(base, RenamedBaseClash);
    println!("PASS ordinary/raw-keyword/already-renamed view getter collisions");
    let mut reverse = ReverseNumberClash::new();
    reverse.set_child(child.clone());
    reverse.set_child_view_1(23);
    assert_eq!(
        (reverse.child_view().value(), reverse.child_view_1()),
        (17, 23)
    );
    agree_view!(reverse, ReverseNumberClash);

    let mut chain = ViewChain::new();
    chain.set_child(child.clone());
    chain.set_child_view_2(child.clone());
    chain.set_child_view_view(23);
    assert_eq!(
        (chain.child_view_2_view().value(), chain.child_view_view()),
        (17, 23)
    );
    agree_view!(chain, ViewChain);

    let mut oneof = OneofClash::new();
    oneof.set_child(child.clone());
    oneof.set_child_view_2(23);
    assert!(oneof.has_child());
    assert_eq!(oneof.child_view().value(), 17);
    agree_view!(oneof, OneofClash);
    oneof.set_other(29);
    assert!(!oneof.has_child() && oneof.has_other());
    assert_eq!((oneof.child_view().value(), oneof.child_view_2()), (0, 23));
    agree_view!(oneof, OneofClash);
    println!("PASS reversed field numbers, stale-view chains, and real oneof transitions");

    let mut presence = PresenceClash::new();
    presence.set_has_thing(child.clone());
    presence.set_thing_view_2(0);
    assert!(presence.has_thing_view_2());
    assert_eq!(presence.thing_view_2_opt(), Some(0));
    assert_eq!(presence.has_thing_view().value(), 17);
    assert_eq!(agree_view!(presence, PresenceClash).thing_view, Some(0));
    presence.clear_thing_view_2();
    assert!(!presence.has_thing_view_2());
    agree_view!(presence, PresenceClash);

    let mut setter = SetterClash::new();
    setter.set_set_thing(child.clone());
    setter.set_thing_view_2(23);
    assert_eq!(setter.set_thing_view().value(), 17);
    assert_eq!(agree_view!(setter, SetterClash).thing_view, 23);

    let mut clear = ClearClash::new();
    clear.set_clear_thing(child.clone());
    clear.set_thing_view_2(23);
    assert_eq!(clear.clear_thing_view().value(), 17);
    clear.clear_thing_view_2();
    assert_eq!(agree_view!(clear, ClearClash).thing_view, None);
    println!("PASS view collisions with presence/setter/clear helpers");

    let mut trap = MessageRenameTrap::new();
    trap.set_child(child.clone());
    trap.set_child_view_2_3(child.clone());
    trap.child_view_2_3_mut().set_value(19);
    trap.set_child_view_2_mut(31);
    trap.set_child_view_2_opt(37);
    trap.set_child_view_2_view(41);
    trap.set_child_view_2_2(43);
    assert_eq!(trap.child_view().value(), 17);
    assert_eq!(trap.child_view_2_3_view().value(), 19);
    assert_eq!(
        (
            trap.child_view_2_mut(),
            trap.child_view_2_opt(),
            trap.child_view_2_view(),
            trap.child_view_2_2()
        ),
        (31, 37, 41, 43)
    );
    assert_eq!(
        agree_view!(trap, MessageRenameTrap)
            .child_view
            .unwrap()
            .value,
        19
    );

    let mut raw_trap = RawHelperTrap::new();
    raw_trap.set_has_thing(child.clone());
    raw_trap.set_thing_view_2_2(23);
    raw_trap.set_has_thing_view_2(31);
    raw_trap.set_set_thing_view_2(37);
    raw_trap.set_clear_thing_view_2(41);
    assert!(raw_trap.has_thing_view_2_2());
    assert_eq!(raw_trap.has_thing_view().value(), 17);
    assert_eq!(agree_view!(raw_trap, RawHelperTrap).thing_view, Some(23));

    let mut suffix_near = ScalarSuffixNear::new();
    suffix_near.set_child(child.clone());
    suffix_near.set_child_view_2(23);
    suffix_near.set_child_view_2_mut(31);
    assert_eq!(
        (suffix_near.child_view_2(), suffix_near.child_view_2_mut()),
        (23, 31)
    );
    agree_view!(suffix_near, ScalarSuffixNear);
    println!("PASS collision suffixes reserve actual helpers without reserving absent helpers");

    let mut proxy = ProxyClash::new();
    proxy.child_mut().set_marker(47);
    proxy.other_mut().set_marker(53);
    proxy.set_child_view_2(23);
    assert_eq!(
        (proxy.child_view().marker(), proxy.other_view().marker()),
        (47, 53)
    );
    agree_view!(proxy, ProxyClash);

    let mut external = ExternClash::new();
    external.child_mut().set_marker(59);
    external.set_child_view_2(23);
    assert_eq!(external.child_view().marker(), 59);
    agree_view!(external, ExternClash);
    println!("PASS genuine proxy-name messages and explicit external type path");

    let mut group = proof::groups::GroupClash::new();
    group.child_mut().set_value(61);
    group.set_child_view_2(23);
    assert_eq!((group.child_view().value(), group.child_view_2()), (61, 23));
    agree::<_, control::proof::groups::GroupClash>(&group);

    let group_near =
        proof::groups::RepeatedGroupNear::parse(&[0x0b, 0x08, 17, 0x0c, 0x10, 23]).unwrap();
    assert_eq!(group_near.child().len(), 1);
    assert_eq!(group_near.child_view(), 23);
    agree::<_, control::proof::groups::RepeatedGroupNear>(&group_near);
    println!("PASS singular-group view collision and repeated-group near miss");
    println!("PASS 25 schema cases against Prost 0.14.4 in this profile");
}
