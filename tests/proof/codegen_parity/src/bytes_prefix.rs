use ::pbrs::{Parse, Serialize};

fn main() {
    let mut message = proof::bytes_prefix::pbrs::new();
    let payload = ::bytes::Bytes::from(vec![0x73; 16]);
    let pointer = payload.as_ptr();
    message.set_payload(payload);
    message.set_payload_shared(::bytes::Bytes::from_static(b"existing-field"));
    assert_eq!(message.payload_shared(), b"existing-field");
    assert_eq!(message.payload_shared_2().as_bytes().as_ptr(), pointer);
    message.set_maybe(::bytes::Bytes::new());
    assert!(message.maybe_shared_opt().unwrap().is_empty());
    message
        .parts_mut()
        .push(::bytes::Bytes::from_static(b"part"));
    message
        .lookup_mut()
        .insert("key", ::bytes::Bytes::from_static(b"value"));
    assert_eq!(message.parts_shared(0).unwrap().as_bytes(), b"part");
    assert_eq!(message.lookup_shared("key").unwrap().as_bytes(), b"value");
    let wire = Serialize::serialize(&message).unwrap();
    let parsed =
        proof::bytes_prefix::pbrs::parse_bytes(::bytes::Bytes::from(wire.clone())).unwrap();
    assert_eq!(Serialize::serialize(&parsed).unwrap(), wire);
    assert_eq!(
        proof::bytes_prefix::pbrs::from_json(&parsed.to_json().unwrap()).unwrap(),
        parsed
    );
    assert_eq!(
        proof::bytes_prefix::pbrs::from_text(&parsed.to_text().unwrap()).unwrap(),
        parsed
    );
    let defaults = proof::bytes_prefix::Defaults::new();
    assert_eq!(
        defaults.payload_shared().as_bytes(),
        b"pbrs::literal runtime_alias::literal"
    );
    assert_eq!(
        proof::bytes_prefix::pbrs::FULL_NAME,
        "proof.bytes_prefix.pbrs"
    );
    let mut other = proof::bytes_prefix::runtime_alias::new();
    other.set_payload(::bytes::Bytes::from_static(b"alias-type"));
    assert_eq!(other.payload_shared().as_bytes(), b"alias-type");
    println!("PASS bytes-prefix {PREFIX} shared-storage-helpers-user-fragments");
}
