use pbrs::{Parse, Serialize};
use prost::Message;

include!(concat!(env!("OUT_DIR"), "/native/mod.rs"));
mod native_external {
    include!(concat!(env!("OUT_DIR"), "/native-external/external.rs"));
}
mod prost_external {
    include!(concat!(
        env!("OUT_DIR"),
        "/prost-external/proof.external.rs"
    ));
}
mod reference {
    pub mod proof {
        pub mod shared {
            include!(concat!(env!("OUT_DIR"), "/prost/proof.shared.rs"));
        }
        pub mod case {
            include!(concat!(env!("OUT_DIR"), "/prost/proof.case.rs"));
        }
    }
}
mod configured {
    pub mod proof {
        pub mod shared {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-configured/proof.shared.rs"
            ));
        }
        pub mod external {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-configured/proof.external.rs"
            ));
        }
        pub mod case {
            include!(concat!(env!("OUT_DIR"), "/prost-configured/proof.case.rs"));
        }
    }
}

fn main() {
    println!("GENERATED_ROOT={}", env!("OUT_DIR"));
    for id in [None, Some(0), Some(u32::MAX)] {
        for choice in [
            None,
            Some(reference::proof::case::payload::Choice::Note(
                "hello".into(),
            )),
            Some(reference::proof::case::payload::Choice::Code(-9)),
        ] {
            let original = reference::proof::case::Payload {
                blob: vec![0, 255, 4, 0],
                counts: [("alpha".into(), 1), ("omega".into(), u32::MAX)].into(),
                optional_id: id,
                nested: Some(reference::proof::shared::outer::Inner { value: i64::MIN }),
                state: reference::proof::shared::outer::State::Ready as i32,
                choice,
                when: Some(prost_types::Timestamp {
                    seconds: -123,
                    nanos: 45,
                }),
                external: Some(prost_external::External {
                    label: "external".into(),
                }),
            };
            let wire = original.encode_to_vec();
            let customized = configured::proof::case::Payload::decode(wire.as_slice()).unwrap();
            let _: &bytes::Bytes = &customized.blob;
            let _: &std::collections::BTreeMap<String, u32> = &customized.counts;
            let _: &Option<Box<configured::proof::shared::outer::Inner>> = &customized.nested;
            assert_eq!(
                customized.state,
                configured::proof::shared::outer::State::StateReady as i32
            );
            assert_eq!(
                <configured::proof::case::Payload as prost::Name>::type_url(),
                "proof.example/proof.case.Payload"
            );
            assert_eq!(
                original,
                reference::proof::case::Payload::decode(customized.encode_to_vec().as_slice())
                    .unwrap()
            );
            let decoded = proof::case::Payload::parse(&wire).unwrap();
            let native_wire = decoded.serialize().unwrap();
            let recovered =
                reference::proof::case::Payload::decode(native_wire.as_slice()).unwrap();
            assert_eq!(original, recovered);
            assert_eq!(decoded.has_optional_id(), id.is_some());
            assert_eq!(decoded.blob(), original.blob.as_slice());
            assert_eq!(decoded.nested().value(), i64::MIN);
            assert_eq!(decoded.when().seconds(), -123);
            assert_eq!(decoded.external().label(), "external");
            assert_eq!(decoded.counts().get("omega"), Some(u32::MAX));
            println!("PASS imported-nested-enum-oneof-optional-map-bytes-wkt-extern id={id:?} choice={:?}", original.choice);
        }
    }
}
