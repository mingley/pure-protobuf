//! Official rust_out Person ClearAndParse/Serialize roundtrip against pbrs.

#![allow(nonstandard_style, unused, dead_code, clippy::all)]

mod example {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

use example::Person;
use protobuf::{AsView, ClearAndParse, Message, MutProxied, Proxied, Serialize};

fn main() {
    fn _traits<M: Message + Proxied + MutProxied + ClearAndParse + Serialize>() {}
    _traits::<Person>();

    let mut p = Person::new();
    p.set_name("ada");
    assert_eq!(p.name().to_str().expect("utf8"), "ada");

    let bytes = Serialize::serialize(&p).expect("Serialize Person");
    let mut q = Person::new();
    ClearAndParse::clear_and_parse(&mut q, &bytes).expect("ClearAndParse Person");
    assert_eq!(q.name().to_str().expect("utf8"), "ada");
    println!("ok person name=ada");
}
