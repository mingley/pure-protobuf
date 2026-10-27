//! The published dynamic sink interface and its sized contiguous helper.

use bytes::{Bytes, BytesMut};
use pbrs::WireOut;

#[derive(Default)]
struct SharedSink {
    bytes: Vec<u8>,
    shared_calls: usize,
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
