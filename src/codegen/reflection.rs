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

/// Render the `FILE_DESCRIPTOR_SET` block once; every target file embeds
/// the identical bytes, so callers share one rendering.
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

pub(crate) fn encode_varint_field(out: &mut Vec<u8>, n: u32, v: u64) {
    crate::wire::encode_tag(out, n, crate::wire::WIRE_VARINT);
    encode_varint(out, v);
}

pub(crate) fn encode_string_field(out: &mut Vec<u8>, n: u32, s: &str) {
    encode_len_field(out, n, s.as_bytes());
}
