include!(concat!(env!("OUT_DIR"), "/native-boxed/mod.rs"));
const REPORT_SELECTORS: bool = true;
include!("../boxed.rs");
