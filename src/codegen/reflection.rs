//! MX-01 split of `super`: reflection (mechanical move, no behavior change).

use super::*;
use crate::wire::{self, WIRE_LEN, decode_tag, encode_len_field, encode_varint, read_len_bytes};

pub(crate) fn extract_fds_file_names(bytes: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        if let Ok((n, w)) = decode_tag(bytes, &mut pos) {
            if n == 1 && w == WIRE_LEN {
                if let Ok(blob) = read_len_bytes(bytes, &mut pos) {
                    let mut file_pos = 0;
                    let mut file_name = None;
                    while file_pos < blob.len() {
                        if let Ok((fn_num, fn_w)) = decode_tag(blob, &mut file_pos) {
                            match (fn_num, fn_w) {
                                (1, WIRE_LEN) => {
                                    if let Ok(name_bytes) = read_len_bytes(blob, &mut file_pos) {
                                        file_name =
                                            Some(String::from_utf8_lossy(name_bytes).into_owned());
                                    }
                                }
                                (3, WIRE_LEN) => {
                                    if let Ok(dep_bytes) = read_len_bytes(blob, &mut file_pos) {
                                        names.push(String::from_utf8_lossy(dep_bytes).into_owned());
                                    }
                                }
                                (_, w) => {
                                    let _ = wire::skip_field(blob, &mut file_pos, w);
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    if let Some(name) = file_name {
                        names.push(name);
                    }
                }
            } else {
                let _ = wire::skip_field(bytes, &mut pos, w);
            }
        } else {
            break;
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Render the `FILE_DESCRIPTOR_SET` block; every target file embeds the
/// identical bytes, so callers share one rendering.
///
/// Default per-file embedding is intentional: any single generated file must
/// compile standalone via `include!`. The opt-in `shared_descriptor_set` mode
/// renders this block once in a sibling helper included by the root registry.
/// Default cross-file sharing happens at runtime instead — generated
/// `generated_pool()` constructors defer to first reflection use, and
/// [`DescriptorPool::from_file_descriptor_set`](crate::DescriptorPool::from_file_descriptor_set)
/// parses identical bytes once per process and shares the parsed pool.
/// Keep this rendering byte-stable: consumers and determinism tests rely on
/// identical output for identical inputs (see the roundtrip test below).
pub(crate) fn fds_hex_block(fds: &[u8]) -> String {
    // 5 chars per byte ("0x..,"), plus indent/newlines. Table push is
    // byte-identical to the old per-byte write!("0x{b:02x},").
    let mut buf = Vec::with_capacity(128 + fds.len() * 5 + fds.len() / 16 * 5 + 8);
    buf.extend_from_slice(
        b"/// FileDescriptorSet bytes for reflection and dynamic schema inspection.\n",
    );
    buf.extend_from_slice(b"pub const FILE_DESCRIPTOR_SET: &[u8] = &[\n");
    for (i, b) in fds.iter().enumerate() {
        if i % 16 == 0 {
            buf.extend_from_slice(b"    ");
        }
        buf.extend_from_slice(&HEX_BYTE_CHUNK[*b as usize]);
        if i % 16 == 15 {
            buf.push(b'\n');
        }
    }
    if fds.len() % 16 != 0 {
        buf.push(b'\n');
    }
    buf.extend_from_slice(b"];\n\n");
    // Table, indent and punctuation are pure ASCII by construction.
    String::from_utf8(buf).expect("hex table output is ASCII-only")
}

/// A single byte-string token avoids an array-expression node for every byte.
/// Only the opt-in shared metadata owner uses this representation; standalone
/// per-file descriptor output remains byte-identical.
pub(crate) fn fds_byte_string_block(fds: &[u8]) -> String {
    let mut buf = Vec::with_capacity(128 + fds.len() * 4);
    buf.extend_from_slice(
        b"/// FileDescriptorSet bytes for reflection and dynamic schema inspection.\n",
    );
    buf.extend_from_slice(b"pub const FILE_DESCRIPTOR_SET: &[u8] = b\"");
    for byte in fds {
        buf.extend_from_slice(b"\\x");
        buf.extend_from_slice(&HEX_BYTE_CHUNK[*byte as usize][2..4]);
    }
    buf.extend_from_slice(b"\";\n\n");
    String::from_utf8(buf).expect("escaped byte-string output is ASCII-only")
}

pub(crate) const SHARED_DESCRIPTOR_MODULE: &str = "__pbrs_shared_descriptors";
pub(crate) const SHARED_DESCRIPTOR_FILE: &str = "__pbrs_shared_descriptors.rs";

/// Render the opt-in metadata owner; parsing still waits for reflection use.
pub(crate) fn shared_descriptor_source(fds_block: &str) -> String {
    let mut source = String::from("// @generated by protoc-gen-pbrs\n");
    source.push_str(fds_block);
    source.push_str(
        r#"pub(crate) static POOL: std::sync::OnceLock<std::sync::Arc<pbrs::DescriptorPool>> = std::sync::OnceLock::new();

#[allow(clippy::expect_used, reason = "embedded descriptor bytes were validated during code generation")]
pub(crate) fn generated_pool() -> std::sync::Arc<pbrs::DescriptorPool> {
    POOL.get_or_init(|| {
        std::sync::Arc::new(pbrs::DescriptorPool::from_file_descriptor_set(FILE_DESCRIPTOR_SET).expect("fds"))
    }).clone()
}
"#,
    );
    source
}

pub(crate) fn encode_varint_field(out: &mut Vec<u8>, n: u32, v: u64) {
    crate::wire::encode_tag(out, n, crate::wire::WIRE_VARINT);
    encode_varint(out, v);
}

pub(crate) fn encode_string_field(out: &mut Vec<u8>, n: u32, s: &str) {
    encode_len_field(out, n, s.as_bytes());
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "unit tests assert success paths directly"
    )]

    use super::*;

    #[test]
    fn shared_byte_string_matches_a_compiled_literal_for_every_byte() {
        const RAW: &[u8] = b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\x29\x2a\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\x34\x35\x36\x37\x38\x39\x3a\x3b\x3c\x3d\x3e\x3f\x40\x41\x42\x43\x44\x45\x46\x47\x48\x49\x4a\x4b\x4c\x4d\x4e\x4f\x50\x51\x52\x53\x54\x55\x56\x57\x58\x59\x5a\x5b\x5c\x5d\x5e\x5f\x60\x61\x62\x63\x64\x65\x66\x67\x68\x69\x6a\x6b\x6c\x6d\x6e\x6f\x70\x71\x72\x73\x74\x75\x76\x77\x78\x79\x7a\x7b\x7c\x7d\x7e\x7f\x80\x81\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x8b\x8c\x8d\x8e\x8f\x90\x91\x92\x93\x94\x95\x96\x97\x98\x99\x9a\x9b\x9c\x9d\x9e\x9f\xa0\xa1\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xab\xac\xad\xae\xaf\xb0\xb1\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xbb\xbc\xbd\xbe\xbf\xc0\xc1\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xcb\xcc\xcd\xce\xcf\xd0\xd1\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xdb\xdc\xdd\xde\xdf\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf0\xf1\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xfb\xfc\xfd\xfe\xff";
        const SOURCE: &str = stringify!(b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\x29\x2a\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\x34\x35\x36\x37\x38\x39\x3a\x3b\x3c\x3d\x3e\x3f\x40\x41\x42\x43\x44\x45\x46\x47\x48\x49\x4a\x4b\x4c\x4d\x4e\x4f\x50\x51\x52\x53\x54\x55\x56\x57\x58\x59\x5a\x5b\x5c\x5d\x5e\x5f\x60\x61\x62\x63\x64\x65\x66\x67\x68\x69\x6a\x6b\x6c\x6d\x6e\x6f\x70\x71\x72\x73\x74\x75\x76\x77\x78\x79\x7a\x7b\x7c\x7d\x7e\x7f\x80\x81\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x8b\x8c\x8d\x8e\x8f\x90\x91\x92\x93\x94\x95\x96\x97\x98\x99\x9a\x9b\x9c\x9d\x9e\x9f\xa0\xa1\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xab\xac\xad\xae\xaf\xb0\xb1\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xbb\xbc\xbd\xbe\xbf\xc0\xc1\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xcb\xcc\xcd\xce\xcf\xd0\xd1\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xdb\xdc\xdd\xde\xdf\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf0\xf1\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xfb\xfc\xfd\xfe\xff");
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(RAW, bytes.as_slice());
        let prefix = "/// FileDescriptorSet bytes for reflection and dynamic schema inspection.\npub const FILE_DESCRIPTOR_SET: &[u8] = ";
        assert_eq!(
            fds_byte_string_block(&bytes),
            format!("{prefix}{SOURCE};\n\n")
        );
        assert_eq!(fds_byte_string_block(&[]), format!("{prefix}b\"\";\n\n"));
    }

    /// Parse the `0x..,` array body back out of a rendered block.
    fn parse_hex_block(block: &str) -> Vec<u8> {
        let body = block.rsplit("&[").next().expect("array body");
        let mut out = Vec::new();
        for chunk in body.split(',') {
            let Some(hex) = chunk.trim().strip_prefix("0x") else {
                continue;
            };
            out.push(u8::from_str_radix(hex, 16).expect("hex byte"));
        }
        out
    }

    #[test]
    fn fds_hex_block_roundtrips_bytes() {
        // Empty, single-byte, exact-wrap (16), wrap-plus-one, and every byte
        // value: the runtime cache keys on these exact bytes, so the emission
        // must reproduce them faithfully for standalone `include!` files.
        let mut cases: Vec<Vec<u8>> = vec![vec![], vec![0x00], vec![0xff]];
        cases.push((0..16).collect());
        cases.push((0..17).collect());
        cases.push((0..=255).collect());
        for fds in &cases {
            let block = fds_hex_block(fds);
            assert!(block.starts_with(
                "/// FileDescriptorSet bytes for reflection and dynamic schema inspection.\n"
            ));
            assert!(block.contains("pub const FILE_DESCRIPTOR_SET: &[u8] = &[\n"));
            assert!(block.ends_with("];\n\n"));
            assert_eq!(parse_hex_block(&block), *fds);
        }
    }
}
