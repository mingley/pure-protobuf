# Benchmarks

## Method

Every row uses the same `.proto`. Most cases are
`TestAllTypesProto3` (TAT), Google's kitchen-sink conformance message.
pbrs types are plugin-generated.
Competitors are prost 0.13 (`prost-build` of that proto), crates.io
`protobuf` 4.35.1-release (`protoc --rust_out kernel=upb`), buffa 0.9.1
owned, and buffa `decode_view` where it exists.

Decode uses pbrs wire bytes. `./bench` (from `bench/`) runs 40000
iterations and reports the median of 15 after warmup.

Builds are release, thin LTO, one codegen unit.
`size_of::<TestAllTypesProto3>()` is 648. Default is ~19 ns.

Each cell is encode ns / decode ns. Payload is encoded size in bytes.
Buffa view has no encode, so that side is `n/a`.

`./bench` exits non-zero if a gated case loses encode or owned decode to
prost, v4, or buffa owned. Twelve cases are gated: the original nine plus
`packed_fixed64_256`, `packed_float_256`, and `repeated_nested_8`. Buffa
view is gated except `tat_populated`, `person`, and the packed-fixed rows
(view does not build an owned `Vec`; person and `tat_populated` sit in a
~3% band versus buffa view and are not a process gate).

JSON, text, proto2 required, maps larger than 64, and WKT are not gated.
1 MiB and 5 MiB rows are reported below and are not gated. Iters drop
with payload (120x9 at 1 MiB, 40x7 at 5 MiB) so the timer stays
memcpy-bound rather than a 40k-iter wall clock.

Numbers below are one Apple M4 Pro. Two consecutive
`./target/release/bench` runs; the second capture is below. Both runs
exited 0 (all twelve gated cases still win encode and owned decode).

## Gated

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| empty TAT | 0 | **26 / 20** | 83 / 130 | 144 / 74 | 67 / 163 | n/a / 115 |
| person | 62 | **35 / 83** | 40 / 198 | 73 / 159 | 42 / 154 | n/a / 81 |
| TAT populated | 87 | **91 / 306** | 161 / 447 | 244 / 398 | 133 / 393 | n/a / 305 |
| packed varint 256 | 388 | **77 / 246** | 537 / 806 | 465 / 893 | 584 / 358 | n/a / 393 |
| map 64 | 500 | **258 / 939** | 663 / 2316 | 989 / 3147 | 411 / 1767 | n/a / 1102 |
| nested depth 8 | 26 | **245 / 130** | 2343 / 1059 | 1131 / 354 | 629 / 1378 | n/a / 1457 |
| strings | 163 | **62 / 153** | 118 / 328 | 191 / 173 | 104 / 311 | n/a / 190 |
| unpacked varint 256 | 896 | **448 / 1141** | 531 / 1592 | 767 / 2513 | 811 / 1601 | n/a / 2440 |
| packed fixed32 256 | 1028 | **53 / 90** | 199 / 715 | 223 / 129 | 176 / 195 | n/a / 151 |
| packed fixed64 256 | 2052 | **65 / 94** | 221 / 752 | 253 / 140 | 182 / 197 | n/a / 155 |
| packed float 256 | 1028 | **53 / 89** | 236 / 715 | 220 / 135 | 192 / 198 | n/a / 147 |
| repeated nested 8 | 38 | **62 / 142** | 120 / 217 | 198 / 287 | 118 / 282 | n/a / 255 |

person uses handwritten `pbrs::testdata::Person` (inline small repeats).
Everything else is generated TestAllTypesProto3.

## Extended

Reported, not gated.

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| bytes | 315 | **73 / 168** | 134 / 508 | 224 / 204 | 122 / 384 | n/a / 219 |
| scalars (bool/enum/float/packed bool) | 77 | **68 / 167** | 163 / 318 | 182 / 223 | 101 / 228 | n/a / 226 |
| unpacked fixed32 256 | 1536 | **218 / 801** | 289 / 1451 | 622 / 1661 | 594 / 1508 | n/a / 2096 |
| oneof string | 23 | **39 / 87** | 96 / 143 | 149 / 99 | 82 / 169 | n/a / 122 |

## Losses

No gated encode or owned-decode loss. `tat_populated` versus buffa view
is a coin-flip on this capture (306 vs 305 ns) and is still not
process-gated. Person versus buffa view is the same ~3% band (83 vs 81)
and is not process-gated. Packed-fixed view rows win on this host but
are not process-gated: view does not materialize a `Vec`.

## Large payloads (reported, not gated)

Same TAT schema. Cells are **microseconds** (encode / decode), not
nanoseconds. One Apple M4 Pro; second of two runs.

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| bytes 1 MiB | 1,000,004 | 12.1 / 12.0 | 12.3 / 24.4 | 12.2 / 12.5 | 12.2 / 12.3 | n/a / **0.12** |
| bytes 5 MiB | 5,000,005 | 61.6 / 59.9 | 61.7 / 123.5 | 60.6 / 60.7 | 65.1 / 60.7 | n/a / **0.12** |
| packed fixed32 1 MiB | 1,000,005 | 12.0 / 12.0 | 158 / 446 | **12.1 / 11.7** | 127 / 12.3 | n/a / 12.7 |
| packed fixed32 5 MiB | 5,000,006 | 72.7 / 65.7 | 788 / 2527 | **60.5 / 66.2** | 629 / 66.3 | n/a / 64.8 |

At 1-5 MiB the v4 Arena/FFI tax is gone. Owned encode/decode of a bytes
blob is a memcpy of the payload. pbrs, v4, and buffa owned sit in the
same band. prost decode is about 2x. buffa `decode_view` on bytes does
not copy (~0.12 µs).

packed-fixed is memcpy for pbrs and v4, and a recode for prost / buffa
owned encode. At 5 MiB v4 encode is a bit faster (60 vs 66 µs). Decode
is a few percent either way. packed-fixed view still copies; it is not
the bytes-view shortcut.

## Why v4 encode is large on small sizes

Every v4 `serialize` allocates an Arena, calls FFI `upb_Encode`, and
copies to `Vec`. Codec work on <1 KiB is tens of ns. Setup is hundreds.
See `docs/upb.md`.

## Re-run

```bash
cd bench && cargo build --release && ./target/release/bench
```

## tonic Codec survey (Apple M4 Pro)

Same-process encode into `BytesMut` (`Serialize::encode` / prost
`Message::encode`). v4 is `Serialize::serialize` (new Arena + FFI +
copy; no EncodeBuf). Not kernel `./bench`. Not in CI. Two consecutive
`./target/release/tonic-bench` runs; second capture below.

`hello` / `hello_4kib` are the old 1-string rows. Everything else is
`proto/codec_cases.proto`: one message per common unary shape, so
gencode is specialized (hello-sized), not TestAllTypes.

Cells are encode ns / decode ns. Combined win/loss is pbrs vs that
column.

`tonic-bench` exits non-zero if `name_4kib` or `blob_4kib` combined
encode+decode loses to prost, if `rpc_sparse` decode loses to prost, or
if `tags_32` decode loses to v4.

### Published 1-string

| case | payload | pbrs | prost | v4 upb | vs prost | vs v4 |
|---|---:|---:|---:|---:|---|---|
| hello | 5 | 5.5 / **10.5** | **3.8** / 19.5 | 35.3 / 43.3 | win | win |
| hello_4kib | 4099 | **44.4 / 85.3** | 47.0 / 142.7 | 96.0 / 243.2 | win | win |

### Common shapes

| case | payload | pbrs | prost | v4 upb | vs prost | vs v4 |
|---|---:|---:|---:|---:|---|---|
| empty | 0 | 1.5 / 1.6 | **0.3 / 0.3** | 25.8 / 31.4 | loss | win |
| id | 2 | **3.3 / 2.6** | 4.3 / 3.3 | 32.9 / 38.6 | win | win |
| scalars | 23 | **14.4 / 15.2** | 20.6 / 16.2 | 47.7 / 57.2 | win | win |
| name_short | 5 | 5.0 / **10.9** | **3.8** / 19.8 | 34.7 / 42.5 | win | win |
| name_80 | 82 | 6.4 / 25.3 | **4.6 / 23.7** | 33.4 / 42.1 | loss | win |
| name_4kib | 4099 | 50.3 / **86.8** | **46.0** / 142.9 | 97.2 / 242.0 | win | win |
| blob_32 | 34 | **5.3 / 21.1** | 5.5 / 35.5 | 34.9 / 39.8 | win | win |
| blob_4kib | 4099 | **43.1 / 64.0** | 47.0 / 127.5 | 96.4 / 98.3 | win | win |
| blob_64kib | 65540 | **564 / 738** | 564 / 1690 | 704 / 760 | win | win |
| envelope | 30 | **17.8 / 50.0** | 27.6 / 55.3 | 50.9 / 69.7 | win | win |
| nest_d4 | 14 | **20.0 / 49.4** | 39.5 / 57.6 | 54.2 / 75.4 | win | win |
| packed_16 | 18 | **5.8 / 36.5** | 31.3 / 82.5 | 41.4 / 80.6 | win | win |
| packed_256 | 387 | **9.7 / 133** | 912 / 723 | 343 / 807 | win | win |
| tags_4 | 27 | 19.1 / **62.0** | **17.7** / 113.1 | 43.8 / 70.8 | win | win |
| tags_32 | 160 | **111 / 276** | 154 / 849 | 116 / 380 | win | win |
| map_8 | 172 | **87 / 266** | 122 / 710 | 125 / 466 | win | win |
| oneof_ok | 6 | **5.4 / 19.9** | 5.8 / 20.9 | 34.3 / 44.9 | win | win |
| rpc_mixed | 176 | **96 / 331** | 164 / 680 | 154 / 347 | win | win |
| rpc_sparse | 2 | **4.0 / 3.6** | 7.2 / 14.0 | 37.4 / 36.4 | win | win |

v4 loses every row. Typical unary `rpc_mixed` is already ~2× prost.
Packed encode is the cached-bytes path (10 vs 912 vs 343).

`tags_32` decode is **276 vs 380** vs v4 (process-gated). Repeated
length-delimited strings now same-tag run and reserve, matching unpacked
scalars. `name_4kib` combined is **137 vs 189** ns vs prost (decode 87
vs 143). `blob_4kib` decode 64 vs 128. `rpc_sparse` decode **3.6 vs 14.0**.

### What to chase

Do not spend the next pass on:

- prost empty (ZST, 0.3 ns)
- short-string encode (5.0 vs 3.8)
- flatten `merge_inner` (#39, made hello worse)

`rpc_sparse` decode is process-gated (pbrs decode must beat prost).
`tags_32` decode is process-gated (pbrs decode must beat v4).

Worth measuring next:

- `name_80` combined (still a loss): 80-byte string is just over the SSO
  cutoff. Leftover is `merge_inner` after a draft heap-copy try (#57)
  that is not shipped.

Keep: packed canonical cache, bytes window, `simdutf8` string arm,
same-tag repeated strings, map/repeated vs prost.

Linux x86_64 1-string line of record after dropping the per-message
`Vec` (#31): hello 6.8 / 45.4 vs 3.8 / 22.1 (combined 52.2 vs 25.8).
4 KiB 36.8 / 153.8 vs 32.7 / 133.4. That host is not this one.

```bash
cd tonic-bench && cargo build --release && ./target/release/tonic-bench
```

## pbrs-grpc vs tonic 0.14 (transport)

Excluded crate `rpc-bench/`. Both sides serve the same official
`grpc.testing.TestService`, generated by `protoc-gen-pbrs` from the same
`.proto`, over the same pbrs codec. The only variable is the gRPC
transport, so a delta here is a transport delta and not a serialization
one.

Host: 4-core Intel Xeon, Linux x86_64. Client, kernel server, and tonic
server all share one tokio runtime, so absolute numbers are contended;
the ratio is the number to read. Three consecutive release runs, all
exited 0.

### Unary latency

`empty_unary` is a 0-byte request and reply. `large_unary` is a
271828-byte request and a 314159-byte reply. 2000 and 200 samples after
warmup.

| run | case | kernel p50 | tonic p50 | kernel p99 | tonic p99 |
|---|---|---:|---:|---:|---:|
| 1 | empty | **54.6 µs** | 88.7 µs | **186 µs** | 42.6 ms |
| 2 | empty | **54.9 µs** | 86.1 µs | **191 µs** | 41.7 ms |
| 3 | empty | **53.9 µs** | 87.4 µs | **110 µs** | 41.8 ms |
| 1 | large | **616 µs** | 1.48 ms | **1.69 ms** | 44.9 ms |
| 2 | large | **822 µs** | 1.71 ms | **1.60 ms** | 45.3 ms |
| 3 | large | **821 µs** | 1.49 ms | **1.04 ms** | 45.3 ms |

Process-gated: the kernel must be strictly faster on both p50 and p99, on
both cases.

The p99 gap is two orders of magnitude, not a rounding difference. tonic's
tail sits at 42-45 ms on every run because its `Channel` is a `tower`
stack with a buffer in front; the kernel issues the request on the calling
task and has nothing to queue behind.

### Sustained unary throughput

Reported, not gated: it moves with core count and scheduler luck. Best of
three 2-second windows per side. `conns` is HTTP/2 connections
(`ChannelConfig::connections` against N tonic channels). Nonzero RPC
errors fail the process, so these are also a correctness check.

| case | conc | conns | kernel QPS | tonic QPS |
|---|---:|---:|---:|---:|
| empty | 1 | 1 | **73.6k** | 2.5-2.9k |
| empty | 16 | 4 | **83.7-84.0k** | 21-27k |
| large | 1 | 1 | **2.4-3.0k** | 90 |
| large | 16 | 4 | **8.3-8.5k** | 5.8-6.2k |

At `conc=1` the kernel sustains close to its inverse latency
(73.6k ≈ 1/13.6 µs of client-side work per RPC), while tonic sustains far
below its own measured latency. That is the same buffering that produces
its p99.

### Server-streaming throughput

One server-streaming RPC of 2000 messages × 1 KiB, best of eight rounds.
Six consecutive runs:

| run | kernel msgs/s | tonic msgs/s | ratio |
|---|---:|---:|---:|
| 1 | **1093k** | 917k | 1.19x |
| 2 | **1028k** | 1025k | 1.00x |
| 3 | **1055k** | 916k | 1.15x |
| 4 | **1393k** | 890k | 1.57x |
| 5 | **849k** | 822k | 1.03x |
| 6 | 825k | 879k | 0.94x |
| median | **1041k** | 903k | **1.15x** |

The kernel is ahead on five of six runs and by 15% at the median, but the
per-run spread is wide enough that a strictly-faster gate would fail on
noise, so the gate is 90% of tonic. That still catches a real regression:
this axis started at **0.24x** (201k against 670k).

Four changes closed it, in the order they mattered:

1. **Batched output.** gRPC messages are length-prefixed, so one HTTP/2
   DATA frame may carry any number of them. Writing one frame per message
   cost a wakeup and often a syscall per message. `OutBatch` accumulates up
   to 32 KiB and writes once. 201k → 419k.
2. **Inline inbound decoding.** `Streaming` reads and decodes on the task
   that calls `message()`, rather than behind a spawned pump task and a
   channel. That removes a task, a queue, and a copy per message on both
   sides. 419k → 843k.
3. **No speculative flow-control reservation.** `h2` buffers up to the
   connection's send budget without waiting, so reserving capacity first was
   a needless round trip through the connection task. Also 12% off large
   unary latency.
4. **Yielding to a saturated producer.** A producer running ahead of the
   network is bounded by its channel depth, so draining it yields only that
   many messages and the write is smaller than it could be. One `yield_now`
   before flushing lets it top the queue up, halving the writes and the
   wakeups. 843k → 1041k median.

Step 4 is conditional on more than one message being queued. Exactly one
means the producer is *not* ahead — a request/response stream, say — and a
scheduling turn there would be pure added latency. That is why unary
latency and the `ping_pong` interop case are unaffected by it.

The remaining structural difference is one task hop: hyper drains a response
body on the connection task, while the kernel drains it on the per-RPC task.
Closing it would mean polling active response streams from the accept loop,
which is a different architecture rather than a tuning change; the batching
above is what makes that hop cheap enough not to decide the result.

### Bidi ping-pong throughput (loopback)

One bidi `FullDuplexCall` of 256 empty request/response pairs, best of eight
rounds after warmup, reported as round-trips/s. Process-gated at 90% of tonic,
the same band as server-streaming: each round-trip waits for a response
before the next request, so scheduler luck shows up as RTT. This axis is a
loopback capture in `rpc-bench`; it is not the 4-core Xeon unary/QPS/stream
tables above.

This host, one release run, kernel and tonic sharing one process:

| kernel round-trips/s | tonic round-trips/s | ratio |
|---|---:|---:|
| **33919** | 22595 | **1.50x** |

The 90% gate passed. These numbers are not the Xeon tables.

### Client-streaming upload throughput (loopback)

One client-streaming `StreamingInputCall` of 2000 messages × 1 KiB, best of
eight rounds after warmup, reported as messages/s. Process-gated at 90% of
tonic, the same band as server-streaming. This axis is a loopback capture in
`rpc-bench`; it is not the 4-core Xeon unary/QPS/stream tables above.

This host, one release run, kernel and tonic sharing one process:

| kernel msgs/s | tonic msgs/s | ratio |
|---|---:|---:|
| **1447599** | 555431 | **2.61x** |

The 90% gate passed. These numbers are not the Xeon tables.

### Re-run

```bash
cd rpc-bench && cargo build --release && ./target/release/rpc-bench
```

## pbrs-grpc vs grpc-go (server)

`rpc-bench` puts client and both servers in one process, which makes it
sensitive to scheduler luck. `scripts/grpc-server-bench.sh` narrows the
question instead: one kernel client, the same `grpc.testing.TestService`, the
same payloads, pointed at two servers in separate processes. The only variable
is the server.

Same 4-core Xeon. Three rounds, 2000 `empty_unary` and 200 `large_unary`
samples each, nanoseconds:

| Round | Server | empty p50 | empty p99 | large p50 | large p99 |
|---|---|---:|---:|---:|---:|
| 1 | **kernel** | **53232** | **71064** | **596055** | **779373** |
| 1 | grpc-go | 77763 | 109962 | 910288 | 1658270 |
| 2 | **kernel** | **54387** | **66724** | **595006** | **863454** |
| 2 | grpc-go | 75316 | 116663 | 939219 | 1587095 |
| 3 | **kernel** | **54260** | **66371** | **602411** | **849000** |
| 3 | grpc-go | 77643 | 113256 | 900704 | 1497070 |

Roughly 1.4x on `empty_unary` p50, 1.7x on its p99, 1.5x on `large_unary` p50,
and 1.8x on its p99. Round-to-round spread is a few percent, because unlike
`rpc-bench` the two servers are not competing for the same runtime.

Reported, not gated: the script needs a Go toolchain and network access to
fetch `google.golang.org/grpc`, so it is not something CI should depend on.

### Bidi ping-pong and upload vs grpc-go (loopback)

The same kernel client also measures empty bidi ping-pong (256 pairs) and
client-streaming upload (2000 × 1 KiB), best of eight rounds after warmup.
This is a loopback capture in `scripts/grpc-server-bench.sh`; it is not the
4-core Xeon unary tables above.

This host, three rounds, kernel client, two servers in separate processes.
Reported, not gated. Later rounds are noisier on a contended machine.

| Round | Server | ping_pong rps | upload msgs/s |
|---|---|---:|---:|
| 1 | **kernel** | **57899** | **695945** |
| 1 | grpc-go | 40998 | 538945 |
| 2 | **kernel** | **58259** | **874125** |
| 2 | grpc-go | 9522 | 464726 |
| 3 | **kernel** | **14103** | **823888** |
| 3 | grpc-go | 7681 | 315210 |

Best of three: ping_pong **58259** vs 40998 (~1.42x), upload **874125** vs
538945 (~1.62x). These numbers are not the Xeon tables.

```bash
./scripts/grpc-server-bench.sh
```
