# pbrs: fewer copies for large `bytes` payloads (plan)

**Status:** Proposal only. No code has been written and nothing has been measured.
**Date:** 2026-09-23.
**Source baseline:** `mingley/pure-protobuf` `main` at `7cdb29a2`
(2026-09-22). The main checkout has uncommitted edits. Do the work in a
separate worktree off `origin/main`.
**Prior art:** [grpc/grpc-rust#1559](https://github.com/grpc/grpc-rust/pull/1559)
(tonic `SliceBuffer`), closed unmerged on 2025-06-20, and issue #1558, which
is still open.

## 0. Governing rule: ship only if it is actually faster or better

Every phase below is an **experiment with a ship-or-kill gate**. Nothing
merges on reasoning alone.

1. **Pre-register.** Before coding a phase, write down its target cells, its
   guard cells, the expected effect and the thresholds from §4. Put them in the
   phase PR description or the task card, and run the baseline first.
2. **Ship** only if every acceptance condition (A1–A5, §4.3) holds on paired,
   randomized runs.
3. **Kill** if A1 fails after a bounded tuning sweep (at most three threshold
   or configuration variants). Also kill if A2 fails and fixing it would lose
   A1. On a kill:
   - do not merge;
   - record the numbers in `docs/inventory/<experiment>.md`, following the
     repo's existing convention for rejected experiments (for example
     `name80-heap-copy.md`);
   - delete the branch.
4. **Program-level stop.** Suppose the Phase 0 profile shows that all targeted
   user-space copies together are **under 5%** of server or client CPU per GiB
   on the large-payload cells, with TLS on. Then stop after Phase 1a, because
   the rest is not worth the complexity.
5. **Measurement boundary.** Loopback runs (including separate processes) are
   valid for the ship gate only when cores are pinned and the runs are labelled
   provisional. Any claim in `docs/benchmarks.md` or the README needs the
   separate-host protocol in `docs/benchmark-contract.md` §8.

## 1. Problem: where copies happen today (verified in source)

The target shape is one message with a few small metadata fields and one
2–8.5 MiB `bytes` field, sent unary or streamed.

| Path | Site | Copies of the payload |
|---|---|---|
| Inbound: frame spans multiple HTTP/2 DATA chunks | `pbrs-grpc/src/wire.rs` `FrameReader::push` (~L692-697): each chunk is `extend_from_slice`d into `carry: BytesMut`, and the known frame length is **not** reserved. Growth is geometric. (tonic 0.14.6 calls `reserve(len)` after the header.) | 1, plus regrowth copies |
| Inbound: frame arrives whole in one chunk | `codec::pop_from_chunk` slices the chunk | 0 |
| Inbound: parse | `decode_frame` calls `T::parse(frame.payload.as_ref())`. Generated `merge_bytes` starts with `wire = None`, and the first bytes/string field then calls `Wire::ensure`, which does `Arc::from(data)` (`src/lazy.rs` L77). That is a full-message copy into an `Arc<[u8]>`, then windows into it. | 1 |
| Outbound: encode | `frame_from_msg` (`wire.rs` ~L300) pre-sizes a `BytesMut` to header plus length, then `put_slice`s the field | 1 (no regrowth) |
| Outbound: HTTP/2 | `send_bytes` hands out slices of the `Bytes` (refcount only). h2 `FramedWrite` chains DATA payloads over 256 B (vectored) or over 1024 B without copying. | 0 |
| TLS (rustls, both directions) | encryption/decryption into and out of record buffers | 1 each way (out of scope) |
| `protobuf-tonic` adapter | `ProtobufDecoder::decode` parses from the borrowed `DecodeBuf` chunk, which hits the same `Wire::ensure` copy. tonic has already copied frames into one contiguous buffer. | tonic's 1 + pbrs's 1 |

**Net today:** inbound costs 2 or more user-space copies, where tonic with
prost `Bytes` costs 1. Outbound costs 1, the same as tonic with pre-sizing.

Supporting evidence from `docs/benchmarks.md`, measured on an M4 Pro:
- Owned decode of 5 MiB `bytes` takes 59.9 µs, which is about memcpy speed.
- `buffa decode_view` takes 0.12 µs.

**Why #1559 would not help this shape:** its `copy_to_bytes` allocates and
copies whenever the requested field spans slices, and its `put` copies too.
prost fields must be contiguous (`Vec<u8>` or `Bytes`), so for a single large
field a tonic-side buffer change cannot be zero-copy. pbrs can be, because it
owns both the runtime (field representation) and the transport (framing and h2
writes).

## 2. Goals and non-goals

**Goals, in order:**
1. Inbound parity with tonic+prost `Bytes` (1a, 1b).
2. Zero user-space copies before TLS on send for large shared fields (2).
3. Zero user-space copies before TLS on receive, even when frames span chunks
   (3). This is the goal #1559 was after.

**Non-goals:**
- Removing TLS copies (kTLS), `sendfile`, or io_uring.
- Wire-format changes.
- Public borrowed-lifetime views. Task card OP-06 owns that decision; this plan
  uses shared ownership (refcounted `Bytes`), not lifetimes.
- Making the tonic adapter zero-copy beyond Phase 1b (tonic's codec API forces
  a contiguous `BytesMut`).

## 3. Hard requirements (all phases)

- **Protocol semantics:** `./scripts/conformance.sh` stays at 100%. Last-wins
  for singular fields, oneof clearing, unknown-field preservation, proto2
  required checks and presence must be identical on every new path. When in
  doubt, **fall back** to the existing contiguous path.
- **API:** additive only. The v4-style `&[u8]` accessors and views keep working
  unchanged. New entry points use default methods or new items, so existing
  `WireOut`/`Parse` implementors and generated code keep compiling.
- **Generated-code compatibility:** the frozen consumers in
  `tests/fixtures/codegen-compat` must build and pass against the new runtime.
  Keep the `pbrs::rt::{Wire::ensure, Wire::window, LazyBytes::from_wire}`
  signatures, or prove old generated code still builds.
- **Safety:** no new `unsafe` (the workspace uses `#![deny(unsafe_code)]`).
  MSRV stays 1.85. `bytes` is already a dependency of `pbrs`.
- **Fuzzing:** new entry points get fuzz coverage. Parsing from `Bytes`, and
  segmented parsing in Phase 3, must match contiguous parsing byte for byte
  (differential), across random chunk boundaries.
- **Public-repo hygiene:** use a synthetic schema such as `BlobChunk`. No
  employer-internal schema names, services, hosts or measurements in the repo.
- **Generated-code size:** report the size delta of generated code. Any
  per-field accessors added must be measured against the goals of task card
  CG-20.

## 4. Measurement plan (Phase 0, required before any code)

### 4.1 Harness additions

- **Schema:** add `proto/blob.proto` with
  `BlobChunk { uint64 id; string owner; fixed64 checksum; bytes data; }` and
  data sizes of 64 KiB, 1 MiB, 4 MiB and 8 MiB. Also add a mixed shape with
  several 4 KiB `bytes` fields, to catch the risk of memory being pinned.
- **Codec (`bench/`):**
  - cells: `parse(&[u8])`, parse-and-touch (read `data`), encode, and encode
    where `data` was set from `Bytes`;
  - once each phase adds them: `parse_from_bytes(Bytes)` and segmented parse
    (Phase 3);
  - a **counting global allocator in the bench binary only** (never the
    library), reporting bytes allocated and allocation count per operation;
  - a `size_of` report for `Wire`, `LazyBytes`, `LazyStr` and a few generated
    messages.
- **Transport (`rpc-bench`):** add a large-payload (LP) suite:
  - shapes: unary upload (8 MiB request, small response); unary download
    (small request, 8 MiB response); client streaming of 1 MiB and 8 MiB
    messages; server streaming of 8 MiB messages;
  - transport profiles: h2c plaintext and TLS;
  - peer DATA-frame profiles: 16 KiB (the tonic/grpc-go default) and 1 MiB
    (the pbrs default `DEFAULT_MAX_FRAME_SIZE`);
  - directions: native pair, tonic client to pbrs server, and pbrs client to
    tonic server.
- **Metrics** (client and server reported separately, per contract §5):
  - goodput;
  - CPU-seconds per GiB (user plus system);
  - p50/p99 at matched offered load;
  - peak and steady RSS;
  - bytes allocated per RPC.
- **Profiles:** flamegraphs (perf or samply) of each LP target cell. They must
  show the targeted memmove sites, under `FrameReader::push`,
  `Wire::from_slice` and `frame_from_msg`. Record each site's share of CPU.
  This feeds the program-level stop rule (§0.4) and A5.

### 4.2 Statistical method (from `docs/benchmark-contract.md` §6)

- At least 5 paired runs per cell, with A/B order randomized per run.
- Warmup as specified in the contract.
- 95% confidence interval on the paired ratio (bootstrap with 10k resamples).
  Point estimates alone are rejected.
- p99 needs at least 10k observations.

### 4.3 Acceptance conditions (per phase)

- **A1 Benefit.** On at least 2 of the phase's pre-registered LP target cells,
  the result is statistically significant (the 95% CI of the paired ratio
  excludes 1.0) and has at least one of these effects:
  - **transport:** at least 10% lower CPU-seconds per GiB (on the side the
    phase targets), or at least 10% higher goodput;
  - **codec:** at least 15% lower ns/op;
  - **memory only:** at least 10% lower peak RSS, counted only if CPU and
    latency do not regress.

  A reduction in allocations alone is not enough.
- **A2 No regressions.**
  - Codec guard cells (hello, person, TAT, `rpc_sparse`, `tags_32`, `name_80`,
    the 4 KiB string/blob cells): no statistically significant slowdown over
    2%.
  - Transport guard cells (the official empty, 1 KiB and 64 KiB unary and
    streaming scenarios): p99 no more than 5% worse (contract §7.1).
  - Peak RSS no more than 10% higher on any cell.
  - For phases that retain buffers (1b, 3): zero RSS growth over a 1-hour soak.
  - `test_competing_small_rpcs_progress_under_bulk_stream_load` passes.
- **A3 Correctness.** All of the following pass:
  - conformance;
  - these checks from `docs/plan/tasks.json`: `core-lib`, `parser`, `shared`,
    `native-lib`, `native-rpc`, `native-hostile`, `native-tls`, `tonic`,
    `consumers`, `codegen`, `native-codegen`;
  - the codegen-compat fixtures;
  - `self-interop` and the Go interop suite;
  - the fuzz targets (`grpc_wire`, plus parse and the new differential
    targets), each for a fixed budget of at least 10 minutes.
- **A4 Safety and API.** The rules in §3 hold: no new `unsafe`, additive API,
  MSRV unchanged.
- **A5 Attribution.** The flamegraph shows the targeted memmove site gone or
  shrunk, and the gain is explained by it.

## 5. Phases

Dependencies: 0 → 1a → 1b → 2. The frame-size experiment (3-alt) runs after
1b and before 3. Phase 3 depends on 1b and on 3-alt's result.

### Phase 1a: reserve once in `FrameReader` (parity fix, small)

- **Change:** when a frame starts spanning chunks and the 5-byte header is
  available (the header itself may be split), call
  `carry.reserve(total - carry.len())` once. Alternative: queue the chunks and
  concatenate once at exact capacity. Benchmark both; keep the simpler one if
  they tie.
- **Hypothesis:** removes the regrowth copies, leaving exactly one copy for a
  frame that spans chunks. Visible only with 16 KiB peer frames.
- **Risk:** a hostile header can make the reader commit memory early. It is
  already capped by `limits.check_decode`, and tonic behaves the same way.
  Evaluate a capped variant, `min(len, 2 × received)`, if `native-hostile` or
  the resource-bound tests object.
- **Target cells:**
  - LP unary upload at 8 MiB with 16 KiB peer frames, plaintext and TLS
    (server CPU per GiB);
  - LP client streaming at 8 MiB.
- **Files:** `pbrs-grpc/src/wire.rs`, plus tests in the existing `FrameReader`
  test module.

### Phase 1b: `Wire` backed by shared `Bytes`, plus a parse-from-`Bytes` entry point

- **Change:**
  1. Let `Wire` wrap `bytes::Bytes` without copying. Evaluate two variants:
     - (A) `Bytes` only, with `from_slice` using `Bytes::copy_from_slice`;
     - (B) an enum of `Arc<[u8]>` and `Bytes`.

     Watch two costs:
     - `Bytes::from(Vec)` allocates a shared header on the first clone, which
       `Arc<[u8]>` avoids.
     - The larger `size_of` affects the layout of small messages.

     Pick by measurement.
  2. Add additive `Wire::from_bytes(Bytes)`, and a public
     `parse_from_bytes(Bytes)`/`merge_from_bytes_shared(Bytes)`. The trait
     method has a default that falls back to `parse(&b)`. Generated code calls
     `merge_inner(data, &mut Some(wire), …)`; the plumbing already exists
     because `merge_inner` takes `&mut Option<Wire>`.
  3. **Sharing threshold `T_share`:** a bytes field of at least `T_share`
     becomes a window (a refcount, no copy). Smaller fields are copied, so a
     tiny field does not pin a large chunk. Sweep 512 B, 4 KiB and 64 KiB.
     Leave the `LazyStr` policy unchanged in this phase.
  4. Zero-copy in and out of messages:
     - `IntoProxied<LazyBytes> for bytes::Bytes`, so a setter taking `Bytes`
       does not copy;
     - a getter that returns `Bytes`: a refcount clone when shared, a copy when
       owned. Prefer one runtime method over per-field codegen, to limit
       generated-code size.
  5. **`pbrs-grpc`:** `decode_frame` uses `parse_from_bytes(frame.payload)`.
     The gzip path uses `Bytes::from(decompressed)`.
  6. **`protobuf-tonic`:** use `src.copy_to_bytes(n)`, which is a zero-copy
     split of tonic's contiguous buffer, then `parse_from_bytes`. This removes
     the pbrs-side copy.
- **Hypothesis:** removes the full-message `Arc::from` copy on parse. Inbound
  costs:

  | Case | Before | After |
  |---|---|---|
  | Frame arrives whole | 1 | 0 |
  | Frame spans chunks (after 1a) | 2 | 1 |
  | tonic adapter | 2 | 1 (tonic's) |
- **Risks:**
  - Windows keep h2 chunk or carry allocations alive.
    - Mitigation: `T_share`, plus a documented `detach()`/owned-conversion
      API.
    - Measurement: a retaining-handler RSS cell (a handler that keeps N parsed
      messages) and the 1-hour soak. Reuse OP-06's methodology for
      quantifying retention.
  - Small messages regress because of the `size_of` growth. The guard cells
    catch this.
- **Target cells:**
  - codec: `parse_from_bytes` vs `parse` at 1, 4 and 8 MiB;
  - transport: LP unary upload and client streaming, server CPU per GiB,
    1 MiB peer frames;
  - transport: tonic client to pbrs-on-tonic server, 8 MiB upload.
- **Files:** `src/lazy.rs`, `src/message.rs`, `src/gen_support.rs`,
  `src/codegen.rs` (only if the entry point needs generated glue),
  `pbrs-grpc/src/wire.rs`, `protobuf-tonic/src/lib.rs`, plus new fuzz and
  differential tests.

### Phase 2: zero-copy send for shared fields

- **Change:**
  1. Add a default method `WireOut::put_shared(&mut self, b: &Bytes)` that
     calls `self.put_slice(b)`. This is non-breaking.
  2. Generated encode for a bytes field: if it holds a shared `Bytes` of at
     least `T_send`, call `out.put_shared(..)`; otherwise use today's path.
     Old generated code simply never calls it.
  3. `pbrs-grpc` gets a segmented sink,
     `{ head: BytesMut, segs: SmallVec<[Bytes; 4]> }`:
     - `put_shared` freezes the current head and pushes the shared segment;
     - `frame_from_msg` or `append_frame` return segments;
     - `send_bytes` is called per segment, with `end` set only on the last;
     - `OutBatch` carries a segment list, and its fullness check uses the total
       byte count;
     - `BytePermit` accounting still uses the total length.
  4. **Fast-path guard:** when a message has no shared field over the
     threshold, the old single-buffer code runs unchanged. The gzip path stays
     contiguous.
- **Hypothesis:** outbound copies of a large shared field go from 1 to 0
  before TLS. With TLS, rustls still makes one copy while encrypting, so the
  expected gain is smaller. It is largest on h2c or UDS.
- **Risks:**
  - Small head segments get copied into h2's buffer (they are tiny).
  - More `send_data` calls per message.
  - Measure small-message streaming guard cells carefully: the `OutBatch` path
    is performance-sensitive (the 32 KiB batch).
- **Target cells:**
  - LP unary download and server streaming at 8 MiB, **server** CPU per GiB,
    plaintext;
  - TLS must be non-negative (no regression).
- **Files:** `src/wire.rs`, `src/codegen.rs`, `pbrs-grpc/src/wire.rs`, plus
  segmented-encode differential tests: segment concatenation must equal
  `serialize()`.

### Phase 3-alt: large `SETTINGS_MAX_FRAME_SIZE` (configuration experiment, before Phase 3)

- **Change:** none in the parse code. The pbrs server advertises a larger max
  frame size (up to 16 MiB − 1), so compliant h2 peers can send a whole message
  in one DATA frame. That frame takes the existing zero-copy `pop_from_chunk`
  path.
- **Verify first:** which peers actually send large DATA frames when allowed
  (h2/tonic, grpc-go, grpc-java, C++), and how that interacts with the flow
  control window (`DEFAULT_WINDOW_SIZE` is 16 MiB).
- **Risk:** head-of-line blocking of small RPCs that share the connection. The
  gate is A2's fairness test plus the p99 of small RPCs under a concurrent
  LP stream.
- **Outcome:** if this gets most inbound cells to zero copies for the peers
  that matter, **Phase 3 is not started**.

### Phase 3: segmented receive for frames that span chunks (the #1559 goal done right)

- **Precondition:** after 1a, 1b and 3-alt, profiling still attributes at least
  5% of server CPU per GiB to the `carry` copy, on LP upload with 16 KiB peer
  frames.
- **Change:**
  1. `FrameReader` keeps spanning frames as a `VecDeque<Bytes>` (no `carry`
     copy) and emits `Payload::Segmented`.
  2. A schema-agnostic scanner walks the top level of the message across
     segments. It finds length-delimited fields of at least `T_splice` that
     cross a segment boundary.
  3. It builds a small contiguous residual message (every other field, copied)
     plus a list of `(field_number, Rope)`.
  4. The generated parser runs on the residual as usual. A new additive
     generated `__splice_bytes(field, rope) -> bool` then attaches each rope to
     a **singular, non-oneof** bytes field.
  5. **Exact-semantics fallback:** the message is flattened and parsed the
     current way when:
     - the field is unknown to the message;
     - the field is repeated, a map, a oneof, nested or a group;
     - the field number appears more than once in the message.
  6. New `LazyBytes::Rope { parts, len, flat: OnceLock<Bytes> }`:
     - the `&[u8]` accessor flattens lazily, once, only if called;
     - new `chunks()`/`as_buf()` accessors;
     - encode emits `put_shared` per part, so proxying or forwarding is fully
       zero-copy.
- **Hypothesis:** inbound copies before TLS go from 1 to 0 for frames that span
  chunks.
- **Complexity budget:** if this needs changes to the generated merge loops
  beyond the additive splice function and the rope variant, stop and re-plan.
  Do not rewrite the parser core.
- **Target cells:** LP unary upload and client streaming with 16 KiB peer
  frames, server CPU per GiB, plaintext and TLS.
- **Extra correctness:** a differential fuzz target. For random messages and
  random chunkings, segmented parse must equal contiguous parse, including
  unknown fields and presence.

## 6. Decision summary

| Phase | Removes | Ship only if (beyond A2–A5) | Kill signal |
|---|---|---|---|
| 0 | n/a | Harness merged and baseline recorded | targeted copies under 5% of CPU per GiB means stop after 1a |
| 1a | regrowth copies on spanning frames | A1 on 16 KiB-frame upload cells | no significant gain |
| 1b | parse copy into `Arc` | A1 on codec and transport upload cells, no RSS growth in soak | small-message guard regressions from `size_of` or `Bytes` overhead |
| 2 | encode copy | A1 on download/server-stream cells (plaintext), no TLS regression | streaming guard regressions |
| 3-alt | spanning-frame copy (configuration only) | A1 plus the fairness test | HOL p99 regressions, or peers ignore the setting |
| 3 | spanning-frame copy | precondition met plus A1 | complexity budget exceeded, or no significant gain |

## 7. Open questions

- `Wire` backing: `Bytes` only or an enum? Decided by the Phase 1b
  measurements.
- Threshold values `T_share`, `T_send` and `T_splice`: one shared constant, or
  separate ones?
- Global behavior vs a codegen option (for example `bytes_backing=shared`):
  default to global if the gates pass, and use an option only if results are
  mixed.
- Coordination with OP-06 (borrowed views and retention), OP-01 (ranking
  bottlenecks) and CG-20 (generated-code size). Decide whether these become
  cards in the `docs/plan/tasks.json` queue (proposed below).

## 8. Proposed task cards (not added to `tasks.json`)

```json
[
  {"id": "ZC-00", "title": "Large-payload benchmark harness and copy-site baseline", "depends_on": [],
   "write": ["proto/blob.proto", "bench/", "rpc-bench/"],
   "accept": ["LP codec and transport cells, counting allocator (bench only), size_of report and flamegraphs recorded", "Program-level stop rule evaluated"],
   "checks": ["codec-bench", "rpc-bench"]},
  {"id": "ZC-01", "title": "Reserve the frame length once in FrameReader", "depends_on": ["ZC-00"],
   "write": ["pbrs-grpc/src/wire.rs"], "accept": ["A1-A5 of the zero-copy plan"],
   "checks": ["native-lib", "native-rpc", "native-hostile", "rpc-bench"]},
  {"id": "ZC-02", "title": "Bytes-backed Wire and parse_from_bytes", "depends_on": ["ZC-01"],
   "write": ["src/lazy.rs", "src/message.rs", "src/gen_support.rs", "pbrs-grpc/src/wire.rs", "protobuf-tonic/src/lib.rs"],
   "accept": ["A1-A5", "1-hour soak shows zero RSS growth", "codegen-compat fixtures unchanged"],
   "checks": ["core-lib", "parser", "shared", "conformance", "tonic", "codec-bench", "rpc-bench"]},
  {"id": "ZC-03", "title": "Zero-copy send via WireOut::put_shared", "depends_on": ["ZC-02"],
   "write": ["src/wire.rs", "src/codegen.rs", "pbrs-grpc/src/wire.rs"],
   "accept": ["A1-A5", "segment concatenation equals serialize()"],
   "checks": ["codegen", "native-codegen", "native-rpc", "native-tls", "rpc-bench"]},
  {"id": "ZC-04", "title": "Large SETTINGS_MAX_FRAME_SIZE experiment", "depends_on": ["ZC-02"],
   "write": ["pbrs-grpc/src/config.rs"], "accept": ["A1 plus the fairness test; peer behavior matrix recorded"],
   "checks": ["native-serving", "rpc-bench"]},
  {"id": "ZC-05", "title": "Segmented receive with rope-backed bytes", "depends_on": ["ZC-03", "ZC-04"],
   "write": ["pbrs-grpc/src/wire.rs", "src/lazy.rs", "src/codegen.rs", "fuzz/"],
   "accept": ["Precondition met", "A1-A5", "differential fuzz across random chunkings"],
   "checks": ["parser", "conformance", "native-hostile", "rpc-bench"]}
]
```

## 9. Evidence references (baseline `7cdb29a2`)

- `src/string.rs:312`: `pub struct ProtoBytes(Vec<u8>)`.
- `src/message.rs:8`: `Parse::parse(serialized: &[u8])`.
- `src/message.rs:43`: `Serialize::encode(&self, out: &mut impl WireOut)`.
- `src/wire.rs:16`: `WireOut { put_u8, put_slice }`, with a blanket impl for
  `BufMut`.
- `src/lazy.rs`:
  - L25-90: `Wire { buf: Arc<[u8]>, start: u32, end: u32 }`;
  - L77: `ensure`, which copies via `from_slice`;
  - L81: `window`;
  - L368: `LazyBytes { Empty, Wire, Owned }`.
- `src/codegen.rs`:
  - L4763/4767: generated `merge_bytes` starts with `wire = None`;
  - L4775: `merge_inner(data, wire: &mut Option<Wire>, …)`.
- `pbrs-grpc/src/wire.rs`:
  - ~L300: `frame_from_msg`;
  - ~L337: `append_frame`/`OutBatch`;
  - L449: `send_bytes`, which slices `Bytes` per unit of send capacity;
  - ~L667-705: `FrameReader`;
  - ~L730: `decode_frame`.
- `pbrs-grpc/src/codec.rs`: `pop_limited` (copy path) and `pop_from_chunk`
  (zero-copy).
- `pbrs-grpc/src/config.rs`: `DEFAULT_MAX_FRAME_SIZE = 1 MiB`,
  `DEFAULT_WINDOW_SIZE = 16 MiB`.
- `protobuf-tonic/src/lib.rs` L61-75: parses from the borrowed `DecodeBuf`
  chunk.
- h2 0.4.18/0.4.19 `src/codec/framed_write.rs`: `CHAIN_THRESHOLD` is 256
  (vectored) or 1024 (not vectored).
- tonic 0.14.6 `src/codec/decode.rs`: `reserve(len)` after the header (~L199)
  and `buf.put(data)` per frame (~L281).
- `docs/benchmark-contract.md` §§6-8 (statistics, thresholds, environment).
- `docs/plan/tasks.json`: check commands and the OP-06, OP-01 and CG-20 cards.
