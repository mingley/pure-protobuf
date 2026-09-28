//! Codec survey: `Serialize::encode` into `BytesMut` vs prost `Message::encode`
//! vs v4 `Serialize::serialize` (Arena+FFI, no EncodeBuf). Same-process, no
//! transport. Not kernel `./bench`. Timing is not in CI; correctness tests are.
//!
//! `hello` / `hello_4kib` stay the published 1-string rows. Common unary
//! shapes come from `proto/codec_cases.proto` (specialized gencode, not
//! TestAllTypes); separate Person layout diagnostics use `proto/person.proto`.

use bytes::BytesMut;
use pbrs::testdata::{Address as PbrsAddress, Person as PbrsPerson};
use pbrs::{AsView, Parse, Serialize};
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
mod pbrs_person {
    #![allow(dead_code, unused, non_snake_case, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/pbrs_person/person.rs"));
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
    U: FnMut(&mut M, bool),
    E: FnMut(&M) -> O,
{
    assert!(
        samples > 0 && iters > 0,
        "mutation samples and iters must be positive"
    );
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mut message = prepare();
        let mut alternate = false;
        let mut step = || {
            alternate = !alternate;
            mutate(&mut message, alternate);
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
    let generated = pbrs_person::Person::parse(actual).expect("generated pbrs reparses person");
    let generated_expected =
        pbrs_person::Person::parse(expected_wire).expect("generated pbrs reference must parse");
    assert_eq!(
        generated.id(),
        expected_id,
        "person mutation: {codec} generated id"
    );
    assert_eq!(
        generated, generated_expected,
        "person mutation: {codec} generated fields"
    );
    let prost: ProstPerson = prost::Message::decode(actual).expect("prost reparses mutated person");
    assert_eq!(prost.id, expected_id, "person mutation: {codec} prost id");
    assert_eq!(
        prost,
        prost::Message::decode(expected_wire).expect("prost reference must parse"),
        "person mutation: {codec} prost fields"
    );
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

#[derive(Clone, Copy)]
struct Row {
    name: &'static str,
    payload: usize,
    iters: u32,
    samples: usize,
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
    run_with_budget(
        name,
        pbrs,
        prost,
        v4,
        check_wire,
        touch_pbrs,
        touch_prost,
        touch_v4,
        None,
    )
}

fn run_with_budget<P, R, V, TP, TR, TV>(
    name: &'static str,
    pbrs: &P,
    prost: &R,
    v4: &V,
    check_wire: bool,
    touch_pbrs: TP,
    touch_prost: TR,
    touch_v4: TV,
    budget: Option<(u32, usize)>,
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
    let (iters, samples) = budget.unwrap_or_else(|| timer_budget(payload));
    assert!(
        iters > 0 && samples > 0,
        "benchmark needs a positive budget"
    );
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
        iters,
        samples,
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

fn touch_handwritten_person(m: &PbrsPerson) -> usize {
    m.id() as usize
        + m.name().as_bytes().len()
        + m.email_opt().map_or(0, |email| email.as_bytes().len())
        + m.tags()
            .iter()
            .map(|tag| tag.as_view().as_bytes().len())
            .sum::<usize>()
        + m.scores()
            .iter()
            .map(|(key, score)| key.as_view().as_bytes().len() + score as usize)
            .sum::<usize>()
        + m.address().city().as_bytes().len()
}

fn touch_generated_person(m: &pbrs_person::Person) -> usize {
    m.id() as usize
        + m.name().as_bytes().len()
        + m.email_opt().map_or(0, |email| email.as_bytes().len())
        + m.tags()
            .iter()
            .map(|tag| tag.as_view().as_bytes().len())
            .sum::<usize>()
        + m.scores()
            .iter()
            .map(|(key, score)| key.as_view().as_bytes().len() + score as usize)
            .sum::<usize>()
        + m.address().city().as_bytes().len()
        + m.extras()
            .iter()
            .map(|(key, value)| key.as_view().as_bytes().len() + value as usize)
            .sum::<usize>()
}

fn touch_prost_person(m: &ProstPerson) -> usize {
    m.id as usize
        + m.name.len()
        + m.email.as_ref().map_or(0, String::len)
        + m.tags.iter().map(String::len).sum::<usize>()
        + m.scores
            .iter()
            .map(|(key, score)| key.len() + *score as usize)
            .sum::<usize>()
        + m.address.as_ref().map_or(0, |address| address.city.len())
        + m.extras
            .iter()
            .map(|(key, value)| key.len() + *value as usize)
            .sum::<usize>()
}

fn touch_v4_person(m: &v4_person::Person) -> usize {
    m.id() as usize
        + m.name().len()
        + (if m.has_email() { m.email().len() } else { 0 })
        + m.tags().iter().map(|tag| tag.len()).sum::<usize>()
        + m.scores()
            .iter()
            .map(|(key, score)| key.len() + score as usize)
            .sum::<usize>()
        + m.address().city().len()
        + m.extras()
            .iter()
            .map(|(key, value)| key.len() + value as usize)
            .sum::<usize>()
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

fn person_extras_wire(base: &[u8]) -> Vec<u8> {
    let mut person = pbrs_person::Person::parse(base).expect("generated Person fixture");
    person.extras_mut().insert("project", 7);
    Serialize::serialize(&person).expect("Person with typed extras")
}

fn run_person_extras_row(input: &[u8], budget: Option<(u32, usize)>) -> Row {
    let generated = pbrs_person::Person::parse(input).expect("generated extras input");
    let prost: ProstPerson = prost::Message::decode(input).expect("prost extras input");
    let v4 = v4_person::Person::parse(input).expect("v4 extras input");
    assert_eq!(generated.extras().iter().count(), 1);
    assert_eq!(prost.extras.get("project"), Some(&7));
    assert!(
        v4.extras()
            .iter()
            .any(|(key, value)| key == "project" && value == 7),
        "v4 must expose the same typed extras field"
    );
    run_with_budget(
        "person_generated_extras",
        &generated,
        &prost,
        &v4,
        true,
        touch_generated_person,
        touch_prost_person,
        touch_v4_person,
        budget,
    )
}

fn mutation_id(alternate: bool) -> i32 {
    if alternate { 42 } else { 43 }
}

fn mutation_name(alternate: bool) -> &'static str {
    if alternate {
        "ada"
    } else {
        "ada lovelace with a longer name"
    }
}

fn run_person_rows(input: &[u8], budget: Option<(u32, usize)>) -> [Row; 2] {
    let handwritten = PbrsPerson::parse(input).expect("handwritten person input");
    let generated = pbrs_person::Person::parse(input).expect("generated person input");
    let prost: ProstPerson = prost::Message::decode(input).expect("prost person input");
    let v4 = v4_person::Person::parse(input).expect("v4 person input");
    assert_eq!(
        Serialize::serialize(&handwritten).expect("handwritten person wire"),
        input
    );
    assert_eq!(
        Serialize::serialize(&generated).expect("generated person wire"),
        input
    );
    let handwritten_row = run_with_budget(
        "person_handwritten",
        &handwritten,
        &prost,
        &v4,
        true,
        touch_handwritten_person,
        touch_prost_person,
        touch_v4_person,
        budget,
    );
    let prost: ProstPerson = prost::Message::decode(input).expect("prost generated-row input");
    let v4 = v4_person::Person::parse(input).expect("v4 generated-row input");
    [
        handwritten_row,
        run_with_budget(
            "person_generated",
            &generated,
            &prost,
            &v4,
            true,
            touch_generated_person,
            touch_prost_person,
            touch_v4_person,
            budget,
        ),
    ]
}

fn person_report(rows: &[Row; 2], extras: &Row) -> String {
    assert_eq!(rows[0].name, "person_handwritten");
    assert_eq!(rows[1].name, "person_generated");
    assert_eq!(extras.name, "person_generated_extras");
    let work = (
        rows[0].payload,
        rows[0].iters,
        rows[0].first_iters,
        rows[0].samples,
    );
    assert!(
        work.0 > 0 && work.1 > 0 && work.2 > 0 && work.2 <= work.1 && work.3 > 0,
        "person rows need measured work"
    );
    assert_eq!(
        work,
        (
            rows[1].payload,
            rows[1].iters,
            rows[1].first_iters,
            rows[1].samples
        ),
        "person rows must use the same input and sample counts"
    );
    assert!(
        extras.payload > work.0,
        "typed extras row must contain additional wire data"
    );
    assert_eq!(
        (extras.iters, extras.first_iters, extras.samples),
        (work.1, work.2, work.3),
        "typed extras row must use the same sample budget"
    );
    let mut report = String::from(
        "## Person layouts (proto/person.proto; diagnostic, ns, not gated)\n\
         | case (pbrs layout) | payload | repeated/parse iters | first-encode iters | samples | pbrs enc first/prewarmed | pbrs dec parse/touch | prost enc first/repeated | prost dec parse/touch | v4 enc first/repeated | v4 dec parse/touch |\n\
         |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for row in rows.iter().chain(std::iter::once(extras)) {
        for value in [
            row.pbrs_enc,
            row.pbrs_fresh_enc,
            row.pbrs_dec,
            row.pbrs_touch,
            row.prost_enc,
            row.prost_first_enc,
            row.prost_dec,
            row.prost_touch,
            row.v4_enc,
            row.v4_first_enc,
            row.v4_dec,
            row.v4_touch,
        ] {
            assert!(
                value.is_finite() && value > 0.0,
                "person report has an invalid measured time for {}",
                row.name
            );
        }
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} |\n",
            row.name,
            row.payload,
            row.iters,
            row.first_iters,
            row.samples,
            row.pbrs_fresh_enc,
            row.pbrs_enc,
            row.pbrs_dec,
            row.pbrs_touch,
            row.prost_first_enc,
            row.prost_enc,
            row.prost_dec,
            row.prost_touch,
            row.v4_first_enc,
            row.v4_enc,
            row.v4_dec,
            row.v4_touch,
        ));
    }
    report.push_str(
        "\nEach row uses a matched wire across its codecs; prost/v4 are timed independently. \
         The two layout rows share an empty-extras wire; the generated-only extras row has \
         one typed tag-16 entry and no handwritten comparator. \
         Touch uses string lengths and scalar values, not a bytewise string scan. \
         Fixed-order, same-process timings do not qualify a speed or memory claim.\n",
    );
    report
}

fn encode_pbrs_person<P: Serialize>(message: &P, dst: &mut BytesMut) {
    dst.clear();
    Serialize::encode(message, dst).expect("pbrs person encode");
}

fn encode_prost_person(message: &ProstPerson, dst: &mut BytesMut) {
    dst.clear();
    prost::Message::encode(message, dst).expect("prost person encode");
}

fn verify_person_mutations(input: &[u8]) {
    let mut pbrs = PbrsPerson::parse(input).expect("pbrs person input");
    let mut generated = pbrs_person::Person::parse(input).expect("generated pbrs person input");
    let mut prost: ProstPerson = prost::Message::decode(input).expect("prost person input");
    let mut v4 = v4_person::Person::parse(input).expect("v4 person input");
    let mut pbrs_dst = BytesMut::new();
    let mut generated_dst = BytesMut::new();
    let mut prost_dst = BytesMut::new();
    encode_pbrs_person(&pbrs, &mut pbrs_dst);
    assert_eq!(&pbrs_dst[..], input, "person: pbrs input wire");
    encode_pbrs_person(&generated, &mut generated_dst);
    assert_eq!(
        &generated_dst[..],
        input,
        "person: generated pbrs input wire"
    );
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
        generated.set_id(id);
        prost.id = id;
        v4.set_id(id);

        encode_pbrs_person(&pbrs, &mut pbrs_dst);
        assert_person_mutation_output("pbrs", &pbrs_dst, &expected_wire, id);
        encode_pbrs_person(&generated, &mut generated_dst);
        assert_person_mutation_output("pbrs generated", &generated_dst, &expected_wire, id);
        encode_prost_person(&prost, &mut prost_dst);
        assert_person_mutation_output("prost", &prost_dst, &expected_wire, id);
        let v4_wire = V4Serialize::serialize(&v4).expect("v4 person mutation wire");
        assert_person_mutation_output("v4", &v4_wire, &expected_wire, id);
    }

    let mut pbrs = PbrsPerson::parse(input).expect("pbrs name mutation input");
    let mut generated = pbrs_person::Person::parse(input).expect("generated name mutation input");
    let mut prost: ProstPerson = prost::Message::decode(input).expect("prost name mutation input");
    let mut v4 = v4_person::Person::parse(input).expect("v4 name mutation input");
    for name in [mutation_name(true), mutation_name(false)] {
        let mut expected = PbrsPerson::parse(input).expect("name mutation reference");
        expected.set_name(name);
        let expected_wire = Serialize::serialize(&expected).expect("name mutation expected wire");
        pbrs.set_name(name);
        generated.set_name(name);
        prost.name = name.to_owned();
        v4.set_name(name);

        encode_pbrs_person(&pbrs, &mut pbrs_dst);
        assert_person_mutation_output("pbrs name", &pbrs_dst, &expected_wire, 7);
        encode_pbrs_person(&generated, &mut generated_dst);
        assert_person_mutation_output("pbrs generated name", &generated_dst, &expected_wire, 7);
        encode_prost_person(&prost, &mut prost_dst);
        assert_person_mutation_output("prost name", &prost_dst, &expected_wire, 7);
        let v4_wire = V4Serialize::serialize(&v4).expect("v4 name mutation wire");
        assert_person_mutation_output("v4 name", &v4_wire, &expected_wire, 7);
    }
}

#[derive(Clone, Copy)]
struct MutationRow {
    name: &'static str,
    transition: &'static str,
    payload: usize,
    iters: u32,
    samples: usize,
    pbrs_ns: f64,
    prost_ns: f64,
    v4_ns: f64,
}

struct MutationLabel {
    layout: &'static str,
    transition: &'static str,
}

fn person_mutation_budget(iters: u32, payload: usize) -> u32 {
    first_encode_budget::<PbrsPerson, ProstPerson, v4_person::Person>(iters, payload).min(
        first_encode_budget::<pbrs_person::Person, ProstPerson, v4_person::Person>(iters, payload),
    )
}

fn measure_person_mutation<P, UP, UR, UV>(
    label: MutationLabel,
    input: &[u8],
    iters: u32,
    samples: usize,
    mut set_pbrs: UP,
    mut set_prost: UR,
    mut set_v4: UV,
) -> MutationRow
where
    P: Parse + Serialize,
    UP: FnMut(&mut P, bool),
    UR: FnMut(&mut ProstPerson, bool),
    UV: FnMut(&mut v4_person::Person, bool),
{
    let mut pbrs_dst = BytesMut::new();
    let mut prost_dst = BytesMut::new();
    let pbrs_ns = median_mutated_encode_ns(
        samples,
        iters,
        || {
            let message = P::parse(input).expect("pbrs mutation parse");
            let mut warm = BytesMut::new();
            encode_pbrs_person(&message, &mut warm);
            std::hint::black_box(&warm[..]);
            message
        },
        |message, alternate| set_pbrs(message, alternate),
        |message| {
            encode_pbrs_person(message, &mut pbrs_dst);
            std::hint::black_box(&pbrs_dst[..]);
        },
    );
    let prost_ns = median_mutated_encode_ns(
        samples,
        iters,
        || {
            let message: ProstPerson = prost::Message::decode(input).expect("prost mutation parse");
            std::hint::black_box(prost::Message::encode_to_vec(&message));
            message
        },
        |message, alternate| set_prost(message, alternate),
        |message| {
            encode_prost_person(message, &mut prost_dst);
            std::hint::black_box(&prost_dst[..]);
        },
    );
    let v4_ns = median_mutated_encode_ns(
        samples,
        iters,
        || {
            let message = v4_person::Person::parse(input).expect("v4 mutation parse");
            std::hint::black_box(V4Serialize::serialize(&message).expect("v4 person warmup"));
            message
        },
        |message, alternate| set_v4(message, alternate),
        |message| {
            std::hint::black_box(V4Serialize::serialize(message).expect("v4 person encode"));
        },
    );
    MutationRow {
        name: label.layout,
        transition: label.transition,
        payload: input.len(),
        iters,
        samples,
        pbrs_ns,
        prost_ns,
        v4_ns,
    }
}

fn run_person_mutations(input: &[u8], iters: u32, samples: usize) -> [MutationRow; 4] {
    verify_person_mutations(input);
    let iters = person_mutation_budget(iters, input.len());
    [
        measure_person_mutation::<PbrsPerson, _, _, _>(
            MutationLabel {
                layout: "person_handwritten",
                transition: "id 42 <-> 43",
            },
            input,
            iters,
            samples,
            |message, alternate| message.set_id(mutation_id(alternate)),
            |message, alternate| message.id = mutation_id(alternate),
            |message, alternate| message.set_id(mutation_id(alternate)),
        ),
        measure_person_mutation::<pbrs_person::Person, _, _, _>(
            MutationLabel {
                layout: "person_generated",
                transition: "id 42 <-> 43",
            },
            input,
            iters,
            samples,
            |message, alternate| message.set_id(mutation_id(alternate)),
            |message, alternate| message.id = mutation_id(alternate),
            |message, alternate| message.set_id(mutation_id(alternate)),
        ),
        measure_person_mutation::<PbrsPerson, _, _, _>(
            MutationLabel {
                layout: "person_handwritten",
                transition: "name ada <-> longer",
            },
            input,
            iters,
            samples,
            |message, alternate| message.set_name(mutation_name(alternate)),
            |message, alternate| message.name = mutation_name(alternate).to_owned(),
            |message, alternate| message.set_name(mutation_name(alternate)),
        ),
        measure_person_mutation::<pbrs_person::Person, _, _, _>(
            MutationLabel {
                layout: "person_generated",
                transition: "name ada <-> longer",
            },
            input,
            iters,
            samples,
            |message, alternate| message.set_name(mutation_name(alternate)),
            |message, alternate| message.name = mutation_name(alternate).to_owned(),
            |message, alternate| message.set_name(mutation_name(alternate)),
        ),
    ]
}

fn mutation_report(rows: &[MutationRow; 4]) -> String {
    let work = (rows[0].payload, rows[0].iters, rows[0].samples);
    assert!(
        work.0 > 0 && work.1 > 0 && work.2 > 0,
        "mutation report needs measured work"
    );
    for (row, expected) in rows.iter().zip([
        ("person_handwritten", "id 42 <-> 43"),
        ("person_generated", "id 42 <-> 43"),
        ("person_handwritten", "name ada <-> longer"),
        ("person_generated", "name ada <-> longer"),
    ]) {
        assert_eq!(
            (row.name, row.transition),
            expected,
            "mutation row identity"
        );
        assert_eq!(
            work,
            (row.payload, row.iters, row.samples),
            "mutation rows must use the same input and sample counts"
        );
    }
    let mut report = String::from(
        "Mutation before encode (diagnostic; mutation+encode ns, parse/pre-warm excluded):\n\
         | case (pbrs layout) | field transition | payload | iterations/sample | samples | pbrs | prost | v4 |\n\
         |---|---|---:|---:|---:|---:|---:|---:|\n",
    );
    for row in rows {
        for (codec, value) in [
            ("pbrs", row.pbrs_ns),
            ("prost", row.prost_ns),
            ("v4", row.v4_ns),
        ] {
            assert!(
                value.is_finite() && value > 0.0,
                "mutation report has invalid {codec} time for {}",
                row.name
            );
        }
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.1} | {:.1} | {:.1} |\n",
            row.name,
            row.transition,
            row.payload,
            row.iters,
            row.samples,
            row.pbrs_ns,
            row.prost_ns,
            row.v4_ns
        ));
    }
    report.push_str(
        "\nName states are \"ada\" and \"ada lovelace with a longer name\". \
         Each row times its own pbrs, prost and v4 mutation independently in fixed order; \
         no gate or speed claim.\n",
    );
    report
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
    let person_wire = person_input_wire();
    let person_rows = run_person_rows(&person_wire, None);
    let extras_wire = person_extras_wire(&person_wire);
    let extras_row = run_person_extras_row(&extras_wire, None);
    println!("{}", person_report(&person_rows, &extras_row));
    let (person_iters, person_samples) = timer_budget(person_wire.len());
    println!(
        "{}",
        mutation_report(&run_person_mutations(
            &person_wire,
            person_iters,
            person_samples
        ))
    );

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
        median_first_encode_ns, median_mutated_encode_ns, mutation_report, pbrs_person,
        person_extras_wire, person_input_wire, person_mutation_budget, person_report,
        run_person_extras_row, run_person_mutations, run_person_rows, touch_generated_person,
        touch_handwritten_person, touch_prost_person, touch_v4_person, v4_person,
        verify_person_mutations,
    };
    use pbrs::testdata::Person as PbrsPerson;
    use pbrs::{AsView, Parse, Serialize};
    use protobuf::{Parse as V4Parse, Serialize as V4Serialize};

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
            |message, alternate| *message = super::mutation_id(alternate),
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
    fn generated_person_matches_the_populated_handwritten_fixture() {
        let input = person_input_wire();
        assert_eq!(input.len(), 62);
        let handwritten = PbrsPerson::parse(&input).expect("handwritten person");
        let generated = pbrs_person::Person::parse(&input).expect("generated person");
        let prost: ProstPerson = prost::Message::decode(input.as_slice()).expect("prost person");
        let v4 = v4_person::Person::parse(&input).expect("v4 person");

        assert_eq!(generated.id(), 7);
        assert_eq!(generated.name().as_bytes(), b"ada lovelace");
        assert_eq!(
            generated.email_opt().expect("email is present").as_bytes(),
            b"ada@example.com"
        );
        assert_eq!(
            generated
                .tags()
                .iter()
                .map(|tag| tag.as_view().to_str().expect("valid tag").to_owned())
                .collect::<Vec<_>>(),
            ["math", "eng"]
        );
        assert_eq!(
            generated
                .scores()
                .iter()
                .map(|(key, value)| (
                    key.as_view().to_str().expect("valid score key").to_owned(),
                    value
                ))
                .collect::<Vec<_>>(),
            [("notes".to_owned(), 12)]
        );
        assert_eq!(generated.address().city().as_bytes(), b"nyc");
        assert_eq!(generated.extras().iter().count(), 0);
        assert_eq!(
            Serialize::serialize(&handwritten).expect("handwritten wire"),
            input
        );
        assert_eq!(
            Serialize::serialize(&generated).expect("generated wire"),
            input
        );
        assert_eq!(V4Serialize::serialize(&v4).expect("v4 wire"), input);
        let touched = touch_handwritten_person(&handwritten);
        assert_eq!(touched, 61);
        assert_eq!(touch_generated_person(&generated), touched);
        assert_eq!(touch_prost_person(&prost), touched);
        assert_eq!(touch_v4_person(&v4), touched);
    }

    #[test]
    fn person_layout_rows_use_same_wire_work_and_report_both_measurements() {
        let input = person_input_wire();
        let rows = run_person_rows(&input, Some((10, 3)));
        let extras = run_person_extras_row(&person_extras_wire(&input), Some((10, 3)));
        assert_eq!(rows[0].name, "person_handwritten");
        assert_eq!(rows[1].name, "person_generated");
        assert_eq!(rows[0].payload, input.len());
        assert_eq!(rows[1].payload, input.len());
        assert_eq!(rows[0].iters, 10);
        assert_eq!(rows[1].iters, 10);
        assert_eq!(rows[0].first_iters, rows[1].first_iters);
        let report = person_report(&rows, &extras);
        assert!(report.contains("| person_handwritten | 62 | 10 | 10 | 3 |"));
        assert!(report.contains("| person_generated | 62 | 10 | 10 | 3 |"));
        assert!(report.contains("| person_generated_extras |"));
        assert!(report.contains("no handwritten comparator"));
        assert!(report.contains("prost/v4 are timed independently"));
        assert!(!report.contains(" | win |"));
        assert!(!report.contains(" | loss |"));

        let mut mismatched = rows;
        mismatched[1].first_iters -= 1;
        assert!(std::panic::catch_unwind(|| person_report(&mismatched, &extras)).is_err());
        let mut invalid = rows;
        invalid[1].v4_touch = f64::NAN;
        assert!(std::panic::catch_unwind(|| person_report(&invalid, &extras)).is_err());
        let mut mislabeled = extras;
        mislabeled.name = "person_handwritten";
        assert!(std::panic::catch_unwind(|| person_report(&rows, &mislabeled)).is_err());
    }

    #[test]
    fn generated_person_extras_compares_only_codecs_with_typed_tag_16() {
        let base = person_input_wire();
        let wire = person_extras_wire(&base);
        assert!(wire.len() > base.len());
        let generated = pbrs_person::Person::parse(&wire).expect("generated extras");
        let (key, value) = generated.extras().iter().next().expect("typed extras");
        assert_eq!(key.as_view().as_bytes(), b"project");
        assert_eq!(value, 7);
        let prost: ProstPerson = prost::Message::decode(wire.as_slice()).expect("prost extras");
        let v4 = v4_person::Person::parse(&wire).expect("v4 extras");
        let touched = touch_generated_person(&generated);
        assert!(touched > touch_generated_person(&pbrs_person::Person::parse(&base).unwrap()));
        assert_eq!(touched, touch_prost_person(&prost));
        assert_eq!(touched, touch_v4_person(&v4));

        let row = run_person_extras_row(&wire, Some((10, 3)));
        assert_eq!(row.name, "person_generated_extras");
        assert_eq!((row.payload, row.iters, row.samples), (wire.len(), 10, 3));
        assert!(
            person_report(&run_person_rows(&base, Some((10, 3))), &row)
                .contains("| person_generated_extras |")
        );

        let mut wrong = generated;
        wrong.extras_mut().insert("project", 9);
        let wrong_wire = Serialize::serialize(&wrong).expect("wrong typed extras wire");
        assert!(
            std::panic::catch_unwind(|| run_person_extras_row(&wrong_wire, Some((10, 3)))).is_err()
        );
    }

    #[test]
    fn person_mutation_checks_all_codecs_and_rejects_mismatches() {
        let input = person_input_wire();
        verify_person_mutations(&input);
        let iters = person_mutation_budget(40_000, input.len());
        let footprint = input.len()
            + size_of::<PbrsPerson>()
                .max(size_of::<pbrs_person::Person>())
                .max(size_of::<ProstPerson>())
                .max(size_of::<v4_person::Person>());
        assert!(iters <= 10_000);
        assert!(usize::try_from(iters).expect("bounded count") * footprint <= 32 * 1024 * 1024);

        let rows = run_person_mutations(&input, 10, 3);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].iters, 10);
        assert_eq!(rows[1].iters, 10);
        assert_eq!(rows[2].iters, 10);
        assert_eq!(rows[3].iters, 10);
        assert_eq!(rows[0].payload, input.len());
        assert_eq!(rows[1].payload, input.len());
        assert_eq!(rows[2].payload, input.len());
        assert_eq!(rows[3].payload, input.len());
        let report = mutation_report(&rows);
        assert!(report.contains("| person_handwritten | id 42 <-> 43 | 62 | 10 | 3 |"));
        assert!(report.contains("| person_generated | id 42 <-> 43 | 62 | 10 | 3 |"));
        assert!(report.contains("| person_handwritten | name ada <-> longer | 62 | 10 | 3 |"));
        assert!(report.contains("| person_generated | name ada <-> longer | 62 | 10 | 3 |"));

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
        let mut wrong_generated = pbrs_person::Person::parse(&input).expect("generated input");
        wrong_generated.set_id(42);
        wrong_generated.set_name("different");
        let wrong_wire = Serialize::serialize(&wrong_generated).expect("wrong generated wire");
        assert!(
            std::panic::catch_unwind(|| {
                assert_person_mutation_output("pbrs generated", &wrong_wire, &expected_wire, 42)
            })
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| assert_person_mutation_output("v4", b"\xff", b"\xff", 42))
                .is_err()
        );
    }

    #[test]
    fn person_mutation_covers_non_id_field_for_both_layouts() {
        let rows = run_person_mutations(&person_input_wire(), 10, 3);
        assert_eq!(rows.len(), 4, "name mutation must have two additional rows");
        let report = mutation_report(&rows);
        assert!(report.contains("| person_handwritten | name "));
        assert!(report.contains("| person_generated | name "));
    }

    #[test]
    fn mutation_report_is_separate_and_fails_closed_on_missing_measurements() {
        let row = MutationRow {
            name: "person_handwritten",
            transition: "id 42 <-> 43",
            payload: 64,
            iters: 10,
            samples: 3,
            pbrs_ns: 3.5,
            prost_ns: 4.0,
            v4_ns: 9.2,
        };
        let generated = MutationRow {
            name: "person_generated",
            pbrs_ns: 5.3,
            prost_ns: 4.1,
            v4_ns: 8.0,
            ..row
        };
        let name_row = MutationRow {
            transition: "name ada <-> longer",
            pbrs_ns: 6.0,
            prost_ns: 6.5,
            v4_ns: 10.1,
            ..row
        };
        let generated_name_row = MutationRow {
            name: "person_generated",
            pbrs_ns: 7.0,
            prost_ns: 6.8,
            v4_ns: 11.2,
            ..name_row
        };
        let rows = [row, generated, name_row, generated_name_row];
        let report = mutation_report(&rows);
        assert_eq!(
            report.lines().next(),
            Some(
                "Mutation before encode (diagnostic; mutation+encode ns, parse/pre-warm excluded):"
            )
        );
        assert_eq!(
            report.lines().nth(3),
            Some("| person_handwritten | id 42 <-> 43 | 64 | 10 | 3 | 3.5 | 4.0 | 9.2 |")
        );
        assert_eq!(
            report.lines().nth(4),
            Some("| person_generated | id 42 <-> 43 | 64 | 10 | 3 | 5.3 | 4.1 | 8.0 |")
        );
        assert_eq!(
            report.lines().nth(5),
            Some("| person_handwritten | name ada <-> longer | 64 | 10 | 3 | 6.0 | 6.5 | 10.1 |")
        );
        assert_eq!(
            report.lines().nth(6),
            Some("| person_generated | name ada <-> longer | 64 | 10 | 3 | 7.0 | 6.8 | 11.2 |")
        );
        assert!(report.contains("Each row times its own pbrs, prost and v4"));
        assert!(report.contains("Name states are \"ada\" and \"ada lovelace with a longer name\""));
        assert!(!report.contains("Excluded:"));
        assert!(!report.contains("First encode after parse"));
        for value in [0.0, f64::NAN, f64::INFINITY] {
            let mut invalid = rows;
            invalid[3].v4_ns = value;
            assert!(std::panic::catch_unwind(|| mutation_report(&invalid)).is_err());
        }
        let mut unmatched = rows;
        unmatched[3].iters = 9;
        assert!(std::panic::catch_unwind(|| mutation_report(&unmatched)).is_err());
        let mut mislabeled = rows;
        mislabeled[3].transition = "id 42 <-> 43";
        assert!(std::panic::catch_unwind(|| mutation_report(&mislabeled)).is_err());
    }
}
