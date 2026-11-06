#![forbid(unsafe_code)]
extern crate pbrs as runtime_alias;
pub mod runtime {
    pub use ::pbrs::*;
}
include!(concat!(
    env!("OUT_DIR"),
    "/native-bytes-prefix-default/mod.rs"
));
const PREFIX: &str = "default";
include!("../bytes_prefix.rs");
