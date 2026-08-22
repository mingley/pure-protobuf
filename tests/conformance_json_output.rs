//! Locks the empty-FDS failure class: without TestAllTypes in
//! `conformance_pool()`, every official JsonOutput case returns
//! `serialize_error: "missing desc"`.

use pbrs::gencode::{
    conformance_pool, TestAllTypesEdition2023, TestAllTypesProto2, TestAllTypesProto3,
};

#[test]
fn conformance_pool_has_test_all_types() {
    let pool = conformance_pool();
    for name in [
        "protobuf_test_messages.proto3.TestAllTypesProto3",
        "protobuf_test_messages.proto2.TestAllTypesProto2",
        "protobuf_test_messages.editions.TestAllTypesEdition2023",
    ] {
        assert!(
            pool.get_message(name).is_some(),
            "missing descriptor {name} (empty conformance FDS)"
        );
    }
}

#[test]
fn proto3_valid_data_scalar_int32_json_output() {
    let mut msg = TestAllTypesProto3::new();
    msg.set_optional_int32(1);
    let json = msg
        .to_json()
        .expect("ProtobufInput.ValidDataScalar JsonOutput");
    assert!(
        json.contains("\"optionalInt32\":1"),
        "unexpected JsonOutput: {json}"
    );
}

#[test]
fn proto2_valid_data_scalar_int32_json_output() {
    let mut msg = TestAllTypesProto2::new();
    msg.set_optional_int32(1);
    let json = msg
        .to_json()
        .expect("ProtobufInput.ValidDataScalar JsonOutput");
    assert!(
        json.contains("\"optionalInt32\":1"),
        "unexpected JsonOutput: {json}"
    );
}

#[test]
fn edition2023_valid_data_scalar_int32_json_output() {
    let mut msg = TestAllTypesEdition2023::new();
    msg.set_optional_int32(1);
    let json = msg
        .to_json()
        .expect("ProtobufInput.ValidDataScalar JsonOutput");
    assert!(
        json.contains("\"optionalInt32\":1"),
        "unexpected JsonOutput: {json}"
    );
}
