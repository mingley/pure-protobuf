# Large-payload baseline (SB-13) — diagnostic results

**Status: diagnostic baseline, not claim-grade.** Single shared macOS host
(Apple M4 Pro, 14 CPUs), closed-loop RPC, `sample(1)` profiles. This is the
pre-change record SB-13 exists to produce: no PK-09/10/11 zero-copy change
may merge without measuring against these numbers. Claim-grade numbers
(pinned Linux hosts, open loop, full latency accounting) come later.

Raw captures live in [large-payload-baseline/](large-payload-baseline/)
(`codec.jsonl`, `rpc.json`, `rpc-go.json`, `prof-*/cell.json` + `top.txt`).

## Pins

| Component | Pin |
|---|---|
| Source | `39cf33ca` (SB-13a/b/c + emit counters; `pbrs-grpc` + `pbrs` `copy-counts` on in devloop) |
| rustc | 1.98.1 (opt, devloop + rpc-bench release) |
| grpc-go server | google.golang.org/grpc@dd51b1c90aaf (v1.85.0-dev), stock interop server, fixed 4 MiB cap |
| tonic | in-tree `rpc-bench --transport=tonic` (pbrs codec via the protobuf-tonic adapter; no prost arm exists in rpc-bench — corrected 2026-09-29, see [TC-10 evidence](tc10-adapter.md)) |

## Copy attribution map

Six bench-only counters, each fed by exactly one funnel site (feature
`copy-counts`; zero cost when off):

| Counter | Funnel | Meaning |
|---|---|---|
| `wire_*` | `Wire::from_slice` (`src/lazy.rs`) | decode backing-buffer copies |
| `emit_*` | `WireOut::put_slice` blanket impl (`src/wire.rs`) + packed fixed fast path (`src/packed.rs`) | encode payload bytes emitted into output |
| `carry_*` | `FrameReader::push` (`pbrs-grpc/src/wire/frame_reader.rs`) | frames straddling a DATA chunk boundary |
| `chunk_slice_*` | `codec::pop_from_chunk` (`pbrs-grpc/src/codec.rs`) | whole frames sliced out of one chunk (zero-copy witness, not a copy) |
| `encode_*` | uncompressed `wire::encode_msg` (`pbrs-grpc/src/wire/encode.rs`) | messages serialized straight into framed buffers |
| `serialize_*` | compressed `encode_msg` | `T::serialize` materializations before compression |

Single-byte stores (`put_u8` tags, 1-byte varints) are not counted; every
`memcpy`-class copy of message bytes is. `emit_bytes` reconciles exactly:
`blob_encode_64kib` reports `100*(65536+7+3) + 99*8` (iter 0 skips the
default checksum), byte-exact against the wire format.

## Codec baselines (devloop blob cells)

`BlobChunk`: u64 id + short owner + u64 checksum + one `bytes` field of the
named size. `mixed`: eight 4 KiB fields (32 KiB total, pins buffer sharing).
pbrs and prost encode byte-identical wire output (asserted in `prepare`).
ns/op is wall mean (total/iters); allocs/bytes from the counting
allocator; wire/emit per op from the counters above (prost is foreign code:
no counters, allocator only).

| cell | ns/op | allocs/op | alloc B/op | wire B/op | emit B/op |
|---|---|---|---|---|---|
| pbrs parse 64kib | 752 | 1.0 | 65584 | 65562 | 0 |
| pbrs touch 64kib | 14360 | 1.0 | 65584 | 65562 | 0 |
| pbrs encode 64kib | 1286 | 2.0 | 131097 | 0 | 65555 |
| pbrs encode_shared 64kib | 957 | 1.0 | 65562 | 0 | 65557 |
| prost parse 64kib | 2472 | 3.0 | 131080 | — | — |
| prost touch 64kib | 16695 | 3.0 | 131080 | — | — |
| prost encode 64kib | 1476 | 3.0 | 131104 | — | — |
| pbrs parse 1mib | 12019 | 1.0 | 1048624 | 1048602 | 0 |
| pbrs touch 1mib | 230750 | 1.0 | 1048624 | 1048602 | 0 |
| pbrs encode 1mib | 22775 | 2.0 | 2097176 | 0 | 1048594 |
| pbrs encode_shared 1mib | 12569 | 1.0 | 1048602 | 0 | 1048597 |
| prost parse 1mib | 24121 | 3.0 | 2097160 | — | — |
| prost touch 1mib | 234788 | 3.0 | 2097160 | — | — |
| prost encode 1mib | 21175 | 3.0 | 2097183 | — | — |
| pbrs parse 4mib | 51178 | 1.0 | 4194352 | 4194331 | 0 |
| pbrs touch 4mib | 897482 | 1.0 | 4194352 | 4194331 | 0 |
| pbrs encode 4mib | 77951 | 2.0 | 8388633 | 0 | 4194323 |
| pbrs encode_shared 4mib | 45786 | 1.0 | 4194331 | 0 | 4194326 |
| prost parse 4mib | 98106 | 3.0 | 8388616 | — | — |
| prost touch 4mib | 945465 | 3.0 | 8388616 | — | — |
| prost encode 4mib | 80158 | 3.0 | 8388640 | — | — |
| pbrs parse 8mib | 108425 | 1.0 | 8388656 | 8388635 | 0 |
| pbrs touch 8mib | 1789329 | 1.0 | 8388656 | 8388635 | 0 |
| pbrs encode 8mib | 156419 | 2.0 | 16777240 | 0 | 8388627 |
| pbrs encode_shared 8mib | 97402 | 1.0 | 8388635 | 0 | 8388630 |
| prost parse 8mib | 204352 | 3.0 | 16777224 | — | — |
| prost touch 8mib | 1893792 | 3.0 | 16777224 | — | — |
| prost encode 8mib | 164217 | 3.0 | 16777247 | — | — |
| pbrs parse mixed | 536 | 3.0 | 33096 | 32796 | 0 |
| pbrs touch mixed | 6990 | 3.0 | 33096 | 32796 | 0 |
| pbrs encode mixed | 1387 | 19.0 | 98611 | 0 | 32785 |
| pbrs encode_shared mixed | 655 | 1.0 | 32796 | 0 | 32787 |
| prost parse mixed | 1088 | 18.0 | 65824 | — | — |
| prost touch mixed | 7529 | 18.0 | 65824 | — | — |
| prost encode mixed | 879 | 10.0 | 65755 | — | — |

Reading: pbrs parse does exactly 1 copy + 1 alloc at every size (~2x faster
than prost's 3-alloc eager decode); touch converges (memory-bound scan);
encode is 1 copy + 1 alloc once the harness-side input fill is excluded
(`encode_shared` vs `encode`: the extra alloc + `memset` is the timed
`vec![0xAB; N]`, identical for prost). Mixed shows the same shape at small
scale (pbrs 3 vs prost 18 allocs on parse).

## RPC baselines (rpc-bench load, closed loop, 8 s unary / 10 s client_stream)

Separate server process + `load` client, `--max-message-size 16777216` both
sides (native serves via `SizedInteropTestService`). `MB/s` counts
req+resp payload bytes × QPS. grpc-go column uses the stock interop server
(no raise flag: 4/8 MiB refused at its fixed 4 MiB cap — recorded, not
retried).

| server | shape | size | QPS | p50 ms | p99 ms | MB/s |
|---|---|---|---|---|---|---|
| native | unary | 64 KiB | 11515 | 0.35 | 0.47 | 1509 |
| native | unary | 1 MiB | 950 | 4.26 | 4.73 | 1993 |
| native | unary | 4 MiB | 208 | 19.37 | 23.42 | 1741 |
| native | unary | 8 MiB | 114 | 35.60 | 50.01 | 1911 |
| tonic | unary | 64 KiB | 9453 | 0.42 | 0.52 | 1239 |
| tonic | unary | 1 MiB | 1053 | 3.78 | 4.29 | 2207 |
| tonic | unary | 4 MiB | 202 | 19.06 | 25.98 | 1695 |
| tonic | unary | 8 MiB | 98 | 41.10 | 47.21 | 1647 |
| grpc-go | unary | 64 KiB | 6461 | 0.61 | 0.84 | 847 |
| grpc-go | unary | 1 MiB | 501 | 8.13 | 10.92 | 1050 |
| grpc-go | unary | 4 MiB | N/A (fixed 4 MiB cap) | — | — | — |
| grpc-go | unary | 8 MiB | N/A (fixed 4 MiB cap) | — | — | — |
| native | client_stream 8×1 MiB | 8 MiB up | 218 RPC/s | 18.59 | 23.15 | 1826 up |
| tonic | client_stream 8×1 MiB | 8 MiB up | 225 RPC/s | 17.64 | 22.36 | 1886 up |

## Live RPC copy attribution (in-process devloop cells)

`rpc.pbrs.unary` (1 KiB up+down, 200 iters): per op `wire`=2 calls/2048 B
(client touches response payload, server touches request payload),
`emit`=4 calls/2052 B (payload + multi-byte length varint each way; the two
1-byte tags go through `push` and stay uncounted), `chunk_slices`=2
(req+resp frames each arrive whole), `encode`=2, `carry`=0,
`serialize`=0. `rpc.pbrs.unary_compressed`: `serialize`=2/op at 89654 B
pre-compression vs 30552 B on the wire. `carry` stays 0 on loopback at
these sizes (frames arrive whole — the zero-copy path holds end to end);
the straddling path is covered by
`pbrs-grpc/src/copy_counts.rs::counts_carry_slices_and_encodes`.

## Profiles (8 MiB codec cells, `sample(1)` top symbols)

| cell | shape |
|---|---|
| pbrs encode | 62.5% `memmove` (the single emit) + 37.4% `memset` (harness input fill) |
| prost encode | 59.9% `memmove` + 40.0% `memset` (same shape; 3 allocs/op vs 2) |
| pbrs parse | 99.9% `memmove` (the single `Wire::from_slice` backing copy) |
| pbrs touch | `memmove` (parse copy) + `memcpy` + sum loop |

Full captures: `prof-*/folded.txt`, `top.txt`, `cell.json`, `meta.json`.
(`memset` vanishes from `encode_shared`, confirming it is harness fill.)

## Reproduce

```sh
# codec sweep (35 cells)
cd bench/devloop && cargo build --release --locked
for cell in $(./target/release/devloop list | awk '/blob/{print $1}'); do
  ./target/release/devloop run-cell "$cell" --iters 100 --warmup 10
done
# RPC sweep: rpc-bench/target/release/rpc-bench server --port P \
#   --transport native --max-message-size 16777216
# then load --server_addr 127.0.0.1:P --transport native --shape unary \
#   --req-bytes N --resp-bytes N --duration-secs 8 --max-message-size 16777216
# profiles: ./scripts/profile.sh --cell codec.pbrs.blob_encode_8mib
```

## Risks / non-claims

- One shared macOS host: wall-clock numbers are host+run-specific; the
  exact counts (allocs, wire/emit bytes, chunk_slices) are the portable
  part of this baseline.
- grpc-go 4/8 MiB cells are N/A by peer design (fixed cap, no flag), not by
  failure; cross-peer LP comparison above 1 MiB needs a custom Go harness
  or the C++ peer.
- `put_u8` single-byte stores are deliberately uncounted (stores, not
  copies); the gap is ≤2 B/message and reconciled in `encode_bytes`.
