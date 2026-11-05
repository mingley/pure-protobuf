use pbrs::{Parse, Serialize};
use prost::Message;

#[allow(dead_code, reason = "reference parent messages only scope nested test types")]
mod reference {
    pub mod proof {
        pub mod collisions {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-collisions/proof.collisions.rs"
            ));
        }
        pub mod collision_use {
            include!(concat!(
                env!("OUT_DIR"),
                "/prost-collisions/proof.collision_use.rs"
            ));
        }
    }
}

fn main() {
    use proof::collisions as native;
    use reference::proof::collisions as peer;
    let reference = reference::proof::collision_use::Request {
        a: Some(peer::a::X { value: 1 }),
        b: Some(peer::b::X { value: 2 }),
        c: Some(peer::c::b::X { value: 3 }),
        foo: Some(peer::Foo { value: 4 }),
        mut_named: Some(peer::FooMut { value: 5 }),
        view_named: Some(peer::FooView { value: 6 }),
        nested_foo: Some(peer::outer::Foo { value: 7 }),
        nested_mut_named: Some(peer::outer::FooMut { value: 8 }),
        nested_view_named: Some(peer::outer::FooView { value: 9 }),
        enum_named: peer::BarMut::Ready as i32,
        mut_items: vec![peer::FooMut { value: 11 }],
        view_map: [("entry".to_owned(), peer::FooView { value: 12 })].into(),
        mut_child: Some(peer::foo_mut::Child { value: 13 }),
    };
    let wire = reference.encode_to_vec();
    let parsed = proof::collision_use::Request::parse(&wire).unwrap();
    assert_eq!(parsed.a().value(), 1);
    assert_eq!(parsed.b().value(), 2);
    assert_eq!(parsed.c().value(), 3);
    assert_eq!(parsed.foo().value(), 4);
    assert_eq!(parsed.mut_named().value(), 5);
    assert_eq!(parsed.view_named().value(), 6);
    assert_eq!(parsed.nested_foo().value(), 7);
    assert_eq!(parsed.nested_mut_named().value(), 8);
    assert_eq!(parsed.nested_view_named().value(), 9);
    assert_eq!(parsed.enum_named().0, reference.enum_named);
    assert_eq!(parsed.mut_items().len(), 1);
    assert_eq!(parsed.view_map().len(), 1);
    assert_eq!(parsed.mut_child().value(), 13);
    assert_eq!(
        reference::proof::collision_use::Request::decode(parsed.serialize().unwrap().as_slice())
            .unwrap(),
        reference
    );
    println!("PASS type-collision imported singular, repeated and map wire values");
    assert_eq!(
        proof::collision_use::request::ViewMapEntryView::Unspecified.0,
        reference::proof::collision_use::request::ViewMapEntryView::Unspecified as i32
    );

    let mut constructed = proof::collision_use::Request::new();
    let mut a = native::a::X::new();
    a.set_value(1);
    constructed.set_a(a);
    let mut b = native::b::X::new();
    b.set_value(2);
    constructed.set_b(b);
    let mut c = native::c::b::X::new();
    c.set_value(3);
    constructed.set_c(c);
    assert_eq!(constructed.a().value(), 1);
    assert_eq!(constructed.b().value(), 2);
    assert_eq!(constructed.c().value(), 3);
    println!("PASS type-collision repeated parent prefixes");

    let mut foo = native::Foo::new();
    foo.set_value(4);
    constructed.set_foo(foo);
    let mut mut_named = native::FooMut_::new();
    mut_named.set_value(5);
    constructed.set_mut_named(mut_named);
    let mut view_named = native::FooView_::new();
    view_named.set_value(6);
    constructed.set_view_named(view_named);
    assert_eq!(constructed.foo().value(), 4);
    assert_eq!(constructed.mut_named().value(), 5);
    assert_eq!(constructed.view_named().value(), 6);
    constructed.set_enum_named(native::BarMut_::Ready);
    assert_eq!(constructed.enum_named().0, reference.enum_named);
    let mut child = native::foo_mut::Child::new();
    child.set_value(13);
    constructed.set_mut_child(child);
    println!("PASS type-collision root message and enum companion names");

    let mut foo = native::outer::Foo::new();
    foo.set_value(7);
    constructed.set_nested_foo(foo);
    let mut mut_named = native::outer::FooMut_::new();
    mut_named.set_value(8);
    constructed.set_nested_mut_named(mut_named);
    let mut view_named = native::outer::FooView_::new();
    view_named.set_value(9);
    constructed.set_nested_view_named(view_named);
    assert_eq!(constructed.nested_foo().value(), 7);
    assert_eq!(constructed.nested_mut_named().value(), 8);
    assert_eq!(constructed.nested_view_named().value(), 9);
    let expected = reference::proof::collision_use::Request {
        mut_items: vec![],
        view_map: Default::default(),
        ..reference
    };
    assert_eq!(
        reference::proof::collision_use::Request::decode(
            constructed.serialize().unwrap().as_slice()
        )
        .unwrap(),
        expected
    );
    println!("PASS type-collision nested public companion aliases");
}
