# Zero-copy for large payloads

The native `pbrs-grpc` transport can send and receive a large `bytes`
field without copying its contents in user space. Use a generated message,
a `Bytes` backing buffer and identity compression. On receive, the layout
of the incoming HTTP/2 DATA also matters.

## Where copies happen today

The table describes a large `bytes` field over native gRPC. It counts
user-space stages that copy payload data. The kernel still moves socket
data. Fragment reassembly can copy again if its buffer grows.

### Native transport (`pbrs-grpc`)

| Layer | Send | Receive |
|---|---|---|
| Generated protobuf message | The encoder can share a `Bytes` field of 32 KiB or more with a segmented sink. | Large `bytes` fields of 4 KiB or more can borrow the parsed wire buffer. |
| gRPC framing | The encoder keeps the envelope and protobuf prefix separate from a large shared field. | The reader slices a complete frame from one DATA chunk. It can also share a body following a separate five-byte envelope. |
| HTTP/2 | On vectored transports, the writer can send the prefix and shared field together without copying the field. | The receiver slices DATA frames from the read buffer. |
| TLS | The provider copies plaintext into its record buffer for encryption. | The transport copies decrypted plaintext out of rustls. |

When the envelope arrives separately, the native reader shares an uncompressed
body of at least 4 KiB that occupies at least half the remaining DATA chunk.
If payload bytes span DATA chunks, the reader assembles a contiguous body.
In a native TCP test with a 314,159-byte response, the split-envelope case
went from 314,159 copied bytes to five. The intact and fragmented controls
kept their original copy counts.
[Receive measurements](https://github.com/mingley/pure-protobuf/actions/runs/45262277747/artifacts/14860742209)

The native sender can put the prefix and one large field in the same HTTP/2
DATA frame. This applies to an uncompressed message in two buffers. The prefix
can be up to 256 bytes, the field must be at least 32 KiB, and the framed
message must fit the configured send buffer and be at most 1 MiB. Pairing
allocates a small descriptor that holds both buffer owners. The peer's
maximum frame size and available flow-control credit determine how much can
travel in one DATA frame.

On ordinary loopback TCP with default native settings, the generated client
sent a 271,828-byte field and received a 314,159-byte field. Across 512 calls
per build, request reassembly fell from 271,840 bytes to zero per call and
response reassembly fell from 314,167 bytes to zero. The vectored writer
copied just the 17-byte request prefix and the 13-byte response prefix.
A smaller peer window or frame limit still split the message across DATA
frames. An 8 KiB send buffer used the per-segment path.
[Paired copy counts and peer-limit controls](https://github.com/mingley/pure-protobuf/actions/runs/45267810626/artifacts/14863238453)

### Forwarding a nested message

A generated encoder can forward the cached wire bytes of
a singular nested message to a segmented sink. The child must occupy at
least 32 KiB and at least a quarter of the logical input buffer passed to
`parse_bytes`. Mutating the child makes the encoder serialize its fields.
A caller-supplied slice can itself refer to a larger allocation.

For a 1 MiB child, a pinned-CPU test retained the original 1,048,583-byte
wire window and reserved four bytes for the parent header. Forwarding to
the segmented sink took 54.6 ns in the candidate and 22.0 µs in the base.
Writing the same message into a contiguous buffer took essentially the
same time in both versions. A child below the retention fraction took 2.0%
longer at the median. The test ends when the sink owns its segments.
[Paired forwarding results](https://github.com/mingley/pure-protobuf/actions/runs/45263902546/artifacts/14861234081)

### Tonic adapter (`protobuf-tonic`)

Tonic 0.14 assembles the incoming message and decompresses it when needed.
The adapter splits and freezes Tonic's decode buffer. Large generated string,
bytes and nested bytes fields can keep that same allocation alive after
the stream is dropped. On send, generated encoders write directly into
Tonic's contiguous output buffer. Verification covers identity and gzip
receive, and identity send.
[Codec ownership tests](../protobuf-tonic/tests/codec_buffers.rs)

### gRPC-Web text

Text mode converts the binary body to base64. The encoder
keeps up to two raw bytes between segments and writes directly into the
encoded output. On two hosted runners, encoding a segmented 1 MiB body
took 13.4% and 11.1% less time across ten pairs per runner. Requested
heap bytes fell from 2,446,714 to 1,398,104 per body on both. The
six-byte contiguous control kept one allocation and changed by −5.0%
and +0.3% in elapsed time. These timings end before the HTTP write.
[Samples and reproduction source](https://github.com/mingley/pure-protobuf/actions/runs/45267765475/artifacts/14864030810)

## What shipped

| Change | Effect | Record |
|---|---|---|
| Parse straight from the network buffer (`Parse::parse_bytes`) | Receive: one full-message copy removed. `bytes` fields of 4 KiB or more point into the received buffer instead of being copied. Smaller fields are copied so they do not keep a large buffer alive. | [PK-09 evidence](evidence/pk09-zero-copy.md) |
| Setters accept `bytes::Bytes` without copying | Your buffer is stored by reference count, not copied. | [PK-09 evidence](evidence/pk09-zero-copy.md) |
| Send large fields as separate segments | Send: a `bytes` field of 32 KiB or more is passed to HTTP/2 as its own buffer instead of being copied into the frame. | [PK-11 evidence](evidence/pk11-send.md) |

Measured effect on one Apple M4 Pro, loopback, closed loop (diagnostic only).
Each change was measured against the build just before it, in RPCs per second:

| Change | Workload | Before | After | Change |
|---|---|---|---|---|
| Receive (PK-09) | Unary, 1 MiB | 950 | 1,095 | +15% |
| Receive (PK-09) | Unary, 4 MiB | 208 | 230 | +11% |
| Receive (PK-09) | Unary, 8 MiB | 114 | 120 | +5% |
| Receive (PK-09) | Client streaming, 8 × 1 MiB | 218 | 233 | +7% |
| Send (PK-11) | Upload, 8 × 1 MiB, plaintext | ~226 | ~238 | +5.5% |
| Send (PK-11) | Upload, 8 × 1 MiB, TLS | 182 | 201 | +10% |
| Send (PK-11) | Server streaming, 8 × 1 MiB | 232 | 239 | +3% |
| Send (PK-11) | Unary, 8 MiB | ~115.5 | ~115.0 | flat (within noise) |

## How to use it

Nothing to turn on. Code generated by a current `protoc-gen-pbrs` uses the
zero-copy paths automatically, and `pbrs-grpc` parses incoming messages with
`parse_bytes`.

When building a message, pass `bytes::Bytes` (re-exported as
`pbrs::rt::Bytes`) to retain a shareable buffer. Moving an owned `Vec<u8>`
into a singular, repeated or map bytes setter also transfers its storage
without copying. Singular setters also accept owned `Cow<[u8]>` and
`Box<[u8]>` without copying. Borrowed slices and borrowed `Cow` values are
copied.

A moved nonempty vector keeps its spare capacity until the last alias is
dropped. If a short payload comes from a much larger allocation, pass its
slice to copy just the payload instead. Empty moved vectors release their
allocation.

```rust
let blob = pbrs::rt::Bytes::from(load_blob());
let mut msg = BlobChunk::new();
msg.set_data(blob); // transfers ownership of the buffer
```

To parse a buffer you already hold as `Bytes`:

```rust
let msg = BlobChunk::parse_bytes(frame)?; // large bytes fields point into `frame`
```

Accessors still return `&[u8]`, so existing code keeps working.

## Things to know

- **Memory retention.** A parsed `bytes` field of 4 KiB or more keeps the whole
  received buffer alive until the message is dropped. If you cache a small
  piece of a large message for a long time, copy it out first. See
  [resource budgets](resource-budgets.md#22-the-backing-buffer-retention-hazard).
- **Field type and sink matter.** Large `bytes` fields and eligible cached
  singular messages can be emitted as separate segments. String fields and
  materialized children follow their own encoding rules. [Runtime design](design.md)
- **Compression.** Gzip consumes a contiguous plaintext payload and produces
  new output bytes.
- **Code generated before this change** still compiles and runs, but it sends
  large fields by copying. Regenerate to get the send-side win.

## What was tried and rejected

Each step had to prove itself on measurements before it could merge.

| Idea | Result | Why it was dropped |
|---|---|---|
| Pair 2 MiB fields on peers configured for larger DATA frames | Removed 4,194,329 reassembly bytes per round trip. Throughput fell in three paired runs. | The 1 MiB cap preserves large-message throughput in these tests. [Larger-frame experiment](benchmarks/native-data-20261123.md#two-mib-field-experiment) |
| Reserve the whole message buffer up front when a message arrives in pieces | 5× less copying in isolation, but **18% slower** end to end at 8 MiB | Slower in practice; the reader waited longer on the socket. [PK-09 evidence](evidence/pk09-zero-copy.md#what-was-reverted-1a-and-why) |
| Ask peers for very large HTTP/2 frames (16 MiB) | No faster, slightly slower | The sender in that test emitted the envelope separately. Raising the limit alone left the message in pieces. [PK-10 gate](evidence/pk10-gate.md) |
| Parse messages that arrive in pieces without joining them first | Not built | The copy it would remove costs only 1.9–2.8% of server CPU. The bar was 5%. [PK-10 gate](evidence/pk10-gate.md) |

## What is still open

- **TLS copies.** The built-in Graviola provider copies plaintext into a
  record before encryption. Rustls returns decrypted plaintext through a read
  buffer. RX-04 tracks provider benchmarks and RX-05 tracks kernel TLS.
  [rustls buffer discussion](https://github.com/rustls/rustls/pull/3292)
- **Fragmented messages.** When the peer's DATA limit or flow-control window
  splits the payload, the parser still needs contiguous bytes for each field.
- **Qualified measurements.** The table under “What shipped” uses one laptop.
  The newer copy counts and timings use hosted Linux runners. The
  [benchmark contract](benchmark-contract.md) describes separate-host qualification.

## More detail

- [Historical Tonic buffer-chain proposal](https://github.com/grpc/grpc-rust/pull/1559)
- [Original zero-copy design and measurement plan](plan/pbrs-zero-copy-large-payloads-plan.md)
- [Original large-payload baseline](evidence/large-payload-baseline.md)
