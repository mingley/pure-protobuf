//! SB-08 checked-in peer gencode: prost 0.13/0.14, buffa 0.9.2,
//! google-protobuf 0.36.2, and quick-protobuf 0.8.1 types.
//!
//! Every type in this module is compiler-generated from a `.proto` schema
//! (see `SB08_PROVENANCE.md` next to this file for generators, pins, source
//! hashes, and regen commands). Nothing here is handwritten: the renames
//! noted in each file header are mechanical path rewrites so side-by-side
//! peer versions resolve to renamed dependencies.
//!
//! Why checked in rather than built by a build script: the 0.36 gencode
//! needs `protoc --rust_out` from protobuf 36.x while the 4.35.1-era path
//! crates it replaces needed 35.x; no single installed `protoc` can
//! regenerate every peer at build time, and the tonic-bench precedent
//! (`docs/plan` BM-03 evidence) is checked-in gencode with a SHA-256
//! manifest plus offline drift tests. The drift tests live in
//! `super::tests` and compare the recorded source hashes against the live
//! schema files; the gencode-to-runtime version assertions inside the
//! generated files fail the build on runtime drift.

macro_rules! peer_mod {
    ($(#[$attr:meta])* $name:ident, $file:literal) => {
        $(#[$attr])*
        #[allow(
            unused,
            non_snake_case,
            non_camel_case_types,
            non_upper_case_globals,
            dead_code,
            unused_imports,
            unused_qualifications,
            unreachable_pub,
            clippy::all,
            reason = "SB-08 checked-in peer gencode"
        )]
        pub mod $name {
            include!($file);
        }
    };
}

peer_mod!(
    /// prost 0.13 `Person`/`Address` from `proto/person.proto`.
    prost13_person,
    "prost13_person.rs"
);
peer_mod!(
    /// prost 0.14 `Person`/`Address` from `proto/person.proto`.
    prost14_person,
    "prost14_person.rs"
);
peer_mod!(
    /// prost 0.14 `TestAllTypesProto3` from upstream
    /// `test_messages_proto3.proto` (0.13 TAT still comes from the
    /// `prost_tat` path crate, which owns the prost 0.13 runtime).
    prost14_tat,
    "prost14_tat.rs"
);
peer_mod!(
    /// quick-protobuf 0.8.1 `Person`/`Address` from `proto/person.proto`.
    /// quick-protobuf cannot parse the TAT schema (pb-rs rejects nested
    /// messages), so this peer is person-shaped cells only.
    qp_person,
    "qp_person.rs"
);

/// buffa 0.9.2 `Person`/`Address` (owned + eager view + lazy view).
#[allow(
    unused,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_imports,
    unused_qualifications,
    unreachable_pub,
    clippy::all,
    reason = "SB-08 checked-in peer gencode"
)]
pub mod buffa092_person {
    pub mod example {
        use super::*;
        include!("buffa092_person/example.mod.rs");
    }
}

/// buffa 0.9.2 `TestAllTypesProto3` (owned + eager view + lazy view).
#[allow(
    unused,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    unused_imports,
    unused_qualifications,
    unreachable_pub,
    clippy::all,
    reason = "SB-08 checked-in peer gencode"
)]
pub mod buffa092_tat {
    pub mod protobuf_test_messages {
        use super::*;
        pub mod proto3 {
            use super::*;
            include!("buffa092_tat/protobuf_test_messages.proto3.mod.rs");
        }
    }
}

peer_mod!(
    /// google-protobuf 0.36.2 `Person`/`Address` (upb kernel) from
    /// `proto/person.proto`. The gencode's `::protobuf::` paths resolve to
    /// the `google-protobuf` crate via the bench `protobuf` rename.
    gpb36_person,
    "gpb36_person/generated.rs"
);
peer_mod!(
    /// google-protobuf 0.36.2 `TestAllTypesProto3` (upb kernel) from
    /// upstream `test_messages_proto3.proto` plus the WKTs it references.
    gpb36_tat,
    "gpb36_tat/generated.rs"
);
