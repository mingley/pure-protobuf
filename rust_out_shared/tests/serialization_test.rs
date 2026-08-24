extern crate pbrs as protobuf;
#[allow(unused_imports)]
mod protos { pub use rust_out_shared::*; }
#[allow(unused_imports)]
use protos::*;

// Protocol Buffers - Google's data interchange format
// Copyright 2023 Google LLC.  All rights reserved.
//
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file or at
// https://developers.google.com/open-source/licenses/bsd


use googletest::prelude::*;
use protobuf::prelude::*;
use protobuf::View;

use paste::paste;
use unittest_proto3_optional_rust_proto::TestProto3Optional;
use unittest_proto3_rust_proto::TestAllTypes as TestAllTypesProto3;
use unittest_rust_proto::{TestAllTypes, TestRequired};

macro_rules! generate_parameterized_serialization_test {
    ($(($type: ident, $name_ext: ident)),*) => {
        paste! { $(
            #[gtest]
            fn [< serialization_zero_length_ $name_ext >]() {
                let mut msg = [< $type >]::new();

                let serialized = msg.serialize().unwrap();
                assert_that!(serialized.len(), eq(0));

                let serialized = msg.as_view().serialize().unwrap();
                assert_that!(serialized.len(), eq(0));

                let serialized = msg.as_mut().serialize().unwrap();
                assert_that!(serialized.len(), eq(0));
            }

            #[gtest]
            fn [< serialize_default_view $name_ext>]() {
                let default = View::<[< $type >]>::default();
                assert_that!(default.serialize().unwrap().len(), eq(0));
            }

            #[gtest]
            fn [< serialize_deserialize_message_ $name_ext>]() {
                let mut msg = [< $type >]::new();
                msg.set_optional_int64(42);
                msg.set_optional_bool(true);
                msg.set_optional_bytes(b"serialize deserialize test");

                let serialized = msg.serialize().unwrap();

                let msg2 = [< $type >]::parse(&serialized).unwrap();
                assert_that!(msg.optional_int64(), eq(msg2.optional_int64()));
                assert_that!(msg.optional_bool(), eq(msg2.optional_bool()));
                assert_that!(msg.optional_bytes(), eq(msg2.optional_bytes()));
            }

            #[gtest]
            fn [< deserialize_empty_ $name_ext>]() {
                assert!([< $type >]::parse(&[]).is_ok());
            }

            #[gtest]
            fn [< deserialize_error_ $name_ext>]() {
                assert!([< $type >]::parse(b"not a serialized proto").is_err());
            }

            #[gtest]
            fn [< set_bytes_with_serialized_data_ $name_ext>]() {
                let mut msg = [< $type >]::new();
                msg.set_optional_int64(42);
                msg.set_optional_bool(true);
                let mut msg2 = [< $type >]::new();
                msg2.set_optional_bytes(msg.serialize().unwrap());
                assert_that!(msg2.optional_bytes(), eq(msg.serialize().unwrap()));
            }

            #[gtest]
            fn [< deserialize_on_previously_allocated_message_ $name_ext>]() {
                let mut msg = [< $type >]::new();
                msg.set_optional_int64(42);
                msg.set_optional_bytes(b"serialize deserialize test");

                let serialized = msg.serialize().unwrap();

                let mut msg2 = Box::new([< $type >]::new());
                msg2.set_optional_bool(true);

                assert!(msg2.clear_and_parse(&serialized).is_ok());
                assert_that!(msg2.optional_int64(), eq(msg.optional_int64()));
                assert_that!(msg2.optional_bytes(), eq(msg.optional_bytes()));
                assert_that!(msg2.optional_bool(), eq(false));
            }

            #[gtest]
            fn [< deserialize_on_previously_allocated_message_mut_ $name_ext>]() {
                let mut msg = [< $type >]::new();
                msg.set_optional_int64(42);
                msg.set_optional_bytes(b"serialize deserialize test");

                let serialized = msg.serialize().unwrap();

                let mut msg2 = Box::new([< $type >]::new());
                let msg2_mut = msg2.as_mut();
                msg2_mut.set_optional_bool(true);

                assert!(msg2_mut.clear_and_parse(&serialized).is_ok());
                assert_that!(msg2.optional_int64(), eq(msg.optional_int64()));
                assert_that!(msg2.optional_bytes(), eq(msg.optional_bytes()));
                assert_that!(msg2.optional_bool(), eq(false));
            }

        )* }
    };
  }

generate_parameterized_serialization_test!(
    (TestAllTypes, editions),
    (TestAllTypesProto3, proto3),
    (TestProto3Optional, proto3_optional)
);

macro_rules! generate_parameterized_int32_byte_size_test {
    ($(($type: ident, $name_ext: ident)),*) => {
        paste! { $(

            #[gtest]
            fn [< test_int32_byte_size_ $name_ext>]() {
                let args = vec![(0, 1), (127, 1), (128, 2), (-1, 10)];
                for arg in args {
                    let value = arg.0;
                    let expected_value_size = arg.1;
                    let mut msg = [< $type >]::new();
                    // tag for optional_int32 only takes 1 byte
                    msg.set_optional_int32(value);
                    let serialized = msg.serialize().unwrap();
                    // 1 byte for tag and n from expected_value_size
                    assert_that!(serialized.len(), eq(expected_value_size + 1), "Test failed. Value: {value}. Expected_value_size: {expected_value_size}.");
                }

            }
        )* }
    };
  }

generate_parameterized_int32_byte_size_test!(
    (TestAllTypes, editions),
    (TestProto3Optional, proto3_optional) /* Test would fail if we were to use
                                           * TestAllTypesProto3: optional_int32 follows "no
                                           * presence" semantics and setting it to 0 (default
                                           * value) will cause it to not be serialized */
);

#[gtest]
fn test_required_field_enforced() {
    // Empty bytes slice is a valid binaryproto with no fields set -- therefore it should not parse
    // as a message with required fields.
    expect_that!(TestRequired::parse(&[]), err(anything()));

    let mut msg = TestRequired::new();
    expect_that!(msg.clear_and_parse(&[]), err(anything()));
}

#[gtest]
fn test_required_field_not_enforced() {
    // Empty bytes slice is a valid binaryproto with no fields set.
    let mut msg = TestRequired::parse_dont_enforce_required(&[]).unwrap();
    expect_that!(msg.has_a(), eq(false));

    msg.set_a(1);
    msg.clear_and_parse_dont_enforce_required(&[]).unwrap();
    expect_that!(msg.has_a(), eq(false));
}

// MiniTable encode used to emit FieldKind::U32/U64/I64 as varint and ignore
// MiniField.ty, so fixed32/64, sfixed32/64, and sint64 roundtrips dropped or
// corrupted the field. Wire tags: field 6 varint = 0x30, field 7 I32 = 0x3d.
#[gtest]
fn test_optional_fixed32_sint64_wire_and_roundtrip() {
    let mut msg = TestAllTypes::new();
    msg.set_optional_sint64(-1);
    msg.set_optional_fixed32(0xa1b2c3d4);

    let serialized = msg.serialize().unwrap();
    assert_that!(
        serialized.as_slice(),
        eq([0x30, 0x01, 0x3d, 0xd4, 0xc3, 0xb2, 0xa1].as_slice())
    );

    let parsed = TestAllTypes::parse(&serialized).unwrap();
    assert_that!(parsed.optional_sint64(), eq(-1));
    assert_that!(parsed.optional_fixed32(), eq(0xa1b2c3d4));
    assert_that!(parsed.has_optional_sint64(), eq(true));
    assert_that!(parsed.has_optional_fixed32(), eq(true));

    msg.set_optional_fixed64(0x0102030405060708);
    msg.set_optional_sfixed32(-2);
    msg.set_optional_sfixed64(-3);
    let serialized = msg.serialize().unwrap();
    let parsed = TestAllTypes::parse(&serialized).unwrap();
    assert_that!(parsed.optional_sint64(), eq(-1));
    assert_that!(parsed.optional_fixed32(), eq(0xa1b2c3d4));
    assert_that!(parsed.optional_fixed64(), eq(0x0102030405060708));
    assert_that!(parsed.optional_sfixed32(), eq(-2));
    assert_that!(parsed.optional_sfixed64(), eq(-3));
}

#[gtest]
fn test_proto3_packed_float_roundtrip() {
    let mut msg = TestAllTypesProto3::new();
    msg.repeated_float_mut().push(1.5);
    msg.repeated_float_mut().push(-2.0);
    let serialized = msg.serialize().unwrap();
    let parsed = TestAllTypesProto3::parse(&serialized).unwrap();
    assert_that!(parsed.repeated_float().len(), eq(2));
    assert_that!(parsed.repeated_float().get(0), some(eq(1.5)));
    assert_that!(parsed.repeated_float().get(1), some(eq(-2.0)));
}
