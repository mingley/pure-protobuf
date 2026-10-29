use pbrs::Serialize;
#[allow(dead_code, nonstandard_style, unused_imports)]
mod candidate {
    include!(concat!(env!("OUT_DIR"), "/proof.rs"));
}
#[allow(dead_code, nonstandard_style, unused_imports)]
mod legacy {
    include!(concat!(env!("OUT_DIR"), "/legacy.rs"));
}

fn compare(bytes: &[u8], group: Option<u32>) {
    let mut old = legacy::Runs::default();
    let mut new = candidate::Runs::default();
    let (mut old_pos, mut new_pos) = (0, 0);
    let old_result = old.merge_inner(bytes, &mut None, &mut old_pos, 0, false, group);
    let new_result = new.merge_inner(bytes, &mut None, &mut new_pos, 0, false, group);
    assert_eq!(
        format!("{old_result:?}"),
        format!("{new_result:?}"),
        "result: {bytes:?}"
    );
    assert_eq!(old_pos, new_pos, "cursor: {bytes:?}");
    assert_eq!(old.serialize(), new.serialize(), "partial state: {bytes:?}");
}
fn main() {
    let prefixes: &[&[u8]] = &[
        &[10, 1, b'a'],
        &[16, 1],
        &[26, 2, 8, 1],
        &[122, 1, b'z'],
        &[130, 1, 1, b'x'],
    ];
    let suffixes: &[&[u8]] = &[
        &[],
        &[10, 1, b'b'],
        &[16, 2],
        &[26, 2, 8, 2],
        &[122, 1, b'y'],
        &[130, 1, 1, b'q'],
        &[32, 1],
        &[48, 7],
        &[8, 3],
        &[138, 0, 1, b'c'],
        &[144, 0, 2],
        &[154, 0, 2, 8, 4],
        &[128],
        &[128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 1],
        &[10, 2, b'x'],
        &[16, 128],
        &[26, 2, 8, 128],
        &[122, 2, b'x'],
        &[44],
        &[52],
    ];
    for prefix in prefixes {
        for suffix in suffixes {
            let bytes = [*prefix, *suffix].concat();
            compare(&bytes, None);
            compare(&bytes, Some(5));
        }
    }
    // Interleaved runs, a nested group with a valid ending, a mismatched
    // ending, and a group truncated after a repeated element.
    for bytes in [
        &[10, 1, b'a', 32, 9, 10, 1, b'b'][..],
        &[43, 10, 1, b'a', 10, 1, b'b', 44][..],
        &[43, 10, 1, b'a', 52][..],
        &[43, 10, 1, b'a'][..],
    ] {
        compare(bytes, None);
    }
    println!(
        "PASS one-byte and multi-byte repeated runs: values, errors, cursor, partial state and groups"
    );
}
