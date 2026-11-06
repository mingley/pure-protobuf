use pbrs::{Parse, Serialize};
use prost::Message;
include!("bytes_reference.rs");

mod arena_required {
    fn pool() -> ::std::sync::Arc<::pbrs::DescriptorPool> {
        static POOL: ::std::sync::OnceLock<::std::sync::Arc<::pbrs::DescriptorPool>> =
            ::std::sync::OnceLock::new();
        POOL.get_or_init(|| {
            ::std::sync::Arc::new(
                ::pbrs::DescriptorPool::from_file_descriptor_set(
                    crate::proof::bytes_legacy::FILE_DESCRIPTOR_SET,
                )
                .unwrap(),
            )
        })
        .clone()
    }
    ::pbrs::impl_generated_message!(
        Legacy,
        LegacyView,
        LegacyMut,
        "proof.bytes_legacy.Required",
        pool()
    );
    impl ::core::default::Default for LegacyView<'_> {
        fn default() -> Self {
            static DEFAULT: ::std::sync::OnceLock<Legacy> = ::std::sync::OnceLock::new();
            LegacyView(::pbrs::gen_support::default_instance(&DEFAULT))
        }
    }
}

fn assert_values(native: &proof::bytes::Example, expected: &reference::proof::bytes::Example) {
    assert_eq!(
        reference::proof::bytes::Example::decode(Serialize::serialize(native).unwrap().as_slice())
            .unwrap(),
        *expected
    );
}

#[derive(Default)]
struct Segments {
    wire: Vec<u8>,
    shared: Vec<bytes::Bytes>,
}
impl pbrs::rt::WireOut for Segments {
    fn put_u8(&mut self, byte: u8) {
        self.wire.push(byte);
    }
    fn put_slice(&mut self, data: &[u8]) {
        self.wire.extend_from_slice(data);
    }
    fn put_shared(&mut self, data: &bytes::Bytes) {
        self.shared.push(data.clone());
        self.wire.extend_from_slice(data);
    }
    fn supports_shared(&self) -> bool {
        true
    }
}

fn typed_mut_shared_merge() {
    use bytes::Bytes;
    use proof::{bytes::Child, bytes_legacy::Required};
    for length in [1, 16, 65_536] {
        let expected = reference::proof::bytes::Child {
            payload: Bytes::from(vec![0xa7; length]),
        };
        let frame = Bytes::from(expected.encode_to_vec());
        let mut owned = Child::new();
        let mut backing = Child::new();
        pbrs::ClearAndParse::merge_from_bytes_shared(&mut owned, frame.clone()).unwrap();
        let mut view = pbrs::AsMut::as_mut(&mut backing);
        pbrs::ClearAndParse::merge_from_bytes_shared(&mut view, frame.clone()).unwrap();
        assert_eq!(
            Serialize::serialize(&owned).unwrap(),
            Serialize::serialize(&view).unwrap()
        );
        let left = owned.payload_shared();
        let right = view.payload_shared();
        assert!(within(left.as_bytes(), &frame));
        assert_eq!(left.as_bytes().as_ptr(), right.as_bytes().as_ptr());
        drop(view);
        drop(backing);
        drop(owned);
        drop(frame);
        assert_eq!(left.as_bytes(), vec![0xa7; length]);
        assert_eq!(right.as_bytes(), left.as_bytes());
    }

    let frame = Bytes::from(vec![0x0a, 1, 7, 0x0a, 3, 9]);
    let mut owned = Child::new();
    let mut backing = Child::new();
    owned.set_payload(Bytes::from_static(b"existing"));
    backing.set_payload(Bytes::from_static(b"existing"));
    let error =
        pbrs::ClearAndParse::merge_from_bytes_shared(&mut owned, frame.clone()).unwrap_err();
    let mut view = pbrs::AsMut::as_mut(&mut backing);
    let view_error =
        pbrs::ClearAndParse::merge_from_bytes_shared(&mut view, frame.clone()).unwrap_err();
    assert_eq!(error.to_string(), view_error.to_string());
    assert_eq!(owned.payload(), &[7]);
    assert_eq!(view.payload(), owned.payload());
    assert_eq!(
        Serialize::serialize(&owned).unwrap(),
        Serialize::serialize(&view).unwrap()
    );
    let retained = view.payload_shared();
    assert!(within(retained.as_bytes(), &frame));
    drop(view);
    drop(backing);
    drop(frame);
    assert_eq!(retained.as_bytes(), &[7]);

    let mut owned = Required::new();
    let mut backing = Required::new();
    for (frame, fails) in [
        (Bytes::new(), true),
        (Bytes::from_static(&[0x0a, 0]), false),
        (Bytes::new(), false),
        (Bytes::from(vec![0x0a, 1, 11, 0x0a, 3, 9]), true),
    ] {
        let result = pbrs::ClearAndParse::merge_from_bytes_shared(&mut owned, frame.clone());
        let mut view = pbrs::AsMut::as_mut(&mut backing);
        let view_result = pbrs::ClearAndParse::merge_from_bytes_shared(&mut view, frame.clone());
        assert_eq!(result.is_err(), fails);
        assert_eq!(view_result.is_err(), fails);
        assert_eq!(
            result.err().map(|error| error.to_string()),
            view_result.err().map(|error| error.to_string())
        );
        assert_eq!(owned.has_data(), view.has_data());
        assert_eq!(owned.data(), view.data());
        assert_eq!(
            Serialize::serialize(&owned).unwrap(),
            Serialize::serialize(&view).unwrap()
        );
    }
    assert_eq!(backing.data_shared().as_bytes(), &[11]);

    for partial in [&[][..], &[0x10, 1][..]] {
        let mut owned = Required::new();
        let mut backing = Required::new();
        let mut view = pbrs::AsMut::as_mut(&mut backing);
        pbrs::ClearAndParse::merge_from_bytes_dont_enforce_required(&mut owned, partial).unwrap();
        pbrs::ClearAndParse::merge_from_bytes_dont_enforce_required(&mut view, partial).unwrap();
        assert!(!owned.has_data());
        assert!(!view.has_data());
        assert_eq!(Serialize::serialize(&owned).unwrap(), partial);
        assert_eq!(Serialize::serialize(&view).unwrap(), partial);
        assert!(pbrs::ClearAndParse::merge_from_bytes(&mut owned, &[]).is_err());
        assert!(pbrs::ClearAndParse::merge_from_bytes(&mut view, &[]).is_err());
        pbrs::ClearAndParse::merge_from_bytes(&mut owned, &[0x0a, 1, 5]).unwrap();
        pbrs::ClearAndParse::merge_from_bytes(&mut view, &[0x0a, 1, 5]).unwrap();
        assert!(owned.has_data() && view.has_data());
        assert_eq!(
            Serialize::serialize(&owned).unwrap(),
            Serialize::serialize(&view).unwrap()
        );
        assert!(Serialize::serialize(&owned).unwrap().ends_with(partial));

        let mut owned = arena_required::Legacy::new();
        let mut backing = arena_required::Legacy::new();
        let mut view = pbrs::AsMut::as_mut(&mut backing);
        pbrs::ClearAndParse::merge_from_bytes_dont_enforce_required(&mut owned, partial).unwrap();
        pbrs::ClearAndParse::merge_from_bytes_dont_enforce_required(&mut view, partial).unwrap();
        assert_eq!(Serialize::serialize(&owned).unwrap(), partial);
        assert_eq!(Serialize::serialize(&view).unwrap(), partial);
        assert!(pbrs::ClearAndParse::merge_from_bytes(&mut owned, &[]).is_err());
        assert!(pbrs::ClearAndParse::merge_from_bytes(&mut view, &[]).is_err());
        pbrs::ClearAndParse::merge_from_bytes(&mut owned, &[0x0a, 1, 5]).unwrap();
        pbrs::ClearAndParse::merge_from_bytes(&mut view, &[0x0a, 1, 5]).unwrap();
        assert_eq!(
            Serialize::serialize(&owned).unwrap(),
            Serialize::serialize(&view).unwrap()
        );
        assert!(Serialize::serialize(&owned).unwrap().ends_with(partial));
    }
    println!("PASS bytes-case typed-mut-shared-merge");
}

fn main() {
    use bytes::Bytes;
    use proof::bytes::{Child, Cold, Example};
    let mut empty = Example::new();
    assert!(empty.chosen_shared_3().is_empty());
    assert!(empty.maybe_shared_opt_2().is_none());
    empty.set_maybe(Bytes::new());
    assert!(empty.has_maybe());
    assert!(empty.maybe_shared_opt_2().unwrap().is_empty());
    empty.clear_maybe();
    assert!(empty.maybe_shared_opt_2().is_none());
    assert!(empty.chunks_shared(0).is_none());
    assert!(empty.lookup_shared("missing").is_none());
    assert_eq!(empty.chosen_shared(), b"");
    assert_eq!(empty.chosen_shared_2(), b"");
    assert_eq!(empty.chosen_shared_opt_15(), b"");
    println!("PASS bytes-case empty-presence-collision-names");

    let defaults = proof::bytes_legacy::Defaults::new();
    assert!(!defaults.has_data());
    assert_eq!(defaults.data(), b"A\0\xff");
    assert_eq!(defaults.data_shared().as_bytes(), b"A\0\xff");
    assert!(defaults.data_shared_opt().is_none());
    assert_eq!(
        proof::bytes_legacy::Defaults::from_json(&defaults.to_json().unwrap()).unwrap(),
        defaults
    );
    assert_eq!(
        proof::bytes_legacy::Defaults::from_text(&defaults.to_text().unwrap()).unwrap(),
        defaults
    );
    let mut required = proof::bytes_legacy::Required::new();
    assert!(proof::bytes_legacy::Required::parse(&[]).is_err());
    required.set_data(Bytes::new());
    let required_wire = Serialize::serialize(&required).unwrap();
    assert_eq!(required_wire, [0x0a, 0]);
    assert!(
        proof::bytes_legacy::Required::parse(&required_wire)
            .unwrap()
            .has_data()
    );
    assert!(reference::proof::bytes_legacy::Required::decode(required_wire.as_slice()).is_ok());
    assert!(
        proof::bytes_legacy::Required::from_json(&required.to_json().unwrap())
            .unwrap()
            .has_data()
    );
    assert!(
        proof::bytes_legacy::Required::from_text(&required.to_text().unwrap())
            .unwrap()
            .has_data()
    );
    let mut cold = Cold::new();
    assert_eq!(cold.first(), b"");
    assert_eq!(cold.second_shared().as_bytes(), b"");
    cold.set_first(Bytes::from_static(b"cold"));
    assert_eq!(
        Cold::parse(&Serialize::serialize(&cold).unwrap())
            .unwrap()
            .first(),
        b"cold"
    );
    pbrs::Clear::clear(&mut cold);
    assert!(cold.first_shared().is_empty());
    println!("PASS bytes-case hot-cold-defaults-proto2-required");

    let payload = Bytes::from(vec![0x6d; 16]);
    let pointer = payload.as_ptr();
    empty.set_chosen(payload);
    let retained: pbrs::ProtoBytes = empty.chosen_shared_3();
    assert_eq!(retained.as_bytes().as_ptr(), pointer);
    let mut vector = Vec::with_capacity(64);
    vector.extend_from_slice(b"moved-vector");
    let pointer = vector.as_ptr();
    empty.set_chosen(vector);
    assert_eq!(empty.chosen_shared_3().as_bytes().as_ptr(), pointer);
    empty.set_chosen(std::borrow::Cow::Owned::<[u8]>(b"cow".to_vec()));
    assert_eq!(empty.chosen_shared_3().as_bytes(), b"cow");
    empty.set_marker(7);
    assert!(!empty.has_raw());
    empty.set_raw(Bytes::new());
    assert!(!empty.has_marker());
    assert!(empty.raw_shared_opt().unwrap().is_empty());
    empty.set_marker(8);
    assert!(empty.raw_shared_opt().is_none());
    println!("PASS bytes-case moved-setters-oneof-replacement");
    typed_mut_shared_merge();

    for len in [1, 16, 4095, 4096] {
        let reference = fixture(len);
        let frame = Bytes::from(reference.encode_to_vec());
        let native = Example::parse_bytes(frame.clone()).unwrap();
        let peer = reference::proof::bytes::Example::decode(frame.clone()).unwrap();
        assert_values(&native, &reference);
        let retained = [
            native.chosen_shared_3(),
            native.maybe_shared(),
            native.chunks_shared(0).unwrap(),
            native.lookup_shared("key").unwrap(),
            native.raw_shared(),
            native.nested().payload_shared(),
            native.children().get(0).unwrap().payload_shared(),
            native.nodes().get(7).unwrap().payload_shared(),
            native.imported().payload_shared(),
            native.imported().nested().payload_shared(),
            native
                .imported()
                .children()
                .get(0)
                .unwrap()
                .payload_shared(),
            native.type_shared(),
        ];
        for value in &retained {
            assert_eq!(value.as_bytes(), vec![0x9b; len]);
            assert!(
                within(value.as_bytes(), &frame),
                "shared payload copied: len={len}"
            );
        }
        assert_eq!(retained[0].as_bytes().as_ptr(), peer.chosen.as_ptr());
        if len < 4096 {
            assert!(
                !within(native.ordinary(), &frame),
                "default small field policy changed"
            );
        }
        let clone = native.clone();
        assert_eq!(
            clone.chosen_shared_3().as_bytes().as_ptr(),
            retained[0].as_bytes().as_ptr()
        );
        assert!(native.chunks_shared(1).unwrap().is_empty());
        assert!(native.lookup_shared("empty").unwrap().is_empty());
        let borrowed = Example::parse(&frame).unwrap();
        assert!(!within(borrowed.chosen_shared_3().as_bytes(), &frame));
        assert_values(&borrowed, &reference);
        let shared = retained[0].as_shared();
        drop(native);
        drop(clone);
        drop(frame);
        assert_eq!(shared.as_ref(), vec![0x9b; len]);
        println!("PASS bytes-case shared-borrowed-lifetime-{len}");
    }

    let reference = fixture(4096);
    let frame = Bytes::from(reference.encode_to_vec());
    let mut native = Example::parse_bytes(frame.clone()).unwrap();
    let retained = native.chosen_shared_3();
    native.set_chosen(b"changed".as_slice());
    assert_eq!(retained.as_bytes(), vec![0x9b; 4096]);
    native.chunks_mut().set(0, Bytes::from_static(b"chunk"));
    native
        .lookup_mut()
        .insert("key", Bytes::from_static(b"map"));
    native
        .nested_mut()
        .set_payload(Bytes::from_static(b"nested"));
    let decoded =
        reference::proof::bytes::Example::decode(Serialize::serialize(&native).unwrap().as_slice())
            .unwrap();
    assert_eq!(decoded.chosen.as_ref(), b"changed");
    assert_eq!(decoded.chunks[0].as_ref(), b"chunk");
    assert_eq!(decoded.lookup["key"].as_ref(), b"map");
    assert_eq!(decoded.nested.unwrap().payload.as_ref(), b"nested");
    println!("PASS bytes-case mutation-cache-invalidation");

    let reference = fixture(pbrs::rt::SHARED_SEND_THRESHOLD);
    let frame = Bytes::from(reference.encode_to_vec());
    let native = Example::parse_bytes(frame).unwrap();
    let mut segments = Segments::default();
    native.write_to(&mut segments);
    assert_eq!(segments.wire, Serialize::serialize(&native).unwrap());
    assert!(segments.shared.len() >= 3);
    assert!(
        segments
            .shared
            .iter()
            .any(|segment| segment.as_ptr() == native.chosen().as_ptr())
    );
    assert!(segments.shared.iter().any(
        |segment| segment.as_ptr() == native.lookup_shared("key").unwrap().as_bytes().as_ptr()
    ));
    // Keep large segmented-buffer ownership checks above in every run.
    // Interpreted memory checks can exercise format conversions separately
    // from hundreds of thousands of escaped text bytes.
    let small_formats = std::env::args().any(|argument| argument == "--small-formats");
    let format_reference = small_formats.then(|| fixture(16));
    let format_message = format_reference.as_ref().map(|value| {
        Example::parse_bytes(Bytes::from(value.encode_to_vec())).unwrap()
    });
    let format_message = format_message.as_ref().unwrap_or(&native);
    let format_reference = format_reference.as_ref().unwrap_or(&reference);
    assert_values(
        &Example::from_json(&format_message.to_json().unwrap()).unwrap(),
        format_reference,
    );
    assert_values(
        &Example::from_text(&format_message.to_text().unwrap()).unwrap(),
        format_reference,
    );
    println!("SCOPE bytes-format-payload={} segmented-payload={}",
        if small_formats { 16 } else { pbrs::rt::SHARED_SEND_THRESHOLD },
        pbrs::rt::SHARED_SEND_THRESHOLD);
    println!("PASS bytes-case json-text-segmented-encoding");

    let mut flat = proof::bytes::Flat::new();
    flat.set_chosen(Bytes::from_static(b"flat"));
    flat.set_maybe(Bytes::new());
    flat.chunks_mut().push(Bytes::from_static(b"chunk"));
    flat.lookup_mut().insert("key", Bytes::from_static(b"map"));
    flat.set_raw(Bytes::from_static(b"\xff"));
    assert_eq!(
        proof::bytes::Flat::from_json(&flat.to_json().unwrap()).unwrap(),
        flat
    );
    assert_eq!(
        proof::bytes::Flat::from_text(&flat.to_text().unwrap()).unwrap(),
        flat
    );
    println!("PASS bytes-case typed-json-text-containers");

    let mut wire = Vec::new();
    for index in 0..20_u8 {
        let mut entry = Vec::new();
        pbrs::rt::encode_len_field(&mut entry, 1, format!("key{}", index % 17).as_bytes());
        pbrs::rt::encode_len_field(&mut entry, 2, &[index; 16]);
        pbrs::rt::encode_len_field(&mut wire, 4, &entry);
    }
    let frame = Bytes::from(wire);
    let mut flat = proof::bytes::Flat::parse_bytes(frame.clone()).unwrap();
    let reference = reference::proof::bytes::Flat::decode(frame.clone()).unwrap();
    let retained = flat.lookup_shared("key2").unwrap();
    assert_eq!(retained.as_bytes(), reference.lookup["key2"].as_ref());
    assert_eq!(retained.as_bytes(), &[19; 16]);
    assert!(within(retained.as_bytes(), &frame));
    assert!(flat.lookup_shared("missing").is_none());
    assert!(flat.lookup_mut().remove("key16"));
    assert!(flat.lookup_shared("key16").is_none());
    assert_eq!(flat.lookup_shared("key2").unwrap().as_bytes(), &[19; 16]);
    assert_eq!(retained.as_bytes(), &[19; 16]);
    println!("PASS bytes-case indexed-map-last-wins-sharing");

    let mut child = Child::new();
    child.set_payload(Bytes::from_static(b"\xff\0"));
    assert_eq!(
        Child::parse(&Serialize::serialize(&child).unwrap())
            .unwrap()
            .payload(),
        b"\xff\0"
    );
    for malformed in [&[0x0a, 3, 1][..], &[0x0a, 0x80][..]] {
        assert!(Child::parse(malformed).is_err());
        assert!(reference::proof::bytes::Child::decode(malformed).is_err());
    }
    println!("PASS bytes-case arbitrary-bytes-malformed-wire");
    if REPORT_SELECTORS {
        print!(
            "{}",
            include_str!(concat!(env!("OUT_DIR"), "/bytes-selectors.txt"))
        );
    }
}
