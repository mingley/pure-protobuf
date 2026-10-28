//! Map decoding keeps entry boundaries without copying an intermediate frame.

use super::{allocated, bytes, scalar};
use pbrs::{Parse, Serialize};

mod api {
    include!(concat!(env!("OUT_DIR"), "/api-codec/codec_cases.rs"));
}

pub(super) fn verify() {
    // Every short string fits existing inline storage. The only allocations
    // are the map's backing object and entry vector; no wire buffer is needed.
    let first = [bytes(1, b"key"), bytes(2, b"before")].concat();
    let last = [bytes(1, b"key"), bytes(2, b"after")].concat();
    let wire = [bytes(1, &first), bytes(1, &last)].concat();
    let mut parsed = None;
    let allocations = allocated(|| parsed = Some(api::Headers::parse(&wire).unwrap()));
    let parsed = parsed.unwrap();
    assert_eq!(allocations, 2, "short map strings copied their parent wire");
    assert_eq!(parsed.h().len(), 1);
    assert_eq!(parsed.h().get("key").unwrap().as_bytes(), b"after");
    assert_eq!(
        api::Headers::parse(&parsed.serialize().unwrap())
            .unwrap()
            .h()
            .get("key")
            .unwrap()
            .as_bytes(),
        b"after"
    );
    println!("PASS short map strings: two container allocations, no wire copy; last wins");

    // Long map strings keep sharing even at nonzero parent offsets. A later
    // entry must not accidentally address bytes relative to an earlier one.
    let key = vec![b'k'; 80];
    let value = vec![b'v'; 96];
    let entry = [bytes(1, &key), bytes(2, &value)].concat();
    let source = pbrs::rt::Bytes::from([bytes(1, &first), bytes(1, &entry)].concat());
    let value_pointer = source.as_ptr().wrapping_add(source.len() - value.len());
    let key_string = std::str::from_utf8(&key).unwrap();
    let parsed = api::Headers::parse_bytes(source.clone()).unwrap();
    assert_eq!(parsed.h().get(key_string).unwrap().as_bytes(), value);
    assert_eq!(
        parsed.h().get(key_string).unwrap().as_bytes().as_ptr(),
        value_pointer,
        "long map strings stopped sharing their caller-owned frame"
    );
    drop(source);
    assert_eq!(parsed.h().get(key_string).unwrap().as_bytes(), value);
    println!("PASS map strings: parent-relative offsets and owned-buffer lifetime");

    for (key, value) in [(b"key".as_slice(), b"\xff".as_slice()), (b"\xff", b"value")] {
        let entry = [bytes(1, key), bytes(2, value)].concat();
        assert!(api::Headers::parse(&bytes(1, &entry)).is_err());
    }
    let malformed = [0x0a, 0x08, b'x'];
    let adjacent = [bytes(1, &malformed), bytes(1, &last)].concat();
    assert!(api::Headers::parse(&adjacent).is_err());
    println!("PASS map strings: UTF-8 and entry bounds reject malformed input");

    macro_rules! nested_offsets {
        ($module:ident, $owned:expr) => {{
            use super::$module::strict_types::BlobRoot;
            let payload = vec![0x5a; 8192];
            let blob = bytes(1, &payload);
            let one = [scalar(1, 4), bytes(2, &blob)].concat();
            let two = [scalar(1, 9), bytes(2, &blob)].concat();
            let wire = [scalar(99, 7), bytes(3, &one), bytes(3, &two)].concat();
            let shared = pbrs::rt::Bytes::from(wire.clone());
            let parsed = BlobRoot::parse_bytes(shared.clone()).unwrap();
            let first = parsed.values().get(4).unwrap();
            let second = parsed.values().get(9).unwrap();
            let a = first.data();
            let b = second.data();
            assert_eq!(a, payload);
            assert_eq!(b, payload);
            if $owned {
                let first_end = scalar(99, 7).len() + bytes(3, &one).len();
                assert_eq!(
                    a.as_ptr(),
                    shared.as_ptr().wrapping_add(first_end - payload.len())
                );
                assert_eq!(
                    b.as_ptr(),
                    shared.as_ptr().wrapping_add(shared.len() - payload.len())
                );
            }
            let copied = BlobRoot::parse(&wire).unwrap();
            assert_eq!(copied.values().get(4).unwrap().data(), payload);
            assert_eq!(copied.values().get(9).unwrap().data(), payload);
            println!(
                "PASS {} map messages: multiple parent offsets, copied and shared input",
                stringify!($module)
            );
        }};
    }
    nested_offsets!(owned, true);
    nested_offsets!(lazy, false);
    nested_offsets!(defaulted, false);
}
