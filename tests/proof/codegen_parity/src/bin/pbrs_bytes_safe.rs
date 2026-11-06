#![forbid(unsafe_code)]
include!(concat!(env!("OUT_DIR"), "/native-bytes-safe/mod.rs"));
const REPORT_SELECTORS: bool = false;
include!("../bytes.rs");
