#![no_main]

use libfuzzer_sys::fuzz_target;

fn ref_decode_varint(buf: &[u8], pos: &mut usize) -> Result<u64, &'static str> {
    let mut result = 0u64;
    let mut shift = 0;
    for i in 0..10 {
        if *pos >= buf.len() {
            return Err("truncated");
        }
        let byte = buf[*pos];
        *pos += 1;
        if i == 9 && byte > 1 {
            return Err("overflow");
        }
        result |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Ok(result);
        }
        shift += 7;
    }
    Err("overflow")
}

fn ref_validate_varints(buf: &[u8]) -> Result<(), &'static str> {
    let mut pos = 0;
    while pos < buf.len() {
        ref_decode_varint(buf, &mut pos)?;
    }
    Ok(())
}

/// Reference packed-payload decode: values decoded before the first error
/// (or all of them) plus whether the payload is well-formed.
fn ref_decode_all(buf: &[u8]) -> (Vec<u64>, bool) {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < buf.len() {
        match ref_decode_varint(buf, &mut pos) {
            Ok(v) => out.push(v),
            Err(_) => return (out, false),
        }
    }
    (out, true)
}

fn ref_zigzag32(n: u64) -> i32 {
    let n = n as u32;
    ((n >> 1) as i32) ^ -((n & 1) as i32)
}

fn ref_zigzag64(n: u64) -> i64 {
    ((n >> 1) as i64) ^ -((n & 1) as i64)
}

/// Packed decode differential for one codec: status plus the full value
/// vector, including the partial prefix when decoding fails partway.
/// Exercises the PK-04 SIMD decode loops on inputs of 16+ bytes.
fn check_codec<C: pbrs::rt::PackedCodec>(
    data: &[u8],
    values: &[u64],
    ok: bool,
    conv: impl Fn(u64) -> C::Elem,
) where
    C::Elem: PartialEq + std::fmt::Debug,
{
    let mut out = Vec::new();
    let status = C::decode(data, &mut out);
    assert_eq!(status.is_ok(), ok, "packed decode status");
    let expect: Vec<C::Elem> = values.iter().map(|x| conv(*x)).collect();
    assert_eq!(out, expect, "packed decode values");
}

fn ref_varint_len(mut value: u64) -> usize {
    let mut n = 1;
    while value >= 0x80 {
        value >>= 7;
        n += 1;
    }
    n
}

fn ref_decode_tag(buf: &[u8], pos: &mut usize) -> Result<(u32, u32), &'static str> {
    let start = *pos;
    let tag = ref_decode_varint(buf, pos)?;
    if *pos - start != ref_varint_len(tag) {
        return Err("overlong tag");
    }
    if tag > u64::from(u32::MAX) {
        return Err("tag overflow");
    }
    let wire = (tag & 7) as u32;
    let number = (tag >> 3) as u32;
    if number == 0 || number > 536_870_911 {
        return Err("illegal field number");
    }
    Ok((number, wire))
}

fn fuzz_varint_diff(data: &[u8]) {
    let mut fast_pos = 0;
    let mut ref_pos = 0;
    let fast = pbrs::rt::decode_varint(data, &mut fast_pos).map_err(|_| ());
    let reference = ref_decode_varint(data, &mut ref_pos).map_err(|_| ());
    assert_eq!(fast.is_ok(), reference.is_ok(), "decode_varint status");
    assert_eq!(fast_pos, ref_pos, "decode_varint position");
    if let (Ok(fast), Ok(reference)) = (fast, reference) {
        assert_eq!(fast, reference, "decode_varint value");
    }

    let mut fast_pos = 0;
    let mut ref_pos = 0;
    let fast = pbrs::rt::decode_tag(data, &mut fast_pos).map_err(|_| ());
    let reference = ref_decode_tag(data, &mut ref_pos).map_err(|_| ());
    assert_eq!(fast.is_ok(), reference.is_ok(), "decode_tag status");
    assert_eq!(fast_pos, ref_pos, "decode_tag position");
    if let (Ok(fast), Ok(reference)) = (fast, reference) {
        assert_eq!(fast, reference, "decode_tag value");
    }

    assert_eq!(
        <pbrs::rt::VarintU64 as pbrs::rt::PackedCodec>::validate(data).is_ok(),
        ref_validate_varints(data).is_ok(),
        "validate_varints status"
    );

    let (values, ok) = ref_decode_all(data);
    check_codec::<pbrs::rt::VarintU64>(data, &values, ok, |v| v);
    check_codec::<pbrs::rt::VarintU32>(data, &values, ok, |v| v as u32);
    check_codec::<pbrs::rt::VarintI32>(data, &values, ok, |v| v as i32);
    check_codec::<pbrs::rt::VarintI64>(data, &values, ok, |v| v as i64);
    check_codec::<pbrs::rt::ZigZag32>(data, &values, ok, ref_zigzag32);
    check_codec::<pbrs::rt::ZigZag64>(data, &values, ok, ref_zigzag64);
    check_codec::<pbrs::rt::Bools>(data, &values, ok, |v| v != 0);
}

fuzz_target!(|data: &[u8]| {
    fuzz_varint_diff(data);
});

#[cfg(test)]
mod tests {
    use super::fuzz_varint_diff;

    #[test]
    fn seeded_varint_and_tag_cases_match_reference() {
        for case in [
            &b""[..],
            &[0],
            &[8],
            &[0x80],
            &[0x80, 0x01],
            &[0xff, 0x01],
            &[0xff, 0xff, 0xff, 0xff, 0x0f],
            &[0x80, 0x00],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
            &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00,
            ],
            // SIMD chunk boundaries: single-byte runs around 16/32.
            &[0x7f; 15][..],
            &[0x7f; 16][..],
            &[0x7f; 17][..],
            &[0x7f; 31][..],
            &[0x7f; 32][..],
            &[0x7f; 33][..],
            // One all-pairs 16-byte chunk (overlong pairs).
            &[
                0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00,
                0x80, 0x00,
            ][..],
            // Pair split across the 16-byte chunk boundary.
            &[
                0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f,
                0x7f, 0x81, 0x01,
            ][..],
            // 10-byte max/overflow and truncation after full SIMD chunks.
            &[
                0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f,
                0x7f, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
            ][..],
            &[
                0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f,
                0x7f, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02,
            ][..],
            &[
                0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f,
                0x7f, 0x7f, 0x80,
            ][..],
        ] {
            fuzz_varint_diff(case);
        }
        // Constructed long shapes: pair runs, packed_256-like payloads.
        let mut pairs32 = Vec::new();
        for i in 0..16u8 {
            pairs32.push(0x80 | i);
            pairs32.push(i);
        }
        let mut p256 = Vec::new();
        for i in 0..128u8 {
            p256.push(i);
        }
        for i in 0..128u8 {
            p256.push(0x80 | i);
            p256.push(0x01);
        }
        let mut alt = Vec::new();
        for i in 0..20u8 {
            alt.push(i);
            alt.push(0x80 | i);
            alt.push(i);
        }
        for case in [&pairs32, &p256, &alt] {
            fuzz_varint_diff(case);
        }
    }
}
