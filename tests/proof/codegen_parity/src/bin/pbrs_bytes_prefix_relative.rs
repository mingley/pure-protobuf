#![forbid(unsafe_code)]
extern crate pbrs as runtime_alias;
pub mod runtime {
    pub use ::pbrs::*;
}
include!(concat!(
    env!("OUT_DIR"),
    "/native-bytes-prefix-relative/mod.rs"
));
const PREFIX: &str = "relative";
include!("../bytes_prefix.rs");
