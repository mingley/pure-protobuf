use pbrs::{Parse, Serialize};
use prost::Message;

include!(concat!(env!("OUT_DIR"), "/native-options/mod.rs"));

mod reference {
    pub mod proof {
        pub mod shared {
            include!(concat!(env!("OUT_DIR"), "/prost-options/proof.shared.rs"));
        }
        pub mod external {
            include!(concat!(env!("OUT_DIR"), "/prost-options/proof.external.rs"));
        }
        pub mod case {
            include!(concat!(env!("OUT_DIR"), "/prost-options/proof.case.rs"));
        }
        pub mod options {
            include!(concat!(env!("OUT_DIR"), "/prost-options/proof.options.rs"));
        }
    }
    include!(concat!(env!("OUT_DIR"), "/prost-options/_.rs"));
}

mod default_domain {
    include!(concat!(env!("OUT_DIR"), "/native-names-default/mod.rs"));
}

mod default_reference {
    include!(concat!(env!("OUT_DIR"), "/prost-names-default/_.rs"));
}

fn names_match<N: pbrs::Name, P: prost::Name>(full: &str, url: &str) {
    assert_eq!(N::NAME, P::NAME);
    assert_eq!(N::PACKAGE, P::PACKAGE);
    assert_eq!(N::full_name(), P::full_name());
    assert_eq!(N::full_name(), full);
    assert_eq!(N::type_url(), P::type_url());
    assert_eq!(N::type_url(), url);
    println!("PASS configured-name {full} {url}");
}

fn any_roundtrip<N, P>(native: &N, reference: &P)
where
    N: pbrs::Name + PartialEq + std::fmt::Debug,
    P: prost::Name + Default + PartialEq + std::fmt::Debug,
{
    let native_any = pbrs::wkt::Any::from_msg(native).unwrap();
    let reference_any = prost_types::Any::from_msg(reference).unwrap();
    assert_eq!(
        native_any.type_url().to_str().unwrap(),
        reference_any.type_url
    );
    assert_eq!(native_any.value(), reference_any.value);
    assert_eq!(native_any.to_msg::<N>().unwrap(), *native);
    assert_eq!(reference_any.to_msg::<P>().unwrap(), *reference);
    let wire = native_any.serialize().unwrap();
    assert_eq!(
        prost_types::Any::decode(wire.as_slice()).unwrap(),
        reference_any
    );
    println!("PASS configured-any {}", N::full_name());
}

fn main() {
    names_match::<proof::case::Payload, reference::proof::case::Payload>(
        "proof.case.Payload",
        "root.example/proof.case.Payload",
    );
    names_match::<proof::shared::Outer, reference::proof::shared::Outer>(
        "proof.shared.Outer",
        "package.example/proof.shared.Outer",
    );
    names_match::<proof::shared::outer::Inner, reference::proof::shared::outer::Inner>(
        "proof.shared.Outer.Inner",
        "exact.example/proof.shared.Outer.Inner",
    );
    names_match::<proof::external::External, reference::proof::external::External>(
        "proof.external.External",
        "suffix.example/proof.external.External",
    );
    names_match::<Loose, reference::Loose>("Loose", "root.example/Loose");
    names_match::<loose::Nested, reference::loose::Nested>(
        "Loose.Nested",
        "trailing.example//Loose.Nested",
    );
    names_match::<default_domain::Loose, default_reference::Loose>("Loose", "/Loose");
    names_match::<default_domain::loose::Nested, default_reference::loose::Nested>(
        "Loose.Nested",
        "/Loose.Nested",
    );

    let reference = reference::proof::shared::outer::Inner { value: i64::MIN };
    let native = proof::shared::outer::Inner::parse(&reference.encode_to_vec()).unwrap();
    any_roundtrip(&native, &reference);
    let loose_reference = reference::Loose {
        child: Some(reference::loose::Nested { value: -99 }),
    };
    let loose_native = Loose::parse(&loose_reference.encode_to_vec()).unwrap();
    any_roundtrip(&loose_native, &loose_reference);
    let default_reference = default_reference::loose::Nested { value: 17 };
    let default_native =
        default_domain::loose::Nested::parse(&default_reference.encode_to_vec()).unwrap();
    any_roundtrip(&default_native, &default_reference);
    let mut timestamp = pbrs::wkt::Timestamp::new();
    timestamp.set_seconds(-123);
    timestamp.set_nanos(45);
    any_roundtrip(
        &timestamp,
        &prost_types::Timestamp {
            seconds: -123,
            nanos: 45,
        },
    );

    for url in [
        "exact.example/proof.shared.Outer.Inner",
        "type.googleapis.com/proof.shared.Outer.Inner",
        "/proof.shared.Outer.Inner",
        "https://types.example/v1/proof.shared.Outer.Inner",
        "types.example//proof.shared.Outer.Inner",
        "proof.shared.Outer.Inner",
        "/.proof.shared.Outer.Inner",
        "types.example/",
        "/",
        "",
        "/proof.shared.Outer",
        "types.example/proof.shared.Outer.Inner?query",
    ] {
        let reference_any = prost_types::Any {
            type_url: url.into(),
            value: reference.encode_to_vec(),
        };
        let native_any = pbrs::wkt::Any::parse(&reference_any.encode_to_vec()).unwrap();
        let reference_result = reference_any.to_msg::<reference::proof::shared::outer::Inner>();
        let native_result = native_any.to_msg::<proof::shared::outer::Inner>();
        assert_eq!(
            native_result.is_ok(),
            reference_result.is_ok(),
            "URL {url:?}"
        );
        if let (Ok(native), Ok(reference)) = (native_result, reference_result) {
            assert_eq!(native.value(), reference.value);
        }
        println!("PASS configured-any-url {url:?}");
    }
    let reference_any = prost_types::Any {
        type_url: "types.example/proof.shared.Outer.Inner".into(),
        value: vec![0x80],
    };
    let native_any = pbrs::wkt::Any::parse(&reference_any.encode_to_vec()).unwrap();
    assert!(reference_any
        .to_msg::<reference::proof::shared::outer::Inner>()
        .is_err());
    assert!(native_any.to_msg::<proof::shared::outer::Inner>().is_err());
    println!("PASS configured-any-malformed truncated payload");

    for state in [0, 1, 92] {
        let reference = reference::proof::case::Payload {
            state,
            ..Default::default()
        };
        let native = proof::case::Payload::parse(&reference.encode_to_vec()).unwrap();
        assert_eq!(native.state().0, state);
        assert_eq!(
            reference::proof::case::Payload::decode(native.serialize().unwrap().as_slice())
                .unwrap(),
            reference
        );
    }
    assert_eq!(proof::shared::outer::State::StateReady.0, 1);
    assert_eq!(
        proof::shared::outer::State::StateReady.0,
        reference::proof::shared::outer::State::StateReady as i32
    );
    println!("PASS configured-enum retained prefix and unknown wire values");

    let native = proof::options::Defaults::parse(&[]).unwrap();
    let reference = reference::proof::options::Defaults::default();
    assert!(!native.has_mood());
    assert!(reference.mood.is_none());
    assert_eq!(native.mood().0, reference.mood() as i32);
    assert_eq!(native.mood(), proof::options::Mood::MoodReady);
    assert_eq!(
        proof::options::Mood::MoodRest,
        proof::options::Mood::MoodReady
    );
    assert!(native.serialize().unwrap().is_empty());
    for mood in [5, 9] {
        let reference = reference::proof::options::Defaults {
            mood: Some(mood),
            moods: vec![5, 9, 5],
        };
        let native = proof::options::Defaults::parse(&reference.encode_to_vec()).unwrap();
        assert!(native.has_mood());
        assert_eq!(native.mood().0, mood);
        assert_eq!(
            reference::proof::options::Defaults::decode(native.serialize().unwrap().as_slice())
                .unwrap(),
            reference
        );
    }
    println!("PASS configured-enum proto2 defaults, aliases and packed values");
}
