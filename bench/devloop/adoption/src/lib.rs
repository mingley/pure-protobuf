//! Public, generated adoption corpora and independent whole-message oracles.

pub mod native {
    #![allow(unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/pbrs/adoption.rs"));
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
