#![forbid(unsafe_code)]
extern crate pbrs as runtime_alias;
pub mod runtime {
    pub use ::pbrs::*;
}
include!(concat!(
    env!("OUT_DIR"),
    "/native-bytes-prefix-absolute/mod.rs"
));
const PREFIX: &str = "absolute";
include!("../bytes_prefix.rs");
