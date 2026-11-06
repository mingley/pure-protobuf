use pbrs::{Parse, Serialize};
use prost::Message;
include!(concat!(env!("OUT_DIR"), "/native-bytes-default/mod.rs"));
include!("../bytes_reference.rs");

fn main() {
    let reference = fixture(16);
    let frame = bytes::Bytes::from(reference.encode_to_vec());
    let native = proof::bytes::Example::parse_bytes(frame.clone()).unwrap();
    assert_eq!(
        reference::proof::bytes::Example::decode(Serialize::serialize(&native).unwrap().as_slice())
            .unwrap(),
        reference
    );
    assert!(!within(native.chosen(), &frame));
    assert_eq!(native.chunks().get(0).unwrap().as_bytes(), &[0x9b; 16]);
    println!("PASS bytes-default existing-lazy-storage-and-copy-policy");
}
