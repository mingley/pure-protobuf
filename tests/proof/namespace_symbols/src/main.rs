#![cfg_attr(feature = "safe", forbid(unsafe_code))]

extern crate pbrs as pb_runtime;

mod runtime {
    pub use ::pbrs::*;
}

mod native_external {
    include!(concat!(env!("OUT_DIR"), "/native-external/external.rs"));
    pub use External as Result;
    pub use ExternalMut as ResultMut;
    pub use ExternalView as ResultView;
}
mod native {
    include!(concat!(env!("OUT_DIR"), "/native/mod.rs"));
}
mod foreign {
    pub mod pbrs {
        pub use crate::native_external::External as Child;
        pub use crate::native_external::ExternalMut as ChildMut;
        pub use crate::native_external::ExternalView as ChildView;
    }
}
mod prost_external {
    include!(concat!(
        env!("OUT_DIR"),
        "/prost-external/proof.external.rs"
    ));
    pub use External as Result;
}
mod prost_symbols {
    include!(concat!(env!("OUT_DIR"), "/prost/proof.symbols.rs"));
}
mod prost_closed {
    include!(concat!(env!("OUT_DIR"), "/prost/proof.closed.rs"));
}

mod checks;
mod dynamic_controls;
mod legacy_compat;

fn main() {
    checks::run();
}
