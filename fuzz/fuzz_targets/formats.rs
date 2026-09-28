#![no_main]

use libfuzzer_sys::fuzz_target;
use pbrs::gencode::{
    TestAllTypesEdition2023, TestAllTypesProto2, TestAllTypesProto3, conformance_pool,
};
use pbrs::{DescriptorPool, DynamicMessage, MessageDescriptor};
use std::sync::{Arc, OnceLock};

const MAX_FUZZ_INPUT_BYTES: usize = 64 * 1024; // 64 KiB
const MAX_OUTPUT_RECHECK_BYTES: usize = 64 * 1024; // 64 KiB

struct FormatDescriptors {
    pool: Arc<DescriptorPool>,
    proto3_desc: Arc<MessageDescriptor>,
    proto2_desc: Arc<MessageDescriptor>,
    edition_desc: Arc<MessageDescriptor>,
    timestamp_desc: Arc<MessageDescriptor>,
    duration_desc: Arc<MessageDescriptor>,
    struct_desc: Arc<MessageDescriptor>,
}

fn descriptors() -> &'static FormatDescriptors {
    static SETUP: OnceLock<FormatDescriptors> = OnceLock::new();
    SETUP.get_or_init(|| {
        let pool = conformance_pool();
        let proto3_desc = pool
            .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
            .expect("TestAllTypesProto3 descriptor must exist in conformance pool");
        let proto2_desc = pool
            .get_message("protobuf_test_messages.proto2.TestAllTypesProto2")
            .expect("TestAllTypesProto2 descriptor must exist in conformance pool");
        let edition_desc = pool
            .get_message("protobuf_test_messages.editions.TestAllTypesEdition2023")
            .expect("TestAllTypesEdition2023 descriptor must exist in conformance pool");
        let timestamp_desc = pool
            .get_message("google.protobuf.Timestamp")
            .expect("Timestamp descriptor must exist in conformance pool");
        let duration_desc = pool
            .get_message("google.protobuf.Duration")
            .expect("Duration descriptor must exist in conformance pool");
        let struct_desc = pool
            .get_message("google.protobuf.Struct")
            .expect("Struct descriptor must exist in conformance pool");
        FormatDescriptors {
            pool,
            proto3_desc,
            proto2_desc,
            edition_desc,
            timestamp_desc,
            duration_desc,
            struct_desc,
        }
    })
}

pub fn fuzz_formats_str(text: &str) {
    let descs = descriptors();

    // 1. Raw JSON parser duplicate-key check
    let _ = pbrs::json::parse(text);

    // 2. DynamicMessage format parsers across varied schema types
    for desc in [
        &descs.proto3_desc,
        &descs.proto2_desc,
        &descs.edition_desc,
        &descs.timestamp_desc,
        &descs.duration_desc,
        &descs.struct_desc,
    ] {
        // from_json variants
        if let Ok(msg) = DynamicMessage::from_json(desc.clone(), text) {
            if let Ok(json_out) = msg.to_json() {
                if json_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                    let reparsed = DynamicMessage::from_json(desc.clone(), &json_out);
                    assert!(
                        reparsed.is_ok(),
                        "re-parsing valid json_output must succeed"
                    );
                }
            }
            if let Ok(text_out) = msg.to_text() {
                if text_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                    let _ = DynamicMessage::from_text(desc.clone(), &text_out);
                }
            }
        }
        let _ = DynamicMessage::from_json_ignore_unknown(desc.clone(), text);
        let _ = DynamicMessage::from_json_with_pool(
            desc.clone(),
            Some(descs.pool.clone()),
            text,
            false,
        );
        let _ =
            DynamicMessage::from_json_with_pool(desc.clone(), Some(descs.pool.clone()), text, true);

        // from_text variants
        if let Ok(msg) = DynamicMessage::from_text(desc.clone(), text) {
            if let Ok(text_out) = msg.to_text() {
                if text_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                    let reparsed = DynamicMessage::from_text(desc.clone(), &text_out);
                    assert!(
                        reparsed.is_ok(),
                        "re-parsing valid text_output must succeed"
                    );
                }
            }
            if let Ok(json_out) = msg.to_json() {
                if json_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                    let _ = DynamicMessage::from_json(desc.clone(), &json_out);
                }
            }
        }
        let _ = DynamicMessage::from_text_with_pool(desc.clone(), Some(descs.pool.clone()), text);
    }

    // 3. Generated typed message format parsers (conformance_json_output paths)
    // Proto3
    if let Ok(msg) = TestAllTypesProto3::from_json(text) {
        if let Ok(json_out) = msg.to_json() {
            if json_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                let reparsed = TestAllTypesProto3::from_json(&json_out);
                assert!(
                    reparsed.is_ok(),
                    "re-parsing generated proto3 json output must succeed"
                );
            }
        }
    }
    let _ = TestAllTypesProto3::from_json_ignore(text, true);
    if let Ok(msg) = TestAllTypesProto3::from_text(text) {
        if let Ok(text_out) = msg.to_text() {
            if text_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                let reparsed = TestAllTypesProto3::from_text(&text_out);
                assert!(
                    reparsed.is_ok(),
                    "re-parsing generated proto3 text output must succeed"
                );
            }
        }
    }

    // Proto2
    if let Ok(msg) = TestAllTypesProto2::from_json(text) {
        if let Ok(json_out) = msg.to_json() {
            if json_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                let reparsed = TestAllTypesProto2::from_json(&json_out);
                assert!(
                    reparsed.is_ok(),
                    "re-parsing generated proto2 json output must succeed"
                );
            }
        }
    }
    let _ = TestAllTypesProto2::from_json_ignore(text, true);
    if let Ok(msg) = TestAllTypesProto2::from_text(text) {
        if let Ok(text_out) = msg.to_text() {
            if text_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                let reparsed = TestAllTypesProto2::from_text(&text_out);
                assert!(
                    reparsed.is_ok(),
                    "re-parsing generated proto2 text output must succeed"
                );
            }
        }
    }

    // Edition 2023
    if let Ok(msg) = TestAllTypesEdition2023::from_json(text) {
        if let Ok(json_out) = msg.to_json() {
            if json_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                let reparsed = TestAllTypesEdition2023::from_json(&json_out);
                assert!(
                    reparsed.is_ok(),
                    "re-parsing generated edition2023 json output must succeed"
                );
            }
        }
    }
    let _ = TestAllTypesEdition2023::from_json_ignore(text, true);
    if let Ok(msg) = TestAllTypesEdition2023::from_text(text) {
        if let Ok(text_out) = msg.to_text() {
            if text_out.len() <= MAX_OUTPUT_RECHECK_BYTES {
                let reparsed = TestAllTypesEdition2023::from_text(&text_out);
                assert!(
                    reparsed.is_ok(),
                    "re-parsing generated edition2023 text output must succeed"
                );
            }
        }
    }
}

pub fn fuzz_formats(data: &[u8]) {
    let data = if data.len() > MAX_FUZZ_INPUT_BYTES {
        &data[..MAX_FUZZ_INPUT_BYTES]
    } else {
        data
    };

    // If data is valid UTF-8, exercise directly as a string slice
    if let Ok(text) = std::str::from_utf8(data) {
        fuzz_formats_str(text);
    } else {
        // Also interpret arbitrary non-UTF-8 bytes via lossy decoding so malformed raw inputs are tested
        let lossy = String::from_utf8_lossy(data);
        fuzz_formats_str(&lossy);
    }
}

fuzz_target!(|data: &[u8]| {
    fuzz_formats(data);
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smoke_formats() {
        let corpus: &[&[u8]] = &[
            b"",
            b"{}",
            b"{\"optionalInt32\": 42}",
            b"optional_int32: 42",
            b"invalid json {",
            b"{\"optionalFloat\": \"NaN\"}",
            b"{\"optionalDouble\": -0.0}",
            b"\xff\xfe\xfd",
        ];
        for input in corpus {
            fuzz_formats(input);
        }
    }
}
