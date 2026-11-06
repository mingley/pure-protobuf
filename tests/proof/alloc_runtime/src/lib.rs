//! Concrete generated native and Prost consumers, compiled for Cortex with alloc.
#![no_std]
extern crate alloc;

mod wire_runtime {
    pub use pbrs::*;
}
mod foreign {
    pub mod std {
        pub use crate::lazy::proof::embedded::{Child, ChildView};
    }
}
mod reference {
    pub mod proof {
        pub mod embedded {
            include!(concat!(env!("OUT_DIR"), "/reference/proof.embedded.rs"));
        }
        pub mod embedded_legacy {
            include!(concat!(
                env!("OUT_DIR"),
                "/reference/proof.embedded_legacy.rs"
            ));
        }
    }
}
mod alloc_extensions {
    include!(concat!(env!("OUT_DIR"), "/extensions/mod.rs"));
}
mod lazy {
    include!(concat!(env!("OUT_DIR"), "/lazy/mod.rs"));
    use proof::embedded::Child;
    include!("cases.rs");
}
mod owned {
    include!(concat!(env!("OUT_DIR"), "/owned/mod.rs"));
    use proof::embedded::Child;
    include!("cases.rs");
}
mod selected {
    include!(concat!(env!("OUT_DIR"), "/selected/mod.rs"));
    use proof::embedded::Child;
    include!("cases.rs");
}
#[forbid(unsafe_code)]
mod safe {
    include!(concat!(env!("OUT_DIR"), "/safe/mod.rs"));
    use proof::embedded::Child;
    include!("cases.rs");
}
mod external {
    include!(concat!(env!("OUT_DIR"), "/external/mod.rs"));
    use crate::foreign::std::Child;
    include!("cases.rs");
}

/// Encode, decode and mutate concrete messages in every generated profile.
///
/// This non-generic function is built for Cortex; host tests execute the same
/// assertions with the runtime's explicit `std` feature enabled.
pub fn exercise_all() {
    lazy::run();
    owned::run();
    selected::run();
    safe::run();
    external::run();
    cache_controls();
    shared_controls();
    extension_controls();
}

fn cache_controls() {
    let cache = pbrs::rt::CachedSize::default();
    assert_eq!(cache.get(), Some(0));
    cache.set(17);
    assert_eq!(cache.get(), Some(17));
    cache.set(u64::from(u32::MAX));
    #[cfg(target_has_atomic = "64")]
    assert_eq!(cache.get(), Some(u64::from(u32::MAX)));
    #[cfg(not(target_has_atomic = "64"))]
    assert_eq!(cache.get(), None);
    cache.dirty();
    assert_eq!(cache.get(), None);
    let compact = pbrs::rt::CachedSize32::default();
    compact.set(u64::from(u32::MAX));
    assert_eq!(compact.get(), None);
}

fn shared_controls() {
    use pbrs::{Parse, Serialize};
    let mut message = selected::proof::embedded::Fields::new();
    message.set_payload(alloc::vec![0x91; 256]);
    message.set_maybe_payload(alloc::vec![0x71; 128]);
    let wire = pbrs::rt::Bytes::from(Serialize::serialize(&message).unwrap());
    let parsed = selected::proof::embedded::Fields::parse_bytes(wire.clone()).unwrap();
    let shared = parsed.payload_shared();
    let optional = parsed.maybe_payload_shared_opt().unwrap();
    drop(parsed);
    drop(wire);
    assert_eq!(shared.as_bytes(), &[0x91; 256]);
    assert_eq!(optional.as_bytes(), &[0x71; 128]);
}

fn extension_controls() {
    use alloc_extensions::proof::alloc_ext::{Host, extensions::VALUE};
    use pbrs::{Parse, Serialize};
    let mut host = Host::new();
    assert_eq!(host.get_extension(&VALUE), 42);
    assert!(!host.has_extension(&VALUE));
    host.set_extension(&VALUE, 42);
    assert!(host.has_extension(&VALUE));
    let wire = Serialize::serialize(&host).unwrap();
    assert_eq!(wire, [0xa8, 0x06, 42]);
    let mut parsed = Host::parse(&wire).unwrap();
    assert_eq!(parsed.get_extension(&VALUE), 42);
    parsed.clear_extension(&VALUE);
    assert!(!parsed.has_extension(&VALUE));
    assert_eq!(parsed.get_extension(&VALUE), 42);
    assert!(Serialize::serialize(&parsed).unwrap().is_empty());
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_native_fields_against_generated_prost() {
        super::exercise_all();
    }
}
