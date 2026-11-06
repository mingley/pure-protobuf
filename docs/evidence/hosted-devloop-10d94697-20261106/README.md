# Full hosted benchmark record at 10d94697

This packet retains the complete artifact from
[GitHub Actions run 42200175946](https://github.com/mingley/pure-protobuf/actions/runs/42200175946).
It compares [`2374cfc1`](https://github.com/mingley/pure-protobuf/commit/2374cfc1e36d4810a94b5aa4ed9fcd7dddf4f87c)
with [`10d94697`](https://github.com/mingley/pure-protobuf/commit/10d94697dc89e3aa89c843720f7150f1186f417a).
These are standard-library runtime measurements. The bare-metal alloc-only
profile and later changes are outside this record.

All 1,634 cells appear on both sides. Allocations, allocated bytes, syscalls,
futex calls and wall time have finite measured values for every cell.
Instructions were **NOT_RUN** because the runner had neither perf nor
Valgrind. Base and head report matching host metadata.

The three canonical fresh encoders use `independent-warmup-v1` on both sides:
warmup objects are separate from measured fresh objects. Validation reports
`qualified_for_noise=true`, with no errors. This qualifies the available
measurement inputs and lifecycles. Nonregression, a win in every benchmark,
and a world ranking remain unproven.

## Selected head rows

These rows compare pbrs with Prost at **10d94697**. Lower is better.
The full archive retains all rows, including every recorded loss.

TAT encoding and owned decode use Prost **0.13.5**. Its [fresh and cached
encoders](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/src/main.rs#L837)
call `Message::encode` into a new `Vec::new()`; native
[`Serialize`](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/src/gen_support.rs#L924)
pre-reserves the computed size. Prost 0.13.5's
[`encode_to_vec`](https://github.com/tokio-rs/prost/blob/d505b184e933e1f9ff5679106ffc51b7e3c2755e/prost/src/message.rs#L63)
already reserves, but is not the TAT reference API measured here. Name decode
and mixed-blob encoding use Prost **0.14.4**; mixed-blob uses `encode_to_vec`
and constructs eight 4 KiB vectors per operation. These are API-specific rows;
latest-release and best-API comparisons remain unproven.

<!-- selected-head-table:start -->
| Operation | pbrs ns/op | Prost ns/op | pbrs allocs/op | Prost allocs/op |
|---|---:|---:|---:|---:|
| Fresh encode | 866.64 | 673.70 | 7 | 5 |
| Cached encode | 169.33 | 541.79 | 1 | 5 |
| Owned decode | 492.53 | 941.16 | 7 | 10 |
| 80-byte name decode | 62.55 | 54.44 | 1 | 1 |
| Mixed blob encode | 2732.75 | 1924.15 | 19 | 10 |
<!-- selected-head-table:end -->

Pbrs has higher wall time in the selected fresh, name-decode and mixed-blob
rows. Fresh and mixed-blob encoding also use more allocations. Allocated
volume differs too: owned decode uses 1,080 bytes/op for pbrs and 277 for
Prost. Both sides' full metrics remain in the archive.

## Measurement limits

The registered devloop enables allocation accounting and pbrs/pbrs-grpc
`copy-counts` hooks. These hooks add work to instrumented paths. This record
does not establish performance with instrumentation disabled.

The [TAT constructor](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/src/main.rs#L502)
sets nine top-level fields, including map 56 `{0:0, 1:1, 2:4, 3:9}`.
[Native output](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/src/generated/test_messages_proto3.rs#L20004)
includes default map key/value subfields;
[Prost 0.13.5](https://github.com/tokio-rs/prost/blob/d505b184e933e1f9ff5679106ffc51b7e3c2755e/prost/src/encoding.rs#L1042)
omits them. Equal known values therefore produce a source-calculated 87 versus
83 bytes: the zero entry is `c2030408001000` versus `c20300`; its outer map
entry must remain. Both [decode/touch paths](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/src/main.rs#L667)
consume the native wire. An older preflight corroborates 87/83; no new 10d wire
execution was performed for this audit. The captured
[touch checksum](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/src/main.rs#L692)
can miss a removed zero entry or same-length payload corruption.

Mixed-blob timing also includes an ownership difference: the
[native constructor](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/src/blob.rs#L296)
borrows eight temporary vectors through `LazyBytes::from_bytes`, copying their
payloads, while the [Prost constructor](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/src/blob.rs#L325)
moves its vectors into the message. The existing
[`LazyBytes::from(Vec<u8>)`](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/src/lazy.rs#L618)
accepts ownership; that alternative was not measured by this cell.

Wall time is secondary evidence from a shared hosted runner. Matching host
metadata does not establish exclusive hardware access, frequency control or
stable host load. No instruction-count comparison is available in this run.

## Inspect or verify

- [official-artifact.zip](official-artifact.zip) contains all eight original
  members: both JSON records and logs, `validation.json`, `summary.md`,
  `compare.txt`, and the base RPC fixture overlay report. The archive bytes
  are unchanged from official artifact **13499573752**.
- [manifest.json](manifest.json) records the official ZIP digest, member
  digests, source pair, coverage, exact selected values and five previously
  inspected benchmark source/API pins. Source links are pinned to the
  measured revision: [encode and decode](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/src/main.rs#L832),
  [mixed blob](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/src/blob.rs#L325),
  [versions](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/Cargo.lock#L694),
  and [instrumentation](https://github.com/mingley/pure-protobuf/blob/10d94697dc89e3aa89c843720f7150f1186f417a/bench/devloop/Cargo.toml#L16).
- [check.py](check.py) reads these files with the Python standard library.
  It checks the hashes, all cell coverage, metrics, lifecycle metadata,
  source/API review and selected table, including the losing rows. It
  performs no network access, remeasurement, source reconstruction or
  execution of archive contents.

```sh
python3 -B docs/evidence/hosted-devloop-10d94697-20261106/check.py
python3 -B docs/evidence/hosted-devloop-10d94697-20261106/check.py --negative-controls
```

The negative controls alter data only in memory. They check rejection of
missing or nonfinite metrics, incompatible fresh profiles, wrong sources,
missing coverage, fabricated instructions, changed official identity,
changed source/API pins and edited or omitted losing rows.
