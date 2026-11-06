include!(concat!(env!("OUT_DIR"), "/native-boxed-default/mod.rs"));
const REPORT_SELECTORS: bool = false;
include!("../boxed.rs");
