//! The published dynamic sink interface and its sized contiguous helper.

use bytes::{Bytes, BytesMut};
use pbrs::WireOut;

#[derive(Default)]
struct SharedSink {
    bytes: Vec<u8>,
    shared_calls: usize,
    last_shared: Bytes,
}

impl WireOut for SharedSink {
    fn put_u8(&mut self, byte: u8) {
        self.bytes.push(byte);
    }

    fn put_slice(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn put_shared(&mut self, bytes: &Bytes) {
        self.shared_calls += 1;
        self.last_shared = bytes.clone();
        self.bytes.extend_from_slice(bytes);
    }

    fn supports_shared(&self) -> bool {
        true
    }
}

fn emit_dynamic(sink: &mut dyn WireOut) -> bool {
    sink.push(10);
    sink.extend_from_slice(&[3]);
    sink.put_shared(&Bytes::from_static(b"rpc"));
    sink.supports_shared()
}

#[test]
fn dynamic_sinks_preserve_default_and_overridden_dispatch() {
    let mut copied = Vec::new();
    assert!(!emit_dynamic(&mut copied));
    assert_eq!(copied, b"\x0a\x03rpc");

    let mut shared = SharedSink::default();
    assert!(emit_dynamic(&mut shared));
    assert_eq!(shared.bytes, copied);
    assert_eq!(shared.shared_calls, 1);
    assert_eq!(shared.last_shared.as_ref(), b"rpc");
}

#[derive(Default)]
struct ContiguousSink {
    bytes: BytesMut,
    contiguous_calls: usize,
}

impl WireOut for ContiguousSink {
    fn put_u8(&mut self, byte: u8) {
        self.bytes.extend_from_slice(&[byte]);
    }

    fn put_slice(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn write_contiguous<R>(&mut self, write: impl FnOnce(&mut BytesMut) -> R) -> R {
        self.contiguous_calls += 1;
        write(&mut self.bytes)
    }
}

fn append_contiguous<W: WireOut>(sink: &mut W) -> u32 {
    sink.write_contiguous(|bytes| {
        bytes.extend_from_slice(b"rpc");
        17
    })
}

#[test]
fn sized_sinks_keep_generic_contiguous_writes_and_return_values() {
    let mut copied = vec![b'!'];
    assert_eq!(append_contiguous(&mut copied), 17);
    assert_eq!(copied, b"!rpc");

    let mut direct = ContiguousSink::default();
    direct.bytes.extend_from_slice(b"!");
    assert_eq!(append_contiguous(&mut direct), 17);
    assert_eq!(direct.bytes.as_ref(), b"!rpc");
    assert_eq!(direct.contiguous_calls, 1);
}

#[cfg(feature = "conformance")]
#[test]
fn generated_cached_child_shares_the_original_wire() -> Result<(), Box<dyn std::error::Error>> {
    use pbrs::{Parse, Serialize, rt, wkt::PbValue};

    for size in [32, rt::SHARED_SEND_THRESHOLD] {
        let mut child = Vec::new();
        // Unknown field in Struct. The cached child must preserve these bytes.
        rt::encode_len_field(&mut child, 9, &vec![0x5a; size]);
        let mut frame = Vec::new();
        // Value.struct_value, a singular lazy child in a oneof.
        rt::encode_len_header(&mut frame, 5, child.len() as u64);
        let start = frame.len();
        frame.extend_from_slice(&child);
        let input = Bytes::from(frame);
        let original = input.slice(start..);
        let value = PbValue::parse_bytes(input.clone())?;
        assert!(value.has_struct_value());

        let mut sink = SharedSink::default();
        value.encode(&mut sink)?;
        assert_eq!(sink.bytes, input);
        if size == rt::SHARED_SEND_THRESHOLD {
            assert_eq!(value.shared_bytes_len(), original.len());
            assert_eq!(sink.shared_calls, 1);
            assert_eq!(sink.last_shared.as_ptr(), original.as_ptr());
            assert_eq!(sink.last_shared, original);
        } else {
            assert_eq!(value.shared_bytes_len(), 0);
            assert_eq!(sink.shared_calls, 0);
        }
    }
    Ok(())
}
