use pbrs::prelude::*;
use pbrs::{Parse, Serialize};
use prost::Message as _;

mod reference {
    pub mod proof {
        pub mod runtime_symbols {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-runtime-symbols/proof.runtime_symbols.rs"
            ));
            pub mod imported {
                include!(concat!(
                    env!("OUT_DIR"),
                    "/prost-runtime-symbols/proof.runtime_symbols.imported.rs"
                ));
            }
        }
    }
}

fn main() {
    // The protobuf names remain literal public Rust names; the generator's
    // own traits and map containers must not resolve to these declarations.
    let mut value = proof::runtime_symbols::Default::new();
    assert_eq!(value.value(), 0);
    value.set_text("cleared");
    value.set_number(11);
    assert!(!value.has_text());
    assert_eq!(value.number(), 11);
    value.clear_number();
    value.set_value(7);
    {
        let mut typed: proof::runtime_symbols::DefaultMut<'_> = value.as_mut();
        pbrs::Clear::clear(&mut typed);
        assert_eq!(typed.value(), 0);
        typed.set_value(9);
    }
    let default_view: proof::runtime_symbols::DefaultView<'static> =
        ::core::default::Default::default();
    assert_eq!(default_view.value(), 0);
    let mut cleared = value.clone();
    pbrs::Clear::clear(&mut cleared);
    assert_eq!(cleared.value(), 0);
    println!("PASS runtime-symbol-default typed mutator and default view");

    let mut map = proof::runtime_symbols::Map::new();
    {
        let mut typed: proof::runtime_symbols::MapMut<'_> = map.as_mut();
        typed.entries_mut().insert("one", value.clone());
    }
    let empty_map_view: proof::runtime_symbols::MapView<'static> =
        ::core::default::Default::default();
    assert!(empty_map_view.entries().is_empty());
    assert_eq!(map.entries().get("one").unwrap().value(), 9);
    println!("PASS runtime-symbol-map runtime container and typed view coexist");

    let state = proof::runtime_symbols::Enum::try_from(1).unwrap();
    assert_eq!(state, proof::runtime_symbols::Enum::Present);
    assert!(<proof::runtime_symbols::Enum as pbrs::Enum>::is_known(1));
    assert!(!<proof::runtime_symbols::Enum as pbrs::Enum>::is_known(900));
    println!("PASS runtime-symbol-enum generated value and runtime trait coexist");

    let mut holder = proof::runtime_symbols::imported::Holder::new();
    holder.set_child(value.clone());
    holder.children_mut().push(value);
    holder.maps_mut().insert("outer", map);
    holder.set_state(state);
    let reference_value = reference::proof::runtime_symbols::Default {
        value: 9,
        choice: None,
    };
    let reference_map = reference::proof::runtime_symbols::Map {
        entries: [("one".to_owned(), reference_value.clone())].into(),
    };
    let reference = reference::proof::runtime_symbols::imported::Holder {
        child: Some(reference_value.clone()),
        children: vec![reference_value],
        maps: [("outer".to_owned(), reference_map)].into(),
        state: reference::proof::runtime_symbols::Enum::Present as i32,
    };
    let native_wire = Serialize::serialize(&holder).unwrap();
    assert_eq!(native_wire, reference.encode_to_vec());
    assert_eq!(
        reference::proof::runtime_symbols::imported::Holder::decode(native_wire.as_slice())
            .unwrap(),
        reference
    );
    let decoded = proof::runtime_symbols::imported::Holder::parse(&native_wire).unwrap();
    assert_eq!(decoded.child().value(), 9);
    assert_eq!(decoded.children().len(), 1);
    assert_eq!(
        decoded
            .maps()
            .get("outer")
            .unwrap()
            .entries()
            .get("one")
            .unwrap()
            .value(),
        9
    );
    assert_eq!(decoded.state(), state);
    println!("PASS runtime-symbol-cross-file wire matches pinned Prost");
}
