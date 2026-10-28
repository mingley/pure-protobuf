//! Legacy generated implementations keep ordinary checking by default.
use pbrs::rt::{LazyMsg, MergeBytes, ParseError, Wire};
use std::sync::atomic::{AtomicUsize, Ordering};

static LEGACY_MERGES: AtomicUsize = AtomicUsize::new(0);
static MODERN_MERGES: AtomicUsize = AtomicUsize::new(0);
static PREVALIDATED_MERGES: AtomicUsize = AtomicUsize::new(0);
static VALIDATIONS: AtomicUsize = AtomicUsize::new(0);

fn decode(wire: &Wire, pos: &mut usize) -> Result<u64, ParseError> {
    let data = wire.as_slice();
    let (number, kind) = pbrs::rt::decode_tag(data, pos)?;
    if number != 1 || kind != pbrs::rt::WIRE_VARINT {
        return Err(ParseError::new("unexpected probe field"));
    }
    let value = pbrs::rt::decode_varint(data, pos)?;
    if *pos != data.len() {
        return Err(ParseError::new("trailing probe bytes"));
    }
    Ok(value)
}

#[derive(Default)]
struct Legacy(u64);
impl MergeBytes for Legacy {
    fn merge_inner(
        &mut self,
        wire: &Wire,
        pos: &mut usize,
        _depth: u32,
        _enforce: bool,
        _until: Option<u32>,
    ) -> Result<(), ParseError> {
        LEGACY_MERGES.fetch_add(1, Ordering::Relaxed);
        self.0 = decode(wire, pos)?;
        Ok(())
    }
}

#[derive(Default, Clone)]
struct Modern(u64);
impl MergeBytes for Modern {
    fn merge_inner(
        &mut self,
        wire: &Wire,
        pos: &mut usize,
        _depth: u32,
        _enforce: bool,
        _until: Option<u32>,
    ) -> Result<(), ParseError> {
        MODERN_MERGES.fetch_add(1, Ordering::Relaxed);
        self.0 = decode(wire, pos)?;
        Ok(())
    }
    fn merge_prevalidated(
        &mut self,
        wire: &Wire,
        pos: &mut usize,
        _depth: u32,
        _enforce: bool,
        _until: Option<u32>,
    ) -> Result<(), ParseError> {
        PREVALIDATED_MERGES.fetch_add(1, Ordering::Relaxed);
        self.0 = decode(wire, pos)?;
        Ok(())
    }
    fn validate_inner(wire: &Wire, pos: &mut usize, _depth: u32) -> Result<(), ParseError> {
        VALIDATIONS.fetch_add(1, Ordering::Relaxed);
        decode(wire, pos).map(|_| ())
    }
}

pub(super) fn verify() {
    let wire = Wire::from_slice(&[0x08, 0x07]);
    let mut pos = 0;
    Legacy::validate_inner(&wire, &mut pos, 0).unwrap();
    let mut old = LazyMsg::<Legacy>::from_validated_wire(wire.clone());
    assert_eq!(old.as_deref().unwrap().0, 7);
    assert_eq!(LEGACY_MERGES.load(Ordering::Relaxed), 2);
    old.get_or_insert().0 = 9;
    assert_eq!(old.as_deref().unwrap().0, 9);
    assert_eq!(LEGACY_MERGES.load(Ordering::Relaxed), 2);

    let mut pos = 0;
    Modern::validate_inner(&wire, &mut pos, 0).unwrap();
    let mut modern = LazyMsg::<Modern>::from_validated_wire(wire.clone());
    let unread_clone = modern.clone();
    assert_eq!(modern.as_deref().unwrap().0, 7);
    assert_eq!(unread_clone.as_deref().unwrap().0, 7);
    assert_eq!(PREVALIDATED_MERGES.load(Ordering::Relaxed), 2);
    assert_eq!(MODERN_MERGES.load(Ordering::Relaxed), 0);
    assert_eq!(VALIDATIONS.load(Ordering::Relaxed), 1);
    modern.get_or_insert().0 = 99;
    assert_eq!(modern.as_deref().unwrap().0, 99);
    assert!(modern.wire_bytes().is_none());
    assert_eq!(unread_clone.as_deref().unwrap().0, 7);

    // The legacy constructor never opts arbitrary input into the new path.
    let legacy_constructor = LazyMsg::<Modern>::from_wire(wire.clone());
    assert_eq!(legacy_constructor.as_deref().unwrap().0, 7);
    assert_eq!(MODERN_MERGES.load(Ordering::Relaxed), 1);
    assert_eq!(PREVALIDATED_MERGES.load(Ordering::Relaxed), 2);
    // A caller-created raw window uses the same legacy constructor.
    // Validation provenance belongs to the lazy slot, not the Wire type.
    let raw_window = LazyMsg::<Modern>::from_wire(wire.window(0, 2));
    assert_eq!(raw_window.as_deref().unwrap().0, 7);
    assert_eq!(MODERN_MERGES.load(Ordering::Relaxed), 2);
    assert_eq!(PREVALIDATED_MERGES.load(Ordering::Relaxed), 2);
    println!(
        "PASS legacy MergeBytes default fallback, opt-in dispatch, constructor isolation and mutation"
    );
}
