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
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00],
        ] {
            fuzz_varint_diff(case);
        }
    }
}
