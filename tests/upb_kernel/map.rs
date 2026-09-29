//! Map kernel paths (`src/runtime/map.rs`).

use pbrs::runtime::{Arena, InnerMapMut, empty_map};
use pbrs::{AsMut, AsView, IntoMut, IntoView};

#[test]
fn empty_map_has_no_entries() {
    let view = empty_map::<pbrs::ProtoString, i32>();
    assert!(view.is_empty());
    assert_eq!(view.len(), 0);
}

#[test]
fn inner_map_mut_carries_raw_and_arena() {
    let arena = Arena::new();
    let inner = InnerMapMut::new(std::ptr::null(), &arena);
    assert!(inner.raw.is_null());
}

// A closed protobuf enum fixture. Using an actual Rust enum makes it impossible
// for an unchecked integer transmute to accidentally pass the invalid-value test
// under Miri: 7 is not a valid Rust discriminant.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClosedEnum {
    Zero = 0,
    One = 1,
}

impl From<ClosedEnum> for i32 {
    fn from(value: ClosedEnum) -> Self {
        match value {
            ClosedEnum::Zero => 0,
            ClosedEnum::One => 1,
        }
    }
}

impl TryFrom<i32> for ClosedEnum {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            _ => Err(()),
        }
    }
}

impl pbrs::__internal::SealedInternal for ClosedEnum {}
impl pbrs::__internal::EntityType for ClosedEnum {
    type Tag = pbrs::__internal::entity_tag::EnumTag;
}
// SAFETY: this fixture's enum schema has exactly the two listed values and
// TryFrom rejects every unnamed number, matching the generated enum contract.
unsafe impl pbrs::__internal::Enum for ClosedEnum {
    const NAME: &'static str = "ClosedEnum";
    fn is_known(value: i32) -> bool {
        matches!(value, 0 | 1)
    }
}
impl pbrs::Proxied for ClosedEnum {
    type View<'msg> = Self;
}
impl pbrs::AsView for ClosedEnum {
    type Proxied = Self;
    fn as_view(&self) -> Self {
        *self
    }
}
impl<'msg> pbrs::IntoView<'msg> for ClosedEnum {
    fn into_view<'shorter>(self) -> Self
    where
        'msg: 'shorter,
    {
        self
    }
}

#[test]
fn closed_enum_map_uses_checked_typed_conversion() {
    let arena = Arena::new();
    let parent = pbrs::runtime::MessagePtr::<CollectionFixture>::new(&arena).unwrap();
    // SAFETY: parent is arena-owned and exclusively accessed here; index zero
    // is the fixture's map slot and the map shares its arena.
    let raw = unsafe {
        parent
            .get_or_create_mutable_map_at_index(0, &arena)
            .unwrap()
    };
    {
        // SAFETY: raw is an empty arena-owned map exclusively borrowed until the
        // mutator drops; its keys/values are populated with the declared types.
        let mut map = unsafe {
            pbrs::MapMut::<i32, ClosedEnum>::from_inner(
                pbrs::__internal::Private,
                InnerMapMut::new(raw, &arena),
            )
        };
        assert!(map.insert(5, ClosedEnum::One));
        assert_eq!(map.get(5), Some(ClosedEnum::One));
        assert_eq!(
            map.as_view().iter().collect::<Vec<_>>(),
            [(5, ClosedEnum::One)]
        );
        assert!(!map.insert(5, ClosedEnum::Zero));
        assert_eq!(map.get(5), Some(ClosedEnum::Zero));
        assert_eq!(map.as_mut().into_mut().get(5), Some(ClosedEnum::Zero));
        assert_eq!(map.as_mut().into_view().get(5), Some(ClosedEnum::Zero));
    }

    // SAFETY: no map view/mutator remains. Inject an unrecognized wire number
    // to exercise the conversion's rejection path, without creating an enum.
    unsafe {
        (*raw).entries.borrow_mut().push((
            6_i32.to_le_bytes().to_vec(),
            pbrs::runtime::FieldKind::I32(7),
        ));
    }
    // SAFETY: the arena owns this raw map for the entire view lifetime. The
    // malformed enum number must be rejected by the typed conversion boundary.
    let view = unsafe { pbrs::MapView::<i32, ClosedEnum>::from_raw_ptr(raw) };
    assert_eq!(view.get(5), Some(ClosedEnum::Zero));
    assert_eq!(view.get(6), None);
    assert_eq!(view.iter().collect::<Vec<_>>(), [(5, ClosedEnum::Zero)]);
}

struct CollectionFixture;
// SAFETY: the empty MiniTable belongs to this fixture; its sole collection
// slot is added through get_or_create_mutable_{map,array}_at_index.
unsafe impl pbrs::runtime::AssociatedMiniTable for CollectionFixture {
    fn mini_table() -> pbrs::runtime::MiniTablePtr {
        pbrs::runtime::MiniTablePtr::dangling()
    }
}

#[test]
fn closed_repeated_enum_rejects_invalid_discriminants() {
    let arena = Arena::new();
    let parent = pbrs::runtime::MessagePtr::<CollectionFixture>::new(&arena).unwrap();
    // SAFETY: parent is arena-owned and exclusively accessed here; index zero
    // is the fixture's repeated slot and its array shares this arena.
    let raw = unsafe {
        parent
            .get_or_create_mutable_array_at_index(0, &arena)
            .unwrap()
    };
    {
        // SAFETY: the newly created empty array is populated with ClosedEnum values
        // and exclusively borrowed by this mutator for less than arena's lifetime.
        let mut values = unsafe {
            pbrs::RepeatedMut::<ClosedEnum>::from_inner(
                pbrs::__internal::Private,
                pbrs::runtime::InnerRepeatedMut::new(raw, &arena),
            )
        };
        values.push(ClosedEnum::Zero);
        values.set(0, ClosedEnum::One);
        assert_eq!(values.get(0), Some(ClosedEnum::One));
        assert_eq!(values.as_mut().into_mut().get(0), Some(ClosedEnum::One));
        assert_eq!(values.as_mut().into_view().get(0), Some(ClosedEnum::One));
        assert_eq!(values.as_view().into_view().get(0), Some(ClosedEnum::One));
        assert_eq!(values.iter().collect::<Vec<_>>(), [ClosedEnum::One]);
    }
    // SAFETY: no array view/mutator remains. Inject only a raw wire number;
    // constructing the rejected Rust discriminant would be UB under Miri.
    unsafe {
        (*raw)
            .items
            .borrow_mut()
            .push(pbrs::runtime::FieldKind::I32(7))
    };
    // SAFETY: the array remains arena-owned for the entire view lifetime;
    // conversion validates enum numbers before creating any ClosedEnum value.
    let view = unsafe { pbrs::RepeatedView::<ClosedEnum>::from_raw_ptr(raw) };
    assert_eq!(view.get(0), Some(ClosedEnum::One));
    assert_eq!(view.get(1), None);
    assert_eq!(view.iter().collect::<Vec<_>>(), [ClosedEnum::One]);
}
