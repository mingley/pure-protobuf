//! SB-08: refreshed Rust codec peers for scoreboard cells.
//!
//! Status: definition only. This module is intentionally self-contained
//! (`std` only, no `pbrs`/peer dependencies) so it compiles standalone:
//!
//! ```sh
//! rustc --edition 2021 --test bench/devloop/cells/codec_peers.rs -o /tmp/codec_peers_test
//! /tmp/codec_peers_test
//! ```
//!
//! # What this is
//!
//! The single place that names the SB-08 peer set (see [`PEERS`]): prost
//! 0.13.5 (historical) and 0.14.4, buffa 0.9.2 (owned plus eager-view and
//! lazy-view columns), google-protobuf 0.36.2 (upb kernel), and
//! quick-protobuf 0.8.1 (person-shaped cells only). It also fixes the
//! per-cell workload contract ([`WORKLOADS`]: fresh, cached, and mutated
//! encode plus parse-only versus parse-and-touch decode, with outputs
//! consumed and checked) and the equivalence pre-checks that run before any
//! timing ([`check_wire_equal`], [`check_touch_equal`]).
//!
//! # Scoreboard rules encoded here
//!
//! * Every scoreboard cell uses generated (not handwritten) pbrs types. The
//!   handwritten-`Person` layout survives only as a `"diagnostic"` role, not
//!   a scoreboard cell.
//! * Peer versions and lockfile hashes are recorded in results. The
//!   checksums in [`PEERS`] mirror `bench/Cargo.lock` at SB-08 time; the
//!   lockfile is the source of truth and this table must move with it.
//! * quick-protobuf parses always borrow: its decode column is borrow-mode,
//!   never owned. It also drops proto3-optional presence and cannot parse
//!   the TAT schema, so it is person-shaped cells only.
//! * Buffa eager views validate the whole tree on decode; lazy views scan
//!   one level and decode nested/repeated message fields on access. Owned,
//!   eager-view, and lazy-view are separate columns.
//!
//! # Follow-up wiring (coordinator; outside SB-08 write scope)
//!
//! This file is not referenced by `bench/devloop/src/main.rs` yet: wiring
//! it in requires editing the cell registry (`mod` declaration plus
//! `codec_cells()` entries) and `bench/devloop/Cargo.toml`, which SB-08
//! must not touch. Two hard constraints shape that wiring:
//!
//! 1. `links=upb` conflict: `protobuf 4.35.1-release` (devloop's current
//!    `codec.v4.*` runtime) and `google-protobuf 0.36.2-release` both
//!    declare `links = "upb"`, and cargo rejects both in one dependency
//!    graph (proven during SB-08: the resolver names both packages and
//!    refuses). Devloop can therefore carry the 0.36 runtime only by
//!    dropping 4.35.1, or keep 4.35.1 and skip 0.36 there. Bench chose the
//!    swap (0.36 in); 4.35.1 stays measured in tonic-bench either way.
//! 2. Same-track unification: `buffa 0.9.1` and `0.9.2` cannot coexist
//!    (cargo unifies same-track pins). Devloop has no buffa dependency
//!    today, so it can add 0.9.2 directly with no conflict.
//!
//! The intended cells, once wired, extend the existing
//! `codec.{pbrs,prost,v4}.*` shape with `codec.prost14.*`,
//! `codec.g36.*` (or a renamed `codec.v4.*` on the 0.36 runtime),
//! `codec.buffa092.{owned,eager,lazy}.*`, and person-only `codec.qp.*`,
//! each running the [`WORKLOADS`] contract below. If wiring grows `unsafe`
//! beyond this file, the QG-01 policy in `docs/unsafe-invariants.md`
//! requires pre-registering the invariants there.

/// One scoreboard peer runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    /// Short JSON id (`prost14`, `g36`, ...).
    pub id: &'static str,
    /// crates.io package name.
    pub package: &'static str,
    /// Exact pinned version.
    pub version: &'static str,
    /// `Cargo.lock` sha256 (`None` for path dependencies).
    pub checksum: Option<&'static str>,
    /// `"current"` (linked) or `"historical"` (retired, measured elsewhere).
    pub role: &'static str,
    /// Which scoreboard columns this peer fills.
    pub columns: &'static [&'static str],
}

/// The SB-08 peer set. Checksums mirror `bench/Cargo.lock`; see the module
/// docs before changing this table without moving the lockfile.
pub const PEERS: &[Peer] = &[
    Peer {
        id: "prost13",
        package: "prost",
        version: "0.13.5",
        checksum: Some("2796faa41db3ec313a31f7624d9286acf277b52de526150b7e69f3debf891ee5"),
        role: "current",
        columns: &[
            "encode",
            "fresh_encode",
            "mutated_encode",
            "decode",
            "touch",
        ],
    },
    Peer {
        id: "prost14",
        package: "prost",
        version: "0.14.4",
        checksum: Some("528ac67416ff8646872a3c02cad9cc4ee5dc9f9540c9b10771855c95cb2e5ae1"),
        role: "current",
        columns: &[
            "encode",
            "fresh_encode",
            "mutated_encode",
            "decode",
            "touch",
        ],
    },
    Peer {
        id: "buffa092",
        package: "buffa",
        version: "0.9.2",
        checksum: Some("a92f2f5df67a9d5ccfc65237bfc954c56328aee40a04a9b92381ecba370be246"),
        role: "current",
        columns: &[
            "encode",
            "fresh_encode",
            "mutated_encode",
            "decode",
            "touch",
            "view_decode",
            "view_touch",
            "lazy_decode",
            "lazy_touch",
        ],
    },
    Peer {
        id: "g36",
        package: "google-protobuf",
        version: "0.36.2-release",
        checksum: Some("eb6a2d5d79f0041702be6081437d082721099677d4a708e80bca4e6a831df343"),
        role: "current",
        columns: &[
            "encode",
            "fresh_encode",
            "mutated_encode",
            "decode",
            "touch",
        ],
    },
    Peer {
        id: "qp",
        package: "quick-protobuf",
        version: "0.8.1",
        checksum: Some("9d6da84cc204722a989e01ba2f6e1e276e190f22263d0cb6ce8526fcdb0d2e1f"),
        role: "current",
        columns: &[
            "encode",
            "fresh_encode",
            "mutated_encode",
            "decode",
            "touch",
        ],
    },
    Peer {
        id: "v4",
        package: "protobuf",
        version: "4.35.1-release",
        checksum: Some("a169648cc34d6f327fea8919ca63f38261fb26405fde8879745dc0a483db328e"),
        role: "historical",
        columns: &[],
    },
    Peer {
        id: "buffa091",
        package: "buffa",
        version: "0.9.1",
        checksum: Some("cf9e6224bc4ee1f189ad257120c156fb05f95b826f5369d620b24984476c200a"),
        role: "historical",
        columns: &[],
    },
];

/// Look up a peer by JSON id.
pub fn find_peer(id: &str) -> Option<&'static Peer> {
    PEERS.iter().find(|p| p.id == id)
}

/// One timed workload all owned codecs in a cell must perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Workload {
    /// Column suffix (`fresh_encode`, `touch`, ...).
    pub name: &'static str,
    /// Whether view-only decoders (buffa views, quick-protobuf parses)
    /// perform this workload too.
    pub views: bool,
}

/// The BM-03 workload contract: fresh, cached, and mutated encode plus
/// parse-only versus parse-and-touch decode, with outputs consumed.
pub const WORKLOADS: &[Workload] = &[
    Workload {
        name: "encode",
        views: false,
    },
    Workload {
        name: "fresh_encode",
        views: false,
    },
    Workload {
        name: "mutated_encode",
        views: false,
    },
    Workload {
        name: "decode",
        views: true,
    },
    Workload {
        name: "touch",
        views: true,
    },
];

/// Equivalence pre-check failure: which check, which peer, and what differed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    /// `wire`, `mutated_wire`, or `touch`.
    pub check: &'static str,
    /// Peer id from [`PEERS`].
    pub peer: &'static str,
    /// Human-readable detail (lengths, checksums, first divergence).
    pub detail: String,
}

/// Byte-identical wire pre-check for byte-stable cases.
pub fn check_wire_equal(
    reference: &[u8],
    peer: &'static str,
    bytes: &[u8],
) -> Result<(), Mismatch> {
    if reference == bytes {
        return Ok(());
    }
    let first = reference.iter().zip(bytes.iter()).position(|(a, b)| a != b);
    Err(Mismatch {
        check: "wire",
        peer,
        detail: format!(
            "len {} vs {}, first divergence at {:?}",
            reference.len(),
            bytes.len(),
            first
        ),
    })
}

/// Mutated-state wire pre-check: both toggle states must match.
pub fn check_mutated_wire_equal(
    peer: &'static str,
    state42: (&[u8], &[u8]),
    state43: (&[u8], &[u8]),
) -> Result<(), Mismatch> {
    for (probe, (reference, bytes)) in [(42, state42), (43, state43)] {
        check_wire_equal(reference, peer, bytes).map_err(|m| Mismatch {
            check: "mutated_wire",
            detail: format!("state {probe}: {}", m.detail),
            ..m
        })?;
    }
    Ok(())
}

/// Touch-checksum pre-check: equal checksums prove equivalent observable
/// content across decoders.
pub fn check_touch_equal(
    reference: u64,
    peer: &'static str,
    checksum: u64,
) -> Result<(), Mismatch> {
    if reference == checksum {
        return Ok(());
    }
    Err(Mismatch {
        check: "touch",
        peer,
        detail: format!("checksum {reference:#x} vs {checksum:#x}"),
    })
}

#[cfg(test)]
mod tests {
    use super::{check_mutated_wire_equal, check_touch_equal, check_wire_equal, PEERS, WORKLOADS};

    #[test]
    fn peer_table_has_five_current_peers() {
        let current: Vec<_> = PEERS.iter().filter(|p| p.role == "current").collect();
        let ids: Vec<_> = current.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["prost13", "prost14", "buffa092", "g36", "qp"]);
    }

    #[test]
    fn peer_versions_are_exact_pins_with_lock_hashes() {
        for p in PEERS {
            assert!(!p.version.is_empty(), "{} has no version", p.id);
            // No semver operators: SB-08 pins are exact.
            assert!(
                !p.version
                    .chars()
                    .any(|c| matches!(c, '^' | '~' | '>' | '<' | '=' | '*' | ',')),
                "{} version {} is not an exact pin",
                p.id,
                p.version
            );
            let sum = p.checksum.expect("every SB-08 peer records a checksum");
            assert_eq!(sum.len(), 64, "{} checksum length", p.id);
            assert!(
                sum.chars().all(|c| c.is_ascii_hexdigit()),
                "{} checksum is not hex",
                p.id
            );
        }
    }

    #[test]
    fn peer_ids_and_packages_are_unique_per_id() {
        let mut ids: Vec<_> = PEERS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PEERS.len(), "duplicate peer id");
        // prost appears twice (0.13 + 0.14) and buffa appears twice
        // (0.9.2 + historical 0.9.1); every other package appears once.
        for (package, want) in [
            ("prost", 2),
            ("buffa", 2),
            ("google-protobuf", 1),
            ("quick-protobuf", 1),
            ("protobuf", 1),
        ] {
            let got = PEERS.iter().filter(|p| p.package == package).count();
            assert_eq!(got, want, "package {package} count");
        }
    }

    #[test]
    fn buffa_has_owned_eager_and_lazy_columns() {
        let buffa = super::find_peer("buffa092").expect("buffa092 row");
        for column in [
            "encode",
            "decode",
            "touch",
            "view_decode",
            "view_touch",
            "lazy_decode",
            "lazy_touch",
        ] {
            assert!(buffa.columns.contains(&column), "buffa092 missing {column}");
        }
    }

    #[test]
    fn workload_contract_covers_bm03_taxonomy() {
        let names: Vec<_> = WORKLOADS.iter().map(|w| w.name).collect();
        assert_eq!(
            names,
            [
                "encode",
                "fresh_encode",
                "mutated_encode",
                "decode",
                "touch"
            ]
        );
        // Views decode and touch but never encode.
        for w in WORKLOADS {
            assert_eq!(
                w.views,
                matches!(w.name, "decode" | "touch"),
                "{} view membership",
                w.name
            );
        }
    }

    #[test]
    fn wire_check_accepts_equal_rejects_unequal() {
        assert!(check_wire_equal(b"abc", "qp", b"abc").is_ok());
        let m = check_wire_equal(b"abc", "qp", b"abd").unwrap_err();
        assert_eq!(m.check, "wire");
        assert_eq!(m.peer, "qp");
        let m = check_wire_equal(b"abc", "qp", b"abcd").unwrap_err();
        assert!(m.detail.contains("3 vs 4"), "length detail: {}", m.detail);
    }

    #[test]
    fn mutated_wire_check_names_the_state() {
        let ok42 = (&b"a"[..], &b"a"[..]);
        let bad43 = (&b"a"[..], &b"b"[..]);
        let m = check_mutated_wire_equal("g36", ok42, bad43).unwrap_err();
        assert_eq!(m.check, "mutated_wire");
        assert_eq!(m.peer, "g36");
        assert!(m.detail.contains("43"), "state detail: {}", m.detail);
        assert!(check_mutated_wire_equal("g36", ok42, ok42).is_ok());
    }

    #[test]
    fn touch_check_compares_checksums() {
        assert!(check_touch_equal(7, "prost14", 7).is_ok());
        let m = check_touch_equal(7, "prost14", 8).unwrap_err();
        assert_eq!(m.check, "touch");
        assert_eq!(m.peer, "prost14");
    }
}
