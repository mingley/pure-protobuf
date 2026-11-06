use super::*;

fn check_wire<C: PackedCodec>(wire: &[u8])
where
    C::Elem: std::fmt::Debug,
{
    C::validate(wire).expect("valid fixture");
    let mut values = Vec::new();
    C::decode(wire, &mut values).expect("legacy decode");
    let mut expected = Vec::new();
    C::encode(&values, &mut expected);
    let mut wrapped = vec![0x80];
    wrapped.extend_from_slice(wire);
    wrapped.push(0x80);
    let backing = Wire::from_slice(&wrapped);
    let mut packed = Packed::<C>::new();
    packed
        .append_wire(backing.window(1, wire.len() + 1))
        .expect("bounded wire");
    for _ in 0..2 {
        assert_eq!(packed.packed_bytes().unwrap_or_default(), expected);
    }
    assert_eq!(packed.clone().packed_bytes().unwrap_or_default(), expected);
    assert_eq!(packed.iter().copied().collect::<Vec<_>>(), values);
    assert_eq!(packed.packed_bytes().unwrap_or_default(), expected);
    let mut clone = packed.clone();
    clone.push(C::Elem::default());
    let mut appended = values.clone();
    appended.push(C::Elem::default());
    let mut appended_wire = Vec::new();
    C::encode(&appended, &mut appended_wire);
    assert_eq!(clone.packed_bytes().unwrap_or_default(), appended_wire);
    assert_eq!(packed.packed_bytes().unwrap_or_default(), expected);
    clone.clear();
    assert!(clone.packed_bytes().is_none());
}

#[test]
fn normalization_matches_legacy_for_all_varint_codecs() {
    let mut corpus = vec![
        vec![],
        vec![0],
        vec![1],
        vec![2],
        vec![0x80, 0],
        vec![0x81, 0],
    ];
    for value in [
        0,
        1,
        127,
        128,
        255,
        i32::MAX as u64,
        1 << 31,
        u32::MAX as u64,
        1 << 32,
        1 << 40,
        i64::MAX as u64,
        1 << 63,
        u64::MAX,
    ] {
        let mut bytes = Vec::new();
        encode_varint(&mut bytes, value);
        corpus.push(bytes.clone());
        if bytes.len() < 10 {
            *bytes.last_mut().expect("nonempty varint") |= 0x80;
            bytes.push(0);
            corpus.push(bytes);
        }
    }
    let mut state = 1u64;
    for _ in 0..128 {
        let mut run = Vec::new();
        for _ in 0..17 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            encode_varint(&mut run, state);
        }
        corpus.push(run);
    }
    corpus.push([vec![1; 256], vec![0x80, 0]].concat());
    for bytes in corpus {
        check_wire::<VarintI32>(&bytes);
        check_wire::<VarintU32>(&bytes);
        check_wire::<VarintI64>(&bytes);
        check_wire::<VarintU64>(&bytes);
        check_wire::<ZigZag32>(&bytes);
        check_wire::<ZigZag64>(&bytes);
        check_wire::<Bools>(&bytes);
    }
}

fn check_invalid<C: PackedCodec>() {
    let mut packed = Packed::<C>::new();
    packed.append_bytes(&[1]).expect("initial valid run");
    let before = packed.packed_bytes().expect("nonempty").to_vec();
    for invalid in [
        vec![0x80],
        vec![0x80; 10],
        vec![0xff; 10],
        [vec![0xff; 9], vec![2]].concat(),
        vec![1, 2, 0x80],
    ] {
        assert!(C::validate(&invalid).is_err());
        assert!(packed.append_bytes(&invalid).is_err());
        let mut parent = invalid.clone();
        parent.push(0);
        // A terminator outside the window cannot complete its final varint.
        let wire = Wire::from_slice(&parent).window(0, invalid.len());
        assert!(packed.append_wire(wire).is_err());
        assert_eq!(packed.packed_bytes().expect("unchanged"), before);
    }
}

#[test]
fn malformed_and_truncated_windows_keep_prior_state() {
    check_invalid::<VarintI32>();
    check_invalid::<VarintU32>();
    check_invalid::<VarintI64>();
    check_invalid::<VarintU64>();
    check_invalid::<ZigZag32>();
    check_invalid::<ZigZag64>();
    check_invalid::<Bools>();
}

#[test]
fn appending_and_mutation_preserve_combined_values() {
    let mut packed = PackedI32::new();
    packed.append_bytes(&[1, 2]).expect("first");
    assert_eq!(packed.packed_bytes(), Some([1, 2].as_slice()));
    packed.append_bytes(&[0x83, 0]).expect("overlong three");
    assert_eq!(packed.packed_bytes(), Some([1, 2, 3].as_slice()));
    packed.reserve(32);
    packed.as_mut().set(1, 129);
    let mut expected = Vec::new();
    VarintI32::encode(&[1, 129, 3], &mut expected);
    assert_eq!(packed.packed_bytes().unwrap_or_default(), expected);
    assert_eq!(packed.iter().copied().collect::<Vec<_>>(), [1, 129, 3]);
    let owned = PackedI32::from_repeated(Repeated::from_vec(vec![1, 129, 3]));
    assert_eq!(owned.packed_bytes(), packed.packed_bytes());
}

#[test]
fn fixed_width_wire_and_float_payload_bits_are_unchanged() {
    check_wire::<FixedU32>(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    check_wire::<FixedI64>(&[0xff; 16]);
    let mut floats = PackedF32::new();
    let bytes = [0x01, 0x00, 0xc0, 0x7f, 0x00, 0x00, 0x00, 0x80];
    floats.append_bytes(&bytes).expect("NaN and negative zero");
    assert_eq!(floats.packed_bytes(), Some(bytes.as_slice()));
    assert_eq!(
        floats
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>(),
        [0x7fc00001, 0x80000000]
    );
    assert_eq!(floats.clone().packed_bytes(), Some(bytes.as_slice()));
}

std::thread_local! {
    static COUNTERS: std::cell::Cell<(usize, usize, usize)> = const { std::cell::Cell::new((0, 0, 0)) };
    static PANIC_DECODE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ERROR_DECODE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PANIC_ENCODE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
enum Custom {}
impl PackedCodec for Custom {
    type Elem = u8;
    fn validate(_buf: &[u8]) -> Result<(), ParseError> {
        COUNTERS.with(|counts| {
            let (v, d, e) = counts.get();
            counts.set((v + 1, d, e));
        });
        Ok(())
    }
    fn decode(buf: &[u8], out: &mut Vec<u8>) -> Result<(), ParseError> {
        assert!(
            !PANIC_DECODE.with(|flag| flag.replace(false)),
            "custom decode panic"
        );
        if ERROR_DECODE.with(|flag| flag.replace(false)) {
            return Err(ParseError::new("custom decode error"));
        }
        COUNTERS.with(|counts| {
            let (v, d, e) = counts.get();
            counts.set((v, d + 1, e));
        });
        out.extend(buf.iter().map(|value| value % 2));
        Ok(())
    }
    fn encode(elems: &[u8], out: &mut Vec<u8>) {
        assert!(
            !PANIC_ENCODE.with(|flag| flag.replace(false)),
            "custom encode panic"
        );
        COUNTERS.with(|counts| {
            let (v, d, e) = counts.get();
            counts.set((v, d, e + 1));
        });
        out.extend_from_slice(elems);
    }
}

#[test]
fn custom_codec_default_keeps_validation_and_normalization() {
    COUNTERS.with(|counts| counts.set((0, 0, 0)));
    let mut packed = Packed::<Custom>::new();
    packed
        .append_wire(Wire::from_slice(&[2, 3]))
        .expect("custom validated");
    assert_eq!(COUNTERS.with(std::cell::Cell::get), (1, 0, 0));
    for _ in 0..3 {
        assert_eq!(packed.packed_bytes(), Some([0, 1].as_slice()));
    }
    assert_eq!(COUNTERS.with(std::cell::Cell::get), (1, 1, 1));
    assert_eq!(packed.clone().packed_bytes(), Some([0, 1].as_slice()));
    assert_eq!(COUNTERS.with(std::cell::Cell::get), (1, 1, 2));
}

#[test]
fn custom_codec_panics_do_not_publish_partial_caches() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    for decode in [false, true] {
        let mut packed = Packed::<Custom>::new();
        packed
            .append_wire(Wire::from_slice(&[2, 3]))
            .expect("custom validated");
        if decode {
            PANIC_DECODE.with(|flag| flag.set(true));
        } else {
            PANIC_ENCODE.with(|flag| flag.set(true));
        }
        assert!(catch_unwind(AssertUnwindSafe(|| packed.packed_bytes())).is_err());
        assert_eq!(packed.packed_bytes(), Some([0, 1].as_slice()));
        packed.push(7);
        assert_eq!(packed.packed_bytes(), Some([0, 1, 7].as_slice()));
    }
}

#[test]
fn unknown_fields_remain_alongside_canonical_packed_wire() {
    use crate::wire::{UnknownField, UnknownFields, encode_len_field};
    let mut packed = PackedI32::new();
    packed.append_bytes(&[1, 2, 3]).expect("packed run");
    let mut unknown = UnknownFields::default();
    unknown.fields.push(UnknownField::Varint {
        number: 9,
        value: 129,
    });
    unknown.fields.push(UnknownField::LengthDelimited {
        number: 10,
        value: vec![0xff, 0x80],
    });
    let mut out = Vec::new();
    encode_len_field(&mut out, 1, packed.packed_bytes().expect("packed"));
    unknown.encode(&mut out);
    let expected = [0x0a, 3, 1, 2, 3, 0x48, 0x81, 1, 0x52, 2, 0xff, 0x80];
    assert_eq!(out, expected);
    assert_eq!(unknown.encoded_len(), 7);
}

fn check_append_decode_failure(panic: bool) {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let mut packed = Packed::<Custom>::new();
    packed
        .append_wire(Wire::from_slice(&[1]))
        .expect("first wire");
    assert_eq!(packed.packed_bytes(), Some([1].as_slice()));
    if panic {
        PANIC_DECODE.with(|flag| flag.set(true));
    } else {
        ERROR_DECODE.with(|flag| flag.set(true));
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        packed.append_wire(Wire::from_slice(&[3]))
    }));
    if panic {
        assert!(result.is_err());
    } else {
        assert!(matches!(result, Ok(Err(_))));
    }
    packed
        .append_wire(Wire::from_slice(&[2]))
        .expect("new valid wire");
    assert_eq!(packed.packed_bytes(), Some([0].as_slice()));
    assert_eq!(packed.iter().copied().collect::<Vec<_>>(), [0]);
}
#[test]
fn append_decode_failure_error_does_not_reuse_previous_encoded_bytes() {
    check_append_decode_failure(false);
}
#[test]
fn append_decode_failure_panic_does_not_reuse_previous_encoded_bytes() {
    check_append_decode_failure(true);
}
