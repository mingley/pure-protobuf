# What to work on next

The immediate goal is a trustworthy comparison of the current gRPC stack,
followed by measured improvements to client and server efficiency. The
[2026-09-29 audit](docs/audit-2026-09-29.md) explains the current gaps. The
[performance program](docs/plan/world-class/README.md) defines the required
comparisons and checks; [task cards](docs/plan/world-class/tasks.json) carry
acceptance criteria and dependencies.

The [October 29 runtime and codegen record](docs/evidence/runtime-and-codegen-20261029/README.md)
records smaller descriptor output and measured repeated-field and varint
changes. Prioritize lazy nested reads, sparse and non-LTO packed parsing, map
construction and warm access, and remaining transport losses. Measure large
consumer builds and verify missing parity and deployment cases before claiming
a general lead. The [parity summary](docs/parity.md) tracks API and deployment
gaps.

Michael Ingley coordinates assignments and reviews. A card marked `done`
means its recorded deliverable was accepted; it does not certify the whole
product. Older [foundation cards](docs/plan/tasks.json) remain authoritative
where they have not been handed to newer cards.

## Start here

Run these from the repository root:

```sh
python3 scripts/plan-lint.py
python3 scripts/plan-status.py --summary
python3 scripts/plan-status.py --ready
python3 scripts/plan-status.py --card SB-21
```

The ready list is generated from both card files, including their dependency
and reconciliation gates. It excludes claimed and blocked work. It is a list
of eligible work, not an instruction to start every lane at once.

## Current priorities

Use Rust 1.100.0. The codec survey covers 28 fixtures and five operations.
Run `python3 tonic-bench/summary.py survey.txt --losses` to see each slower
fixture. Retain the raw samples, including ties and losses.
[Workload index](docs/benchmarks.md) · [Run the survey](docs/benchmarks/running.md#message-codecs)

1. **Full reads and first encode.** Profile materializing a recursive child
   at depths four and eight. A specialization cut depth-eight full reads by
   4.5% across four pairs, but raised parse-only time by 1.3% there and by
   4.4% on a 32-byte blob. Keep both controls and the generated binary size
   in the comparison. Also track the release's sparse-path owned decode
   against upb and use the survey to select the remaining first encodes.
   [Focused read](https://github.com/mingley/pure-protobuf/actions/runs/44676444591/artifacts/14601979831) ·
   [Repeat and size](https://github.com/mingley/pure-protobuf/actions/runs/44676803807/artifacts/14602394883)
2. **Live client uploads.** Measure 64 messages of 512 bytes on a fixed
   server with the same codec. A larger first buffer saved 24% of requested
   allocation bytes per RPC. Interactive eight-message bidi lost 2.45% in
   throughput in the paired median and slowed in all six pairs. Keep
   interactive, pipelined and already-closed streams in the next comparison.
   [Upload and bidi samples](https://github.com/mingley/pure-protobuf/actions/runs/44676257278/artifacts/14603755339)
3. **Multiplexed server streams.** Hold the client and Prost codec fixed.
   Collapsing ready-handler polls changed throughput by −0.65% and +0.45%
   on two hosts. Measure server CPU per RPC and end-to-end p99 alongside
   throughput, with unary and single-stream controls. Profile per-call
   allocations before changing the connection scheduler again. Include the
   [large-unary latency case](docs/benchmarks/release-v0.2.2.md#grpc-over-tcp) when testing buffer changes.
   [Host one](https://github.com/mingley/pure-protobuf/actions/runs/44677356908/artifacts/14602583169) ·
   [Host two](https://github.com/mingley/pure-protobuf/actions/runs/44677356908/artifacts/14603406558)
4. **Generate large schemas.** Time both the public descriptor-set API and
   the protoc plugin. Skipping a temporary request saved 1.9–3.3% CPU on
   two 1,000-message descriptor sets with two workers, but raised plugin CPU
   by 13% on source-heavy lean generation. Retain the 512-file, large and
   small controls, compare every output path and byte, and time downstream
   builds separately.
   [Generator samples](https://github.com/mingley/pure-protobuf/actions/runs/44677471464/artifacts/14603116892)

[Earlier allocation measurements](docs/benchmarks/devloop-preparation-20261118.md) ·
[Earlier runtime and generator checks](docs/evidence/safe-hotpaths-20261117/README.md)

### Handoff snapshot & remaining TODO queue

Two transport/framing optimization tracks just landed on `main` (dev-loop
measurements on macOS arm64 against `7caa502e` and `tonic 0.14.6`, not
dedicated-host leadership claims):

- **Track 1 — gRPC server & client hot-path trims (`pbrs-grpc`):** zero-alloc
  `Arc<[SubchannelRef]>` LB picker path (`lb/mod.rs`), lazy `ConnectInfo`
  request extensions (`lazy_ext.rs`, `request.rs`, `metadata.rs`), inline
  `HoldCancel` via `pin_project_lite`, elided unused `Instant::now()` calls,
  and per-RPC dispatch/drain trims (`server/{connection,drain,rpc}.rs`,
  `tonic_server.rs`, `tower_client.rs`, `client/{config_glue,streaming}.rs`).
- **Track 2 — Single-buffer gRPC framing, H2 chunk coalescing & pooled `flate2`
  compression (`pbrs` + `pbrs-grpc`):** `shared_bytes_len`,
  `prefers_slice_decode`, and `decode_payload_slice` across `src/{lazy,wire,rt,message,gen_support}.rs`
  and `src/codegen/{encode,messages,parse}.rs` (with regenerated
  `src/generated/*.rs`); single-buffer 5-byte gRPC frame + message encode,
  `OutBatch` stream coalescing, 1–8 MiB `FrameReader` staging, and H2 chunk
  coalescing (`pbrs-grpc/src/wire/*`, `pbrs-grpc/src/h2_backend/*`); and
  thread-local pooled `flate2::Compress`/`Decompress` engines in
  `pbrs-grpc/src/compression/{gzip,deflate}.rs` (`1.00` alloc per 1 KiB
  gzip/deflate round-trip).
- **Measured dev-loop deltas (`bench/devloop`):**
  - `tonic_transport.native_native.query.d4.v6.unary`: `139.02 µs -> 89.68 µs`
    (`-35.5%`; `1.50x` faster than `tonic` at `134.25 µs`).
  - `tonic_transport.native_native.query.d4.v6.server_stream`:
    `153.93 µs -> 113.82 µs` (`-26.1%`; `1.40x` faster than `tonic` at
    `159.67 µs`).
  - `rpc.pbrs.unary`: `141.56 µs -> 129.78 µs` (`-8.3%`), allocs `29.9 -> 20.9`
    (`-30.1%`) vs `rpc.tonic.unary` `158.98 µs` / `72.0` allocs.
  - `rpc.pbrs.server_stream`: `111.42 µs -> 102.85 µs` (`-7.7%`), allocs
    `47.9 -> 35.0` (`-27.0%`) vs `rpc.tonic.server_stream` `164.83 µs` / `84.0`
    allocs.
  - `rpc.pbrs.server_stream_compressed`: `871.78 µs -> 746.45 µs` (`-14.4%`),
    allocs `759.7 -> 241.9` (`-68.2%`), allocated bytes `21.2 MB -> 416 KB`
    (`-98.04%`, `51x` reduction).

#### Remaining TODOs (ordered execution plan)

1. **Track 3 — `pbrs` codec hot-path elimination (`PK-31`, `PK-32`, `PK-14`,
   `PK-27`, `PK-29`):**
   - **Owned `Map` O(N^2) insert (`src/map.rs`, `PK-31`):** `Map::insert` and
     `MapMut::insert_owned` linearly scan `entries.iter().rposition(...)` on
     every insert unless an index was already built by a read (`maps.n512.fresh_encode`
     is `1,064 µs` vs `prost` `75.8 µs`). Build `LastWinsIndex` lazily at
     `INDEX_THRESHOLD` (`16`), add an O(1) ascending-key append fast path on
     `LastWinsIndex::upsert`, and track `all_unique` for O(1) `len`/`iter`.
   - **Thin-pointer `Repeated<T>` & unboxed hot `optional` string/bytes
     (`src/repeated.rs`, `src/codegen/messages.rs`, `PK-32`):** replace
     double-allocated `Repeated<T>(Option<Box<Vec<T>>>)` (`2` heap allocs per
     non-empty repeated field) with a single-allocation thin-pointer header
     (`null == empty`, preserving `zeroed_message`, ZST/over-aligned safety,
     and `docs/unsafe-invariants.md` documentation + Miri coverage); replace
     `Option<Box<LazyStr>>` / `Option<Box<LazyBytes>>` on hot (`Dense`)
     `optional`/`oneof` fields with niche/sentinel `OptLazyStr`/`OptLazyBytes`
     while keeping boxed `ColdPlacement` on wide (`Sparse`) messages.
     The empty mutable-slice allocation is fixed. A full storage replacement
     must still preserve public mutable-`Vec` behavior, zero-copy `Vec`
     conversions, zeroed defaults, alignment and drop semantics.
   - **Packed encoding storage (`src/packed.rs`, `PK-14`):** both
     [decoded-slice borrowing candidates](docs/evidence/held-packed-borrow-20261117/README.md)
     remain held after warmed lookup regressions. Retain their cold-allocation
     gains and losses when evaluating the next design. The safe public
     `MEMCPY_SAFE` flag cannot authorize a generic unsafe cast; use concrete
     scalar implementations and keep checked byte lengths. The earlier inline
     varint-cache experiment also remains unshipped.
   - **Eager/inline submessage decode (`src/lazy.rs`, `src/codegen/parse.rs`,
     `PK-27`, `PK-29`):** bypass `LazyMsg<T>` `Wire::window` `Arc` clones and
     `MemoCell` double-buffering on `owned_decode` / `read_all` paths for
     recursive query trees and `Any`-bearing records.
2. **Track 4 — Non-xDS gRFC completion (`docs/grfc.md`, `GF-11`, `GF-12`,
   `CH-12`):**
   - Transport & security: `GF-11` RFC 9113 TLS 1.2 cipher blocklist
     (`GOAWAY(INADEQUATE_SECURITY)`), `A18` automatic HTTP/2 keepalive coupling
     beyond the explicit TCP user timeout, `A105` subchannel pool scaling, and
     `A97` JWT/Bearer call credentials (`XD-06c`).
   - Extensibility, fault & observability: `CH-12` (`A52`/`A110`) public
     `ResolverFactory` / `LbPolicyFactory` registries preserving zero-alloc
     built-in picks, `A33` fault injection filter, `A38` admin service bundle
     (channelz + CSDS), `GF-12` Tower layer parity, and remaining OTel/retry
     instruments (`A45`, `A72`, `A78`, `A79`, `A94`, `A96`, `A108`).
3. **Track 5 — xDS control plane & full 92/92 gRFC reconciliation (`XD-01`
   through `XD-12`, `GF-09`):**
   - Implement modular xDS bootstrap/ADS/LDS/RDS/CDS/EDS/LRS/CSDS plus HTTP
     filters including newly tracked `A93` (`ext_proc`, `A93-xds-ext-proc.md`),
     `A41` RBAC, `A33` fault injection, `A55`/`A60` session affinity, and
     reconcile all `92` `A`/`G` rows in [gRFC coverage](docs/grfc.md).
4. **Dedicated-host qualification (`SB-21`, `SB-24`, `SB-27`, `RX-09`, `SV-09`,
   `CL-08`):** re-run separate-process Linux CPU/p99/allocation/headroom
   campaigns before making any general or claim-grade superiority statement.

The [October 28 follow-up](docs/evidence/optimizations-20261028/README.md)
rejects the lazy enum, map iteration variants, generated-output move/shrink
and both borrowed-wire variants after measured regressions. Keep their raw
comparisons and source manifests. The selected transport counter and framing
repairs address correctness; their full RPC comparison retains losses and
does not establish a speed improvement. Ordinary iteration, collection, construction,
both message profiles and consumer build costs must accompany any next attempt.

Small and duplicate-heavy map iteration still loses to standard HashMap;
only five of the 42 selected warm-map rows beat it. Lazy deep full reads still lose
to Prost, while the owned profile has one near-parity parsing loss. Target
those full lifecycles alongside small release generation, large generated
output and downstream consumer build costs. The corrected completion timer
must be used for new CLI comparisons; older polling-wall results cannot
support a speed claim.

Streaming scheduling remains a separate priority: allocation reductions do
not establish lower endpoint CPU. The compressed-output follow-up leaves
upload-client, download-server and large-identity losses against tonic. The
initial framer-only change produced no RPC allocation win. C's identity
reservation was rejected: its 25.4% byte reduction came with about 50% higher
client CPU and 56% higher server CPU in longer confirmation. Keep the existing
identity growth policy while investigating alternatives. Concrete next steps:

- Closed uploads: [write_closed_upload](pbrs-grpc/src/client/streaming.rs)
  now coalesces compressed closed-upload frames into bounded 256 KiB slabs via
  `OutBatch::new_closed_upload` while preserving pre-send codec/budget error
  ordering. Re-run matched noisy, compressible, and mixed-size comparisons
  before claiming an endpoint CPU win.
- Downloads: [drain_to_wire](pbrs-grpc/src/server/drain.rs) now coordinates
  HTTP/2 send-buffer drain (`poll_send_drained`) before the next large
  compressed encode so workspace backing is reclaimed without unconditional
  yields or higher ledger caps. Re-run the full matched streaming matrix to
  measure the end-to-end throughput and CPU effect.
- Large identity input: [FrameReader](pbrs-grpc/src/wire/frame_reader.rs) now
  stages multi-chunk large identity payloads with bounded metadata and makes one
  exact contiguous copy after the full payload arrives without reserving from
  an untrusted length prefix, and `WireStream` drops decoder staging at
  terminal state. Confirm against the matched large-identity benchmark rows.

Keep large encode retention within the shared 16 MiB ledger and 4 MiB per
backing allocation. Scheduling and ownership changes must survive the full
matched lifecycle comparison, including small interactive streams.

The caller-worker prototype and small-map-cache prototype were rejected;
preserve their data rather than treating them as accepted optimizations.
The [explicit alloc-only profile](docs/guides/alloc-only.md) now has concrete
Cortex compilation and std-backed host consumers. Firmware execution, broader
target coverage and its performance remain unverified. Selected
[ordered maps](docs/guides/maps.md), [byte storage](docs/guides/bytes.md) and
[boxed children](docs/guides/codegen.md#box-selected-nested-fields) now have
compiled consumers; their documented boundaries still apply. Every component
gain must survive the full matched matrix.

The [allocation and generator follow-up](docs/evidence/efficiency-20261027/README.md)
removes MiniTable length-calculation buffers and repeated imported-path work.
Cold generator timing and peak-memory losses remain, and a dense parser lookup
was rejected after slowing every repeated-fragment pair.

The [message merging and gRPC costs record](docs/evidence/grpc-parity-and-costs-20261027/README.md)
adds final-tree required checks for dynamic and MiniTable messages, direct Tower
stream input, less frame copying and shared generator metadata. It keeps the
map-validation, codegen timing and endpoint losses alongside the improvements.

The [bounded encoding and streaming record](docs/evidence/grpc-encode-reuse-20261026/README.md)
adds 3,855 passing workspace tests, 448 allocation captures / 83,160 successful
RPCs and 180 counters-off CPU captures / 272,160 successful RPCs. Large compressed
streams reuse bounded plaintext storage and avoid compressed-output overshoot;
an early-response race is fixed. Allocation costs fall in the measured large
gzip cases. CPU remains mixed, noise traffic still allocates more than tonic,
and a 64-message identity client case regresses. Keep those losses in scope. The separate
[compatibility repair](docs/evidence/grpc-compatibility-fallback-20261026/README.md)
restores `dyn WireOut`, the default native MSRV profile and consumer gate checks.

Source `af00f348` fixes required-field checks after singular proto2 message merges.
Split child fields, same-case oneofs, maps, groups, MessageSet extensions, cold
fields and parallel generation have detecting controls. Affected consumer bindings
must be regenerated. The two affected bundled proto2 outputs are regenerated;
eleven other outputs are byte-identical. Required-reachable validation can now
allocate a temporary tree.

The [October 26 record](docs/evidence/grpc-required-merge-20261026/README.md) has
33 generated-consumer checks, 113 core checks, 44 runtime/kernel checks and three
official conformance runs, each passing 5,631 binary/JSON and 909 text cases.
Benchmark `eac85b04` adds checked zero/noise payloads for Prost TestService: five
unit checks, 24 positive captures / 48 RPCs and eight expected rejection controls
pass. These are correctness checks, not performance results.

The [compatibility follow-up](docs/evidence/grpc-compatibility-fallback-20261026/README.md)
records the hosted public-API, MSRV and shared-suite inventory failures and their
repairs. Full hosted CI remains separate from the recorded local checks.

The earlier runtime `933bf877` passes 3,840 all-feature workspace tests and 961
selected ASAN/leak tests. Its [dated record](docs/evidence/grpc-ownership-20261024/README.md)
also has 1,116 native/instruction captures / 29,700 successful RPCs; bulk,
shipping-build and tracing controls remain separate. Those results do not
qualify later source changes.

The [benchmarks](docs/benchmarks.md) identify the next costs. An earlier
1 KiB unary server comparison improves from 45 to 23.5 µs/RPC, with a fresh
tonic median of 32.5 µs/RPC. Broader concurrency, transport and latency
comparisons remain open. Earlier allocation measurements retain their own pins.
Four-message 1 MiB noise bidi still requests 35.53% more client bytes and
37.23% more server bytes than tonic/prost; zero-filled traffic now requests
13.50% / 14.39% fewer bytes. The 64 × 1 KiB identity pipeline client requests
2.98% more bytes than baseline. Earlier instruction counts and source regressions
retain their own pins in the [historical tables](docs/benchmarks/grpc-20261024.md).

| Work | Next result | Completion criterion |
|---|---|---|
| Unary-server CPU and scheduling | Extend the measured wake-coalescing improvement to broader workloads. | Five unary and pipeline server pairs improve, across 1,890,000 successful RPCs. Keep fresh controls, sibling fairness, cancellation, cooperative scheduling and all losing repeats; dedicated-host qualification remains open. |
| Identity/gzip bytes and bulk | Reduce remaining noise and small-pipeline costs. | Bounded reuse now has endpoint measurements across zero/noise and one/four/64-message streams. Custom contiguous codecs keep their original path. Preserve the remaining noise and 64-message identity-client losses. |
| QG-06: sustained resources | Run full exact-source general/deadline days on a host retained for at least 24 hours. | Both `933bf877` attempts are interrupted, not running. Deadline progress ends at 12.35 measured hours; general progress ends at 13,673 cancellation cycles without a final duration. Preserve finite limits, ownership and recovery. |
| Remaining backend destruction | Audit generic current-frame panic, queue-clear, reset and EOF owners beyond the repaired retired DATA frames. | Independent bounded controls before broader safety claims; no hidden work detached from resource ownership. |
| Remaining protobuf compatibility | Extend the cross-runtime corpus for groups, extensions, unknowns and the remaining upb kernel. | Keep split-field, oneof/map, recursion and malformed-wire controls; generated-code conformance does not prove every kernel path. |
| MiniTable parse cost | Reduce the measured cost of the group and unknown-field repair. | The 100-fragment non-group fixture rises about 5% with no allocation increase. Preserve unknowns, required fields, recursion and group framing; three attempted layout changes did not improve it. |
| PK-27, PK-29: owned read-all decode | Extend the explicit owned profile and investigate sparse lazy traversal without repeated validation. | Preserve required/merge/oneof/extensions/unknown/recursion semantics and retention. Keep full-read, partial-read and encode costs separate; default lazy storage is unchanged. |
| Handler isolation and SB-27 | Complete per-side shipping CPU, latency, throughput, entropy, field-access, overload and physical-network comparisons. | Keep inline/task handoff costs, sibling fairness, actual concurrency and all losing scopes. |
| CH-12, credentials and telemetry | Finish discovery/xDS, async OAuth2/JWT, typed extensions and remaining instruments. | Supported external consumers, bounded refresh/cancellation/labels and wire-compatible behavior. |
| Platforms and release | Complete required feature/platform/consumer profiles and dedicated-host headroom/statistics. | Exact-source release checks and claimed workload scopes pass. |

Both `933bf877` days and the earlier `7c02eb38` attempts are **interrupted**, with no terminal resource
outcome or application failure established. Preserved progress and the newer boot
indicate an environment discontinuity; the exit cause is unavailable. They are
not running and cannot qualify the current source. Fresh `933bf877` retry grants
are prepared but have not launched. Older `beb4ecd9` general/cache days passed
only their recorded source and scope. The [execution decision](docs/decisions/grpc-execution.md)
explains ownership and comparison boundaries; [the benchmark contract](docs/benchmark-contract.md)
sets performance requirements. Production readiness, full specification coverage
and broad superiority remain open.

<a id="dominance-program"></a>

## Workload comparisons

Goal: use less CPU, allocate less and complete calls faster than tonic/prost
across the defined workloads, for both client and server with prost and pbrs
messages. Measure each side and metric separately. The
[workload comparison plan](docs/plan/dominance/README.md) defines acceptance
criteria and assigns each recorded loss to a task card.
Start with SB-27 (after SB-26) and RX-10, then address the recorded losses: SV-09,
CL-08, CL-07, CL-09, PK-26, PK-27 and PK-29.

## Adoption program

Goal: teams on prost and tonic adopt pbrs one layer at a time, without
rewriting handlers, middleware or build pipelines, and get clearly lower CPU
and memory cost. The [adoption program](docs/plan/adoption/README.md) defines
targets, phases and cards. Start with SB-26 (measure what adopters run), then
the P0 cards: GN-10, PK-26 then PK-27, TC-25, TC-29, TC-30 and AD-04.

### Findings that opened the program (2026-09-29)

An adoption evaluation against a production-shaped schema set and
tonic-based service stack filed these cards. Dev-loop evidence is in each
card's `notes`.

| Card | Priority | Finding |
|---|---|---|
| GN-10 | P0 | Fields named `default`, `clone` or `serialize` generate code that does not compile. |
| PK-26 | P0 | Message getters take a global mutex and hash a `TypeId` on every read of a present field. |
| PK-27 | P0 | Reading every field after decode costs 2-5x prost instructions on deep or `Any`-heavy messages. |
| TC-25 | P0 | TLS hard-wires the graviola provider; callers cannot pass rustls configs, providers or cert resolvers. |
| GN-11 | P1 | Each generated file carries private well-known-type copies; `::pbrs::wkt` in the docs does not exist. |
| TC-26 | P1 | Services cannot mount in tonic-typed frameworks (`NamedService`, tonic connect-info types). |
| TC-27 | P1 | The original adapter covered unary only; all four shapes now have typed Tower adapters. Continue checking stream ownership and middleware lifetime semantics. |
| TC-28 | P1 | The config-only `codec_path` route from tonic-prost-build works but is untested and undocumented. |
| PK-28 | P2 | Serialized field order differs from prost, C++ and Go. |

In the same evaluation, prost messages on pbrs-grpc cost 5-7% fewer
instructions and up to 29% fewer allocated bytes per unary RPC than on
tonic. pbrs messages cost more CPU on either transport once handlers read
every field (PK-27).

## Work that should wait

- **Custom HTTP/2 engine:** H2-04 is blocked by the recorded no-go decision.
  H2-16 can revisit it after current-backend profiles demonstrate at least
  15% modeled gain on primary client/server cells or a concrete correctness
  blocker. Completing a design document does not satisfy that gate.
- **Runtime and kernel experiments:** `io_uring`, kernel TLS and new arenas
  need a measured bottleneck, a bounded experiment and their existing gates.
- **Broader compatibility:** the Rust frontend, complete upb replacement and
  xDS keep their own lanes. They can proceed with available ownership, but
  they do not substitute for proving the shipping transport is efficient.

## Working in parallel

Assign one owner to each changing module. In particular, SB-21 and SB-24
share benchmark code; RX-09 and resource work can share transport files;
GN-02/GN-03 share generated-code emission. Sequence those edits or split their
write sets explicitly. The [worker protocol](docs/plan/world-class/README.md#worker-protocol)
defines the handoff: source revision, hypothesis, checks, artifacts, reviewer
and remaining limits.

Use `mingley` for GitHub work and `michael.ingley@gmail.com` for commits.
Preserve unrelated local edits and use an isolated worktree when necessary.

## Promotion is a separate decision

[GR work packages](docs/ROADMAP.md) describe supported product profiles.
QL-03 is performance qualification, QL-04 is applicable official-suite
coverage, and QL-05 is API/support promotion. None is closed by this audit.
A release follows the [release policy](docs/RELEASE.md); a push to `main`
does not publish crates.
