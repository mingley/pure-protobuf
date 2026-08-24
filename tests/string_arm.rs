//! Isolate name_4kib vs blob_4kib using shipped Parse/Serialize.

use pbrs::gencode::TestAllTypesProto3;
use pbrs::rt::{require_utf8, LazyBytes, LazyStr, Wire};
use pbrs::{Parse, Serialize};
use std::time::Instant;

fn median_ns(samples: usize, iters: u32, mut f: impl FnMut()) -> f64 {
    let mut xs: Vec<f64> = (0..samples)
        .map(|_| {
            for _ in 0..iters / 10 {
                f();
            }
            let t = Instant::now();
            for _ in 0..iters {
                f();
                std::hint::black_box(());
            }
            t.elapsed().as_secs_f64() * 1e9 / f64::from(iters)
        })
        .collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[samples / 2]
}

#[test]
fn fourk_string_vs_bytes_parse_components() {
    let payload = vec![b'x'; 4096];
    let mut smsg = TestAllTypesProto3::new();
    smsg.set_optional_string(std::str::from_utf8(&payload).unwrap());
    let sw = Serialize::serialize(&smsg).expect("string wire");
    let mut bmsg = TestAllTypesProto3::new();
    bmsg.set_optional_bytes(payload.as_slice());
    let bw = Serialize::serialize(&bmsg).expect("bytes wire");
    assert_eq!(sw.len(), bw.len());

    let iters = 20_000u32;
    let samples = 7usize;
    let parse_s = median_ns(samples, iters, || {
        let _ = TestAllTypesProto3::parse(&sw).unwrap();
    });
    let parse_b = median_ns(samples, iters, || {
        let _ = TestAllTypesProto3::parse(&bw).unwrap();
    });
    let utf8 = median_ns(samples, iters, || {
        let _ = std::str::from_utf8(&payload).unwrap();
    });
    let req = median_ns(samples, iters, || {
        require_utf8(&payload).unwrap();
    });
    let ensure = median_ns(samples, iters, || {
        let mut slot = None;
        let _ = Wire::ensure(&mut slot, &sw);
    });
    let parse_span = median_ns(samples, iters, || {
        let mut slot = None;
        // skip 3-byte tag/len prefix: 0x0a 0x80 0x20, payload at 3..4099
        let _ = LazyStr::from_parse_span(&mut slot, &sw, 3, sw.len()).unwrap();
    });
    let bytes_wire = median_ns(samples, iters, || {
        let mut slot = None;
        let w = Wire::ensure(&mut slot, &bw).window(3, bw.len());
        let _ = LazyBytes::from_wire(w);
    });

    eprintln!(
        "string_arm TAT parse string={parse_s:.1} bytes={parse_b:.1} delta={:.1} utf8={utf8:.1} require_utf8={req:.1} ensure={ensure:.1} from_parse_span={parse_span:.1} bytes_from_wire={bytes_wire:.1} wire_len={}",
        parse_s - parse_b,
        sw.len()
    );
    assert!(parse_s > 0.0 && parse_b > 0.0, "shipped Parse must run");
}
