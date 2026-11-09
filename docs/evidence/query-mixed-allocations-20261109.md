# Query and mixed-byte allocation probes, November 9

Two hosted allocation probes found pbrs requesting more allocations than
Prost in several nonempty Query cases and all four measured mixed-byte cases. These are
synchronous component counts. They do not measure CPU time, latency,
throughput, retained memory, or gRPC calls.

Both builds used release mode on hosted Ubuntu 24.04 x86-64 with Rust 1.99.1.
The devloop dependency graph enabled the pbrs and pbrs-grpc `copy-counts`
hooks. A process-wide wrapper around the System allocator counted successful
allocation and reallocation requests. Requested bytes are not live or peak
memory, and the relaxed counters can include other threads. `black_box` is
a best-effort optimization barrier.

## Sources and checks

- [Query source](https://github.com/mingley/pure-protobuf/blob/422937f2baf17019c028715997730b4ecca9d900/bench/devloop/src/query_alloc_probe.rs),
  [workflow run](https://github.com/mingley/pure-protobuf/actions/runs/42686442810),
  and [raw artifact](https://github.com/mingley/pure-protobuf/actions/runs/42686442810/artifacts/13705678921).
  The artifact ZIP SHA-256 is
  `73d05aabaed6454084c8a40d95ed584d084677f41aa5da772f2368234dc18e3d`.
  The run's allocation controls, byte and ownership checks, two reports,
  repeat checks, and negative controls passed. **The overall run failed its
  formatting gate.** The counts below are retained as results from that
  explicitly failed preflight, not as a passing full run. A later
  [formatted rerun](https://github.com/mingley/pure-protobuf/actions/runs/42689455114)
  passed its checks; the numbers below remain from the earlier capture.
- [Mixed-byte source](https://github.com/mingley/pure-protobuf/blob/4a2d8e26134a41eaf71e449ab29f5f50890933c1/bench/devloop/src/mixed_ownership.rs),
  [workflow run](https://github.com/mingley/pure-protobuf/actions/runs/42687551418),
  and [raw artifact](https://github.com/mingley/pure-protobuf/actions/runs/42687551418/artifacts/13706690277).
  Its artifact ZIP SHA-256 is
  `ae8b7dae793b9de9a3f1237e172e96af769a25793e177f25713c7cbbb76028f6`.
  Three diagnostic tests, the four captures, their controls, and cleanup
  passed.
- A separate [evidence reader](https://github.com/mingley/pure-protobuf/actions/runs/42690707434/job/129409371242)
  verified both GitHub-reported ZIP digests, run and job metadata, row
  inventories, repeat agreement, and recorded differences, and printed all
  rows. Its complete validation step and exact-ref cleanup passed.

The [retained reader records](2026-11-09-query-mixed-allocation-reader.json)
were reconstructed from that validated log. They are not the original artifact
ZIPs or a complete copy of every raw repeat.

## Mixed bytes

Each operation constructs a `BlobMixed` with eight 4,096-byte fields,
encodes it, and drops the output and message. The borrowed case creates a
source vector inside the counted operation and copies from its slice; it is
not a measurement of an already-existing borrowed input. The moved case
transfers the vector. Exact and spare capacity are separate cases.

Each cell below is **allocation requests / requested bytes** per added
operation, calculated from `(2N - N) / N` with N = 1,000. Four fresh
processes ran in N, 2N, 2N, N order. Both repeats at each size matched;
case order inside each process stayed fixed. All recorded reallocations
were zero.

| Source | Capacity | pbrs | Prost | pbrs minus Prost |
|---|---|---:|---:|---:|
| Borrowed | Exact | 19 / 98,675 | 18 / 98,523 | +1 / +152 |
| Borrowed | Spare | 19 / 131,443 | 18 / 131,291 | +1 / +152 |
| Moved | Exact | 11 / 65,907 | 10 / 65,755 | +1 / +152 |
| Moved | Spare | 19 / 98,867 | 10 / 98,523 | +9 / +344 |

The ID is the iteration index. IDs in the added 1,000 operations are
1,000–1,999, whose tag and varint use three wire bytes each. The first
1,000 IDs average 2.870 bytes. Thus the added operations average
32,795.000 wire bytes per message versus 32,794.870 at N. A wire-length
difference does not necessarily produce the same difference in allocation
requests.

The extra eight requests in pbrs's moved/spare case are consistent with its
buffer conversion. [`ProtoBytes::from(Vec<u8>)`](../../src/string.rs)
delegates to `Bytes::from(Vec<u8>)`. The pinned
[`bytes` 1.12.1 implementation](https://github.com/tokio-rs/bytes/blob/v1.12.1/src/bytes.rs)
uses a direct path when length equals capacity and otherwise allocates a
`Shared` box. Eight fields account for eight such conversions. This is a
source-supported explanation of the observed difference, not an independent
allocation-site trace. Retaining spare capacity also affects memory lifetime,
which these counts do not measure.

## Query

The Query probe separates cloning, decoding, the first complete field read,
and response encoding. Unique and shared `Bytes` inputs are prepared
outside each measured window; shared inputs keep another owner alive.
Empty bytes have no ownership contrast. The clone starts from a fully read
prepared message. Each measured clone or decoded message is dropped inside
its window, so the first-read row includes message destruction. The
first-read and response-encode stages start with fresh decoded messages
prepared before counting. The response messages are fully read but not
previously encoded; their output and message are both dropped inside the
window. Response encoding uses a contiguous stand-in for the private native
sink and includes a five-byte gRPC prefix; it is not a transport measurement.

Cells are **allocation requests / requested bytes** per added operation from
N = 200 and 2N = 400. Each report contains two measurements at each size;
two independent reports agreed on the input, response, layout, and sample
fields. All recorded reallocations were zero. “Both” means unique and shared
input modes had the same result; the clone uses its prepared-message mode.
Combining request and response decode/read in a row means their measured
values matched, not that their costs should be summed.

| Fixture | Stage | Input mode | pbrs | Prost |
|---|---|---|---:|---:|
| Empty | Clone | Prepared | 0 / 0 | 0 / 0 |
| Empty | Decode; first read; request and response decode/read | Both | 0 / 0 | 0 / 0 |
| Empty | Response encode | Both | 1 / 5 | 1 / 5 |
| One predicate | Clone | Prepared | 5 / 296 | 3 / 78 |
| One predicate | Request decode | Unique | 6 / 536 | 3 / 272 |
| One predicate | Request decode | Shared | 5 / 512 | 3 / 272 |
| One predicate | First read | Both | 1 / 40 | 0 / 0 |
| One predicate | Request and response decode/read | Unique | 7 / 576 | 3 / 272 |
| One predicate | Request and response decode/read | Shared | 6 / 552 | 3 / 272 |
| One predicate | Response encode | Both | 1 / 31 | 1 / 31 |
| Query d4/v6 | Clone | Prepared | 39 / 2,340 | 21 / 1,182 |
| Query d4/v6 | Request decode | Unique | 40 / 4,192 | 22 / 3,959 |
| Query d4/v6 | Request decode | Shared | 39 / 4,168 | 21 / 3,935 |
| Query d4/v6 | First read | Both | 8 / 700 | 0 / 0 |
| Query d4/v6 | Request and response decode/read | Unique | 48 / 4,892 | 22 / 3,959 |
| Query d4/v6 | Request and response decode/read | Shared | 47 / 4,868 | 21 / 3,935 |
| Query d4/v6 | Response encode | Both | 1 / 298 | 1 / 298 |

The d4/v6 input is 293 bytes, with FNV-1a identifier
`af055fbbc4e54c32`. FNV identifies this fixture; it is not a cryptographic
proof. The full output, both decoders, field-read checksums, and response
round trips were checked before the samples. The empty, one-predicate, and
d4/v6 inputs are different workloads and should not be extrapolated to
other messages.
