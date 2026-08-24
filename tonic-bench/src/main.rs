//! Codec survey: `Serialize::encode` into `BytesMut` vs prost `Message::encode`
//! vs v4 `Serialize::serialize` (Arena+FFI, no EncodeBuf). Same-process, no
//! transport. Not kernel `./bench`. Not in CI.
//!
//! `hello` / `hello_4kib` stay the published 1-string rows. The rest are
//! common unary shapes from `proto/codec_cases.proto` (specialized gencode,
//! not TestAllTypes).

use bytes::BytesMut;
use pbrs::{Parse, Serialize};
use protobuf::{Parse as V4Parse, Serialize as V4Serialize};
use protobuf_tonic::hello::HelloRequest as PbrsHello;
use std::time::Instant;

mod helloworld {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/prost/helloworld.rs"));
}
mod pbrs_cases {
    #![allow(dead_code, unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/pbrs/codec_cases.rs"));
}
mod prost_cases {
    #![allow(dead_code, unused, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/prost/cases.rs"));
}
mod v4_cases {
    #![allow(clippy::all, dead_code, unused, nonstandard_style)]
    include!(concat!(env!("OUT_DIR"), "/v4/generated.rs"));
}

use helloworld::HelloRequest as ProstHello;

fn median_ns<F, R>(samples: usize, iters: u32, mut f: F) -> f64
where
    F: FnMut() -> R,
{
    let mut xs: Vec<f64> = (0..samples).map(|_| bench_ns(iters, &mut f)).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[samples / 2]
}

fn bench_ns<F, R>(iters: u32, mut f: F) -> f64
where
    F: FnMut() -> R,
{
    for _ in 0..iters / 10 {
        std::hint::black_box(f());
    }
    let t = Instant::now();
    for _ in 0..iters {
        std::hint::black_box(f());
    }
    t.elapsed().as_secs_f64() * 1e9 / f64::from(iters)
}

fn timer_budget(payload: usize) -> (u32, usize) {
    if payload >= 32_000 {
        (4_000, 9)
    } else {
        (40_000, 15)
    }
}

struct Row {
    name: &'static str,
    payload: usize,
    pbrs_enc: f64,
    pbrs_dec: f64,
    prost_enc: f64,
    prost_dec: f64,
    v4_enc: f64,
    v4_dec: f64,
}

fn run<P, R, V>(name: &'static str, pbrs: &P, prost: &R, v4: &V, check_wire: bool) -> Row
where
    P: Parse + Serialize,
    R: prost::Message + Default,
    V: V4Parse + V4Serialize,
{
    let pbrs_wire = Serialize::serialize(pbrs).expect("pbrs wire");
    let mut prost_wire = Vec::new();
    prost::Message::encode(prost, &mut prost_wire).expect("prost wire");
    let v4_wire = V4Serialize::serialize(v4).expect("v4 wire");
    if check_wire {
        assert_eq!(pbrs_wire, prost_wire, "{name}: pbrs vs prost wire");
        assert_eq!(pbrs_wire, v4_wire, "{name}: pbrs vs v4 wire");
    } else {
        let _ = R::decode(pbrs_wire.as_slice()).expect("{name}: prost parses pbrs");
        let _ = V::parse(&pbrs_wire).expect("{name}: v4 parses pbrs");
    }
    let payload = pbrs_wire.len();
    let (iters, samples) = timer_budget(payload);
    let mut dst = BytesMut::new();
    let pbrs_enc = median_ns(samples, iters, || {
        dst.clear();
        Serialize::encode(pbrs, &mut dst).expect("pbrs encode");
        dst.len()
    });
    let pbrs_dec = median_ns(samples, iters, || P::parse(&pbrs_wire).expect("pbrs parse"));
    let prost_enc = median_ns(samples, iters, || {
        dst.clear();
        prost::Message::encode(prost, &mut dst).expect("prost encode");
        dst.len()
    });
    let prost_dec = median_ns(samples, iters, || {
        R::decode(pbrs_wire.as_slice()).expect("prost decode")
    });
    let v4_enc = median_ns(samples, iters, || {
        V4Serialize::serialize(v4).expect("v4 encode").len()
    });
    let v4_dec = median_ns(samples, iters, || V::parse(&pbrs_wire).expect("v4 parse"));
    Row {
        name,
        payload,
        pbrs_enc,
        pbrs_dec,
        prost_enc,
        prost_dec,
        v4_enc,
        v4_dec,
    }
}

fn pbrs_hello(name: &str) -> PbrsHello {
    let mut m = PbrsHello::new();
    m.set_name(name);
    m
}

fn prost_hello(name: &str) -> ProstHello {
    ProstHello {
        name: name.to_string(),
    }
}

fn meta_pbrs() -> pbrs_cases::Meta {
    let mut m = pbrs_cases::Meta::new();
    m.set_id(7);
    m.set_ts(1_700_000_000);
    m.set_trace("abc123");
    m
}

fn meta_prost() -> prost_cases::Meta {
    prost_cases::Meta {
        id: 7,
        ts: 1_700_000_000,
        trace: "abc123".into(),
    }
}

fn meta_v4() -> v4_cases::Meta {
    let mut m = v4_cases::Meta::new();
    m.set_id(7);
    m.set_ts(1_700_000_000);
    m.set_trace("abc123");
    m
}

fn pbrs_node(depth: i32) -> pbrs_cases::Node {
    let mut n = pbrs_cases::Node::new();
    n.set_n(depth);
    if depth > 1 {
        n.set_child(pbrs_node(depth - 1));
    }
    n
}

fn prost_node(depth: i32) -> prost_cases::Node {
    prost_cases::Node {
        n: depth,
        child: if depth > 1 {
            Some(Box::new(prost_node(depth - 1)))
        } else {
            None
        },
    }
}

fn v4_node(depth: i32) -> v4_cases::Node {
    let mut n = v4_cases::Node::new();
    n.set_n(depth);
    if depth > 1 {
        n.set_child(v4_node(depth - 1));
    }
    n
}

fn print_table(title: &str, rows: &[Row]) {
    println!("{title}");
    println!();
    println!("| case | payload | pbrs enc/dec | prost enc/dec | v4 enc/dec | vs prost | vs v4 |");
    println!("|---|---:|---:|---:|---:|---|---|");
    for r in rows {
        let vs_prost = if r.pbrs_enc + r.pbrs_dec < r.prost_enc + r.prost_dec {
            "win"
        } else {
            "loss"
        };
        let vs_v4 = if r.pbrs_enc + r.pbrs_dec < r.v4_enc + r.v4_dec {
            "win"
        } else {
            "loss"
        };
        println!(
            "| {} | {} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {vs_prost} | {vs_v4} |",
            r.name, r.payload, r.pbrs_enc, r.pbrs_dec, r.prost_enc, r.prost_dec, r.v4_enc, r.v4_dec
        );
    }
    println!();
}

fn main() {
    let hello_short = "ada";
    let hello_4k = "x".repeat(4096);
    let name_80 = "x".repeat(80);
    let blob_32 = vec![0x5a; 32];
    let blob_4k = vec![0x5a; 4096];
    let blob_64k = vec![0x5a; 64 * 1024];

    let p_hello = pbrs_hello(hello_short);
    let r_hello = prost_hello(hello_short);
    // v4 has no helloworld here; Name is the same 1-string shape.
    let mut v_name = v4_cases::Name::new();
    v_name.set_name(hello_short);

    let p_hello4 = pbrs_hello(&hello_4k);
    let r_hello4 = prost_hello(&hello_4k);

    let mut p_name = pbrs_cases::Name::new();
    p_name.set_name(hello_short);
    let r_name = prost_cases::Name {
        name: hello_short.into(),
    };

    let mut p_name80 = pbrs_cases::Name::new();
    p_name80.set_name(name_80.as_str());
    let r_name80 = prost_cases::Name {
        name: name_80.clone(),
    };
    let mut v_name80 = v4_cases::Name::new();
    v_name80.set_name(name_80.as_str());

    let mut p_name4k = pbrs_cases::Name::new();
    p_name4k.set_name(hello_4k.as_str());
    let r_name4k = prost_cases::Name {
        name: hello_4k.clone(),
    };
    let mut v_name4k = v4_cases::Name::new();
    v_name4k.set_name(hello_4k.as_str());

    let mut p_id = pbrs_cases::Id::new();
    p_id.set_id(7);
    let r_id = prost_cases::Id { id: 7 };
    let mut v_id = v4_cases::Id::new();
    v_id.set_id(7);

    let mut p_sc = pbrs_cases::Scalars::new();
    p_sc.set_id(7);
    p_sc.set_seq(3);
    p_sc.set_ok(true);
    p_sc.set_status(1);
    p_sc.set_ts(1_700_000_000);
    p_sc.set_lat(1.5);
    let r_sc = prost_cases::Scalars {
        id: 7,
        seq: 3,
        ok: true,
        status: 1,
        ts: 1_700_000_000,
        lat: 1.5,
    };
    let mut v_sc = v4_cases::Scalars::new();
    v_sc.set_id(7);
    v_sc.set_seq(3);
    v_sc.set_ok(true);
    v_sc.set_status(v4_cases::Status::Ok);
    v_sc.set_ts(1_700_000_000);
    v_sc.set_lat(1.5);

    let mut p_b32 = pbrs_cases::Blob::new();
    p_b32.set_payload(blob_32.as_slice());
    let r_b32 = prost_cases::Blob {
        payload: blob_32.clone(),
    };
    let mut v_b32 = v4_cases::Blob::new();
    v_b32.set_payload(blob_32.as_slice());

    let mut p_b4k = pbrs_cases::Blob::new();
    p_b4k.set_payload(blob_4k.as_slice());
    let r_b4k = prost_cases::Blob {
        payload: blob_4k.clone(),
    };
    let mut v_b4k = v4_cases::Blob::new();
    v_b4k.set_payload(blob_4k.as_slice());

    let mut p_b64 = pbrs_cases::Blob::new();
    p_b64.set_payload(blob_64k.as_slice());
    let r_b64 = prost_cases::Blob {
        payload: blob_64k.clone(),
    };
    let mut v_b64 = v4_cases::Blob::new();
    v_b64.set_payload(blob_64k.as_slice());

    let mut p_env = pbrs_cases::Envelope::new();
    p_env.set_meta(meta_pbrs());
    p_env.set_body("hello body");
    let r_env = prost_cases::Envelope {
        meta: Some(meta_prost()),
        body: "hello body".into(),
    };
    let mut v_env = v4_cases::Envelope::new();
    v_env.set_meta(meta_v4());
    v_env.set_body("hello body");

    let p_nest = pbrs_node(4);
    let r_nest = prost_node(4);
    let v_nest = v4_node(4);

    let mut p_ids16 = pbrs_cases::Ids::new();
    p_ids16.set_ids(0..16);
    let r_ids16 = prost_cases::Ids {
        ids: (0..16).collect(),
    };
    let mut v_ids16 = v4_cases::Ids::new();
    for i in 0..16 {
        v_ids16.ids_mut().push(i);
    }

    let mut p_ids256 = pbrs_cases::Ids::new();
    p_ids256.set_ids(0..256);
    let r_ids256 = prost_cases::Ids {
        ids: (0..256).collect(),
    };
    let mut v_ids256 = v4_cases::Ids::new();
    for i in 0..256 {
        v_ids256.ids_mut().push(i);
    }

    let tags4 = ["alpha", "beta", "gamma", "delta"];
    let mut p_tags4 = pbrs_cases::Tags::new();
    for t in tags4 {
        p_tags4.tags_mut().push(t);
    }
    let r_tags4 = prost_cases::Tags {
        tags: tags4.iter().map(|s| (*s).to_string()).collect(),
    };
    let mut v_tags4 = v4_cases::Tags::new();
    for t in tags4 {
        v_tags4.tags_mut().push(t);
    }

    let tag32: Vec<String> = (0..32).map(|i| format!("t{i:02}")).collect();
    let mut p_tags32 = pbrs_cases::Tags::new();
    for t in &tag32 {
        p_tags32.tags_mut().push(t.as_str());
    }
    let r_tags32 = prost_cases::Tags {
        tags: tag32.clone(),
    };
    let mut v_tags32 = v4_cases::Tags::new();
    for t in &tag32 {
        v_tags32.tags_mut().push(t.as_str());
    }

    let hdrs = [
        ("content-type", "application/json"),
        ("accept", "*/*"),
        ("user-agent", "bench"),
        ("x-request-id", "abc"),
        ("x-trace", "1"),
        ("host", "example"),
        ("authorization", "none"),
        ("cache-control", "no-store"),
    ];
    let mut p_map = pbrs_cases::Headers::new();
    for (k, v) in hdrs {
        p_map.h_mut().insert(k, v);
    }
    let r_map = prost_cases::Headers {
        h: hdrs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
    };
    let mut v_map = v4_cases::Headers::new();
    for (k, v) in hdrs {
        v_map.h_mut().insert(k, v);
    }

    let mut p_ok = pbrs_cases::PbResult::new();
    p_ok.set_ok("fine");
    let r_ok = prost_cases::Result {
        kind: Some(prost_cases::result::Kind::Ok("fine".into())),
    };
    let mut v_ok = v4_cases::Result::new();
    v_ok.set_ok("fine");

    let mut p_rpc = pbrs_cases::Rpc::new();
    p_rpc.set_id(99);
    p_rpc.set_method("Get");
    p_rpc.set_path("/v1/items");
    p_rpc.set_user("ada");
    p_rpc.set_meta(meta_pbrs());
    p_rpc.set_ids(0..8);
    for t in tags4 {
        p_rpc.tags_mut().push(t);
    }
    for (k, v) in hdrs.iter().take(4) {
        p_rpc.headers_mut().insert(*k, *v);
    }
    p_rpc.set_extra(&b"extra"[..]);
    let r_rpc = prost_cases::Rpc {
        id: 99,
        method: "Get".into(),
        path: "/v1/items".into(),
        user: "ada".into(),
        meta: Some(meta_prost()),
        ids: (0..8).collect(),
        tags: tags4.iter().map(|s| (*s).to_string()).collect(),
        headers: hdrs
            .iter()
            .take(4)
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        extra: b"extra".to_vec(),
    };
    let mut v_rpc = v4_cases::Rpc::new();
    v_rpc.set_id(99);
    v_rpc.set_method("Get");
    v_rpc.set_path("/v1/items");
    v_rpc.set_user("ada");
    v_rpc.set_meta(meta_v4());
    for i in 0..8 {
        v_rpc.ids_mut().push(i);
    }
    for t in tags4 {
        v_rpc.tags_mut().push(t);
    }
    for (k, v) in hdrs.iter().take(4) {
        v_rpc.headers_mut().insert(*k, *v);
    }
    v_rpc.set_extra(&b"extra"[..]);

    let mut p_sparse = pbrs_cases::Rpc::new();
    p_sparse.set_id(99);
    let r_sparse = prost_cases::Rpc {
        id: 99,
        ..Default::default()
    };
    let mut v_sparse = v4_cases::Rpc::new();
    v_sparse.set_id(99);

    // hello has no v4 twin in this crate; Name is the same 1-string shape.
    let published = [run("hello", &p_hello, &r_hello, &v_name, true), {
        let mut v = v4_cases::Name::new();
        v.set_name(hello_4k.as_str());
        run("hello_4kib", &p_hello4, &r_hello4, &v, true)
    }];

    let survey = [
        run(
            "empty",
            &pbrs_cases::Empty::new(),
            &prost_cases::Empty {},
            &v4_cases::Empty::new(),
            true,
        ),
        run("id", &p_id, &r_id, &v_id, true),
        run("scalars", &p_sc, &r_sc, &v_sc, true),
        run("name_short", &p_name, &r_name, &v_name, true),
        run("name_80", &p_name80, &r_name80, &v_name80, true),
        run("name_4kib", &p_name4k, &r_name4k, &v_name4k, true),
        run("blob_32", &p_b32, &r_b32, &v_b32, true),
        run("blob_4kib", &p_b4k, &r_b4k, &v_b4k, true),
        run("blob_64kib", &p_b64, &r_b64, &v_b64, true),
        run("envelope", &p_env, &r_env, &v_env, true),
        run("nest_d4", &p_nest, &r_nest, &v_nest, true),
        run("packed_16", &p_ids16, &r_ids16, &v_ids16, true),
        run("packed_256", &p_ids256, &r_ids256, &v_ids256, true),
        run("tags_4", &p_tags4, &r_tags4, &v_tags4, true),
        run("tags_32", &p_tags32, &r_tags32, &v_tags32, true),
        run("map_8", &p_map, &r_map, &v_map, false),
        run("oneof_ok", &p_ok, &r_ok, &v_ok, true),
        run("rpc_mixed", &p_rpc, &r_rpc, &v_rpc, false),
        run("rpc_sparse", &p_sparse, &r_sparse, &v_sparse, true),
    ];

    println!("# Codec survey (encode into BytesMut; v4 serialize is Arena+FFI)");
    println!("iters=40000 samples=15 except payload>=32KiB (4000x9). median. release thin-LTO.");
    println!("pbrs vs prost vs crates.io protobuf 4.35.1-release (upb).");
    println!("map_8 / rpc_mixed skip byte-equal (HashMap order); cross-parse still checked.");
    println!();
    print_table("## Published 1-string (hello.proto)", &published);
    print_table("## Common shapes (codec_cases.proto)", &survey);

    let mut failed = false;
    for r in survey.iter() {
        if r.name != "name_4kib" && r.name != "blob_4kib" {
            continue;
        }
        let ours = r.pbrs_enc + r.pbrs_dec;
        let prost = r.prost_enc + r.prost_dec;
        if ours >= prost {
            eprintln!(
                "perf gate failed: {} combined {:.1} vs prost {:.1}",
                r.name, ours, prost
            );
            failed = true;
        }
    }
    if failed {
        std::process::exit(1);
    }
}
