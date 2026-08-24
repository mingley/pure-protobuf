//! Official `protoc --rust_out` (4.35.1-release, kernel=upb) for proto/person.proto
//! linked against pbrs as `protobuf`.

extern crate pbrs as protobuf;

#[allow(nonstandard_style, unused, unreachable_pub, clippy::all)]
#[path = "person.u.pb.rs"]
mod internal_do_not_use_person;

pub use internal_do_not_use_person::*;
