use pbrs::{
    Cardinality, DescriptorPool, DynamicMessage, FieldDescriptor, FieldType, MessageDescriptor,
    Presence, RECURSION_LIMIT,
};
use std::sync::Arc;

fn nest_desc() -> (Arc<DescriptorPool>, Arc<MessageDescriptor>) {
    let mut child = FieldDescriptor::new(
        "child",
        1,
        FieldType::Message,
        Cardinality::Optional,
        Presence::Explicit,
    );
    child.type_name = Some("Nest".into());
    let desc = MessageDescriptor::builder("Nest").field(child).build();
    let mut pool = DescriptorPool::new();
    let desc = pool.register_message(desc);
    (Arc::new(pool), desc)
}

fn nest_bytes(depth: u32) -> Vec<u8> {
    let mut inner = Vec::new();
    for _ in 0..depth {
        let mut wrapped = Vec::new();
        pbrs::rt::encode_len_field(&mut wrapped, 1, &inner);
        inner = wrapped;
    }
    inner
}

#[test]
fn nest_at_limit_ok_and_over_limit_err() {
    let (pool, desc) = nest_desc();
    let ok = nest_bytes(RECURSION_LIMIT);
    DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), &ok)
        .expect("depth == RECURSION_LIMIT must parse");

    let too_deep = nest_bytes(RECURSION_LIMIT + 1);
    DynamicMessage::parse_with_pool(desc, Some(pool), &too_deep)
        .expect_err("depth > RECURSION_LIMIT must fail");
}

#[test]
fn parse_trait_too_deep_is_err() {
    let (pool, desc) = nest_desc();
    let mut msg = DynamicMessage::new(desc);
    msg.set_pool(pool);
    use pbrs::ClearAndParse;
    let too_deep = nest_bytes(RECURSION_LIMIT + 1);
    assert!(ClearAndParse::merge_from_bytes(&mut msg, &too_deep).is_err());
    let shallow = nest_bytes(1);
    ClearAndParse::merge_from_bytes(&mut msg, &shallow).expect("shallow nest");
}
