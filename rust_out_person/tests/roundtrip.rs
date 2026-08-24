use pbrs::{Parse, Serialize};
use rust_out_person::Person;

#[test]
fn official_rust_out_person_parse_serialize_roundtrip() {
    let mut p = Person::new();
    p.set_id(7);
    p.set_name("ada");
    p.set_email("ada@ex");
    p.tags_mut().push("a");
    p.tags_mut().push("b");
    p.scores_mut().insert("math", 99);
    p.address_mut().set_city("nyc");
    p.extras_mut().insert("k", 1);

    assert_eq!(p.id(), 7);
    assert_eq!(p.name(), "ada");
    assert!(p.has_email());
    assert_eq!(p.email(), "ada@ex");
    assert_eq!(p.tags().len(), 2);
    assert_eq!(p.scores().len(), 1);
    assert!(p.has_address());
    assert_eq!(p.address().city(), "nyc");
    assert_eq!(p.extras().len(), 1);

    let bytes = Serialize::serialize(&p).expect("serialize");
    let q = Person::parse(&bytes).expect("parse");
    assert_eq!(q.id(), 7);
    assert_eq!(q.name(), "ada");
    assert_eq!(q.email(), "ada@ex");
    assert_eq!(q.tags().len(), 2);
    assert_eq!(q.scores().len(), 1);
    assert_eq!(q.address().city(), "nyc");
    assert_eq!(q.extras().len(), 1);

    let again = Person::parse(&Serialize::serialize(&q).unwrap()).unwrap();
    assert_eq!(again.id(), p.id());
    assert_eq!(again.name(), p.name());
    assert_eq!(again.email(), p.email());
    assert_eq!(again.address().city(), p.address().city());
    assert_eq!(again.tags().len(), p.tags().len());
    assert_eq!(again.scores().len(), p.scores().len());
}
