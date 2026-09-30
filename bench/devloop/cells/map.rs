//! PK-23 map workloads shared by the dev-loop cell registry.
//!
//! The lookup and iteration cells operate on the same parsed generated
//! message used by the parse-and-touch cell. Fixture construction happens
//! before the allocator/timing window.

use pbrs::gencode::TestAllTypesProto3;
use pbrs::prelude::*;

pub const CELLS: &[(&str, &str)] = &[
    ("codec.pbrs.map_8_parse_touch", "pbrs"),
    ("codec.pbrs.map_8_get", "pbrs"),
    ("codec.pbrs.map_8_iter", "pbrs"),
    ("codec.pbrs.map_64_parse_touch", "pbrs"),
    ("codec.pbrs.map_64_get", "pbrs"),
    ("codec.pbrs.map_64_iter", "pbrs"),
];

pub fn size_for_cell(cell: &str) -> Option<i32> {
    if cell.starts_with("codec.pbrs.map_8_") {
        Some(8)
    } else if cell.starts_with("codec.pbrs.map_64_") {
        Some(64)
    } else {
        None
    }
}

pub fn specimen(size: i32) -> TestAllTypesProto3 {
    let mut message = TestAllTypesProto3::new();
    for key in 0..size {
        message.map_int32_int32_mut().insert(key, key * key);
    }
    message
}

pub fn touch(message: &TestAllTypesProto3) -> u64 {
    message
        .map_int32_int32()
        .iter()
        .fold(0u64, |sum, (key, value)| {
            sum.wrapping_add(key as u64).wrapping_add(value as u64)
        })
}

pub fn work(cell: &str, wire: &[u8], message: &TestAllTypesProto3) -> Option<u64> {
    let size = size_for_cell(cell)?;
    let value = if cell.ends_with("_parse_touch") {
        touch(&TestAllTypesProto3::parse(wire).expect("map decode"))
    } else if cell.ends_with("_get") {
        message
            .map_int32_int32()
            .get(size - 1)
            .expect("last map key") as u64
    } else if cell.ends_with("_iter") {
        touch(message)
    } else {
        return None;
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_exercise_every_entry_and_last_key() {
        for size in [8, 64] {
            let message = specimen(size);
            assert_eq!(message.map_int32_int32().len(), size as usize);
            assert_eq!(
                message.map_int32_int32().get(size - 1),
                Some((size - 1) * (size - 1))
            );
            let wire = message.serialize().unwrap();
            assert_eq!(
                touch(&message),
                touch(&TestAllTypesProto3::parse(&wire).unwrap())
            );
        }
    }
}
