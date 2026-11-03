//! Serialized descriptor names must be valid before entering the symbol table.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "regression tests assert descriptor validation and symbol lookup results"
)]

use pbrs::DescriptorPool;

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(u8::try_from(value & 0x7f).expect("seven bits") | 0x80);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("last seven bits"));
}

fn encode_len_field(out: &mut Vec<u8>, field: u32, payload: &[u8]) {
    varint(out, u64::from((field << 3) | 2));
    varint(out, u64::try_from(payload.len()).expect("fixture length"));
    out.extend_from_slice(payload);
}

fn encode_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
    varint(out, u64::from(field << 3));
    varint(out, value);
}

fn text(out: &mut Vec<u8>, field: u32, value: &str) {
    encode_len_field(out, field, value.as_bytes());
}

fn descriptor(kind: &str, name: &str, package: &str) -> Vec<u8> {
    let mut file = Vec::new();
    text(&mut file, 1, "names.proto");
    text(&mut file, 2, package);
    text(&mut file, 12, "proto3");

    let mut message = Vec::new();
    text(
        &mut message,
        1,
        if kind == "message" { name } else { "_Message1" },
    );
    let mut field = Vec::new();
    text(
        &mut field,
        1,
        if kind == "field" { name } else { "_field1" },
    );
    encode_varint_field(&mut field, 3, 1);
    encode_varint_field(&mut field, 4, 1);
    encode_varint_field(&mut field, 5, 5);
    if kind == "oneof" {
        let mut oneof = Vec::new();
        text(&mut oneof, 1, name);
        encode_len_field(&mut message, 8, &oneof);
        encode_varint_field(&mut field, 9, 0);
    }
    encode_len_field(&mut message, 2, &field);
    if kind == "nested_message" {
        let mut nested = Vec::new();
        text(&mut nested, 1, name);
        encode_len_field(&mut message, 3, &nested);
    }
    encode_len_field(&mut file, 4, &message);

    let mut en = Vec::new();
    text(&mut en, 1, if kind == "enum" { name } else { "_Enum1" });
    let mut value = Vec::new();
    text(
        &mut value,
        1,
        if kind == "enum_value" { name } else { "_ZERO1" },
    );
    encode_varint_field(&mut value, 2, 0);
    encode_len_field(&mut en, 2, &value);
    encode_len_field(&mut file, 5, &en);

    let mut svc = Vec::new();
    text(
        &mut svc,
        1,
        if kind == "service" { name } else { "_Service1" },
    );
    let mut method = Vec::new();
    text(
        &mut method,
        1,
        if kind == "method" { name } else { "_Method1" },
    );
    let message_type = if package.is_empty() {
        "._Message1".to_owned()
    } else {
        format!(".{package}._Message1")
    };
    text(&mut method, 2, &message_type);
    text(&mut method, 3, &message_type);
    encode_len_field(&mut svc, 2, &method);
    encode_len_field(&mut file, 6, &svc);

    let mut fds = Vec::new();
    encode_len_field(&mut fds, 1, &file);
    fds
}

fn assert_rejected(fds: &[u8], context: &str) {
    assert!(
        DescriptorPool::from_file_descriptor_set(fds).is_err(),
        "{context}"
    );
    assert!(
        pbrs::codegen::generate_from_file_descriptor_set(fds, &["names.proto".to_owned()]).is_err(),
        "code generator accepted {context}"
    );
}

#[test]
fn scheduled_descriptor_crasher_is_rejected() {
    let bytes = include_bytes!("../fuzz/corpus/descriptors/crash-52f06f01d32b69d1.bin");
    assert_rejected(bytes, "the retained descriptor fuzzer regression");
    // The fuzzer also interprets each seed as a CodeGeneratorRequest. That
    // independent entry point must complete without an assertion or panic.
    drop(pbrs::codegen::generate_from_code_generator_request(bytes));
}

#[test]
fn packages_require_nonempty_identifier_components() {
    for package in [
        ".pkg",
        "pkg.",
        "pkg..nested",
        "9pkg",
        "pkg/path",
        "pkg\0x",
        "pkg.é",
    ] {
        assert_rejected(&descriptor("message", "_Message1", package), package);
    }
}

#[test]
fn every_declared_symbol_name_uses_protobuf_identifier_grammar() {
    for kind in [
        "message",
        "nested_message",
        "field",
        "oneof",
        "enum",
        "enum_value",
        "service",
        "method",
    ] {
        for name in [
            "",
            ".Name",
            "Name.part",
            "9Name",
            "Name/path",
            "Name\0x",
            "Náme",
        ] {
            assert_rejected(
                &descriptor(kind, name, "_root.Sub_2"),
                &format!("{kind}: {name:?}"),
            );
        }
    }
}

#[test]
fn empty_package_and_ascii_identifiers_keep_resolvable_symbols() {
    for package in ["", "_root.Sub_2"] {
        let fds = descriptor("nested_message", "_Nested2", package);
        let pool = DescriptorPool::from_file_descriptor_set(&fds).expect("valid descriptor");
        for name in pool.collect_names() {
            let message = pool.get_message(&name).expect("collected message resolves");
            assert_eq!(message.full_name, name);
            assert_eq!(
                pool.get_message(&format!(".{name}"))
                    .expect("absolute name")
                    .full_name,
                name
            );
        }
        for name in pool.collect_enum_names() {
            assert_eq!(
                pool.get_enum(&name)
                    .expect("collected enum resolves")
                    .full_name,
                name
            );
        }
        for service in pool.collect_services() {
            assert_eq!(
                pool.get_service(&service.full_name)
                    .expect("service resolves")
                    .full_name,
                service.full_name
            );
        }
        let files =
            pbrs::codegen::generate_from_file_descriptor_set(&fds, &["names.proto".to_owned()])
                .expect("generate valid names");
        assert!(!files.is_empty());
    }
}
