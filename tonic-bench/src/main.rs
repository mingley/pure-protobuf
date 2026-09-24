//! Codec survey: `Serialize::encode` into `BytesMut` vs prost `Message::encode`
//! vs v4 `Serialize::serialize` (Arena+FFI, no EncodeBuf). Same-process, no
//! transport. Not kernel `./bench`. Not in CI.
//!
//! `hello` / `hello_4kib` stay the published 1-string rows. The rest are
//! common unary shapes from `proto/codec_cases.proto` (specialized gencode,
//! not TestAllTypes).

use bytes::BytesMut;
use pbrs::testdata::{Address as PbrsAddress, Person as PbrsPerson};
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
mod v4_person {
    #![allow(clippy::all, dead_code, unused, nonstandard_style)]
    mod internal_do_not_use_person {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../rust_out_person/src/person.u.pb.rs"
        ));
    }
    pub(crate) use internal_do_not_use_person::*;
}

use helloworld::HelloRequest as ProstHello;

#[derive(Clone, PartialEq, prost::Message)]
struct ProstAddress {
    #[prost(string, tag = "1")]
    city: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ProstPerson {
    #[prost(int32, tag = "1")]
    id: i32,
    #[prost(string, tag = "2")]
    name: String,
    #[prost(string, optional, tag = "3")]
    email: Option<String>,
    #[prost(string, repeated, tag = "4")]
    tags: Vec<String>,
    #[prost(map = "string, int32", tag = "5")]
    scores: std::collections::HashMap<String, i32>,
    #[prost(message, optional, tag = "6")]
    address: Option<ProstAddress>,
    #[prost(map = "string, int32", tag = "16")]
    extras: std::collections::HashMap<String, i32>,
}

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

fn median_first_encode_ns<M, F, E, O>(
    samples: usize,
    iters: u32,
    mut prepare: F,
    mut encode: E,
) -> f64
where
    F: FnMut() -> M,
    E: FnMut(&M) -> O,
{
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        for _ in 0..iters / 10 {
            let message = prepare();
            std::hint::black_box(encode(&message));
        }
        let messages: Vec<M> = (0..iters).map(|_| prepare()).collect();
        let start = Instant::now();
        for message in &messages {
            std::hint::black_box(encode(message));
        }
        times.push(start.elapsed().as_secs_f64() * 1e9 / f64::from(iters));
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    times[samples / 2]
}

fn first_encode_budget<P, R, V>(iters: u32, payload: usize) -> u32 {
    let estimated_bytes = payload
        .saturating_add(
            std::mem::size_of::<P>()
                .max(std::mem::size_of::<R>())
                .max(std::mem::size_of::<V>()),
        )
        .max(1);
    let by_memory = u32::try_from(32 * 1024 * 1024 / estimated_bytes)
        .expect("32 MiB divided by at least one byte fits u32");
    assert!(iters > 0, "first encode needs a positive iteration count");
    assert!(
        by_memory > 0,
        "first encode exceeds the 32 MiB preparation budget"
    );
    iters.min(10_000).min(by_memory)
}

fn median_mutated_encode_ns<M, F, U, E, O>(
    samples: usize,
    iters: u32,
    mut prepare: F,
    mut mutate: U,
    mut encode: E,
) -> f64
where
    F: FnMut() -> M,
    U: FnMut(&mut M, i32),
    E: FnMut(&M) -> O,
{
    assert!(
        samples > 0 && iters > 0,
        "mutation samples and iters must be positive"
    );
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mut message = prepare();
        let mut last_id = 43;
        let mut step = || {
            last_id = if last_id == 42 { 43 } else { 42 };
            mutate(&mut message, last_id);
            std::hint::black_box(encode(&message));
        };
        for _ in 0..iters / 10 {
            step();
        }
        let start = Instant::now();
        for _ in 0..iters {
            step();
        }
        times.push(start.elapsed().as_secs_f64() * 1e9 / f64::from(iters));
    }
    times.sort_by(|a, b| a.partial_cmp(b).expect("finite mutation timings"));
    times[samples / 2]
}

fn assert_person_mutation_output(
    codec: &str,
    actual: &[u8],
    expected_wire: &[u8],
    expected_id: i32,
) {
    assert_eq!(actual, expected_wire, "person mutation: {codec} wire");
    let parsed = PbrsPerson::parse(actual).expect("person mutation must reparse");
    let expected = PbrsPerson::parse(expected_wire).expect("checked person reference must parse");
    assert_eq!(parsed.id(), expected_id, "person mutation: {codec} id");
    assert_eq!(parsed, expected, "person mutation: {codec} fields");
    let prost: ProstPerson = prost::Message::decode(actual).expect("prost reparses mutated person");
    assert_eq!(prost.id, expected_id, "person mutation: {codec} prost id");
    let v4 = v4_person::Person::parse(actual).expect("v4 reparses mutated person");
    assert_eq!(v4.id(), expected_id, "person mutation: {codec} v4 id");
}

fn assert_same_output<P: Parse + PartialEq>(
    name: &str,
    codec: &str,
    actual: &[u8],
    expected_wire: &[u8],
    expected: &P,
    byte_stable: bool,
) {
    if byte_stable {
        assert_eq!(actual, expected_wire, "{name}: {codec} wire");
    } else {
        let parsed = P::parse(actual).expect("pbrs parses comparator wire");
        assert!(&parsed == expected, "{name}: {codec} decoded fields");
    }
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
    pbrs_enc: f64,       // cached encode (pre-warmed length/canonical cache)
    pbrs_fresh_enc: f64, // direct first encode after parse, before canonical cache
    first_iters: u32,
    pbrs_dec: f64,   // parse only (message dropped)
    pbrs_touch: f64, // parse-and-touch (reading parsed fields)
    prost_enc: f64,
    prost_first_enc: f64,
    prost_dec: f64,
    prost_touch: f64,
    v4_enc: f64,
    v4_first_enc: f64,
    v4_dec: f64,
    v4_touch: f64,
}

fn run<P, R, V, TP, TR, TV>(
    name: &'static str,
    pbrs: &P,
    prost: &R,
    v4: &V,
    check_wire: bool,
    touch_pbrs: TP,
    touch_prost: TR,
    touch_v4: TV,
) -> Row
where
    P: Parse + Serialize + PartialEq,
    R: prost::Message + Default,
    V: V4Parse + V4Serialize,
    TP: Fn(&P) -> usize,
    TR: Fn(&R) -> usize,
    TV: Fn(&V) -> usize,
{
    let pbrs_wire = Serialize::serialize(pbrs).expect("pbrs wire");
    let mut prost_wire = Vec::new();
    prost::Message::encode(prost, &mut prost_wire).expect("prost wire");
    let v4_wire = V4Serialize::serialize(v4).expect("v4 wire");
    let parsed_pbrs = P::parse(&pbrs_wire).expect("pbrs parses pbrs wire");
    let parsed_prost = R::decode(pbrs_wire.as_slice()).expect("prost parses pbrs wire");
    let parsed_v4 = V::parse(&pbrs_wire).expect("v4 parses pbrs wire");
    assert_same_output(
        name,
        "prost",
        &prost_wire,
        &pbrs_wire,
        &parsed_pbrs,
        check_wire,
    );
    assert_same_output(name, "v4", &v4_wire, &pbrs_wire, &parsed_pbrs, check_wire);
    let mut dst = BytesMut::new();
    let first_pbrs = P::parse(&pbrs_wire).expect("pbrs first precheck parse");
    Serialize::encode(&first_pbrs, &mut dst).expect("pbrs first wire");
    assert_same_output(
        name,
        "pbrs first",
        &dst,
        &pbrs_wire,
        &parsed_pbrs,
        check_wire,
    );
    dst.clear();
    prost::Message::encode(&parsed_prost, &mut dst).expect("prost first wire");
    assert_same_output(
        name,
        "prost first",
        &dst,
        &pbrs_wire,
        &parsed_pbrs,
        check_wire,
    );
    let v4_first_wire = V4Serialize::serialize(&parsed_v4).expect("v4 first wire");
    assert_same_output(
        name,
        "v4 first",
        &v4_first_wire,
        &pbrs_wire,
        &parsed_pbrs,
        check_wire,
    );
    let expected_touch = touch_pbrs(&parsed_pbrs);
    assert_eq!(
        expected_touch,
        touch_prost(&parsed_prost),
        "{name}: pbrs vs prost touch"
    );
    assert_eq!(
        expected_touch,
        touch_v4(&parsed_v4),
        "{name}: pbrs vs v4 touch"
    );

    let payload = pbrs_wire.len();
    let (iters, samples) = timer_budget(payload);
    let first_iters = first_encode_budget::<P, R, V>(iters, payload);
    let pbrs_enc = median_ns(samples, iters, || {
        dst.clear();
        Serialize::encode(pbrs, &mut dst).expect("pbrs encode");
        std::hint::black_box(&dst[..]);
    });
    let pbrs_dec = median_ns(samples, iters, || P::parse(&pbrs_wire).expect("pbrs parse"));
    let pbrs_fresh_enc = median_first_encode_ns(
        samples,
        first_iters,
        || P::parse(&pbrs_wire).expect("pbrs first parse"),
        |message| {
            dst.clear();
            Serialize::encode(message, &mut dst).expect("pbrs first encode");
            std::hint::black_box(&dst[..]);
        },
    );
    let pbrs_touch = median_ns(samples, iters, || {
        let msg = P::parse(&pbrs_wire).expect("pbrs parse");
        touch_pbrs(&msg)
    });
    let prost_enc = median_ns(samples, iters, || {
        dst.clear();
        prost::Message::encode(prost, &mut dst).expect("prost encode");
        std::hint::black_box(&dst[..]);
    });
    let prost_dec = median_ns(samples, iters, || {
        R::decode(pbrs_wire.as_slice()).expect("prost decode")
    });
    let prost_first_enc = median_first_encode_ns(
        samples,
        first_iters,
        || R::decode(pbrs_wire.as_slice()).expect("prost first parse"),
        |message| {
            dst.clear();
            prost::Message::encode(message, &mut dst).expect("prost first encode");
            std::hint::black_box(&dst[..]);
        },
    );
    let prost_touch = median_ns(samples, iters, || {
        let msg = R::decode(pbrs_wire.as_slice()).expect("prost decode");
        touch_prost(&msg)
    });
    let v4_enc = median_ns(samples, iters, || {
        V4Serialize::serialize(v4).expect("v4 encode")
    });
    let v4_dec = median_ns(samples, iters, || V::parse(&pbrs_wire).expect("v4 parse"));
    let v4_first_enc = median_first_encode_ns(
        samples,
        first_iters,
        || V::parse(&pbrs_wire).expect("v4 first parse"),
        |message| V4Serialize::serialize(message).expect("v4 first encode"),
    );
    let v4_touch = median_ns(samples, iters, || {
        let msg = V::parse(&pbrs_wire).expect("v4 parse");
        touch_v4(&msg)
    });
    Row {
        name,
        payload,
        pbrs_enc,
        pbrs_fresh_enc,
        first_iters,
        pbrs_dec,
        pbrs_touch,
        prost_enc,
        prost_first_enc,
        prost_dec,
        prost_touch,
        v4_enc,
        v4_first_enc,
        v4_dec,
        v4_touch,
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

fn touch_node_pbrs(n: &pbrs_cases::Node) -> usize {
    let mut count = n.n() as usize;
    let mut cur = n;
    while cur.has_child() {
        cur = cur.child();
        count += cur.n() as usize;
    }
    count
}

fn touch_node_prost(n: &prost_cases::Node) -> usize {
    let mut count = n.n as usize;
    let mut cur = n;
    while let Some(c) = &cur.child {
        cur = c;
        count += cur.n as usize;
    }
    count
}

fn touch_node_v4(n: &v4_cases::Node) -> usize {
    let mut count = n.n() as usize;
    if n.has_child() {
        let mut cur = n.child();
        count += cur.n() as usize;
        while cur.has_child() {
            cur = cur.child();
            count += cur.n() as usize;
        }
    }
    count
}

fn print_table(title: &str, rows: &[Row]) {
    println!("{title}");
    println!();
    println!(
        "| case | payload | pbrs enc (fresh/cached) | pbrs dec (parse/touch) | prost enc/dec/touch | v4 enc/dec/touch | vs prost | vs v4 |"
    );
    println!("|---|---:|---:|---:|---:|---:|---|---|");
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
            "| {} | {} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} / {:.1} | {:.1} / {:.1} / {:.1} | {vs_prost} | {vs_v4} |",
            r.name,
            r.payload,
            r.pbrs_fresh_enc,
            r.pbrs_enc,
            r.pbrs_dec,
            r.pbrs_touch,
            r.prost_enc,
            r.prost_dec,
            r.prost_touch,
            r.v4_enc,
            r.v4_dec,
            r.v4_touch
        );
    }
    println!();
}

fn print_first_encodes(rows: &[Row]) {
    println!("First encode after parse (diagnostic; ns, preparation excluded):");
    println!("| case | prepared messages/sample | pbrs | prost | v4 |");
    println!("|---|---:|---:|---:|---:|");
    for r in rows {
        println!(
            "| {} | {} | {:.1} | {:.1} | {:.1} |",
            r.name, r.first_iters, r.pbrs_fresh_enc, r.prost_first_enc, r.v4_first_enc
        );
    }
    println!();
}

fn person_input_wire() -> Vec<u8> {
    let mut address = PbrsAddress::new();
    address.set_city("nyc");
    let mut person = PbrsPerson::new();
    person.set_id(7);
    person.set_name("ada lovelace");
    person.set_email("ada@example.com");
    person.tags_mut().push("math");
    person.tags_mut().push("eng");
    person.scores_mut().insert("notes", 12);
    person.set_address(address);
    Serialize::serialize(&person).expect("person input wire")
}

fn encode_pbrs_person(message: &PbrsPerson, dst: &mut BytesMut) {
    dst.clear();
    Serialize::encode(message, dst).expect("pbrs person encode");
}

fn encode_prost_person(message: &ProstPerson, dst: &mut BytesMut) {
    dst.clear();
    prost::Message::encode(message, dst).expect("prost person encode");
}

fn verify_person_mutations(input: &[u8]) {
    let mut pbrs = PbrsPerson::parse(input).expect("pbrs person input");
    let mut prost: ProstPerson = prost::Message::decode(input).expect("prost person input");
    let mut v4 = v4_person::Person::parse(input).expect("v4 person input");
    let mut pbrs_dst = BytesMut::new();
    let mut prost_dst = BytesMut::new();
    encode_pbrs_person(&pbrs, &mut pbrs_dst);
    assert_eq!(&pbrs_dst[..], input, "person: pbrs input wire");
    encode_prost_person(&prost, &mut prost_dst);
    assert_eq!(&prost_dst[..], input, "person: prost input wire");
    assert_eq!(
        V4Serialize::serialize(&v4).expect("v4 person input wire"),
        input,
        "person: v4 input wire"
    );

    for id in [42, 43] {
        let mut expected = PbrsPerson::parse(input).expect("person mutation reference");
        expected.set_id(id);
        let expected_wire = Serialize::serialize(&expected).expect("person expected wire");
        pbrs.set_id(id);
        prost.id = id;
        v4.set_id(id);

        encode_pbrs_person(&pbrs, &mut pbrs_dst);
        assert_person_mutation_output("pbrs", &pbrs_dst, &expected_wire, id);
        encode_prost_person(&prost, &mut prost_dst);
        assert_person_mutation_output("prost", &prost_dst, &expected_wire, id);
        let v4_wire = V4Serialize::serialize(&v4).expect("v4 person mutation wire");
        assert_person_mutation_output("v4", &v4_wire, &expected_wire, id);
    }
}

#[derive(Clone, Copy)]
struct MutationRow {
    name: &'static str,
    payload: usize,
    iters: u32,
    samples: usize,
    pbrs_ns: f64,
    prost_ns: f64,
    v4_ns: f64,
}

fn run_person_mutation() -> MutationRow {
    let input = person_input_wire();
    verify_person_mutations(&input);
    let (iters, samples) = timer_budget(input.len());
    let iters =
        first_encode_budget::<PbrsPerson, ProstPerson, v4_person::Person>(iters, input.len());
    let mut pbrs_dst = BytesMut::new();
    let mut prost_dst = BytesMut::new();
    let pbrs_ns = median_mutated_encode_ns(
        samples,
        iters,
        || {
            let message = PbrsPerson::parse(&input).expect("pbrs mutation parse");
            let mut warm = BytesMut::new();
            encode_pbrs_person(&message, &mut warm);
            std::hint::black_box(&warm[..]);
            message
        },
        |message, id| message.set_id(id),
        |message| {
            encode_pbrs_person(message, &mut pbrs_dst);
            std::hint::black_box(&pbrs_dst[..]);
        },
    );
    let prost_ns = median_mutated_encode_ns(
        samples,
        iters,
        || {
            let message: ProstPerson =
                prost::Message::decode(input.as_slice()).expect("prost mutation parse");
            std::hint::black_box(prost::Message::encode_to_vec(&message));
            message
        },
        |message, id| message.id = id,
        |message| {
            encode_prost_person(message, &mut prost_dst);
            std::hint::black_box(&prost_dst[..]);
        },
    );
    let v4_ns = median_mutated_encode_ns(
        samples,
        iters,
        || {
            let message = v4_person::Person::parse(&input).expect("v4 mutation parse");
            std::hint::black_box(V4Serialize::serialize(&message).expect("v4 person warmup"));
            message
        },
        |message, id| message.set_id(id),
        |message| {
            std::hint::black_box(V4Serialize::serialize(message).expect("v4 person encode"));
        },
    );
    MutationRow {
        name: "person_handwritten",
        payload: input.len(),
        iters,
        samples,
        pbrs_ns,
        prost_ns,
        v4_ns,
    }
}

fn mutation_report(row: &MutationRow) -> String {
    assert!(
        row.iters > 0 && row.samples > 0 && row.payload > 0,
        "mutation report needs measured work"
    );
    for (codec, value) in [
        ("pbrs", row.pbrs_ns),
        ("prost", row.prost_ns),
        ("v4", row.v4_ns),
    ] {
        assert!(
            value.is_finite() && value > 0.0,
            "mutation report has invalid {codec} time"
        );
    }
    format!(
        "Mutation before encode (diagnostic; mutation+encode ns, parse/pre-warm excluded):\n\
         | case | id transition | payload | iterations/sample | samples | pbrs | prost | v4 |\n\
         |---|---|---:|---:|---:|---:|---:|\n\
         | {} | 42 <-> 43 | {} | {} | {} | {:.1} | {:.1} | {:.1} |\n\n\
         Excluded: person_generated (pbrs generated Person is not wired in tonic-bench; adding build.rs generation is outside this slice).\n",
        row.name, row.payload, row.iters, row.samples, row.pbrs_ns, row.prost_ns, row.v4_ns
    )
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
    let published = [
        run(
            "hello",
            &p_hello,
            &r_hello,
            &v_name,
            true,
            |m| m.name().as_bytes().len(),
            |m| m.name.len(),
            |m| m.name().len(),
        ),
        {
            let mut v = v4_cases::Name::new();
            v.set_name(hello_4k.as_str());
            run(
                "hello_4kib",
                &p_hello4,
                &r_hello4,
                &v,
                true,
                |m| m.name().as_bytes().len(),
                |m| m.name.len(),
                |m| m.name().len(),
            )
        },
    ];

    let survey = [
        run(
            "empty",
            &pbrs_cases::Empty::new(),
            &prost_cases::Empty {},
            &v4_cases::Empty::new(),
            true,
            |_| 0,
            |_| 0,
            |_| 0,
        ),
        run(
            "id",
            &p_id,
            &r_id,
            &v_id,
            true,
            |m| m.id() as usize,
            |m| m.id as usize,
            |m| m.id() as usize,
        ),
        run(
            "scalars",
            &p_sc,
            &r_sc,
            &v_sc,
            true,
            |m| {
                m.id() as usize
                    + m.seq() as usize
                    + (m.ok() as usize)
                    + (i32::from(m.status()) as usize)
                    + m.ts() as usize
                    + m.lat() as usize
            },
            |m| {
                m.id as usize
                    + m.seq as usize
                    + (m.ok as usize)
                    + m.status as usize
                    + m.ts as usize
                    + m.lat as usize
            },
            |m| {
                m.id() as usize
                    + m.seq() as usize
                    + (m.ok() as usize)
                    + (i32::from(m.status()) as usize)
                    + m.ts() as usize
                    + m.lat() as usize
            },
        ),
        run(
            "name_short",
            &p_name,
            &r_name,
            &v_name,
            true,
            |m| m.name().as_bytes().len(),
            |m| m.name.len(),
            |m| m.name().len(),
        ),
        run(
            "name_80",
            &p_name80,
            &r_name80,
            &v_name80,
            true,
            |m| m.name().as_bytes().len(),
            |m| m.name.len(),
            |m| m.name().len(),
        ),
        run(
            "name_4kib",
            &p_name4k,
            &r_name4k,
            &v_name4k,
            true,
            |m| m.name().as_bytes().len(),
            |m| m.name.len(),
            |m| m.name().len(),
        ),
        run(
            "blob_32",
            &p_b32,
            &r_b32,
            &v_b32,
            true,
            |m| m.payload().len(),
            |m| m.payload.len(),
            |m| m.payload().len(),
        ),
        run(
            "blob_4kib",
            &p_b4k,
            &r_b4k,
            &v_b4k,
            true,
            |m| m.payload().len(),
            |m| m.payload.len(),
            |m| m.payload().len(),
        ),
        run(
            "blob_64kib",
            &p_b64,
            &r_b64,
            &v_b64,
            true,
            |m| m.payload().len(),
            |m| m.payload.len(),
            |m| m.payload().len(),
        ),
        run(
            "envelope",
            &p_env,
            &r_env,
            &v_env,
            true,
            |m| {
                m.meta().id() as usize
                    + m.meta().trace().as_bytes().len()
                    + m.body().as_bytes().len()
            },
            |m| {
                m.meta
                    .as_ref()
                    .map(|x| x.id as usize + x.trace.len())
                    .unwrap_or(0)
                    + m.body.len()
            },
            |m| m.meta().id() as usize + m.meta().trace().len() + m.body().len(),
        ),
        run(
            "nest_d4",
            &p_nest,
            &r_nest,
            &v_nest,
            true,
            touch_node_pbrs,
            touch_node_prost,
            touch_node_v4,
        ),
        run(
            "packed_16",
            &p_ids16,
            &r_ids16,
            &v_ids16,
            true,
            |m| m.ids().iter().sum::<i64>() as usize,
            |m| m.ids.iter().sum::<i64>() as usize,
            |m| m.ids().iter().sum::<i64>() as usize,
        ),
        run(
            "packed_256",
            &p_ids256,
            &r_ids256,
            &v_ids256,
            true,
            |m| m.ids().iter().sum::<i64>() as usize,
            |m| m.ids.iter().sum::<i64>() as usize,
            |m| m.ids().iter().sum::<i64>() as usize,
        ),
        run(
            "tags_4",
            &p_tags4,
            &r_tags4,
            &v_tags4,
            true,
            |m| m.tags().iter().map(|s| s.as_bytes().len()).sum(),
            |m| m.tags.iter().map(|s| s.len()).sum(),
            |m| m.tags().iter().map(|s| s.len()).sum(),
        ),
        run(
            "tags_32",
            &p_tags32,
            &r_tags32,
            &v_tags32,
            true,
            |m| m.tags().iter().map(|s| s.as_bytes().len()).sum(),
            |m| m.tags.iter().map(|s| s.len()).sum(),
            |m| m.tags().iter().map(|s| s.len()).sum(),
        ),
        run(
            "map_8",
            &p_map,
            &r_map,
            &v_map,
            false,
            |m| {
                m.h()
                    .iter()
                    .map(|(k, v)| k.as_bytes().len() + v.as_bytes().len())
                    .sum()
            },
            |m| m.h.iter().map(|(k, v)| k.len() + v.len()).sum(),
            |m| m.h().iter().map(|(k, v)| k.len() + v.len()).sum(),
        ),
        run(
            "oneof_ok",
            &p_ok,
            &r_ok,
            &v_ok,
            true,
            |m| m.ok_opt().map(|s| s.as_bytes().len()).unwrap_or(0),
            |m| match &m.kind {
                Some(prost_cases::result::Kind::Ok(s)) => s.len(),
                _ => 0,
            },
            |m| if m.has_ok() { m.ok().len() } else { 0 },
        ),
        run(
            "rpc_mixed",
            &p_rpc,
            &r_rpc,
            &v_rpc,
            false,
            |m| {
                m.id() as usize
                    + m.method().as_bytes().len()
                    + m.path().as_bytes().len()
                    + m.user().as_bytes().len()
                    + m.meta().id() as usize
                    + m.ids().iter().sum::<i64>() as usize
                    + m.tags().iter().map(|s| s.as_bytes().len()).sum::<usize>()
                    + m.headers()
                        .iter()
                        .map(|(k, v)| k.as_bytes().len() + v.as_bytes().len())
                        .sum::<usize>()
                    + m.extra().len()
            },
            |m| {
                m.id as usize
                    + m.method.len()
                    + m.path.len()
                    + m.user.len()
                    + m.meta.as_ref().map(|x| x.id as usize).unwrap_or(0)
                    + m.ids.iter().sum::<i64>() as usize
                    + m.tags.iter().map(|s| s.len()).sum::<usize>()
                    + m.headers
                        .iter()
                        .map(|(k, v)| k.len() + v.len())
                        .sum::<usize>()
                    + m.extra.len()
            },
            |m| {
                m.id() as usize
                    + m.method().len()
                    + m.path().len()
                    + m.user().len()
                    + m.meta().id() as usize
                    + m.ids().iter().sum::<i64>() as usize
                    + m.tags().iter().map(|s| s.len()).sum::<usize>()
                    + m.headers()
                        .iter()
                        .map(|(k, v)| k.len() + v.len())
                        .sum::<usize>()
                    + m.extra().len()
            },
        ),
        run(
            "rpc_sparse",
            &p_sparse,
            &r_sparse,
            &v_sparse,
            true,
            |m| m.id() as usize,
            |m| m.id as usize,
            |m| m.id() as usize,
        ),
    ];

    println!("# Codec survey (encode into BytesMut; v4 serialize is Arena+FFI)");
    println!("iters=40000 samples=15 except payload>=32KiB (4000x9). median. release thin-LTO.");
    println!("pbrs vs prost vs crates.io protobuf 4.35.1-release (upb).");
    println!("map_8 / rpc_mixed skip byte-equal (HashMap order); decoded values checked.");
    println!(
        "First encode is directly timed from separately parsed messages with matched input counts."
    );
    println!(
        "pbrs may retain lazy wire backing; prost owns fields; v4 uses an upb Arena. No views."
    );
    println!();
    print_table(
        "## Published 1-string (hello.proto; v4 uses wire-equivalent cases.Name)",
        &published,
    );
    print_first_encodes(&published);
    print_table("## Common shapes (codec_cases.proto)", &survey);
    print_first_encodes(&survey);
    println!("{}", mutation_report(&run_person_mutation()));

    let mut failed = false;
    for r in survey.iter() {
        if r.name == "rpc_sparse" {
            if r.pbrs_dec >= r.prost_dec {
                eprintln!(
                    "perf gate failed: rpc_sparse decode {:.1} vs prost {:.1}",
                    r.pbrs_dec, r.prost_dec
                );
                failed = true;
            }
            continue;
        }
        if r.name == "tags_32" {
            if r.pbrs_dec >= r.v4_dec {
                eprintln!(
                    "perf gate failed: tags_32 decode {:.1} vs v4 {:.1}",
                    r.pbrs_dec, r.v4_dec
                );
                failed = true;
            }
            continue;
        }
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

#[cfg(test)]
mod tests {
    use super::{
        MutationRow, ProstPerson, assert_person_mutation_output, first_encode_budget,
        median_first_encode_ns, median_mutated_encode_ns, mutation_report, person_input_wire,
        v4_person, verify_person_mutations,
    };
    use pbrs::testdata::Person as PbrsPerson;
    use pbrs::{Parse, Serialize};

    #[test]
    fn first_encode_prepares_and_encodes_every_sample_once() {
        let mut prepared = 0usize;
        let mut encoded = Vec::new();
        let measurement = median_first_encode_ns(
            3,
            10,
            || {
                prepared += 1;
                prepared
            },
            |message| {
                encoded.push(*message);
                *message
            },
        );
        assert!(measurement.is_finite());
        assert_eq!(prepared, 33);
        assert_eq!(encoded, (1..=33).collect::<Vec<_>>());
    }

    #[test]
    fn first_encode_budget_limits_the_estimated_prepared_footprint() {
        let payload = 64 * 1024;
        let count = first_encode_budget::<u64, u64, u64>(40_000, payload);
        assert!(count <= 10_000);
        assert!(
            usize::try_from(count).expect("bounded count") * (payload + size_of::<u64>())
                <= 32 * 1024 * 1024
        );
        assert_eq!(
            first_encode_budget::<u64, u64, u64>(40_000, 20 * 1024 * 1024),
            1
        );
        assert!(
            std::panic::catch_unwind(|| {
                first_encode_budget::<u64, u64, u64>(40_000, 33 * 1024 * 1024)
            })
            .is_err()
        );
    }

    #[test]
    fn mutation_helper_reuses_each_parsed_message_and_alternates_before_every_encode() {
        let mut prepared = 0usize;
        let mut encoded = Vec::new();
        let measurement = median_mutated_encode_ns(
            3,
            10,
            || {
                prepared += 1;
                7
            },
            |message, id| *message = id,
            |message| {
                encoded.push(*message);
                vec![*message as u8]
            },
        );
        assert!(measurement.is_finite());
        assert_eq!(prepared, 3);
        assert_eq!(encoded.len(), 33);
        assert_eq!(
            encoded,
            [42, 43, 42, 43, 42, 43, 42, 43, 42, 43, 42].repeat(3)
        );
    }

    #[test]
    fn person_mutation_checks_all_codecs_and_rejects_mismatches() {
        let input = person_input_wire();
        verify_person_mutations(&input);
        let iters =
            first_encode_budget::<PbrsPerson, ProstPerson, v4_person::Person>(40_000, input.len());
        let footprint = input.len()
            + size_of::<PbrsPerson>()
                .max(size_of::<ProstPerson>())
                .max(size_of::<v4_person::Person>());
        assert!(iters <= 10_000);
        assert!(usize::try_from(iters).expect("bounded count") * footprint <= 32 * 1024 * 1024);

        let mut expected = PbrsPerson::parse(&input).expect("reference parse");
        expected.set_id(42);
        let expected_wire = Serialize::serialize(&expected).expect("reference wire");
        let mut wrong = PbrsPerson::parse(&input).expect("wrong parse");
        wrong.set_id(43);
        let wrong_wire = Serialize::serialize(&wrong).expect("wrong wire");
        assert!(
            std::panic::catch_unwind(|| {
                assert_person_mutation_output("prost", &wrong_wire, &expected_wire, 42)
            })
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| assert_person_mutation_output("v4", b"\xff", b"\xff", 42))
                .is_err()
        );
    }

    #[test]
    fn mutation_report_is_separate_and_fails_closed_on_missing_measurements() {
        let row = MutationRow {
            name: "person_handwritten",
            payload: 64,
            iters: 10,
            samples: 3,
            pbrs_ns: 3.5,
            prost_ns: 4.0,
            v4_ns: 9.2,
        };
        let report = mutation_report(&row);
        assert_eq!(
            report.lines().next(),
            Some(
                "Mutation before encode (diagnostic; mutation+encode ns, parse/pre-warm excluded):"
            )
        );
        assert_eq!(
            report.lines().nth(3),
            Some("| person_handwritten | 42 <-> 43 | 64 | 10 | 3 | 3.5 | 4.0 | 9.2 |")
        );
        assert!(report.contains("Excluded: person_generated ("));
        assert!(!report.contains("First encode after parse"));
        for value in [0.0, f64::NAN, f64::INFINITY] {
            assert!(
                std::panic::catch_unwind(|| mutation_report(&MutationRow {
                    v4_ns: value,
                    ..row
                }))
                .is_err()
            );
        }
    }
}
