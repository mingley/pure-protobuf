# Query allocation diagnostic

This opt-in, synchronous probe separates work in the historical adoption
`query.d4.v6` unary workload. It does not run an RPC or modify the pbrs
runtime. The original hosted comparison at source
`10d94697dc89e3aa89c843720f7150f1186f417a` reported 157 allocations per RPC
for native+pbrs and 125.01 for tonic+Prost. Those profiles vary both transport
and codec. The same capture reports 87.03 for native+Prost and 195.01 for
tonic+pbrs. It is historical, not a measurement of a later source revision.

## Run

From the repository root, install the devloop build prerequisites (including
CMake and a C++ compiler), then build the pinned protobuf compiler and
sources. The devloop includes the v4 comparison crate, whose build requires
that compiler's built-in Rust output:

```sh
scripts/build-pinned-protoc.sh
PATH="$PWD/target/pinned-protoc-build:$PATH" \
  cargo run --locked --release --manifest-path bench/devloop/Cargo.toml \
  --features query-allocation-diagnostic --bin query-allocation -- 200 > query-allocation.json
```

The optional argument is N, from 1 to 10,000; the default is 200. Record the
source commit, `bench/devloop/Cargo.lock` hash, compiler version, host, and
command with the output. The binary runs synchronously without a Tokio runtime
and wraps the System allocator to count process-wide successful `alloc`,
`alloc_zeroed`, and `realloc` calls. Relaxed snapshots can include activity
from other threads. Reallocations are included in the total; requested bytes for
a reallocation are its full new size. Counters saturate and reject overflow.
Deallocations and peak or retained memory are not measured.

For each case and size N and 2N, the order is pbrs, Prost, Prost, pbrs. Two
repeats, raw counts, and both N-to-2N per-operation differences appear in
JSON. A zero-allocation control and an opaque allocation control check the
counter before sampling. The program rejects differing repeats after emitting
the report, as well as any negative N-to-2N count difference. Warmup uses
distinct objects before measured samples. This is a local component
diagnostic, not an endpoint latency, throughput, CPU, or ranking result.
The devloop manifest enables pbrs and pbrs-grpc `copy-counts` hooks, so this
is not a counters-off shipping-build measurement. `black_box` is best effort;
results depend on the compiler and build profile.

## Inputs and stages

The exact query uses depth 4 and variant 6. Both codecs start from the same
293-byte Prost-prepared protobuf input, FNV-1a identifier
`af055fbbc4e54c32`. FNV is an input identifier, not a cryptographic proof.
The program requires the initial and fully-read cloned native messages to
encode to exactly the same request bytes, and separately checks decoded
values, complete-read checksums, and response decoding. Controls use an empty
Query and the same root with one predicate and no children, subqueries, or
traversals. The report records `size_of` for each message and the repeated/lazy containers; layout alone
does not measure retained or peak memory.

- **fully_read_request_clone** clones and drops a prepared request. Like the
  adoption harness, the native fixture is decoded, serialized, fully read,
  serialized again for validation, and checked with a separate cloned witness.
  Before each measured sample, the fixture is cloned once outside the counter
  window. The measured loop clones and drops that held value while the original
  fixture remains alive. This matches the harness's outside-the-loop clone and
  preserves the sharing that both live values can cause.
- **request_decode** calls the native transport's public
  `CodecMessage::decode_payload(Bytes)`, then drops the message.
- **request_first_read** decodes fresh inputs outside the counter window and
  counts the first complete read and drop. It includes lazy materialization.
- **request_decode_read** counts decoding and the complete read together.
- **response_decode_read** repeats that scope on the respective codec's
  separately encoded response payload.
- **response_encode** prepares fresh decoded, fully read messages outside the
  window, then counts a single first encode, frame drop, and message drop per
  object. It does not serialize those measured objects during setup.

Unique and shared `Bytes` inputs are prepared outside each counting window.
The shared case holds an extra owner throughout that window; nonempty inputs
are checked for the intended uniqueness. Empty bytes have no ownership
contrast. These buffers do not reproduce every HTTP/2 receive-buffer layout.

Response encoding calls the public `CodecMessage` methods through a small
contiguous stand-in for the private native `SegSink`, with a five-byte gRPC
prefix. It preallocates the claimed frame size, uses the contiguous-write
override for Prost, and rejects a shared segment. All three payloads are below
the 32 KiB shared-segment threshold. A separate uncounted sample is decoded
back to the expected Prost value; every counted output is compared byte for
byte with that codec's uncounted sample.
The report includes the native and Prost response length and fingerprint
separately. The stand-in omits transport, framing budgets, server/client
tasks, compression, and TLS; its counts are not an allocation-site breakdown
of the original RPC.

This probe retains separate fresh and fully read lifecycles. The historical
standalone codec `read_all` cell used `Parse::parse(&[u8])`, while this probe
uses the gRPC `Bytes` decode path, so their counts must not be treated as
interchangeable or simply summed. Use later site-specific tracing and an
end-to-end paired RPC experiment to test any optimization hypothesis.
