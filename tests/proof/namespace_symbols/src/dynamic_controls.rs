//! Direct macro hygiene controls bypass the generator's established Pb names.
#![allow(dead_code)]

pub struct Clone;
pub struct PartialEq;
pub struct Eq;
pub struct Default;
#[allow(non_camel_case_types)]
pub struct std;
pub struct Option;
pub struct Result;
pub struct Vec;
pub struct Box;
pub struct String;
pub struct Some(pub i32);
pub struct None(pub i32);
pub struct Ok(pub i32);
pub struct Err(pub i32);

fn pool() -> ::std::sync::Arc<pbrs::DescriptorPool> {
    static POOL: ::std::sync::OnceLock<::std::sync::Arc<pbrs::DescriptorPool>> =
        ::std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        ::std::sync::Arc::new(
            pbrs::DescriptorPool::from_file_descriptor_set(
                crate::native::proof::symbols::FILE_DESCRIPTOR_SET,
            )
            .unwrap(),
        )
    })
    .clone()
}

pbrs::impl_generated_message!(
    Legacy,
    LegacyView,
    LegacyMut,
    "proof.symbols.Record",
    pool(),
    default_view
);

pub fn run() {
    use pbrs::prelude::*;
    fn require_send_sync<T: ::core::marker::Send + ::core::marker::Sync>() {}
    require_send_sync::<Legacy>();
    assert_eq!(
        ::core::mem::size_of::<LegacyView<'_>>(),
        ::core::mem::size_of::<&Legacy>()
    );
    let empty = <LegacyView<'_> as ::core::default::Default>::default();
    let other_empty = <LegacyView<'_> as ::core::default::Default>::default();
    assert!(::core::ptr::eq(empty.0, other_empty.0));
    assert!(empty.serialize().unwrap().is_empty());
    let value = Legacy::from_json("{\"count\":7}").unwrap();
    let wire = value.serialize().unwrap();
    assert_eq!(wire, [8, 7]);
    assert_eq!(Legacy::parse(&wire).unwrap(), value);
    assert_eq!(Legacy::from_text(&value.to_text().unwrap()).unwrap(), value);
    let mut copied = Legacy::new();
    copied.copy_from(value.as_view());
    assert_eq!(copied, value);
    copied.clear();
    assert!(copied.serialize().unwrap().is_empty());
    crate::legacy_compat::run();
    println!("PASS exported legacy macro under standard type and value-constructor shadows");
}
