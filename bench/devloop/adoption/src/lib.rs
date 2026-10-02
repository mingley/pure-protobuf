//! Public, generated adoption corpora and independent whole-message oracles.

pub mod native {
    #![allow(unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/pbrs/adoption.rs"));
    pub use crate::native_google::protobuf::{Any, AnyMut, AnyView};
}

/// One generated owner for native Google types used by both application sets.
/// The independent prost owner remains at [`google::protobuf`].
pub mod native_google {
    pub mod protobuf {
        #![allow(unused, non_snake_case, clippy::all)]
        mod any {
            include!(concat!(
                env!("OUT_DIR"),
                "/pbrs_wkt/any/google/protobuf/any.rs"
            ));
        }
        mod descriptor {
            include!(concat!(
                env!("OUT_DIR"),
                "/pbrs_wkt/descriptor/google/protobuf/descriptor.rs"
            ));
        }
        pub use any::{Any, AnyMut, AnyView};
        pub use descriptor::*;
    }
}

pub mod google {
    pub mod protobuf {
        #![allow(clippy::all, reason = "prost-generated well-known types")]
        include!(concat!(env!("OUT_DIR"), "/prost/google.protobuf.rs"));
    }
}

pub mod prost_types {
    include!(concat!(env!("OUT_DIR"), "/prost/adoption.rs"));
}

pub mod options {
    #![allow(unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/option_modules.rs"));
}

mod corpus;
pub use corpus::*;
mod fresh;
pub use fresh::*;
pub mod rpc;
pub mod workloads;

#[cfg(test)]
mod generated_ownership_tests {
    use super::*;
    use pbrs::{AsView, Parse, Serialize};

    #[test]
    fn native_google_owners_and_compatibility_aliases_stay_independent_of_prost() {
        let mut value = native::Any::new();
        value.set_type_url("type.googleapis.com/adoption.Payload");
        value.set_value(vec![8, 23]);
        let mut entity = native::Entity::new();
        entity.set_payload(value);
        let view: native::AnyView<'_> = entity.payload().as_view();
        assert_eq!(view.value(), [8, 23]);
        let mut mutable: native::AnyMut<'_> = pbrs::AsMut::as_mut(entity.payload_mut());
        mutable.set_type_url("type.googleapis.com/adoption.Payload");
        let decoded =
            <native_google::protobuf::Any as Parse>::parse(&entity.payload().serialize().unwrap())
                .unwrap();
        assert_eq!(
            decoded.as_view().type_url(),
            "type.googleapis.com/adoption.Payload"
        );
        let json = decoded.to_json().unwrap();
        let expanded: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(expanded["@type"], "type.googleapis.com/adoption.Payload");
        assert_eq!(expanded["sequence"], "23");
        assert_eq!(
            native::Any::from_json(&json).unwrap().serialize().unwrap(),
            decoded.serialize().unwrap()
        );

        let prost: google::protobuf::Any =
            prost::Message::decode(entity.payload().serialize().unwrap().as_slice()).unwrap();
        assert_eq!(prost.value, [8, 23]);
        assert_eq!(prost.type_url.as_bytes(), decoded.type_url().as_bytes());

        let mut descriptor = native_google::protobuf::DescriptorProto::new();
        descriptor.set_name("Record00");
        let wire = descriptor.serialize().unwrap();
        assert_eq!(
            <native_google::protobuf::DescriptorProto as Parse>::parse(&wire)
                .unwrap()
                .name(),
            "Record00"
        );
    }
}
