#![forbid(unsafe_code)]
use pbrs::Serialize;
include!(concat!(
    env!("OUT_DIR"),
    "/native-bytes-lightweight-safe/mod.rs"
));

fn main() {
    let mut message = proof::bytes::Flat::new();
    message.set_chosen(bytes::Bytes::from_static(b"chosen"));
    message.set_maybe(bytes::Bytes::new());
    message
        .chunks_mut()
        .push(bytes::Bytes::from_static(b"chunk"));
    message
        .lookup_mut()
        .insert("key", bytes::Bytes::from_static(b"value"));
    message.set_raw(bytes::Bytes::from_static(b"\xff"));
    let json = proof::bytes::Flat::from_json(&message.to_json().unwrap()).unwrap();
    let text = proof::bytes::Flat::from_text(&message.to_text().unwrap()).unwrap();
    assert_eq!(
        Serialize::serialize(&json).unwrap(),
        Serialize::serialize(&message).unwrap()
    );
    assert_eq!(
        Serialize::serialize(&text).unwrap(),
        Serialize::serialize(&message).unwrap()
    );
    println!("PASS bytes-lightweight typed-json-text-containers-forbid-unsafe");
}
