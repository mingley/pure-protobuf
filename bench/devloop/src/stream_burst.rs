//! Opt-in allocation diagnostic for large inline gRPC server-stream replies.
//!
//! This is separate from the frozen `devloop/1` cell registry. The allocator
//! observes both endpoints in one process and wall time is instrumented.

use pbrs_adoption_corpus::{
    rpc::{Inputs, Pair},
    workloads::Specimen,
};
use pbrs_grpc::codec::prost::Message as TransportMessage;
use pbrs_grpc::{Channel, Framed, Request, Response, Router, Rpc, Service, Status, Streaming};
use prost::Message as _;
use serde_json::{Value, json};
use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Debug;
use std::hint::black_box;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const PATH: &str = "/adoption.Echo/ServerStream";
const HARNESS: &[u8] = include_bytes!("stream_burst.rs");
const MANIFEST: &[u8] = include_bytes!("../Cargo.toml");
const LOCK: &[u8] = include_bytes!("../Cargo.lock");
const CORPUS_RPC: &[u8] = include_bytes!("../adoption/src/rpc.rs");
const CORPUS_MANIFEST: &[u8] = include_bytes!("../adoption/Cargo.toml");
const CORPUS_BUILD: &[u8] = include_bytes!("../adoption/build.rs");
const CORPUS_LIB: &[u8] = include_bytes!("../adoption/src/lib.rs");
const WORKLOADS: &[u8] = include_bytes!("../adoption/src/workloads.rs");
const DEVLOOP_BUILD: &[u8] = include_bytes!("../build.rs");
const CORPUS: &[u8] = include_bytes!("../adoption/src/corpus.rs");
const SPARSE_WALKS: &[u8] = include_bytes!("../adoption/src/sparse_walks.rs");
const SPARSE_PROTO: &[u8] = include_bytes!("../adoption/proto/sparse.proto");
const ADOPTION_PROTO: &[u8] = include_bytes!("../adoption/proto/adoption.proto");
const SERVER_DRAIN: &[u8] = include_bytes!("../../../pbrs-grpc/src/server/drain.rs");

struct CountingAlloc;
static ARMED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static COUNTER_OVERFLOW: AtomicBool = AtomicBool::new(false);

// Allocator callbacks cannot allocate or panic. Preserve the previous value
// and invalidate the sample if any counter cannot represent an update.
fn checked_add(counter: &AtomicU64, amount: u64, overflow: &AtomicBool) {
    if counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(amount)
        })
        .is_err()
    {
        overflow.store(true, Ordering::Relaxed);
    }
}

fn note_allocation(size: usize, reallocated: bool) {
    checked_add(&ALLOCS, 1, &COUNTER_OVERFLOW);
    match u64::try_from(size) {
        Ok(bytes) => checked_add(&BYTES, bytes, &COUNTER_OVERFLOW),
        Err(_) => COUNTER_OVERFLOW.store(true, Ordering::Relaxed),
    }
    if reallocated {
        checked_add(&REALLOCS, 1, &COUNTER_OVERFLOW);
    }
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ARMED.load(Ordering::Relaxed) {
            note_allocation(layout.size(), false);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, layout, new_size) };
        if !result.is_null() && ARMED.load(Ordering::Relaxed) {
            note_allocation(new_size, true);
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: CountingAlloc = CountingAlloc;

struct AllocGuard;

impl AllocGuard {
    fn arm() -> Self {
        ALLOCS.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        REALLOCS.store(0, Ordering::Relaxed);
        COUNTER_OVERFLOW.store(false, Ordering::Relaxed);
        ARMED.store(true, Ordering::Relaxed);
        pbrs::copy_counts::reset_copy_counts();
        pbrs_grpc::reset_copy_counts();
        Self
    }

    fn totals(&self) -> (u64, u64, u64, bool) {
        (
            ALLOCS.load(Ordering::Relaxed),
            BYTES.load(Ordering::Relaxed),
            REALLOCS.load(Ordering::Relaxed),
            COUNTER_OVERFLOW.load(Ordering::Relaxed),
        )
    }
}

impl Drop for AllocGuard {
    fn drop(&mut self) {
        ARMED.store(false, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Producer {
    Live8,
    Ready128,
}

impl Producer {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "live8" => Some(Self::Live8),
            "ready128" => Some(Self::Ready128),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Live8 => "live8",
            Self::Ready128 => "ready128",
        }
    }

    fn capacity(self) -> usize {
        match self {
            Self::Live8 => 8,
            Self::Ready128 => 128,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Case {
    SparseV0,
    QueryD3V0,
}

impl Case {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "sparse-v0" => Some(Self::SparseV0),
            "query-d3-v0" => Some(Self::QueryD3V0),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SparseV0 => "sparse-v0",
            Self::QueryD3V0 => "query-d3-v0",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Options {
    case: Case,
    producer: Producer,
    replies: usize,
    iterations: u64,
    warmup: u64,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, &'static str> {
        let [case, producer, replies, iterations, warmup] = args else {
            return Err("expected CASE PRODUCER REPLIES ITERATIONS WARMUP");
        };
        let case = Case::parse(case).ok_or("case must be sparse-v0 or query-d3-v0")?;
        let producer = Producer::parse(producer).ok_or("producer must be live8 or ready128")?;
        let replies = replies.parse().map_err(|_| "invalid reply count")?;
        if ![4, 64, 128].contains(&replies) {
            return Err("reply count must be 4, 64, or 128");
        }
        let iterations = iterations.parse().map_err(|_| "invalid iterations")?;
        if !(1..=400).contains(&iterations) {
            return Err("iterations must be between 1 and 400");
        }
        let warmup = warmup.parse().map_err(|_| "invalid warmup")?;
        if warmup > 1_000 {
            return Err("warmup must be at most 1000");
        }
        Ok(Self {
            case,
            producer,
            replies,
            iterations,
            warmup,
        })
    }

    fn id(self) -> String {
        format!(
            "stream-burst/v1/{}/{}/{}",
            self.case.name(),
            self.producer.name(),
            self.replies
        )
    }
}

struct Echo<P> {
    read: fn(&P) -> u64,
    checksum: u64,
    producer: Producer,
    replies: usize,
}

impl<P> Service for Echo<P>
where
    P: prost::Message + Default + Clone + Send + Sync + 'static,
{
    const NAME: &'static str = "adoption.Echo";

    async fn call(&self, rpc: Rpc) {
        let read = self.read;
        let checksum = self.checksum;
        let producer = self.producer;
        let replies = self.replies;
        rpc.server_streaming(move |request: Request<TransportMessage<P>>| async move {
            let message = request.into_inner();
            assert_eq!(black_box(read(&message.0)), checksum);
            let (tx, stream) = Streaming::channel(producer.capacity());
            match producer {
                Producer::Live8 => {
                    drop(tokio::spawn(async move {
                        for _ in 0..replies {
                            if tx.send(message.clone()).await.is_err() {
                                break;
                            }
                        }
                    }));
                }
                Producer::Ready128 => {
                    for _ in 0..replies {
                        tx.send(message.clone())
                            .await
                            .map_err(|_| Status::cancelled())?;
                    }
                    drop(tx);
                }
            }
            Ok::<_, Status>(Response::new(stream))
        })
        .await;
    }
}

async fn roundtrip<P>(
    channel: &Channel,
    input: &TransportMessage<P>,
    read: fn(&P) -> u64,
    checksum: u64,
    replies: usize,
    full_validation: bool,
) -> u64
where
    P: prost::Message + Default + Clone + PartialEq + Debug + Send + Sync + 'static,
{
    let mut stream = channel
        .server_streaming::<TransportMessage<P>, TransportMessage<P>>(
            PATH,
            Request::new(input.clone()),
        )
        .await
        .expect("stream response")
        .into_inner();
    let mut count = 0usize;
    let mut sink = 0u64;
    while let Some(response) = stream.message().await.expect("stream message") {
        assert_eq!(black_box(read(&response.0)), checksum);
        if full_validation {
            assert_eq!(&response.0, &input.0);
            assert_eq!(response.0.encoded_len(), input.0.encoded_len());
        }
        count += 1;
        sink = sink.wrapping_add(checksum);
    }
    assert_eq!(count, replies);
    stream.trailers().await.expect("OK trailers");
    sink
}

fn sha256(bytes: &[u8]) -> String {
    let mut child = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("sha256sum");
    child
        .stdin
        .take()
        .expect("sha256sum stdin")
        .write_all(bytes)
        .expect("write sha256 input");
    let output = child.wait_with_output().expect("sha256sum output");
    assert!(output.status.success(), "sha256sum failed");
    let text = String::from_utf8(output.stdout).expect("sha256 output is UTF-8");
    let digest = text.split_whitespace().next().expect("sha256 digest");
    assert!(
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid sha256 digest"
    );
    digest.to_owned()
}

fn git(args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git");
    assert!(output.status.success(), "git command failed: {args:?}");
    output.stdout
}

fn provenance() -> Value {
    // These are compile-time embedded file bytes; the worktree state is an
    // additional runtime observation and must be checked by the caller.
    let head = String::from_utf8(git(&["rev-parse", "--verify", "HEAD"]))
        .expect("git head")
        .trim()
        .to_owned();
    let tracked_diff = git(&["diff", "--no-ext-diff", "--binary", "HEAD", "--"]);
    let status = git(&["status", "--porcelain=v1", "--untracked-files=all"]);
    let executable = std::env::current_exe().expect("executable");
    let binary = std::fs::read(executable).expect("read executable");
    json!({
        "runtime_git_head": head,
        "runtime_git_dirty": !status.is_empty(),
        "runtime_git_status_sha256": sha256(&status),
        "runtime_tracked_diff_sha256": sha256(&tracked_diff),
        "executable_sha256": sha256(&binary),
        "embedded_sources_sha256": {
            "harness": sha256(HARNESS),
            "manifest": sha256(MANIFEST),
            "lockfile": sha256(LOCK),
            "corpus_rpc": sha256(CORPUS_RPC),
            "corpus_manifest": sha256(CORPUS_MANIFEST),
            "corpus_build": sha256(CORPUS_BUILD),
            "corpus_lib": sha256(CORPUS_LIB),
            "workloads": sha256(WORKLOADS),
            "devloop_build": sha256(DEVLOOP_BUILD),
            "corpus": sha256(CORPUS),
            "sparse_walks": sha256(SPARSE_WALKS),
            "sparse_proto": sha256(SPARSE_PROTO),
            "adoption_proto": sha256(ADOPTION_PROTO),
            "subject_server_drain": sha256(SERVER_DRAIN),
        },
    })
}

async fn run_pair<N, P>(options: Options, pair: Pair<N, P>, provenance: Value) -> Value
where
    N: pbrs::Parse + pbrs::Serialize + Clone,
    P: prost::Message + Default + Clone + PartialEq + Debug + Send + Sync + 'static,
{
    pair.assert_equal_wire_work();
    let wire = pair.prost.encode_to_vec();
    assert_eq!(wire.len(), pair.bytes);
    let wire_hex: String = wire.iter().map(|byte| format!("{byte:02x}")).collect();
    let input = TransportMessage(pair.prost.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    let echo = Echo {
        read: pair.read_prost,
        checksum: pair.checksum,
        producer: options.producer,
        replies: options.replies,
    };
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(echo)
            .serve_listener(listener)
            .await
            .expect("server");
    });
    let channel = Channel::connect(address).await.expect("connect");

    roundtrip(
        &channel,
        &input,
        pair.read_prost,
        pair.checksum,
        options.replies,
        true,
    )
    .await;
    for _ in 0..options.warmup {
        black_box(
            roundtrip(
                &channel,
                &input,
                pair.read_prost,
                pair.checksum,
                options.replies,
                false,
            )
            .await,
        );
    }

    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut checksum = 0u64;
    for _ in 0..options.iterations {
        checksum = checksum.wrapping_add(
            roundtrip(
                &channel,
                &input,
                pair.read_prost,
                pair.checksum,
                options.replies,
                false,
            )
            .await,
        );
    }
    let wall_ns = u64::try_from(start.elapsed().as_nanos()).expect("wall time fits u64");
    let (allocations, allocated_bytes, reallocations, overflow_sampled) = guard.totals();
    drop(guard);
    assert!(
        !overflow_sampled,
        "sampled allocation counter overflow; sample is invalid"
    );
    black_box(checksum);
    server.abort();
    let _ = server.await;

    json!({
        "schema": "devloop/stream-burst/1",
        "id": options.id(),
        "case": options.case.name(),
        "producer": options.producer.name(),
        "channel_capacity": options.producer.capacity(),
        "reply_count": options.replies,
        "warmup": options.warmup,
        "iterations": options.iterations,
        "runtime_workers": 2,
        "input_wire_fingerprint": pair.input_wire_fingerprint,
        "input_wire_hex": wire_hex,
        "request_bytes": pair.bytes,
        "response_bytes_per_message": pair.bytes,
        "message_size": std::mem::size_of::<TransportMessage<P>>(),
        "batch_item_size": std::mem::size_of::<Result<Framed<TransportMessage<P>>, Status>>(),
        "checksum": pair.checksum,
        "preflight": "full equality for every response, read checksum, count and OK trailers",
        "measurement": {
            "scope": "process-wide client and server; realloc counts full new size",
            "allocations": allocations,
            "allocated_bytes": allocated_bytes,
            "reallocations": reallocations,
            "overflow_sampled": overflow_sampled,
            "snapshot": "non-atomic relaxed loads while armed; hooks may update between or after reads",
            "wall_ns": wall_ns,
            "wall_time_role": "secondary instrumented diagnostic",
            "instructions": "not_run",
        },
        "target": {
            "arch": std::env::consts::ARCH,
            "os": std::env::consts::OS,
            "debug_assertions": cfg!(debug_assertions),
            "devloop_features": {
                "stream-burst-diagnostic": cfg!(feature = "stream-burst-diagnostic"),
                "bridge": cfg!(feature = "bridge"),
            },
            "manifest_requested_dependency_features": ["prost", "tonic", "copy-counts"],
        },
        "provenance": provenance,
    })
}

async fn run(options: Options, provenance: Value) -> Value {
    match options.case {
        Case::SparseV0 => {
            let Inputs::Sparse(pair) = Inputs::prepare(Specimen::Sparse(0)) else {
                unreachable!("sparse corpus");
            };
            run_pair(options, *pair, provenance).await
        }
        Case::QueryD3V0 => {
            let Inputs::Query(pair) = Inputs::prepare(Specimen::Query {
                depth: 3,
                variant: 0,
            }) else {
                unreachable!("query corpus");
            };
            run_pair(options, pair, provenance).await
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = Options::parse(&args).unwrap_or_else(|error| {
        eprintln!(
            "{error}\nusage: stream-burst CASE PRODUCER REPLIES ITERATIONS WARMUP\n\
             CASE: sparse-v0 | query-d3-v0; PRODUCER: live8 | ready128"
        );
        std::process::exit(2);
    });
    let provenance = provenance();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let result = runtime.block_on(run(options, provenance));
    println!(
        "{}",
        serde_json::to_string(&result).expect("serialize result")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use pbrs_adoption_corpus::prost_types;

    #[test]
    fn options_give_each_workload_a_distinct_identity() {
        let mut ids = std::collections::HashSet::new();
        for case in ["sparse-v0", "query-d3-v0"] {
            for producer in ["live8", "ready128"] {
                for replies in ["4", "64", "128"] {
                    let args = [case, producer, replies, "1", "0"].map(str::to_owned);
                    let options = Options::parse(&args).expect("valid options");
                    assert!(ids.insert(options.id()));
                }
            }
        }
        let bad = ["sparse-v0", "ready128", "63", "1", "0"].map(str::to_owned);
        assert!(Options::parse(&bad).is_err());
        let zero = ["sparse-v0", "live8", "4", "0", "0"].map(str::to_owned);
        assert!(Options::parse(&zero).is_err());
        let too_many = ["sparse-v0", "live8", "4", "401", "0"].map(str::to_owned);
        assert!(Options::parse(&too_many).is_err());
        let too_warm = ["sparse-v0", "live8", "4", "1", "1001"].map(str::to_owned);
        assert!(Options::parse(&too_warm).is_err());
    }

    #[test]
    fn counter_overflow_is_detected_without_wrapping() {
        let counter = AtomicU64::new(u64::MAX - 1);
        let overflow = AtomicBool::new(false);
        checked_add(&counter, 1, &overflow);
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        assert!(!overflow.load(Ordering::Relaxed));
        checked_add(&counter, 1, &overflow);
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        assert!(overflow.load(Ordering::Relaxed));
    }

    #[test]
    fn frozen_sparse_fixture_stays_large_inline() {
        let Inputs::Sparse(pair) = Inputs::prepare(Specimen::Sparse(0)) else {
            unreachable!("sparse corpus");
        };
        pair.assert_equal_wire_work();
        assert!(
            std::mem::size_of::<Result<Framed<TransportMessage<prost_types::Sparse>>, Status>>()
                > std::mem::size_of::<Result<Framed<TransportMessage<prost_types::Query>>, Status>>(
                )
        );
        assert_eq!((pair.read_prost)(&pair.prost), pair.checksum);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sparse_preflight_checks_every_reply_at_each_burst_size() {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            for producer in [Producer::Live8, Producer::Ready128] {
                for replies in [4, 64, 128] {
                    let Inputs::Sparse(pair) = Inputs::prepare(Specimen::Sparse(0)) else {
                        unreachable!("sparse corpus");
                    };
                    let options = Options {
                        case: Case::SparseV0,
                        producer,
                        replies,
                        iterations: 1,
                        warmup: 0,
                    };
                    let report = run_pair(options, *pair, Value::Null).await;
                    assert_eq!(report["id"], options.id());
                    assert_eq!(report["reply_count"], replies);
                    assert_eq!(report["iterations"], 1);
                    assert!(
                        report["measurement"]["allocations"]
                            .as_u64()
                            .is_some_and(|count| count > 0)
                    );
                    assert!(
                        report["measurement"]["allocated_bytes"]
                            .as_u64()
                            .is_some_and(|bytes| bytes > 0)
                    );
                    assert_eq!(report["measurement"]["overflow_sampled"], false);
                }
            }
            let Inputs::Query(pair) = Inputs::prepare(Specimen::Query {
                depth: 3,
                variant: 0,
            }) else {
                unreachable!("query corpus");
            };
            let options = Options {
                case: Case::QueryD3V0,
                producer: Producer::Ready128,
                replies: 64,
                iterations: 1,
                warmup: 0,
            };
            let report = run_pair(options, pair, Value::Null).await;
            assert_eq!(report["id"], options.id());
            assert_eq!(report["reply_count"], 64);
        })
        .await
        .expect("all stream burst preflights complete");
    }
}
