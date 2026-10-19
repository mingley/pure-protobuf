# Benchmarks

Recorded local measurements for `pbrs` and `pbrs-grpc`. A number applies only
to the workload, versions, and host named in its section.

**Time and memory:** lower is better. **Throughput:** higher is better.

A public comparison has to meet the [benchmark contract](benchmark-contract.md).
The tables here are the recorded runs, not that campaign.

## At a glance

| Comparison | Result in the recorded run |
|---|---|
| Codec vs prost, v4 upb, and buffa owned | pbrs wins encode and owned decode on all 12 gated cases (Apple M4 Pro). |
| Common message shapes vs prost and v4 | pbrs wins most cached-encode-plus-parse rows. prost is faster on `empty`, `id`, and `name_80`. |
| gRPC transport vs tonic 0.14, same codec, matched settings | pbrs-grpc is faster on p50 latency, unary throughput, server-streaming, and upload. tonic is faster on empty-unary p99 and ping-pong (Apple M4 Pro, two runs). |
| gRPC server vs grpc-go, same client | The native server is faster on unary latency in three rounds on a 4-core Xeon. |

Buffa **view** borrows the input buffer instead of copying it. It is not an
owned decode, so it is not part of the owned-decode gate. On a bytes payload
it can look much faster because it does not copy.

## Codec

Host: Apple M4 Pro. Release build, thin LTO, one codegen unit. `./bench`
runs 40,000 iterations and reports the median of 15 samples after warmup.
Each cell is **encode / decode** in nanoseconds. Payload is the encoded size.

pbrs types are generated from `TestAllTypesProto3`, except `person`, which is
the handwritten `pbrs::testdata::Person`. Competitors: prost 0.13, protobuf
4.35.1 (`kernel=upb`), buffa 0.9.1 owned, and buffa view where it exists.
View has no encode (`n/a`).

Both of two consecutive runs exited 0. The second run is below. All 12 gated
rows still win encode and owned decode against prost, v4, and buffa owned.

### Gated cases

| Case | Bytes | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| empty | 0 | 26 / 20 | 83 / 130 | 144 / 74 | 67 / 163 | n/a / 115 |
| person | 62 | 35 / 83 | 40 / 198 | 73 / 159 | 42 / 154 | n/a / 81 |
| TAT populated | 87 | 91 / 306 | 161 / 447 | 244 / 398 | 133 / 393 | n/a / 305 |
| packed varint 256 | 388 | 77 / 246 | 537 / 806 | 465 / 893 | 584 / 358 | n/a / 393 |
| map 64 | 500 | 258 / 939 | 663 / 2316 | 989 / 3147 | 411 / 1767 | n/a / 1102 |
| nested depth 8 | 26 | 245 / 130 | 2343 / 1059 | 1131 / 354 | 629 / 1378 | n/a / 1457 |
| strings | 163 | 62 / 153 | 118 / 328 | 191 / 173 | 104 / 311 | n/a / 190 |
| unpacked varint 256 | 896 | 448 / 1141 | 531 / 1592 | 767 / 2513 | 811 / 1601 | n/a / 2440 |
| packed fixed32 256 | 1028 | 53 / 90 | 199 / 715 | 223 / 129 | 176 / 195 | n/a / 151 |
| packed fixed64 256 | 2052 | 65 / 94 | 221 / 752 | 253 / 140 | 182 / 197 | n/a / 155 |
| packed float 256 | 1028 | 53 / 89 | 236 / 715 | 220 / 135 | 192 / 198 | n/a / 147 |
| repeated nested 8 | 38 | 62 / 142 | 120 / 217 | 198 / 287 | 118 / 282 | n/a / 255 |

`person` and TAT populated are within about 3% of buffa view. Those two view
rows are reported, not process-gated. View does not build an owned `Vec`.

### Other small cases

Reported, not gated. Same units.

| Case | Bytes | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| person, generated layout | 62 | 37 / 198 | 40 / 197 | 76 / 162 | 40 / 160 | n/a / 80 |
| bytes | 315 | 73 / 168 | 134 / 508 | 224 / 204 | 122 / 384 | n/a / 219 |
| scalars | 77 | 68 / 167 | 163 / 318 | 182 / 223 | 101 / 228 | n/a / 226 |
| unpacked fixed32 256 | 1536 | 218 / 801 | 289 / 1451 | 622 / 1661 | 594 / 1508 | n/a / 2096 |
| oneof string | 23 | 39 / 87 | 96 / 143 | 149 / 99 | 82 / 169 | n/a / 122 |

The handwritten `person` decode (83 ns) and the generated layout (198 ns) are
two complete implementations of the same 62-byte message. The gap is not
isolated to one container type.

### Large payloads

Same schema. Cells are **encode / decode in microseconds**.

| Case | Bytes | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| bytes 1 MiB | 1,000,004 | 12.1 / 12.0 | 12.3 / 24.4 | 12.2 / 12.5 | 12.2 / 12.3 | n/a / 0.12 |
| bytes 5 MiB | 5,000,005 | 61.6 / 59.9 | 61.7 / 123.5 | 60.6 / 60.7 | 65.1 / 60.7 | n/a / 0.12 |
| packed fixed32 1 MiB | 1,000,005 | 12.0 / 12.0 | 158 / 446 | 12.1 / 11.7 | 127 / 12.3 | n/a / 12.7 |
| packed fixed32 5 MiB | 5,000,006 | 72.7 / 65.7 | 788 / 2527 | 60.5 / 66.2 | 629 / 66.3 | n/a / 64.8 |

Owned encode and decode of a large bytes blob is a copy. pbrs, v4, and buffa
owned are in the same band. prost decode is about twice as slow. Buffa view
on bytes does not copy (0.12 µs). At 5 MiB, v4 packed-fixed32 encode is faster
than pbrs (60.5 vs 72.7 µs).

Small v4 encodes are slower because each `serialize` allocates an arena, calls
into C, and copies into a `Vec`. See [upb](upb.md). The copying
`parse(&[u8])` path is what these rows time. The shared-buffer path is
[zero-copy](zero-copy.md).

```bash
cd bench && cargo build --release && ./target/release/bench
```

## Message shapes

Host: Apple M4 Pro. Same-process encode and parse, not the kernel `./bench`
above. v4 is `Serialize::serialize` (new arena, C call, copy). Two consecutive
`tonic-bench` runs; the second is below. Cells are nanoseconds.

The fresh-encode column is from an older harness. It estimated first encode by
subtracting medians, and clamped the result to cached encode. Treat **cached
encode** and **parse** as the comparison. The current `tonic-bench` times
first encode directly; these historical rows have not been rerun.

`vs prost` and `vs v4` compare pbrs cached encode plus parse.

### One string

| Case | Bytes | pbrs encode fresh/cached | pbrs decode parse/touch | prost encode/decode/touch | v4 encode/decode/touch | vs prost | vs v4 |
|---|---:|---:|---:|---:|---:|---|---|
| hello | 5 | 6.1 / 5.5 | 11.4 / 11.7 | 1.5 / 22.2 / 23.3 | 34.2 / 44.3 / 49.9 | faster | faster |
| hello_4kib | 4099 | 53.8 / 46.1 | 86.8 / 86.7 | 48.4 / 147.3 / 148.8 | 105.6 / 274.2 / 262.0 | faster | faster |

### Common shapes

| Case | Bytes | pbrs encode fresh/cached | pbrs decode parse/touch | prost encode/decode/touch | v4 encode/decode/touch | vs prost | vs v4 |
|---|---:|---:|---:|---:|---:|---|---|
| empty | 0 | 2.0 / 1.3 | 0.3 / 0.3 | 0.3 / 0.3 / 0.3 | 27.5 / 32.2 / 32.4 | slower | faster |
| id | 2 | 3.7 / 3.3 | 2.3 / 2.0 | 1.0 / 3.6 / 3.6 | 31.9 / 38.3 / 44.8 | slower | faster |
| scalars | 23 | 14.2 / 13.8 | 12.9 / 12.9 | 21.1 / 16.1 / 16.5 | 48.2 / 58.3 / 91.9 | faster | faster |
| name_short | 5 | 5.6 / 4.7 | 9.2 / 9.8 | 1.5 / 24.9 / 23.3 | 35.1 / 45.4 / 51.1 | faster | faster |
| name_80 | 82 | 6.4 / 6.4 | 21.9 / 20.7 | 4.4 / 23.7 / 22.2 | 32.2 / 43.5 / 48.6 | slower | faster |
| name_4kib | 4099 | 55.3 / 45.1 | 92.9 / 89.7 | 48.1 / 144.4 / 148.5 | 106.1 / 265.4 / 263.0 | faster | faster |
| blob_32 | 34 | 5.6 / 5.0 | 17.9 / 18.1 | 5.7 / 38.6 / 39.8 | 35.4 / 40.9 / 47.8 | faster | faster |
| blob_4kib | 4099 | 75.0 / 44.2 | 65.4 / 65.2 | 46.4 / 132.0 / 156.5 | 101.5 / 106.4 / 107.3 | faster | faster |
| blob_64kib | 65540 | 1044.5 / 588.7 | 741.8 / 1310.7 | 578.9 / 1674.2 / 1657.4 | 713.9 / 740.7 / 743.1 | faster | faster |
| envelope | 30 | 18.2 / 18.2 | 51.4 / 97.4 | 28.0 / 53.5 / 57.0 | 50.4 / 77.5 / 106.8 | faster | faster |
| nest_d4 | 14 | 20.3 / 20.3 | 50.4 / 144.9 | 40.0 / 56.6 / 59.5 | 52.4 / 78.0 / 122.4 | faster | faster |
| packed_16 | 18 | 147.0 / 7.0 | 37.6 / 132.6 | 32.3 / 85.4 / 89.3 | 40.2 / 82.1 / 146.1 | faster | faster |
| packed_256 | 387 | 1233.2 / 9.7 | 149.2 / 832.0 | 925.4 / 736.5 / 740.5 | 365.4 / 820.2 / 1805.3 | faster | faster |
| tags_4 | 27 | 19.2 / 19.2 | 71.3 / 69.4 | 18.9 / 115.2 / 112.8 | 44.0 / 74.9 / 96.4 | faster | faster |
| tags_32 | 160 | 145.9 / 113.8 | 304.3 / 336.4 | 158.4 / 865.3 / 859.2 | 118.1 / 388.2 / 526.6 | faster | faster |
| map_8 | 172 | 95.8 / 88.8 | 287.3 / 439.7 | 119.8 / 724.8 / 729.4 | 126.2 / 452.0 / 470.6 | faster | faster |
| oneof_ok | 6 | 6.0 / 5.5 | 22.2 / 21.3 | 4.9 / 27.1 / 26.0 | 32.8 / 43.8 / 50.4 | faster | faster |
| rpc_mixed | 176 | 218.3 / 94.9 | 349.5 / 527.7 | 155.4 / 708.0 / 701.3 | 152.9 / 366.8 / 484.9 | faster | faster |
| rpc_sparse | 2 | 4.9 / 4.9 | 4.5 / 5.3 | 7.5 / 16.5 / 16.3 | 39.5 / 36.4 / 41.6 | faster | faster |

prost losses are `empty`, `id`, and `name_80`. v4 loses every cached-encode
plus parse row in this capture. A cached packed encode (9.7 ns) is a replay
of bytes already encoded; the 1233 ns fresh cell is the old estimate, not a
measured cold/cached ratio.

```bash
PROTOC="$PWD/target/pinned-protoc-build/protoc" CARGO_BUILD_JOBS=2 \
  cargo build --release --locked --manifest-path tonic-bench/Cargo.toml
./target/release/tonic-bench
```

## gRPC vs tonic

Both sides serve `grpc.testing.TestService` with the same pbrs codec. The
difference is the transport.

Use the **matched-settings** run below. An older Xeon run used different TCP
and HTTP/2 settings on the tonic peer. Those numbers are in
[Older unmatched runs](#older-unmatched-runs) and are not a transport
comparison.

### Matched settings (Apple M4 Pro)

TCP_NODELAY, 16 MiB windows, 1 MiB frames, 256 streams, no adaptive window,
16 KiB header list. Release. Two consecutive runs.

Latency is microseconds. The faster cell is marked.

| Case | pbrs-grpc p50 | tonic p50 | pbrs-grpc p99 | tonic p99 |
|---|---:|---:|---:|---:|
| empty, run 1 | **70.1** | 94.5 | 123.0 | **110.3** |
| empty, run 2 | **69.5** | 94.6 | 131.4 | **109.4** |
| large, run 1 | **345.6** | 378.8 | **384.7** | 463.4 |
| large, run 2 | **348.3** | 377.8 | **412.2** | 426.3 |

`empty` is a 0-byte request and reply. `large` is a 271,828-byte request and
a 314,159-byte reply.

| Case | Concurrency | Connections | pbrs-grpc QPS | tonic QPS |
|---|---:|---:|---:|---:|
| empty | 1 | 1 | **13.1k** | 9.8k |
| large | 1 | 1 | **2.5k** | 2.0k |
| empty | 16 | 4 | **30.0k** | 18.3k |
| large | 16 | 4 | **3.0k** | 2.2k |

| Workload | pbrs-grpc | tonic | Ratio |
|---|---:|---:|---:|
| Server stream, messages/s | **636–672k** | 461–472k | about 1.4× |
| Ping-pong, round-trips/s | 12.8–13.8k | **13.3–14.5k** | 0.95× |
| Upload, messages/s | **683–755k** | 305–333k | about 2.2× |

pbrs-grpc leads p50 latency, unary throughput, server-streaming, and upload.
tonic leads empty-unary p99 and ping-pong. Two runs on one laptop do not
fix the uncertainty of that p99 gap.

```bash
cd rpc-bench && cargo build --release && ./target/release/rpc-bench
```

`FAIRNESS` lines show the settings each endpoint actually used. `REFUSED`
means an endpoint did not match, and the comparison should not be read.

## gRPC server vs grpc-go

One native client, two servers in separate processes, same
`TestService` and payloads. The only change is the server. Host: 4-core
Intel Xeon. Three rounds. Latency is nanoseconds.

| Round | Server | empty p50 | empty p99 | large p50 | large p99 |
|---|---|---:|---:|---:|---:|
| 1 | pbrs-grpc | **53,232** | **71,064** | **596,055** | **779,373** |
| 1 | grpc-go | 77,763 | 109,962 | 910,288 | 1,658,270 |
| 2 | pbrs-grpc | **54,387** | **66,724** | **595,006** | **863,454** |
| 2 | grpc-go | 75,316 | 116,663 | 939,219 | 1,587,095 |
| 3 | pbrs-grpc | **54,260** | **66,371** | **602,411** | **849,000** |
| 3 | grpc-go | 77,643 | 113,256 | 900,704 | 1,497,070 |

About 1.4× on empty p50, 1.7× on empty p99, 1.5× on large p50, and 1.8× on
large p99.

Ping-pong is 256 empty pairs. Upload is 2,000 messages of 1 KiB. Best of
eight rounds after warmup. Later rounds are noisier.

| Round | Server | Ping-pong, round-trips/s | Upload, messages/s |
|---|---|---:|---:|
| 1 | pbrs-grpc | **57,899** | **695,945** |
| 1 | grpc-go | 40,998 | 538,945 |
| 2 | pbrs-grpc | **58,259** | **874,125** |
| 2 | grpc-go | 9,522 | 464,726 |
| 3 | pbrs-grpc | **14,103** | **823,888** |
| 3 | grpc-go | 7,681 | 315,210 |

Best of three: ping-pong 58,259 vs 40,998 (about 1.4×), upload 874,125 vs
538,945 (about 1.6×). Not a CI gate; the script needs a Go toolchain.

```bash
./scripts/grpc-server-bench.sh
```

## Older unmatched runs

These Xeon runs shared one Tokio runtime and did not match TCP or window
settings. The tonic tails (about 42 ms) and the 30× QPS gap came from that
mismatch. They are kept so the old capture is findable. Do not cite them as
a transport result. The [matched-settings](#matched-settings-apple-m4-pro)
section replaces them.

Unary latency, three runs. `empty` is 0 bytes. `large` is 271,828 in and
314,159 out.

| Run | Case | pbrs-grpc p50 | tonic p50 | pbrs-grpc p99 | tonic p99 |
|---|---|---:|---:|---:|---:|
| 1 | empty | 54.6 µs | 88.7 µs | 186 µs | 42.6 ms |
| 2 | empty | 54.9 µs | 86.1 µs | 191 µs | 41.7 ms |
| 3 | empty | 53.9 µs | 87.4 µs | 110 µs | 41.8 ms |
| 1 | large | 616 µs | 1.48 ms | 1.69 ms | 44.9 ms |
| 2 | large | 822 µs | 1.71 ms | 1.60 ms | 45.3 ms |
| 3 | large | 821 µs | 1.49 ms | 1.04 ms | 45.3 ms |

| Case | Concurrency | Connections | pbrs-grpc QPS | tonic QPS |
|---|---:|---:|---:|---:|
| empty | 1 | 1 | 73.6k | 2.5–2.9k |
| empty | 16 | 4 | 83.7–84.0k | 21–27k |
| large | 1 | 1 | 2.4–3.0k | 90 |
| large | 16 | 4 | 8.3–8.5k | 5.8–6.2k |

Server stream, 2,000 messages of 1 KiB, best of eight rounds:

| Run | pbrs-grpc msgs/s | tonic msgs/s | Ratio |
|---|---:|---:|---:|
| 1 | 1,093k | 917k | 1.19× |
| 2 | 1,028k | 1,025k | 1.00× |
| 3 | 1,055k | 916k | 1.15× |
| 4 | 1,393k | 890k | 1.57× |
| 5 | 849k | 822k | 1.03× |
| 6 | 825k | 879k | 0.94× |
| median | 1,041k | 903k | 1.15× |

Separate loopback captures, one process, not the Xeon tables: ping-pong
33,919 vs 22,595 round-trips/s (1.50×); upload 1,447,599 vs 555,431
messages/s (2.61×).

## Where to go next

| Question | Document |
|---|---|
| What counts as a publishable comparison? | [Benchmark contract](benchmark-contract.md) |
| Which measurement is current? | [Scoreboard](scoreboard.md) |
| How do I find a hot function? | [Profiling](profiling.md) |
| What do large copies cost? | [Zero-copy](zero-copy.md) |
| What did code generation cost? | [Codegen matrix](evidence/sb09-codegen-matrix.md) |
