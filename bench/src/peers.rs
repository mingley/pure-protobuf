//! SB-08 peer codecs: version table, specimen converters, touch checksums.
//!
//! Converters build each peer's specimen from the pbrs reference message.
//! Touch functions implement one shared checksum formula per decoder so that
//! equal checksums prove equivalent observable content (BM-03 semantics).
//! The timed harness in [`crate`] runs byte-equality, cross-parse, and
//! touch-checksum equivalence pre-checks over every peer before timing.

use crate::peer_gen::buffa092_person::example::Person as B092Person;
use crate::peer_gen::buffa092_person::example::PersonLazyView as B092PersonLazy;
use crate::peer_gen::buffa092_person::example::PersonView as B092PersonView;
use crate::peer_gen::buffa092_tat::protobuf_test_messages::proto3::TestAllTypesProto3 as B092Tat;
use crate::peer_gen::buffa092_tat::protobuf_test_messages::proto3::TestAllTypesProto3LazyView as B092TatLazy;
use crate::peer_gen::buffa092_tat::protobuf_test_messages::proto3::TestAllTypesProto3View as B092TatView;
use crate::peer_gen::buffa092_tat::protobuf_test_messages::proto3::__buffa::oneof::test_all_types_proto3::OneofField as B092Oneof;
use crate::peer_gen::buffa092_tat::protobuf_test_messages::proto3::__buffa::view::oneof::test_all_types_proto3::OneofField as B092OneofView;
use crate::peer_gen::buffa092_tat::protobuf_test_messages::proto3::test_all_types_proto3::NestedMessage as B092Nested;
use crate::peer_gen::gpb36_person::Person as G36Person;
use crate::peer_gen::gpb36_tat::TestAllTypesProto3 as G36Tat;
use crate::peer_gen::gpb36_tat::TestAllTypesProto3View as G36TatView;
use crate::peer_gen::gpb36_tat::test_all_types_proto3::NestedEnum as G36NestedEnum;
use crate::peer_gen::prost13_person::Person as P13Person;
use crate::peer_gen::prost14_person::Person as P14Person;
use crate::peer_gen::prost14_tat::TestAllTypesProto3 as P14Tat;
use crate::peer_gen::prost14_tat::test_all_types_proto3::NestedMessage as P14Nested;
use crate::peer_gen::prost14_tat::test_all_types_proto3::OneofField as P14Oneof;
use crate::peer_gen::qp_person::Person as QpPerson;
use crate::person_generated;
use pbrs::gencode::TestAllTypesProto3;
use pbrs::prelude::*;
use pbrs::testdata::Person;

// ---------------------------------------------------------------------------
// Peer versions and lockfile hashes (accept criterion SB-08/2).
// ---------------------------------------------------------------------------

/// The exact lockfile this binary was built from, embedded at compile time
/// so the reported peer versions and checksums cannot drift from the build.
pub(crate) const CARGO_LOCK: &str = include_str!("../Cargo.lock");

/// One scoreboard peer: the JSON `id`, the crates.io package, and the exact
/// pinned version from `bench/Cargo.toml`.
pub(crate) struct PeerPin {
    pub id: &'static str,
    pub package: &'static str,
    pub version: &'static str,
    pub role: &'static str,
    pub note: &'static str,
}

/// Peers linked into this binary (`current`, checksums read from the embedded
/// lockfile) plus the retired SB-08 peers (`historical`, checksums copied
/// from the pre-SB-08 lockfile at 785c5e35 and labeled as such).
pub(crate) const PEER_PINS: &[PeerPin] = &[
    PeerPin {
        id: "prost13",
        package: "prost",
        version: "0.13.5",
        role: "current",
        note: "historical prost column via the prost_tat path crate",
    },
    PeerPin {
        id: "prost14",
        package: "prost",
        version: "0.14.4",
        role: "current",
        note: "refreshed prost column, checked-in gencode",
    },
    PeerPin {
        id: "buffa092",
        package: "buffa",
        version: "0.9.2",
        role: "current",
        note: "owned + eager view + lazy view columns, checked-in gencode",
    },
    PeerPin {
        id: "g36",
        package: "google-protobuf",
        version: "0.36.2-release",
        role: "current",
        note: "Google runtime, upb kernel, checked-in gencode",
    },
    PeerPin {
        id: "qp",
        package: "quick-protobuf",
        version: "0.8.1",
        role: "current",
        note: "person-shaped cells only; pb-rs rejects the TAT schema",
    },
    PeerPin {
        id: "v4",
        package: "protobuf",
        version: "4.35.1-release",
        role: "historical",
        note: "kept in tonic-bench and devloop codec.v4 cells; cannot link beside google-protobuf (links=upb)",
    },
    PeerPin {
        id: "buffa091",
        package: "buffa",
        version: "0.9.1",
        role: "historical",
        note: "retired by SB-08; same-track unification forbids 0.9.1 beside 0.9.2",
    },
];

/// Checksums of the retired peers, copied verbatim from the pre-SB-08
/// `bench/Cargo.lock` (base 785c5e35). Current-peer checksums always come
/// from the embedded lockfile instead; see [`lock_checksum`].
pub(crate) const HISTORICAL_CHECKSUMS: &[(&str, &str)] = &[
    (
        "4.35.1-release",
        "a169648cc34d6f327fea8919ca63f38261fb26405fde8879745dc0a483db328e",
    ),
    (
        "0.9.1",
        "cf9e6224bc4ee1f189ad257120c156fb05f95b826f5369d620b24984476c200a",
    ),
];

/// Parse one `(version, checksum)` pair out of the embedded lockfile.
/// Returns `None` for path dependencies (no checksum) and absent packages.
pub(crate) fn lock_entry(package: &str, version: &str) -> Option<(String, Option<String>)> {
    let mut name = "";
    let mut vers = "";
    let mut sum: Option<String> = None;
    let mut in_pkg = false;
    let flush = |name: &str, vers: &str, sum: &Option<String>| {
        (name == package && vers == version).then(|| (vers.to_owned(), sum.clone()))
    };
    for line in CARGO_LOCK.lines() {
        if line == "[[package]]" {
            if in_pkg && let Some(hit) = flush(name, vers, &sum) {
                return Some(hit);
            }
            in_pkg = true;
            name = "";
            vers = "";
            sum = None;
        } else if in_pkg {
            if let Some(v) = line.strip_prefix("name = ") {
                name = v.trim_matches('"');
            } else if let Some(v) = line.strip_prefix("version = ") {
                vers = v.trim_matches('"');
            } else if let Some(v) = line.strip_prefix("checksum = ") {
                sum = Some(v.trim_matches('"').to_owned());
            }
        }
    }
    if in_pkg {
        flush(name, vers, &sum)
    } else {
        None
    }
}

/// Short helper for the JSON `peers` section: `(version, checksum-or-null)`.
pub(crate) fn lock_checksum(package: &str, version: &str) -> Option<String> {
    lock_entry(package, version).and_then(|(_, sum)| sum)
}

// ---------------------------------------------------------------------------
// Person fixture: one plain-data specimen feeding every peer builder.
// ---------------------------------------------------------------------------

/// Plain-data Person specimen. Both pbrs layouts (handwritten testdata and
/// compiler-generated) convert into this, and every peer builder consumes
/// it, so the handwritten/generated person cases share all peer code.
pub(crate) struct PersonFixture {
    pub id: i32,
    pub name: String,
    pub email: Option<String>,
    pub tags: Vec<String>,
    pub scores: Vec<(String, i32)>,
    pub city: String,
    pub extras: Vec<(String, i32)>,
}

impl PersonFixture {
    pub fn from_handwritten(m: &Person) -> Self {
        PersonFixture {
            id: m.id(),
            name: m.name().to_str().unwrap_or("").to_string(),
            email: m.email_opt().map(|s| s.to_str().unwrap_or("").to_string()),
            tags: m
                .tags()
                .iter()
                .map(|s| s.as_view().to_str().unwrap_or("").to_string())
                .collect(),
            scores: m
                .scores()
                .iter()
                .map(|(k, v)| (k.as_view().to_str().unwrap_or("").to_string(), v))
                .collect(),
            city: m.address().city().to_str().unwrap_or("").to_string(),
            // The handwritten layout predates field 16; its wire bytes never
            // carry extras, so peers built from it agree on empty extras.
            extras: Vec::new(),
        }
    }

    pub fn from_generated(m: &person_generated::Person) -> Self {
        PersonFixture {
            id: m.id(),
            name: m.name().to_str().unwrap_or("").to_string(),
            email: m.email_opt().map(|s| s.to_str().unwrap_or("").to_string()),
            tags: m
                .tags()
                .iter()
                .map(|s| s.as_view().to_str().unwrap_or("").to_string())
                .collect(),
            scores: m
                .scores()
                .iter()
                .map(|(k, v)| (k.as_view().to_str().unwrap_or("").to_string(), v))
                .collect(),
            city: m.address().city().to_str().unwrap_or("").to_string(),
            extras: m
                .extras()
                .iter()
                .map(|(k, v)| (k.as_view().to_str().unwrap_or("").to_string(), v))
                .collect(),
        }
    }
}

pub(crate) fn prost13_person_of(f: &PersonFixture) -> P13Person {
    P13Person {
        id: f.id,
        name: f.name.clone(),
        email: f.email.clone(),
        tags: f.tags.clone(),
        scores: f.scores.iter().cloned().collect(),
        address: Some(crate::peer_gen::prost13_person::Address {
            city: f.city.clone(),
        }),
        extras: f.extras.iter().cloned().collect(),
    }
}

pub(crate) fn prost14_person_of(f: &PersonFixture) -> P14Person {
    P14Person {
        id: f.id,
        name: f.name.clone(),
        email: f.email.clone(),
        tags: f.tags.clone(),
        scores: f.scores.iter().cloned().collect(),
        address: Some(crate::peer_gen::prost14_person::Address {
            city: f.city.clone(),
        }),
        extras: f.extras.iter().cloned().collect(),
    }
}

pub(crate) fn g36_person_of(f: &PersonFixture) -> G36Person {
    let mut v = G36Person::new();
    v.set_id(f.id);
    v.set_name(f.name.as_str());
    if let Some(e) = &f.email {
        v.set_email(e.as_str());
    }
    for t in &f.tags {
        v.tags_mut().push(t.as_str());
    }
    for (k, val) in &f.scores {
        v.scores_mut().insert(k.as_str(), *val);
    }
    v.address_mut().set_city(f.city.as_str());
    for (k, val) in &f.extras {
        v.extras_mut().insert(k.as_str(), *val);
    }
    v
}

pub(crate) fn buffa092_person_of(f: &PersonFixture) -> B092Person {
    B092Person {
        id: f.id,
        name: f.name.clone(),
        email: f.email.clone(),
        tags: f.tags.clone(),
        scores: f.scores.iter().cloned().collect(),
        address: Some(crate::peer_gen::buffa092_person::example::Address {
            city: f.city.clone(),
            ..Default::default()
        })
        .into(),
        extras: f.extras.iter().cloned().collect(),
        ..Default::default()
    }
}

/// Bare (undelimited) quick-protobuf encode. `serialize_into_vec` writes a
/// length prefix, so scoreboard bytes go through the `MessageWrite` trait
/// method directly.
pub(crate) fn qp_encode<M: quick_protobuf::MessageWrite>(m: &M) -> Vec<u8> {
    let mut buf = Vec::with_capacity(m.get_size());
    {
        let mut w = quick_protobuf::Writer::new(&mut buf);
        quick_protobuf::MessageWrite::write_message(m, &mut w).expect("qp encode");
    }
    buf
}

/// Bare (undelimited) quick-protobuf parse. `deserialize_from_slice` reads a
/// length prefix first, so scoreboard parses call `from_reader` directly and
/// borrow from the input, exactly like a view decode.
pub(crate) fn qp_decode(bytes: &[u8]) -> quick_protobuf::Result<QpPerson<'_>> {
    use quick_protobuf::MessageRead as _;
    let mut reader = quick_protobuf::BytesReader::from_bytes(bytes);
    QpPerson::from_reader(&mut reader, bytes)
}

/// quick-protobuf has no owned parses (it always borrows), but an owned
/// specimen builds fine from `Cow::Owned` with a `'static` lifetime.
pub(crate) fn qp_person_of(f: &PersonFixture) -> QpPerson<'static> {
    use std::borrow::Cow;
    QpPerson {
        id: f.id,
        name: Cow::Owned(f.name.clone()),
        // Fidelity caveat (see SB08_PROVENANCE.md): quick-protobuf 0.8.1
        // drops proto3-optional presence, so `email` is a plain string.
        // Absent and explicitly-empty both encode to nothing and both touch
        // as zero, so the fixed fixtures stay exactly equivalent.
        email: Cow::Owned(f.email.clone().unwrap_or_default()),
        tags: f.tags.iter().map(|t| Cow::Owned(t.clone())).collect(),
        scores: f
            .scores
            .iter()
            .map(|(k, v)| (Cow::Owned(k.clone()), *v))
            .collect(),
        address: Some(crate::peer_gen::qp_person::Address {
            city: Cow::Owned(f.city.clone()),
        }),
        extras: f
            .extras
            .iter()
            .map(|(k, v)| (Cow::Owned(k.clone()), *v))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// TAT converters: pbrs reference -> each peer's specimen.
// ---------------------------------------------------------------------------

pub(crate) fn prost_of(m: &TestAllTypesProto3) -> prost_tat::TestAllTypesProto3 {
    let nested = m.optional_nested_message_opt().map(|n| {
        Box::new(prost_tat::test_all_types_proto3::NestedMessage {
            a: n.a(),
            ..Default::default()
        })
    });
    let rec = m.recursive_message_opt().map(|r| Box::new(prost_of(r)));
    prost_tat::TestAllTypesProto3 {
        optional_int32: m.optional_int32(),
        optional_int64: m.optional_int64(),
        optional_uint32: m.optional_uint32(),
        optional_string: m.optional_string().to_str().unwrap_or("").to_string(),
        optional_bytes: m.optional_bytes().to_vec(),
        optional_nested_message: nested,
        optional_string_piece: m.optional_string_piece().to_str().unwrap_or("").to_string(),
        optional_cord: m.optional_cord().to_str().unwrap_or("").to_string(),
        recursive_message: rec,
        repeated_int32: m.repeated_int32().iter().collect(),
        map_int32_int32: m.map_int32_int32().iter().collect(),
        packed_int32: m.packed_int32().iter().collect(),
        repeated_string: m
            .repeated_string()
            .iter()
            .map(|s| s.as_view().to_str().unwrap_or("").to_string())
            .collect(),
        optional_bool: m.optional_bool(),
        optional_float: m.optional_float(),
        optional_nested_enum: i32::from(m.optional_nested_enum()),
        repeated_bytes: m
            .repeated_bytes()
            .iter()
            .map(|b| b.as_bytes().to_vec())
            .collect(),
        packed_fixed32: m.packed_fixed32().iter().collect(),
        packed_fixed64: m.packed_fixed64().iter().collect(),
        packed_float: m.packed_float().iter().collect(),
        packed_bool: m.packed_bool().iter().collect(),
        unpacked_int32: m.unpacked_int32().iter().collect(),
        unpacked_fixed32: m.unpacked_fixed32().iter().collect(),
        repeated_nested_message: m
            .repeated_nested_message()
            .iter()
            .map(|n| prost_tat::test_all_types_proto3::NestedMessage {
                a: n.a(),
                ..Default::default()
            })
            .collect(),
        oneof_field: m
            .oneof_uint32_opt()
            .map(prost_tat::test_all_types_proto3::OneofField::OneofUint32)
            .or_else(|| {
                m.oneof_string_opt().map(|s| {
                    prost_tat::test_all_types_proto3::OneofField::OneofString(
                        s.to_str().unwrap_or("").to_string(),
                    )
                })
            }),
        ..Default::default()
    }
}

pub(crate) fn prost14_of(m: &TestAllTypesProto3) -> P14Tat {
    let nested = m.optional_nested_message_opt().map(|n| {
        Box::new(P14Nested {
            a: n.a(),
            ..Default::default()
        })
    });
    let rec = m.recursive_message_opt().map(|r| Box::new(prost14_of(r)));
    P14Tat {
        optional_int32: m.optional_int32(),
        optional_int64: m.optional_int64(),
        optional_uint32: m.optional_uint32(),
        optional_string: m.optional_string().to_str().unwrap_or("").to_string(),
        optional_bytes: m.optional_bytes().to_vec(),
        optional_nested_message: nested,
        optional_string_piece: m.optional_string_piece().to_str().unwrap_or("").to_string(),
        optional_cord: m.optional_cord().to_str().unwrap_or("").to_string(),
        recursive_message: rec,
        repeated_int32: m.repeated_int32().iter().collect(),
        map_int32_int32: m.map_int32_int32().iter().collect(),
        packed_int32: m.packed_int32().iter().collect(),
        repeated_string: m
            .repeated_string()
            .iter()
            .map(|s| s.as_view().to_str().unwrap_or("").to_string())
            .collect(),
        optional_bool: m.optional_bool(),
        optional_float: m.optional_float(),
        optional_nested_enum: i32::from(m.optional_nested_enum()),
        repeated_bytes: m
            .repeated_bytes()
            .iter()
            .map(|b| b.as_bytes().to_vec())
            .collect(),
        packed_fixed32: m.packed_fixed32().iter().collect(),
        packed_fixed64: m.packed_fixed64().iter().collect(),
        packed_float: m.packed_float().iter().collect(),
        packed_bool: m.packed_bool().iter().collect(),
        unpacked_int32: m.unpacked_int32().iter().collect(),
        unpacked_fixed32: m.unpacked_fixed32().iter().collect(),
        repeated_nested_message: m
            .repeated_nested_message()
            .iter()
            .map(|n| P14Nested {
                a: n.a(),
                ..Default::default()
            })
            .collect(),
        oneof_field: m.oneof_uint32_opt().map(P14Oneof::OneofUint32).or_else(|| {
            m.oneof_string_opt()
                .map(|s| P14Oneof::OneofString(s.to_str().unwrap_or("").to_string()))
        }),
        ..Default::default()
    }
}

pub(crate) fn g36_of(m: &TestAllTypesProto3) -> G36Tat {
    let mut v = G36Tat::new();
    v.set_optional_int32(m.optional_int32());
    v.set_optional_int64(m.optional_int64());
    v.set_optional_uint32(m.optional_uint32());
    v.set_optional_string(m.optional_string().to_str().unwrap_or(""));
    v.set_optional_bytes(m.optional_bytes());
    if let Some(n) = m.optional_nested_message_opt() {
        v.optional_nested_message_mut().set_a(n.a());
    }
    if let Some(s) = m
        .optional_string_piece()
        .to_str()
        .ok()
        .filter(|s| !s.is_empty())
    {
        v.set_optional_string_piece(s);
    }
    if let Some(s) = m.optional_cord().to_str().ok().filter(|s| !s.is_empty()) {
        v.set_optional_cord(s);
    }
    if let Some(r) = m.recursive_message_opt() {
        v.set_recursive_message(g36_of(r));
    }
    for i in m.repeated_int32().iter() {
        v.repeated_int32_mut().push(i);
    }
    for (k, val) in m.map_int32_int32().iter() {
        v.map_int32_int32_mut().insert(k, val);
    }
    for i in m.packed_int32().iter() {
        v.packed_int32_mut().push(i);
    }
    for s in m.repeated_string().iter() {
        v.repeated_string_mut()
            .push(s.as_view().to_str().unwrap_or(""));
    }
    v.set_optional_bool(m.optional_bool());
    v.set_optional_float(m.optional_float());
    v.set_optional_nested_enum(G36NestedEnum::from(i32::from(m.optional_nested_enum())));
    for b in m.repeated_bytes().iter() {
        v.repeated_bytes_mut().push(b.as_bytes());
    }
    for i in m.packed_fixed32().iter() {
        v.packed_fixed32_mut().push(i);
    }
    for i in m.packed_fixed64().iter() {
        v.packed_fixed64_mut().push(i);
    }
    for i in m.packed_float().iter() {
        v.packed_float_mut().push(i);
    }
    for i in m.packed_bool().iter() {
        v.packed_bool_mut().push(i);
    }
    for i in m.unpacked_int32().iter() {
        v.unpacked_int32_mut().push(i);
    }
    for i in m.unpacked_fixed32().iter() {
        v.unpacked_fixed32_mut().push(i);
    }
    for n in m.repeated_nested_message().iter() {
        let mut inner = crate::peer_gen::gpb36_tat::test_all_types_proto3::NestedMessage::new();
        inner.set_a(n.a());
        v.repeated_nested_message_mut().push(inner);
    }
    if let Some(x) = m.oneof_uint32_opt() {
        v.set_oneof_uint32(x);
    } else if let Some(s) = m.oneof_string_opt() {
        v.set_oneof_string(s.to_str().unwrap_or(""));
    }
    v
}

pub(crate) fn buffa092_of(m: &TestAllTypesProto3) -> B092Tat {
    let nested = m.optional_nested_message_opt().map(|n| B092Nested {
        a: n.a(),
        ..Default::default()
    });
    B092Tat {
        optional_int32: m.optional_int32(),
        optional_int64: m.optional_int64(),
        optional_uint32: m.optional_uint32(),
        optional_string: m.optional_string().to_str().unwrap_or("").to_string(),
        optional_bytes: m.optional_bytes().to_vec(),
        optional_nested_message: nested.into(),
        optional_string_piece: m.optional_string_piece().to_str().unwrap_or("").to_string(),
        optional_cord: m.optional_cord().to_str().unwrap_or("").to_string(),
        recursive_message: m.recursive_message_opt().map(buffa092_of).into(),
        repeated_int32: m.repeated_int32().iter().collect(),
        map_int32_int32: m.map_int32_int32().iter().collect(),
        packed_int32: m.packed_int32().iter().collect(),
        repeated_string: m
            .repeated_string()
            .iter()
            .map(|s| s.as_view().to_str().unwrap_or("").to_string())
            .collect(),
        optional_bool: m.optional_bool(),
        optional_float: m.optional_float(),
        // 0.9.2 wraps enums in `EnumValue`, which implements `From<i32>`.
        optional_nested_enum: i32::from(m.optional_nested_enum()).into(),
        repeated_bytes: m
            .repeated_bytes()
            .iter()
            .map(|b| b.as_bytes().to_vec())
            .collect(),
        packed_fixed32: m.packed_fixed32().iter().collect(),
        packed_fixed64: m.packed_fixed64().iter().collect(),
        packed_float: m.packed_float().iter().collect(),
        packed_bool: m.packed_bool().iter().collect(),
        unpacked_int32: m.unpacked_int32().iter().collect(),
        unpacked_fixed32: m.unpacked_fixed32().iter().collect(),
        repeated_nested_message: m
            .repeated_nested_message()
            .iter()
            .map(|n| B092Nested {
                a: n.a(),
                ..Default::default()
            })
            .collect(),
        oneof_field: m
            .oneof_uint32_opt()
            .map(B092Oneof::OneofUint32)
            .or_else(|| {
                m.oneof_string_opt()
                    .map(|s| B092Oneof::OneofString(s.to_str().unwrap_or("").to_string()))
            }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Touch checksums: one shared formula per decoder; equal checksums prove
// equivalent observable content, and timing the walk exposes deferred
// materialization (lazy strings, wire-backed views) that parse-and-drop
// hides. Every timed closure returns its output through `black_box`.
// ---------------------------------------------------------------------------

pub(crate) fn touch_ours_tat(m: &TestAllTypesProto3) -> u64 {
    let mut acc = m.optional_int32() as u64;
    acc = acc.wrapping_add(m.optional_int64() as u64);
    acc = acc.wrapping_add(m.optional_uint32() as u64);
    acc = acc.wrapping_add(m.optional_string().as_bytes().len() as u64);
    acc = acc.wrapping_add(m.optional_bytes().len() as u64);
    if let Some(n) = m.optional_nested_message_opt() {
        acc = acc.wrapping_add(n.a() as u64);
    }
    acc = acc.wrapping_add(m.optional_string_piece().as_bytes().len() as u64);
    acc = acc.wrapping_add(m.optional_cord().as_bytes().len() as u64);
    if let Some(r) = m.recursive_message_opt() {
        acc = acc.wrapping_add(touch_ours_tat(r));
    }
    for i in m.repeated_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for (k, v) in m.map_int32_int32().iter() {
        acc = acc.wrapping_add(k as u64).wrapping_add(v as u64);
    }
    for i in m.packed_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for s in m.repeated_string().iter() {
        acc = acc.wrapping_add(s.as_view().as_bytes().len() as u64);
    }
    acc = acc.wrapping_add(m.optional_bool() as u64);
    acc = acc.wrapping_add(m.optional_float() as u64);
    acc = acc.wrapping_add(i32::from(m.optional_nested_enum()) as u64);
    for b in m.repeated_bytes().iter() {
        acc = acc.wrapping_add(b.as_bytes().len() as u64);
    }
    for i in m.packed_fixed32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for i in m.packed_fixed64().iter() {
        acc = acc.wrapping_add(i);
    }
    for f in m.packed_float().iter() {
        acc = acc.wrapping_add(f as u64);
    }
    for b in m.packed_bool().iter() {
        acc = acc.wrapping_add(b as u64);
    }
    for i in m.unpacked_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for i in m.unpacked_fixed32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for n in m.repeated_nested_message().iter() {
        acc = acc.wrapping_add(n.a() as u64);
    }
    if let Some(x) = m.oneof_uint32_opt() {
        acc = acc.wrapping_add(x as u64);
    }
    if let Some(s) = m.oneof_string_opt() {
        acc = acc.wrapping_add(s.as_bytes().len() as u64);
    }
    acc
}

pub(crate) fn touch_prost_tat(m: &prost_tat::TestAllTypesProto3) -> u64 {
    let mut acc = m.optional_int32 as u64;
    acc = acc.wrapping_add(m.optional_int64 as u64);
    acc = acc.wrapping_add(m.optional_uint32 as u64);
    acc = acc.wrapping_add(m.optional_string.len() as u64);
    acc = acc.wrapping_add(m.optional_bytes.len() as u64);
    if let Some(n) = &m.optional_nested_message {
        acc = acc.wrapping_add(n.a as u64);
    }
    acc = acc.wrapping_add(m.optional_string_piece.len() as u64);
    acc = acc.wrapping_add(m.optional_cord.len() as u64);
    if let Some(r) = m.recursive_message.as_deref() {
        acc = acc.wrapping_add(touch_prost_tat(r));
    }
    for i in &m.repeated_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for (k, v) in &m.map_int32_int32 {
        acc = acc.wrapping_add(*k as u64).wrapping_add(*v as u64);
    }
    for i in &m.packed_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for s in &m.repeated_string {
        acc = acc.wrapping_add(s.len() as u64);
    }
    acc = acc.wrapping_add(m.optional_bool as u64);
    acc = acc.wrapping_add(m.optional_float as u64);
    acc = acc.wrapping_add(m.optional_nested_enum as u64);
    for b in &m.repeated_bytes {
        acc = acc.wrapping_add(b.len() as u64);
    }
    for i in &m.packed_fixed32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in &m.packed_fixed64 {
        acc = acc.wrapping_add(*i);
    }
    for f in &m.packed_float {
        acc = acc.wrapping_add(*f as u64);
    }
    for b in &m.packed_bool {
        acc = acc.wrapping_add(*b as u64);
    }
    for i in &m.unpacked_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in &m.unpacked_fixed32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for n in &m.repeated_nested_message {
        acc = acc.wrapping_add(n.a as u64);
    }
    match &m.oneof_field {
        None => {}
        Some(prost_tat::test_all_types_proto3::OneofField::OneofUint32(x)) => {
            acc = acc.wrapping_add(*x as u64);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofNestedMessage(n)) => {
            acc = acc.wrapping_add(n.a as u64);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofString(s)) => {
            acc = acc.wrapping_add(s.len() as u64);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofBytes(b)) => {
            acc = acc.wrapping_add(b.len() as u64);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofBool(b)) => {
            acc = acc.wrapping_add(*b as u64);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofUint64(x)) => {
            acc = acc.wrapping_add(*x);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofFloat(f)) => {
            acc = acc.wrapping_add(*f as u64);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofDouble(f)) => {
            acc = acc.wrapping_add(*f as u64);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofEnum(x)) => {
            acc = acc.wrapping_add(*x as u64);
        }
        Some(prost_tat::test_all_types_proto3::OneofField::OneofNullValue(x)) => {
            acc = acc.wrapping_add(*x as u64);
        }
    }
    acc
}

pub(crate) fn touch_prost14_tat(m: &P14Tat) -> u64 {
    let mut acc = m.optional_int32 as u64;
    acc = acc.wrapping_add(m.optional_int64 as u64);
    acc = acc.wrapping_add(m.optional_uint32 as u64);
    acc = acc.wrapping_add(m.optional_string.len() as u64);
    acc = acc.wrapping_add(m.optional_bytes.len() as u64);
    if let Some(n) = &m.optional_nested_message {
        acc = acc.wrapping_add(n.a as u64);
    }
    acc = acc.wrapping_add(m.optional_string_piece.len() as u64);
    acc = acc.wrapping_add(m.optional_cord.len() as u64);
    if let Some(r) = m.recursive_message.as_deref() {
        acc = acc.wrapping_add(touch_prost14_tat(r));
    }
    for i in &m.repeated_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for (k, v) in &m.map_int32_int32 {
        acc = acc.wrapping_add(*k as u64).wrapping_add(*v as u64);
    }
    for i in &m.packed_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for s in &m.repeated_string {
        acc = acc.wrapping_add(s.len() as u64);
    }
    acc = acc.wrapping_add(m.optional_bool as u64);
    acc = acc.wrapping_add(m.optional_float as u64);
    acc = acc.wrapping_add(m.optional_nested_enum as u64);
    for b in &m.repeated_bytes {
        acc = acc.wrapping_add(b.len() as u64);
    }
    for i in &m.packed_fixed32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in &m.packed_fixed64 {
        acc = acc.wrapping_add(*i);
    }
    for f in &m.packed_float {
        acc = acc.wrapping_add(*f as u64);
    }
    for b in &m.packed_bool {
        acc = acc.wrapping_add(*b as u64);
    }
    for i in &m.unpacked_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in &m.unpacked_fixed32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for n in &m.repeated_nested_message {
        acc = acc.wrapping_add(n.a as u64);
    }
    match &m.oneof_field {
        None => {}
        Some(P14Oneof::OneofUint32(x)) => {
            acc = acc.wrapping_add(*x as u64);
        }
        Some(P14Oneof::OneofNestedMessage(n)) => {
            acc = acc.wrapping_add(n.a as u64);
        }
        Some(P14Oneof::OneofString(s)) => {
            acc = acc.wrapping_add(s.len() as u64);
        }
        Some(P14Oneof::OneofBytes(b)) => {
            acc = acc.wrapping_add(b.len() as u64);
        }
        Some(P14Oneof::OneofBool(b)) => {
            acc = acc.wrapping_add(*b as u64);
        }
        Some(P14Oneof::OneofUint64(x)) => {
            acc = acc.wrapping_add(*x);
        }
        Some(P14Oneof::OneofFloat(f)) => {
            acc = acc.wrapping_add(*f as u64);
        }
        Some(P14Oneof::OneofDouble(f)) => {
            acc = acc.wrapping_add(*f as u64);
        }
        Some(P14Oneof::OneofEnum(x)) => {
            acc = acc.wrapping_add(*x as u64);
        }
        Some(P14Oneof::OneofNullValue(x)) => {
            acc = acc.wrapping_add(*x as u64);
        }
    }
    acc
}

pub(crate) fn touch_g36_tat(m: &G36Tat) -> u64 {
    touch_g36_tat_view(m.as_view())
}

pub(crate) fn touch_g36_tat_view(m: G36TatView<'_>) -> u64 {
    let mut acc = m.optional_int32() as u64;
    acc = acc.wrapping_add(m.optional_int64() as u64);
    acc = acc.wrapping_add(m.optional_uint32() as u64);
    acc = acc.wrapping_add(m.optional_string().len() as u64);
    acc = acc.wrapping_add(m.optional_bytes().len() as u64);
    if m.has_optional_nested_message() {
        acc = acc.wrapping_add(m.optional_nested_message().a() as u64);
    }
    acc = acc.wrapping_add(m.optional_string_piece().len() as u64);
    acc = acc.wrapping_add(m.optional_cord().len() as u64);
    if m.has_recursive_message() {
        acc = acc.wrapping_add(touch_g36_tat_view(m.recursive_message()));
    }
    for i in m.repeated_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for (k, v) in m.map_int32_int32().iter() {
        acc = acc.wrapping_add(k as u64).wrapping_add(v as u64);
    }
    for i in m.packed_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for s in m.repeated_string().iter() {
        acc = acc.wrapping_add(s.len() as u64);
    }
    acc = acc.wrapping_add(m.optional_bool() as u64);
    acc = acc.wrapping_add(m.optional_float() as u64);
    acc = acc.wrapping_add(i32::from(m.optional_nested_enum()) as u64);
    for b in m.repeated_bytes().iter() {
        acc = acc.wrapping_add(b.len() as u64);
    }
    for i in m.packed_fixed32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for i in m.packed_fixed64().iter() {
        acc = acc.wrapping_add(i);
    }
    for f in m.packed_float().iter() {
        acc = acc.wrapping_add(f as u64);
    }
    for b in m.packed_bool().iter() {
        acc = acc.wrapping_add(b as u64);
    }
    for i in m.unpacked_int32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for i in m.unpacked_fixed32().iter() {
        acc = acc.wrapping_add(i as u64);
    }
    for n in m.repeated_nested_message().iter() {
        acc = acc.wrapping_add(n.a() as u64);
    }
    if m.has_oneof_uint32() {
        acc = acc.wrapping_add(m.oneof_uint32() as u64);
    }
    if m.has_oneof_string() {
        acc = acc.wrapping_add(m.oneof_string().len() as u64);
    }
    acc
}

pub(crate) fn touch_b092_tat(m: &B092Tat) -> u64 {
    let mut acc = m.optional_int32 as u64;
    acc = acc.wrapping_add(m.optional_int64 as u64);
    acc = acc.wrapping_add(m.optional_uint32 as u64);
    acc = acc.wrapping_add(m.optional_string.len() as u64);
    acc = acc.wrapping_add(m.optional_bytes.len() as u64);
    if let Some(n) = m.optional_nested_message.as_option() {
        acc = acc.wrapping_add(n.a as u64);
    }
    acc = acc.wrapping_add(m.optional_string_piece.len() as u64);
    acc = acc.wrapping_add(m.optional_cord.len() as u64);
    if let Some(r) = m.recursive_message.as_option() {
        acc = acc.wrapping_add(touch_b092_tat(r));
    }
    for i in &m.repeated_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for (k, v) in &m.map_int32_int32 {
        acc = acc.wrapping_add(*k as u64).wrapping_add(*v as u64);
    }
    for i in &m.packed_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for s in &m.repeated_string {
        acc = acc.wrapping_add(s.len() as u64);
    }
    acc = acc.wrapping_add(m.optional_bool as u64);
    acc = acc.wrapping_add(m.optional_float as u64);
    acc = acc.wrapping_add(m.optional_nested_enum.to_i32() as u64);
    for b in &m.repeated_bytes {
        acc = acc.wrapping_add(b.len() as u64);
    }
    for i in &m.packed_fixed32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in &m.packed_fixed64 {
        acc = acc.wrapping_add(*i);
    }
    for f in &m.packed_float {
        acc = acc.wrapping_add(*f as u64);
    }
    for b in &m.packed_bool {
        acc = acc.wrapping_add(*b as u64);
    }
    for i in &m.unpacked_int32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in &m.unpacked_fixed32 {
        acc = acc.wrapping_add(*i as u64);
    }
    for n in &m.repeated_nested_message {
        acc = acc.wrapping_add(n.a as u64);
    }
    match &m.oneof_field {
        None => {}
        Some(B092Oneof::OneofUint32(x)) => {
            acc = acc.wrapping_add(*x as u64);
        }
        Some(B092Oneof::OneofNestedMessage(n)) => {
            acc = acc.wrapping_add(n.a as u64);
        }
        Some(B092Oneof::OneofString(s)) => {
            acc = acc.wrapping_add(s.len() as u64);
        }
        Some(B092Oneof::OneofBytes(b)) => {
            acc = acc.wrapping_add(b.len() as u64);
        }
        Some(B092Oneof::OneofBool(b)) => {
            acc = acc.wrapping_add(*b as u64);
        }
        Some(B092Oneof::OneofUint64(x)) => {
            acc = acc.wrapping_add(*x);
        }
        Some(B092Oneof::OneofFloat(f)) => {
            acc = acc.wrapping_add(*f as u64);
        }
        Some(B092Oneof::OneofDouble(f)) => {
            acc = acc.wrapping_add(*f as u64);
        }
        Some(B092Oneof::OneofEnum(e)) => {
            acc = acc.wrapping_add(e.to_i32() as u64);
        }
        Some(B092Oneof::OneofNullValue(e)) => {
            acc = acc.wrapping_add(e.to_i32() as u64);
        }
    }
    acc
}

pub(crate) fn touch_b092_tat_view(m: &B092TatView<'_>) -> u64 {
    let mut acc = m.optional_int32 as u64;
    acc = acc.wrapping_add(m.optional_int64 as u64);
    acc = acc.wrapping_add(m.optional_uint32 as u64);
    acc = acc.wrapping_add(m.optional_string.len() as u64);
    acc = acc.wrapping_add(m.optional_bytes.len() as u64);
    if let Some(n) = m.optional_nested_message.as_option() {
        acc = acc.wrapping_add(n.a as u64);
    }
    acc = acc.wrapping_add(m.optional_string_piece.len() as u64);
    acc = acc.wrapping_add(m.optional_cord.len() as u64);
    if let Some(r) = m.recursive_message.as_option() {
        acc = acc.wrapping_add(touch_b092_tat_view(r));
    }
    for i in m.repeated_int32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for &(k, v) in m.map_int32_int32.iter() {
        acc = acc.wrapping_add(k as u64).wrapping_add(v as u64);
    }
    for i in m.packed_int32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for s in m.repeated_string.iter() {
        acc = acc.wrapping_add(s.len() as u64);
    }
    acc = acc.wrapping_add(m.optional_bool as u64);
    acc = acc.wrapping_add(m.optional_float as u64);
    acc = acc.wrapping_add(m.optional_nested_enum.to_i32() as u64);
    for b in m.repeated_bytes.iter() {
        acc = acc.wrapping_add(b.len() as u64);
    }
    for i in m.packed_fixed32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in m.packed_fixed64.iter() {
        acc = acc.wrapping_add(*i);
    }
    for f in m.packed_float.iter() {
        acc = acc.wrapping_add(*f as u64);
    }
    for b in m.packed_bool.iter() {
        acc = acc.wrapping_add(*b as u64);
    }
    for i in m.unpacked_int32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in m.unpacked_fixed32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for n in m.repeated_nested_message.iter() {
        acc = acc.wrapping_add(n.a as u64);
    }
    touch_b092_oneof_view(&m.oneof_field, acc)
}

/// Shared oneof arm: the lazy TAT view reuses the eager oneof view enum.
fn touch_b092_oneof_view(m: &Option<B092OneofView<'_>>, mut acc: u64) -> u64 {
    match m {
        None => {}
        Some(B092OneofView::OneofUint32(x)) => {
            acc = acc.wrapping_add(*x as u64);
        }
        Some(B092OneofView::OneofNestedMessage(n)) => {
            acc = acc.wrapping_add(n.a as u64);
        }
        Some(B092OneofView::OneofString(s)) => {
            acc = acc.wrapping_add(s.len() as u64);
        }
        Some(B092OneofView::OneofBytes(b)) => {
            acc = acc.wrapping_add(b.len() as u64);
        }
        Some(B092OneofView::OneofBool(b)) => {
            acc = acc.wrapping_add(*b as u64);
        }
        Some(B092OneofView::OneofUint64(x)) => {
            acc = acc.wrapping_add(*x);
        }
        Some(B092OneofView::OneofFloat(f)) => {
            acc = acc.wrapping_add(*f as u64);
        }
        Some(B092OneofView::OneofDouble(f)) => {
            acc = acc.wrapping_add(*f as u64);
        }
        Some(B092OneofView::OneofEnum(e)) => {
            acc = acc.wrapping_add(e.to_i32() as u64);
        }
        Some(B092OneofView::OneofNullValue(e)) => {
            acc = acc.wrapping_add(e.to_i32() as u64);
        }
    }
    acc
}

pub(crate) fn touch_b092_tat_lazy(m: &B092TatLazy<'_>) -> u64 {
    let mut acc = m.optional_int32 as u64;
    acc = acc.wrapping_add(m.optional_int64 as u64);
    acc = acc.wrapping_add(m.optional_uint32 as u64);
    acc = acc.wrapping_add(m.optional_string.len() as u64);
    acc = acc.wrapping_add(m.optional_bytes.len() as u64);
    if let Some(n) = m
        .optional_nested_message
        .get()
        .expect("lazy nested message")
    {
        acc = acc.wrapping_add(n.a as u64);
    }
    acc = acc.wrapping_add(m.optional_string_piece.len() as u64);
    acc = acc.wrapping_add(m.optional_cord.len() as u64);
    if let Some(r) = m.recursive_message.get().expect("lazy recursive message") {
        acc = acc.wrapping_add(touch_b092_tat_lazy(&r));
    }
    for i in m.repeated_int32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for &(k, v) in m.map_int32_int32.iter() {
        acc = acc.wrapping_add(k as u64).wrapping_add(v as u64);
    }
    for i in m.packed_int32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for s in m.repeated_string.iter() {
        acc = acc.wrapping_add(s.len() as u64);
    }
    acc = acc.wrapping_add(m.optional_bool as u64);
    acc = acc.wrapping_add(m.optional_float as u64);
    acc = acc.wrapping_add(m.optional_nested_enum.to_i32() as u64);
    for b in m.repeated_bytes.iter() {
        acc = acc.wrapping_add(b.len() as u64);
    }
    for i in m.packed_fixed32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in m.packed_fixed64.iter() {
        acc = acc.wrapping_add(*i);
    }
    for f in m.packed_float.iter() {
        acc = acc.wrapping_add(*f as u64);
    }
    for b in m.packed_bool.iter() {
        acc = acc.wrapping_add(*b as u64);
    }
    for i in m.unpacked_int32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for i in m.unpacked_fixed32.iter() {
        acc = acc.wrapping_add(*i as u64);
    }
    for n in m.repeated_nested_message.iter() {
        let n = n.expect("lazy repeated nested message");
        acc = acc.wrapping_add(n.a as u64);
    }
    touch_b092_oneof_view(&m.oneof_field, acc)
}

pub(crate) fn touch_ours_person(m: &Person) -> u64 {
    let mut acc = m.id() as u64;
    acc = acc.wrapping_add(m.name().as_bytes().len() as u64);
    if let Some(e) = m.email_opt() {
        acc = acc.wrapping_add(e.as_bytes().len() as u64);
    }
    for t in m.tags().iter() {
        acc = acc.wrapping_add(t.as_view().as_bytes().len() as u64);
    }
    for (k, v) in m.scores().iter() {
        acc = acc
            .wrapping_add(k.as_view().as_bytes().len() as u64)
            .wrapping_add(v as u64);
    }
    acc = acc.wrapping_add(m.address().city().as_bytes().len() as u64);
    acc
}

pub(crate) fn touch_generated_person(m: &person_generated::Person) -> u64 {
    let mut acc = m.id() as u64;
    acc = acc.wrapping_add(m.name().as_bytes().len() as u64);
    if let Some(e) = m.email_opt() {
        acc = acc.wrapping_add(e.as_bytes().len() as u64);
    }
    for t in m.tags().iter() {
        acc = acc.wrapping_add(t.as_view().as_bytes().len() as u64);
    }
    for (k, v) in m.scores().iter() {
        acc = acc
            .wrapping_add(k.as_view().as_bytes().len() as u64)
            .wrapping_add(v as u64);
    }
    acc = acc.wrapping_add(m.address().city().as_bytes().len() as u64);
    for (k, v) in m.extras().iter() {
        acc = acc
            .wrapping_add(k.as_view().as_bytes().len() as u64)
            .wrapping_add(v as u64);
    }
    acc
}

pub(crate) fn touch_prost13_person(m: &P13Person) -> u64 {
    let mut acc = m.id as u64;
    acc = acc.wrapping_add(m.name.len() as u64);
    if let Some(e) = &m.email {
        acc = acc.wrapping_add(e.len() as u64);
    }
    for t in &m.tags {
        acc = acc.wrapping_add(t.len() as u64);
    }
    for (k, v) in &m.scores {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    if let Some(a) = &m.address {
        acc = acc.wrapping_add(a.city.len() as u64);
    }
    for (k, v) in &m.extras {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    acc
}

pub(crate) fn touch_prost14_person(m: &P14Person) -> u64 {
    let mut acc = m.id as u64;
    acc = acc.wrapping_add(m.name.len() as u64);
    if let Some(e) = &m.email {
        acc = acc.wrapping_add(e.len() as u64);
    }
    for t in &m.tags {
        acc = acc.wrapping_add(t.len() as u64);
    }
    for (k, v) in &m.scores {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    if let Some(a) = &m.address {
        acc = acc.wrapping_add(a.city.len() as u64);
    }
    for (k, v) in &m.extras {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    acc
}

pub(crate) fn touch_g36_person(m: &G36Person) -> u64 {
    let mut acc = m.id() as u64;
    acc = acc.wrapping_add(m.name().len() as u64);
    if m.has_email() {
        acc = acc.wrapping_add(m.email().len() as u64);
    }
    for t in m.tags().iter() {
        acc = acc.wrapping_add(t.len() as u64);
    }
    for (k, v) in m.scores().iter() {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(v as u64);
    }
    if m.has_address() {
        acc = acc.wrapping_add(m.address().city().len() as u64);
    }
    for (k, v) in m.extras().iter() {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(v as u64);
    }
    acc
}

pub(crate) fn touch_b092_person(m: &B092Person) -> u64 {
    let mut acc = m.id as u64;
    acc = acc.wrapping_add(m.name.len() as u64);
    if let Some(e) = &m.email {
        acc = acc.wrapping_add(e.len() as u64);
    }
    for t in &m.tags {
        acc = acc.wrapping_add(t.len() as u64);
    }
    for (k, v) in &m.scores {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    if let Some(a) = m.address.as_option() {
        acc = acc.wrapping_add(a.city.len() as u64);
    }
    for (k, v) in &m.extras {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    acc
}

pub(crate) fn touch_b092_person_view(m: &B092PersonView<'_>) -> u64 {
    let mut acc = m.id as u64;
    acc = acc.wrapping_add(m.name.len() as u64);
    if let Some(e) = m.email {
        acc = acc.wrapping_add(e.len() as u64);
    }
    for t in m.tags.iter() {
        acc = acc.wrapping_add(t.len() as u64);
    }
    for (k, v) in m.scores.iter() {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    if let Some(a) = m.address.as_option() {
        acc = acc.wrapping_add(a.city.len() as u64);
    }
    for (k, v) in m.extras.iter() {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    acc
}

pub(crate) fn touch_b092_person_lazy(m: &B092PersonLazy<'_>) -> u64 {
    let mut acc = m.id as u64;
    acc = acc.wrapping_add(m.name.len() as u64);
    if let Some(e) = m.email {
        acc = acc.wrapping_add(e.len() as u64);
    }
    for t in m.tags.iter() {
        acc = acc.wrapping_add(t.len() as u64);
    }
    for (k, v) in m.scores.iter() {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    if let Some(a) = m.address.get().expect("lazy person address") {
        acc = acc.wrapping_add(a.city.len() as u64);
    }
    for (k, v) in m.extras.iter() {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    acc
}

pub(crate) fn touch_qp_person(m: &QpPerson<'_>) -> u64 {
    let mut acc = m.id as u64;
    acc = acc.wrapping_add(m.name.len() as u64);
    // Unconditional: quick-protobuf drops proto3-optional presence, and an
    // empty email touches as zero either way, so equivalence still holds.
    acc = acc.wrapping_add(m.email.len() as u64);
    for t in &m.tags {
        acc = acc.wrapping_add(t.len() as u64);
    }
    for (k, v) in &m.scores {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    if let Some(a) = &m.address {
        acc = acc.wrapping_add(a.city.len() as u64);
    }
    for (k, v) in &m.extras {
        acc = acc.wrapping_add(k.len() as u64).wrapping_add(*v as u64);
    }
    acc
}

// ---------------------------------------------------------------------------
// Offline drift tests: the checked-in gencode is only valid for the exact
// schemas and peer versions it was generated from. These tests fail closed
// on drift. Schemas are embedded with `include_str!` (no test-time IO).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{CARGO_LOCK, HISTORICAL_CHECKSUMS, PEER_PINS, lock_checksum};

    /// Recorded sha256 of `proto/person.proto` at generation time.
    const PERSON_PROTO_SHA256: &str =
        "13ec16099b636bb46467e28199347c79ba26e72223462c1c3060e28d6c1620c5";
    /// Recorded sha256 of upstream `test_messages_proto3.proto` (protobuf v35.1).
    const TAT_PROTO_SHA256: &str =
        "c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873";

    const PERSON_PROTO: &str = include_str!("../../proto/person.proto");
    const TAT_PROTO: &str =
        include_str!("../../third_party/protobuf/src/google/protobuf/test_messages_proto3.proto");

    /// Minimal SHA-256 (FIPS 180-4) so drift tests need no extra dependency.
    /// Self-checked against the "abc" vector below.
    fn sha256_hex(data: &[u8]) -> String {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut h: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        let mut msg = data.to_vec();
        let bit_len = (data.len() as u64).wrapping_mul(8);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_be_bytes());
        let (blocks, rest) = msg.as_chunks::<64>();
        assert!(rest.is_empty(), "sha256 padding must fill whole blocks");
        for block in blocks {
            let mut w = [0u32; 64];
            for (i, word) in w.iter_mut().take(16).enumerate() {
                *word = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let t1 = hh
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                hh = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
            h[5] = h[5].wrapping_add(f);
            h[6] = h[6].wrapping_add(g);
            h[7] = h[7].wrapping_add(hh);
        }
        h.iter().map(|w| format!("{w:08x}")).collect()
    }

    #[test]
    fn sha256_self_check() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn schemas_match_generation_hashes() {
        assert_eq!(
            sha256_hex(PERSON_PROTO.as_bytes()),
            PERSON_PROTO_SHA256,
            "proto/person.proto drifted from the SB-08 checked-in gencode"
        );
        assert_eq!(
            sha256_hex(TAT_PROTO.as_bytes()),
            TAT_PROTO_SHA256,
            "test_messages_proto3.proto drifted from the SB-08 checked-in gencode"
        );
    }

    #[test]
    fn current_peers_resolve_in_lockfile() {
        for pin in PEER_PINS.iter().filter(|p| p.role == "current") {
            let sum = lock_checksum(pin.package, pin.version);
            assert!(
                sum.is_some_and(|s| s.len() == 64),
                "pin {} {} {} missing a 64-hex checksum in bench/Cargo.lock",
                pin.id,
                pin.package,
                pin.version
            );
        }
        // The lockfile is embedded whole; a resolver slip that drops a peer
        // package entirely must also fail here.
        for pkg in ["prost", "buffa", "google-protobuf", "quick-protobuf"] {
            assert!(
                CARGO_LOCK.contains(&format!("name = \"{pkg}\"")),
                "package {pkg} missing from the embedded bench/Cargo.lock"
            );
        }
    }

    #[test]
    fn historical_checksums_are_well_formed() {
        for (vers, sum) in HISTORICAL_CHECKSUMS {
            assert!(!vers.is_empty());
            assert_eq!(sum.len(), 64, "historical checksum for {vers}");
            assert!(sum.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }
}
