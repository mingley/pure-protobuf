//! Generate field-wise typed messages (not DynamicMessage wrappers, not upb).
#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    reason = "protoc plugin / compile_protos is sync build-time IO, not an async worker"
)]

pub use crate::dynamic::{Comments, SourceCodeInfo, SourceLocation};
use std::fmt::Write as _;

/// Lowercase hex digits for byte-literal emission without `core::fmt`.
pub(crate) const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Pre-rendered `0x..,` chunks, one per byte value.
pub(crate) const HEX_BYTE_CHUNK: [[u8; 5]; 256] = build_hex_byte_chunk();

const fn build_hex_byte_chunk() -> [[u8; 5]; 256] {
    let mut table = [[0u8; 5]; 256];
    let mut b = 0usize;
    while b < 256 {
        table[b] = [b'0', b'x', HEX_DIGITS[b >> 4], HEX_DIGITS[b & 0xf], b','];
        b += 1;
    }
    table
}

mod compat_stubs;
mod config;
mod descriptors;
mod encode;
mod extensions;
mod json;
mod messages;
mod naming;
mod native_stubs;
mod parse;
pub mod prost_stubs;
mod reflection;
mod text;
mod tonic_stubs;

pub(crate) use compat_stubs::*;
pub(crate) use config::*;
pub(crate) use descriptors::*;
pub(crate) use encode::*;
pub(crate) use extensions::*;
pub(crate) use json::*;
pub(crate) use messages::*;
pub(crate) use naming::*;
pub(crate) use native_stubs::*;
pub(crate) use parse::*;
pub(crate) use reflection::*;
pub(crate) use text::*;
pub(crate) use tonic_stubs::*;

pub use config::{CodegenError, Config, Stubs};
pub use descriptors::{
    compile_protos, encode_code_generator_response, encode_code_generator_response_error,
    generate_from_code_generator_request, generate_from_file_descriptor_set,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn missing_import_source_restores_requested_path_from_relative_protoc_error() {
        let input = PathBuf::from("schemas/failing_import.proto");
        let includes = [PathBuf::from("schemas")];
        let (missing, reported) = parse_missing_import(
            "failing_import.proto:3:1: Import \"nonexistent_dependency.proto\" was not found.",
        )
        .expect("missing import diagnostic");
        assert_eq!(missing, "nonexistent_dependency.proto");
        assert_eq!(
            missing_import_source(reported, std::slice::from_ref(&input), &includes),
            input
        );
        assert_eq!(
            missing_import_source(PathBuf::new(), std::slice::from_ref(&input), &includes),
            input
        );
        assert_eq!(
            missing_import_source(
                PathBuf::from("elsewhere.proto"),
                std::slice::from_ref(&input),
                &includes,
            ),
            PathBuf::from("elsewhere.proto")
        );
    }

    #[test]
    fn test_sanitize_doc_line_backticks() {
        assert_eq!(sanitize_doc_line("`single"), "\\`single");
        assert_eq!(sanitize_doc_line("`foo` and `bar"), "`foo` and \\`bar");
        assert_eq!(sanitize_doc_line("``code`inside``"), "``code`inside``");
        assert_eq!(
            sanitize_doc_line("already \\`escaped"),
            "already \\`escaped"
        );
    }

    #[test]
    fn test_sanitize_doc_line_links_and_urls() {
        assert_eq!(sanitize_doc_line("[BrokenLink]"), "\\[BrokenLink\\]");
        assert_eq!(
            sanitize_doc_line("[BrokenLink][ref]"),
            "\\[BrokenLink\\]\\[ref\\]"
        );
        assert_eq!(sanitize_doc_line("[unclosed"), "\\[unclosed");
        assert_eq!(
            sanitize_doc_line("[Valid](https://example.com)"),
            "[Valid](https://example.com)"
        );
        assert_eq!(
            sanitize_doc_line("See https://example.com/path?a=1 for details."),
            "See <https://example.com/path?a=1> for details."
        );
        assert_eq!(
            sanitize_doc_line("<https://already.autolink.com>"),
            "<https://already.autolink.com>"
        );
    }

    #[test]
    fn test_sanitize_doc_line_html_and_inequalities() {
        assert_eq!(sanitize_doc_line("<custom-tag>"), "\\<custom-tag\\>");
        assert_eq!(sanitize_doc_line("<T>"), "\\<T\\>");
        assert_eq!(
            sanitize_doc_line("Map<string, int32>"),
            "Map\\<string, int32\\>"
        );
        assert_eq!(sanitize_doc_line("1 < 2 && 5 > 3"), "1 \\< 2 && 5 \\> 3");
    }

    #[test]
    fn test_sanitize_doc_line_indentation() {
        assert_eq!(
            sanitize_doc_line("    indented 4 spaces"),
            "  indented 4 spaces"
        );
        assert_eq!(
            sanitize_doc_line("  indented 2 spaces"),
            "  indented 2 spaces"
        );
    }

    #[test]
    fn test_emit_doc_line_code_fences() {
        let mut src = String::new();
        let mut in_fence = false;
        emit_doc_line(&mut src, "```", "", &mut in_fence);
        assert!(in_fence);
        assert_eq!(src, "/// ```text\n");
        emit_doc_line(&mut src, "invalid rust syntax !@#$", "", &mut in_fence);
        assert_eq!(src, "/// ```text\n/// invalid rust syntax !@#$\n");
        emit_doc_line(&mut src, "```", "", &mut in_fence);
        assert!(!in_fence);
        assert_eq!(src, "/// ```text\n/// invalid rust syntax !@#$\n/// ```\n");
    }

    #[test]
    fn test_starts_with_ignore_ascii_non_boundary() {
        // Fuzzer crash: prefix length landing inside a multibyte char must
        // return false, not panic on byte slicing.
        assert!(!starts_with_ignore_ascii("abé", "abc"));
        assert!(!starts_with_ignore_ascii("é", "ex"));
        assert!(starts_with_ignore_ascii("FOOBar", "foo"));
        assert!(!starts_with_ignore_ascii("short", "much longer prefix"));
    }

    #[test]
    fn bytes_fields_encode_via_shared_helper() {
        // PK-11: bytes fields offer their buffer to segmented sinks; string
        // fields keep the copying helper.
        use crate::dynamic::{Cardinality, FieldDescriptor, FieldType, Presence};

        fn field(field_type: FieldType) -> FieldDescriptor {
            FieldDescriptor::new(
                "data",
                2,
                field_type,
                Cardinality::Optional,
                Presence::Implicit,
            )
        }

        let mut src = String::new();
        emit_write(&mut src, &field(FieldType::Bytes), "self");
        assert!(
            src.contains("encode_len_field_shared(out, 2, &self.data"),
            "{src}"
        );
        let mut src = String::new();
        emit_write(&mut src, &field(FieldType::String), "self");
        assert!(src.contains("encode_len_field(out, 2,"), "{src}");
        assert!(!src.contains("encode_len_field_shared"), "{src}");
    }

    #[test]
    fn package_keyword_segments_escape_in_modules_and_paths() {
        assert_eq!(mod_ident("type"), "r#type");
        assert_eq!(mod_ident("match"), "r#match");
        assert_eq!(mod_ident("v3"), "v3");
        assert_eq!(mod_ident("self"), "self_");
        assert_eq!(pkg_mod_path("envoy.type.v3"), "crate::envoy::r#type::v3");
        assert_eq!(pkg_mod_path("bench.cg19"), "crate::bench::cg19");
    }
}
