#![no_main]

use libfuzzer_sys::fuzz_target;
use pbrs::gencode::TestAllTypesProto3;
use pbrs::testdata::Person;
use pbrs::{DescriptorPool, DynamicMessage, MessageDescriptor, Parse, Serialize};
use std::sync::{Arc, OnceLock};

const MAX_FUZZ_INPUT_BYTES: usize = 64 * 1024; // 64 KiB

pub fn fuzz_wire(data: &[u8]) {
    let data = if data.len() > MAX_FUZZ_INPUT_BYTES {
        &data[..MAX_FUZZ_INPUT_BYTES]
    } else {
        data
    };

    // 1. Generated TestAllTypesProto3: covers all scalar types, repeated, packed, maps, oneofs
    if let Ok(msg) = TestAllTypesProto3::parse(data) {
        if let Ok(bytes) = msg.serialize() {
            let reparsed = TestAllTypesProto3::parse(&bytes);
            assert!(
                reparsed.is_ok(),
                "re-parsing serialized TestAllTypesProto3 must succeed"
            );
            if let Ok(reparsed_msg) = reparsed {
                if let Ok(re_bytes) = reparsed_msg.serialize() {
                    assert_eq!(
                        bytes, re_bytes,
                        "wire round-trip serialization must be idempotent for TestAllTypesProto3"
                    );
                }
            }
        }
    }

    // 2. Hand-written Person: covers v4 accessor shape and nested messages
    if let Ok(msg) = Person::parse(data) {
        if let Ok(bytes) = msg.serialize() {
            let reparsed = Person::parse(&bytes);
            assert!(
                reparsed.is_ok(),
                "re-parsing serialized Person must succeed"
            );
            if let Ok(reparsed_msg) = reparsed {
                if let Ok(re_bytes) = reparsed_msg.serialize() {
                    assert_eq!(
                        bytes, re_bytes,
                        "wire round-trip serialization must be idempotent for Person"
                    );
                }
            }
        }
    }

    // 3. DynamicMessage parsing: schema-driven dynamic wire decoding
    static DYN_SETUP: OnceLock<(Arc<DescriptorPool>, Arc<MessageDescriptor>)> = OnceLock::new();
    let (pool, desc) = DYN_SETUP.get_or_init(|| {
        let pool = pbrs::gencode::conformance_pool();
        let desc = pool
            .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
            .expect("TestAllTypesProto3 descriptor must exist in conformance pool");
        (pool, desc)
    });

    if let Ok(msg) = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), data) {
        if let Ok(bytes) = msg.serialize() {
            let reparsed = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), &bytes);
            assert!(
                reparsed.is_ok(),
                "re-parsing serialized DynamicMessage must succeed"
            );
            if let Ok(reparsed_msg) = reparsed {
                if let Ok(re_bytes) = reparsed_msg.serialize() {
                    assert_eq!(
                        bytes, re_bytes,
                        "wire round-trip serialization must be idempotent for DynamicMessage"
                    );
                }
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    fuzz_wire(data);
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smoke_seeded_corpus() {
        let corpus: &[&[u8]] = &[
            b"",
            &[0x08, 0xff],
            &[
                0x08, 0x01, 0x12, 0x03, b'a', b'd', b'a', 0x1a, 0x06, b'a', b'd', b'a', b'@',
                b'e', b'x', 0x32, 0x05, 0x0a, 0x03, b'n', b'y', b'c',
            ],
            &[0x08, 0x07, 0x72, 0x03, b'a', b'd', b'a'],
        ];
        for input in corpus {
            fuzz_wire(input);
        }
    }
}
