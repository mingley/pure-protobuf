# World-class gRPC program

**Snapshot:** 2026-09-26. **Source baseline:**
[`e9272edc`](https://github.com/mingley/pure-protobuf/tree/e9272edc801326f6aad371d4021e5998b57af6ce).
**Coordinator and scope approver:** Michael Ingley.
**Work queue:** [`tasks.json`](tasks.json) (169 cards, 14 lanes, dependency-checked and linted).

This program has four goals:

1. **Win the most performance categories** on client, server, codec and codegen,
   against the strongest Rust and non-Rust gRPC stacks.
2. **Replace upb** with a pure-Rust kernel that runs Google's official Rust
   protobuf API and gencode unmodified, passes Google's tests and runs faster
   than upb.
3. **Offer a better, faster alternative to tonic** that tonic users can adopt
   incrementally.
4. **Over the long term, support every applicable gRFC,** including proxyless
   xDS, backed by official interop evidence.

This plan extends the [leadership execution plan](../README.md) and its
[legacy cards](../tasks.json). It does not replace them. Legacy cards that are
done stay done. The open legacy cards are either handed to a card here or kept
as they are; the [reconciliation](#relationship-to-the-existing-plan) section
lists which. The main change is that performance work no longer has to wait
for dedicated hardware. Workers optimize against deterministic
[dev-loop metrics](#scoreboard). Claims need the existing
[benchmark contract](../../benchmark-contract.md).

"Fastest in the world" is the aim, not something to claim. What gets published
is category wins on a named, reproducible matrix, with the losses shown
([contract §10.2](../../benchmark-contract.md#102-claim-scope)).

## Where we stand

Sources: three code audits and one external survey, all done on 2026-09-26.
The audits read `6b8e2a3b` plus the then-uncommitted A6 work (unary
service-config retries, throttling, pushback and hedging), which landed
unchanged as `02390322` and `e9272edc`. MX-00 reconciles the docs with that
work before any client-side card starts.

| Area | Strengths today | Gaps that decide leadership |
|---|---|---|
| Codec | Official conformance v35.1, 5,631 + 909 cases, no skip list. Twelve codec cells favored pbrs over prost, v4/upb and buffa on one Apple M4 Pro host (historical, unqualified). Lazy strings, bytes and packed fields. Encode writes straight into a pre-sized buffer. | Varint decode is scalar; there is no SIMD or table-driven parse. `Parse` accepts only `&[u8]`, so frames split across chunks are coalesced and large `bytes` fields are copied again. JSON and text build `serde_json::Value` trees or fall back through dynamic messages. Recorded losses: `name_80` against prost, and 5 MiB packed-fixed encode against upb. Retained memory is not measured. |
| Codegen | Deterministic, byte-stable output. Multi-file layout contracts. Builds from descriptor sets with no `protoc`. | The one pinned comparison with upb (six messages) loses 8 of 12 metrics: clean check 8.77 s vs 5.68 s, release build 37.1 s vs 10.0 s, generation 295 ms vs 179 ms. The runtime crate always compiles the generator, reflection, JSON and text. Compiling `.proto` files still needs `protoc` (CG-16 is blocked because no Rust frontend supports editions). |
| Transport | Native client and server on `h2` 0.4.19. All four call shapes, TLS/mTLS, UDS, limits, drain, health, reflection, keepalive. No task spawn per unary call on the client. | No transport seam: `h2` types are used directly across client, server and wire. `h2` guards each connection with two mutexes. The server spawns one task per RPC and allocates redundant per-RPC state. There is no BDP flow control, no runtime seam, and no thread-per-core or `io_uring` mode. |
| Evidence | Separate-process harnesses, open-loop load, endpoint CPU/RSS accounting, WorkerService building blocks, pinned Go and C++ peers, strict interop registry. | The recorded tonic transport wins are **not valid yet**. tonic's `serve_with_incoming` ignores its `tcp_nodelay` setting, flow-control windows were not matched, one runtime was shared, and sample counts were small; SB-01 fixes this. The bundled official QPS scenarios cannot run, because the worker rejects their thread controls; SB-10 fixes this. No CI tracks performance. There are no C++/Go codec comparators and no cross-stack server matrix. |
| upb replacement | Official `--rust_out` gencode for Person and 19 upstream shared-test crates (233 tests) links against a pure-Rust stand-in in `src/runtime.rs`. | The stand-in is not a kernel. It finds fields by linear scan, keeps boxed per-message slot vectors and uses an `Rc<RefCell>` "arena". Several entry points are stubs: enum tables return null, there is no extension registry, `message_eq` always returns false, `debug_string` returns `"<msg>"`, and unknown fields are skipped. `protoc --rust_out` accepts only `kernel=cpp` or `kernel=upb`. Google now ships `google-protobuf` 0.36.x; the `protobuf` v4 crate is superseded. |
| Better tonic | The `protobuf-tonic` adapter covers all four shapes, gzip, interceptors, health and reflection on tonic 0.14. | The native crate has no `tower` integration and cannot carry prost messages. Generated types are not `prost::Message`. There is no tonic-shaped API mode or migration proof. The adapter's name (`pbrs-tonic`?) is an open maintainer decision. |
| gRFCs | See the [gRFC matrix](../../grfc.md). A8, A9, A15, A17 and A90 ship; A6 is partial (unary policy retry and hedging landed in `02390322`; streaming policy retry is GF-07). | No resolver or LB architecture, no ORCA, xDS, channelz, OTel, binary logging, authz, CRL or SPIFFE. `grpc/proposal` has 89 merged A-series files as of commit `6342be7`. |

External context, dated 2026-09-26:

- **Rust gRPC.** tonic 0.14.6 is developed in `grpc/grpc-rust` and runs
  Tower + hyper + `h2`. Google's `grpc` 0.9.0 preview (2026-05-28) still sits
  on hyper and tonic machinery, and its server module is private.
  `hyperium/h2#531` (mutex contention) is open and PR #917 is unmerged.
- **Google's position.** A maintainer
  [comment on protobuf#24880](https://github.com/protocolbuffers/protobuf/issues/24880)
  (2025-12-16) says a pure-Rust kernel would most likely be "rewriting upb
  itself into Rust", able to back Python, Ruby and PHP too. They have no
  timeline, and their main objection is the correctness and maintenance cost
  of "yet another parser", not speed. The UK lane therefore aims at *upb
  semantics in Rust* behind the official API and treats the review bar as a
  requirement.
- **grpc_bench**
  ([report dated 2026-04-22](https://github.com/LesnyRumcajs/grpc_bench/discussions/559)).
  With 4 server CPUs: Vert.x 149,776 req/s, tonic 136,638 req/s (14.6 MiB),
  grpc-go 117,082 req/s. With 1 CPU, single-thread tonic leads at
  102,754 req/s. These are reference points to re-measure on our own hosts
  (SB-17), never to compare across hosts.
- **Techniques worth borrowing.** upb MiniTables with fasttable dispatch,
  C++ TcParser tail-call tables, Buf's hyperpb (a runtime-compiled parser VM
  for Go dynamic messages), buffa's owned/view/lazy split, grpc-go's loopy
  writer and BDP pings, and Kestrel's stream pooling.

## Scoreboard

The category list lives in [`tasks.json`](tasks.json)
(`scoreboard_categories`); SB-02 makes it the authoritative scoreboard.
The word **leadership** is reserved for claim-grade results. Every
optimization card names the categories it targets.

| Group | Categories | Comparators (pinned by SB-02) |
|---|---|---|
| A. Codec | A1 fresh encode, A2 cached encode, A15 mutate-then-encode, A3 owned decode, A4 borrowed decode, A5 parse+touch, A6 merge, A7 size, A8 clone, A9/A10 JSON, A11 text, A12 dynamic decode, A13 allocations/retained memory, A14 1-64 MiB payloads | prost, buffa (owned, eager view, lazy view kept separate), `google-protobuf` (upb), C++ protobuf (arena), upb C, Go protobuf, vtprotobuf, hyperpb (A12 only) |
| B. Codegen | B1 generation time, B2 generator RSS, B3 generated bytes, B4 clean check, B5 incremental check, B6 release build, B7 build RSS, B8 binary size, B9 no `protoc`/no C compiler | prost-build + tonic-build, `protoc --rust_out` (upb), buffa-build |
| C. Client | C1 unary latency, C2 QPS/core, C3 streaming msgs/s/core, C4 large messages, C5 instructions/allocations per RPC, C6 memory, C7 time to first RPC, C8 CPU/RPC at matched load | tonic, grpc-go, grpc-java (Netty), grpc-c++, grpc-dotnet, volo-grpc, connect-rust, Google `grpc` |
| D. Server | D1 latency at fixed load, D2 QPS/core within the p99 SLO, D3 streaming/core, D4 CPU/allocations per RPC, D5 memory per connection/stream, D6 accept and TLS handshake rate, D7 overload goodput/fairness, D8 multi-core scaling | same as C, plus the Vert.x and Quarkus rows from grpc_bench |
| E. End-to-end | E1 grpc_bench-style, E2 official WorkerService scenarios, E3 TLS, E4 compression, E5 real-network RTT | same as C/D |
| F. Gates | F1 conformance and unmodified upstream Rust tests, F2 interop, F3 HTTP/2 negative tests and h2spec, F4 gRFC coverage, F5 xDS interop, F6 fuzz/Miri, F7 pure-Rust graph | These must never regress. A win that breaks a gate does not count. |

Evidence comes in two tiers. They are never mixed.

| Tier | Purpose | Rule |
|---|---|---|
| **Dev-loop** (SB-03/04) | Fast iteration on any Linux host and in CI | Instructions retired, exact allocation counts, syscalls and copies per operation, then wall time. A change is a **dev-loop win** when instructions or allocations fall by at least 2% on the targeted cells and no primary cell regresses by more than 1% (2% for RPC cells), with all F gates green. Results are labeled "dev-loop" and are never called leadership. |
| **Claim-grade** (SB-15, SB-22, SB-16) | Publishable category results | [Benchmark contract](../../benchmark-contract.md) §6-§10: dedicated x86_64 and arm64 hosts, five or more randomized paired runs, 95% intervals, percentile sample floors, and the §7 margins (for example ≥20% lower CPU per RPC, or ≥20% higher sustainable throughput, with no p99 regression above 5%). |

## Strategy

The bets are ordered by leverage and dependency. Each has a go/no-go gate, so
a bet that fails can stop without wrecking the others.

1. **Make the evidence honest, then fast (SB, M0).** Fix the tonic
   comparator. Make the official scenarios runnable. Add deterministic
   dev-loop metrics and a CI regression lane. Then build the cross-stack
   matrix, the cross-language codec peers and the codegen comparators.
   Nothing else can claim a win until SB-01 through SB-04 land.
2. **Split the monoliths for parallel work (MX, M0).** `src/codegen.rs`
   (9.1k lines), `server.rs` (4.6k), `client.rs` (4.4k), `src/runtime.rs`
   (2.5k) and `pbrs-grpc/src/wire.rs` (1.7k) are serialization points for
   workers. Split them mechanically, with byte-identical output, into named
   submodules that later cards write individually (MX-01..05). First, bring
   the docs in line with the just-landed A6 retry work (MX-00).
3. **Codec leadership (PK, M1).** Profile first (PK-01). Then: SWAR/SIMD
   varints; a hybrid table-driven parser that keeps inline parsing for small
   hot messages and uses shared tables for wide or cold ones, which cuts both
   parse time and compile time; `Bytes`-owning and segmented parsing;
   scatter-gather encode; borrowed views; streaming JSON/text; a
   hyperpb-style dynamic parser; and an arena experiment.
4. **Codegen leadership (GN, PK-02/03, M1).** Stop compiling the generator
   into the runtime. Shrink generated code using GN-01's measured attribution.
   Make the generator the fastest. Ship a pure-Rust `.proto` frontend with
   editions, preferably by contributing upstream (GN-05 decides), proved by a
   differential corpus against `protoc`.
5. **Transport leadership in two steps (CL, SV, H2, RX; M2 then M3).** Take
   quick wins on the current `h2` backend first: unary call specialization,
   header templates, lock-free picking, static routing, no redundant per-RPC
   allocation. Add the transport seam (H2-02) regardless of what comes next.
   If H2-01's measurements show at least a 15% achievable gain, build
   **pbrs-h2**: a sans-IO, single-owner HTTP/2 engine specialized for gRPC
   (pre-encoded headers generated per method, vectored writes, BDP flow
   control, no per-connection mutex). One exact candidate SHA must pass
   h2spec, the official HTTP/2 negative tests, 24 CPU-hours of fuzzing per
   target and a 24-hour soak (H2-15) before it becomes the default (H2-14). Thread-per-core, `io_uring` and kTLS are measured experiments
   behind the runtime seam.
6. **Replace upb (UK, M4; runs in parallel from M0).** Inventory the
   official ABI. Write the upb-in-Rust design. Build a fusing arena, full
   MiniDescriptor decoding, MiniTable-native layout, a fasttable-class
   decoder, a single-pass encoder, extensions, maps and reflection. Run
   **every upstream shared suite unmodified** and a rust_out conformance
   testee. Beat upb on Google's own messages. Ship a `[patch]` drop-in for
   `google-protobuf` and prove it with an unmodified Google `grpc`
   helloworld. Then prepare an upstream packet that meets their review bar;
   posting it needs approval.
7. **Better tonic (TC, M5).** Make the native codec generic, so prost
   messages work over the faster transport. Add `tower` service and
   middleware integration for axum co-hosting, a tonic-shaped API mode, and
   migrations of tonic's own examples with published diffs. TC-01 settles
   names and product shape first.
8. **Fleet and gRFCs (CH, XD, GF; M6/M7).** Build a channel architecture on
   gRPC's model (resolver, service config, LB tree, subchannels, picker) with
   a picker that neither locks nor allocates. Then the LB policies (pick_first
   and round_robin from the legacy FL cards, WRR with ORCA, ring hash, least
   request, priority, outlier detection). Then xDS, after a build-versus-reuse
   decision about `grpc/grpc-rust`'s `xds-client` alphas, then observability
   and security.

## Target architecture

```text
            application code and generated stubs
  +---------------------------+-------------------------------+
  | pbrs plugin gencode       | official rust_out gencode     |
  | owned structs, views      | google-protobuf API (UK lane) |
  +-------------+-------------+---------------+---------------+
                |   shared wire primitives    |
  +-------------v-------------+ +-------------v---------------+
  | pbrs runtime              | | upb-in-Rust kernel          |
  | inline + table parse,     | | MiniTables, fusing arena,   |
  | Bytes/segmented parse,    | | fasttable-class decoder,    |
  | streaming JSON/text       | | single-pass encoder         |
  +-------------+-------------+ +-------------+---------------+
                +------ codec trait (TC-02) --+   optional prost codec (TC-03)
  +---------------------------------------------------------------+
  | pbrs-grpc client: channel -> resolver -> LB tree -> picker    |
  | pbrs-grpc server: accept -> static router -> handlers         |
  | tower adapters (TC-04/05), tonic-shaped mode (TC-06)          |
  +------------------- HTTP/2 transport seam (H2-02) -------------+
  | h2 backend (today, later fallback) | pbrs-h2 sans-IO engine   |
  +------------------- runtime seam (RX-01) ----------------------+
  | Tokio work-stealing | thread-per-core (RX-02) | io_uring (RX-03?) |
  +---------------------------------------------------------------+
  alongside: resolver/LB registries, ORCA, xDS client, OTel, channelz, authz
```

Rules that keep the architecture honest:

- **One wire engine per concern.** The plugin path and the official-API path
  share varint, UTF-8, packed and unknown-field primitives. UK-18 decides how
  far they converge.
- **Seams before engines.** Add the transport seam (H2-02) and the runtime
  seam (RX-01) first, with no behavior change. A new engine sits behind a
  seam and a feature flag until its gates pass.
- **Few new crates.** The maintainer prefers small, cohesive releases, and
  the current release rule is three qualified crates together. Each proposed
  crate is a maintainer decision: the runtime/build split (PK-02), `pbrs-h2`
  (H2-03), kernel placement (UK-02), better-tonic packaging and the
  `pbrs-tonic` name (TC-01), and xDS placement (XD-01).
- **Pure Rust in shipping profiles.** QG-04 audits every shipping feature
  set. C/C++/Go/Java peers, `h2spec` and the upb C harness are test tools and
  never ship.

## Milestones and exit criteria

Milestones group outcomes. They are not dates, and tracks run in parallel:
M4 (upb) and M5 (tonic) can start during M0.

| Milestone | Exit criteria |
|---|---|
| **M0** Truthful baseline and fast feedback | Tonic comparator fixed and its old claims retracted (SB-01). The seven core official scenarios run end-to-end as diagnostics (SB-10). Dev-loop harness and CI lane live (SB-03/04). Docs reconciled with the landed A6 retry work (MX-00) and monoliths split (MX-01..05). Pure-Rust audit and Miri policy in CI (QG-04/01). Corpora frozen (SB-05). Scoreboard published with honest standing (SB-02). |
| **M1** Codec and codegen wins | Dev-loop wins on at least 80% of primary A cells against **every** comparator, including C++ protobuf and upb C. No unexplained A-cell loss above 5%. (Claim-grade leadership, meaning contract §7.2's ≥15% lower encode plus owned-decode CPU across common unary shapes, is decided only by SB-15.) B4 and B6 beat prost-build and the upb reference on the 100- and 1,000-message corpora. The pure-Rust frontend matches `protoc` on the whole differential corpus (GN-07b). Rust-only consumers build (GN-09). |
| **M2** Transport wins on the `h2` backend | On the same host, pbrs-grpc wins dev-loop C2 and D2 cells at 1 and 4 CPUs against the required peers (tonic, grpc-go, grpc-c++; SB-11) and every optional peer that runs (SB-18). Its grpc_bench entry beats the re-measured leaders in a diagnostic reproduction (SB-17). F2/F3 green. The transport seam has landed (H2-02). |
| **M3** pbrs-h2 engine and runtime modes | H2-14 decision recorded. pbrs-h2 uses at least 15% less CPU per RPC than the `h2` backend on primary cells, and one candidate SHA passed H2-15 (h2spec, negative interop, 24 h soak, 24 CPU-hours of fuzzing per target). Thread-per-core mode scales better than work-stealing (RX-02). |
| **M4** upb replacement kernel | Rerunning the UK-01 inventory shows zero stubs (UK-10). Every upstream shared suite passes unmodified (UK-11). The rust_out conformance testee has zero unexpected results (UK-12). Dev-loop wins against upb C on Google's messages (UK-13). Unmodified `grpc` helloworld and downstream workspaces run on the `[patch]` facades for every supported release pin (UK-14), with a scheduled drift gate (UK-19). Upstream packet ready (UK-16). |
| **M5** Better tonic | Prost messages run over the native transport (TC-03). Axum co-hosting example (TC-04). tonic's examples ported with published diffs (TC-07). Parity matrix published (TC-08). |
| **M6** Fleet | Channel architecture with every LB policy qualified against the picker budget (CH-11). Core xDS (A27/A28/A30/A29/A36) passing local psm-interop cases (XD-10). |
| **M7** Observability, security and full gRFC profile | GF-09: every merged A- and G-series gRFC in the [gRFC matrix](../../grfc.md) is **shipped with tests** or an **approved boundary with a reason**; nothing is left planned or partial. This feeds legacy QL-04. |
| **M8** Claims | Claim-grade codec/codegen (SB-15) and transport/end-to-end (SB-22) campaigns and the generated scoreboard (SB-16) feed QL-03. Wording follows contract §10.2. |

Longest dependency chains (from [`tasks.json`](tasks.json)). Staff these first:

| Chain | Cards |
|---|---|
| pbrs-h2 default (13) | SB-02 → SB-03 → SB-11 → H2-01 → H2-03 → H2-04 → H2-04b → H2-05 → H2-05b → H2-06 → H2-07 → H2-15 → H2-14 |
| Full gRFC profile (9) | MX-00 → MX-02 → CH-02 → CH-03 → CH-08 → XD-05 → XD-05b → XD-10 → GF-09 |
| upb drop-in (8) | UK-01 → UK-02 → UK-03 → UK-05 → UK-06 → UK-08 → UK-11 → UK-14 |
| Borrowed views (8) | SB-02 → SB-03 → SB-08 → PK-01 → PK-05 → PK-06 → PK-07 → PK-20 |
| Claims (7) | SB-02 → SB-03 → SB-11 → SB-17 → SB-21 → SB-22 → SB-16 |
| Rust-only codegen (7) | GN-05 → GN-08 → GN-06 → GN-06b → GN-07 → GN-07b → GN-09 |

## First assignments

Fifteen cards have no open dependencies. Start with this wave. Their write
scopes are disjoint except where noted (see `write_hotspots` in
[`tasks.json`](tasks.json)).

| Card | Executor | Why now | Coordination note |
|---|---|---|---|
| MX-00 | small | The just-landed A6 docs contradict each other on hedging; client-side cards build on this. | Docs only; records the base SHA. |
| SB-01 | small | Every transport claim is invalid until the comparator is fair. | Shares `rpc-bench/tests/` with SB-10; use separate test files. |
| SB-10 | small | Lets the core official scenarios run (carries legacy BM-08..11). | See SB-01. |
| SB-02 | coordinator | Unblocks SB-03, SB-05 and GN-01. | Decision only. |
| MX-01 | small | Six later lanes edit codegen. | Rebase over any open codegen PR first. |
| MX-03 | small | SV, H2, XD and TC edit the server. | Declares submodules from `server.rs`; `lib.rs` untouched. |
| MX-04 | small | UK cards need disjoint runtime modules and the `upb_kernel` test target. | Before UK-03/04. |
| MX-05 | small | Ten cards edit `pbrs-grpc/src/wire.rs`. | Independent of MX-03. |
| QG-04 | small | Keeps every later dependency choice pure-Rust. | Edits `ci.yml`. |
| QG-01 | small | Miri gate before the new kernels exist. | Edits `compatibility.yml`; serialize with GF-08. |
| UK-01 | small | Starts the upb critical path; needs only fetched upstream sources. | Read-only against `src/runtime.rs`. |
| GN-05 | coordinator | The frontend route sets the whole GN lane. | Maintainer approval required. |
| TC-01 | coordinator | Settles naming and product shape (includes the open `pbrs-tonic` decision). | Maintainer approval required. |
| CH-01 | coordinator | The channel design gates all LB and xDS work. | Design only; implementation waits for MX-02. |
| GF-08 | small | Cheap drift detector for the gRFC catalogue. | P2; fill spare capacity. |

Second wave, as dependencies clear: MX-02 (after MX-00), SB-03, SB-05,
GN-01, GN-08, UK-02 (coordinator), then SB-04, SB-14, SB-13, SB-08.
Ready legacy work (BM-03, BM-08, CG-14, DX-07, EX-20, IO-08/09, OB-03,
PB-07, RT-07) continues in parallel under the legacy plan; the
[ready query](#ready-query) lists both.

## Worker protocol

The legacy [small-executor contract](../README.md#small-executor-contract)
still applies in full. This program adds the following rules.

1. **Pick and claim.** Run the [ready query](#ready-query) and claim one card
   through the coordinator, who sets `status: in_progress` and `implementer`.
   Work from the exact base SHA in a separate worktree, on a branch named
   `<user>/<card-id>-<slug>`.
2. **Check the premise first.** Read the card, its dependencies' artifacts and
   the current source. If the gap is already closed, or a measurement
   disproves the hypothesis, report the evidence and stop. The coordinator
   re-scopes the card. A disproved hypothesis is a useful result.
3. **Measure before editing (every PK, GN, CL, SV, H2 and RX optimization).**
   Capture dev-loop baseline JSON and a flamegraph on the base SHA. State one
   hypothesis and name the target categories. Make one coherent change.
   Capture the after-JSON and compare it. Fill in the perf-PR template
   (SB-14). A change that fails the dev-loop win rule does not merge as a
   performance change.
4. **Gates never regress.** Run the card's `checks` plus every gate it can
   affect: conformance (F1) for codec/codegen changes; interop and hostile
   tests (F2/F3) for transport changes; Miri for new `unsafe`. Add a failing
   regression test before fixing a defect.
5. **Scope.** Stay inside the card's `write` list. Change generated files
   only through the regeneration scripts. An `L` card may span several PRs,
   but each PR must leave the tree green. The coordinator may split any `L`
   card into lettered children (for example `XD-04b`) before assigning it.
   If scope grows, split before continuing. Never run two cards listed
   together under one `write_hotspots` path at the same time; that list
   includes open legacy cards. Legacy cards that name a pre-split file (for
   example FL cards on `pbrs-grpc/src/client.rs`) write the matching
   post-split submodule, and must not run while the MX split is in flight.
6. **Dependencies and unsafe.** Get review before adding any dependency;
   shipping profiles must pass QG-04. Every new `unsafe` needs a `SAFETY`
   comment, an entry in [unsafe invariants](../../unsafe-invariants.md) and
   Miri coverage.
7. **Claims.** Label every number `dev-loop` or `claim-grade`. Never write
   "fastest" without a named matrix. Never add a code path that fires only
   under benchmark traffic ([contract §10.3](../../benchmark-contract.md#103-no-benchmark-specific-escape-hatches)).
8. **Return.** Report the diff or commit, the exact commands and results,
   dev-loop JSON paths, pins and remaining limits. A card is `done` only after
   review confirms every `accept` item.
9. **Stop and escalate** on: a public API or crate-name decision; any change
   to protocol semantics or security defaults; a threshold change; a new
   dependency; any external action (upstream PRs or comments, publishing,
   cloud spend, sending data to third parties). Each of these needs explicit
   maintainer approval.

Card fields: `executor` is `small` (one focused worker), `coordinator`
(design or decision; produces a record in `docs/decisions/`) or `operator`
(needs hardware, people or external approval). `size` is S (about one PR),
M (a few PRs) or L (several PRs; split whenever possible). `relates` points
to legacy cards whose intent the card carries. `planned_checks` are commands
that do not exist yet; the card named in `introduced_by` must create and
document them, and every card that cites one depends on that card.
`write_hotspots` lists paths shared by cards with no dependency order
between them; the coordinator serializes those.

Assignment prompt for a worker:

```text
Implement CARD_ID at BASE_SHA, and nothing else.
Read docs/plan/world-class/README.md (worker protocol), the CARD_ID object in
docs/plan/world-class/tasks.json, docs/plan/README.md (small-executor contract)
and the artifacts of its dependencies. Stay within the write scope.
For optimization cards, capture dev-loop baseline JSON before editing.
Meet every accept item; run the listed checks and affected gates.
Do not add dependencies, change public APIs or thresholds, or take external
actions without approval. Stop with a bounded blocker if scope expands.
Return changed files, commands and results, dev-loop JSON and limitations.
```

## File ownership

Hot files serialize work. After the MX splits land, assign by submodule.

| Path | Owning lanes | Rule |
|---|---|---|
| `src/codegen.rs` → `src/codegen/{config,descriptors,naming,messages,parse,encode,json,text,reflection,native_stubs,tonic_stubs}.rs` | GN; PK-07/08/11/14/16/19/21; SV-04; H2-10; TC-03/06 (new `prost_stubs.rs`, `compat_stubs.rs`) | One writer per file. Output changes regenerate in the same PR. |
| `src/wire.rs`, `src/lazy.rs`, `src/string.rs`, `src/packed.rs`, `src/map.rs` | PK | Serialize. The UK lane reads them but does not write them without coordination (UK-18). |
| `src/runtime.rs` → `src/runtime/{arena,mini_table,layout,decode,encode,extension,map,array,reflect}.rs` | UK | One UK card per module file; MX-04 creates them. |
| `pbrs-grpc/src/client/{channel,pool,call,unary,streaming,retry,config_glue}.rs` | CL, CH, GF-07, H2-02, RX | Serialize per file. Everything waits for MX-00. |
| `pbrs-grpc/src/server/{accept,connection,dispatch,router,rpc,drain}.rs` | SV, XD-07, H2-02, RX, GF | Serialize per file. |
| `pbrs-grpc/src/wire/{headers,encode,frame_reader,out_batch,send}.rs` | CL-03, SV-05/06, PK-09..11, H2-02/10, TC-02, RX-07 | Serialize per file (MX-05 creates them). |
| `pbrs-grpc/src/transport/`, `pbrs-h2/` | H2 | H2 only, once H2-02 lands. |
| `bench/devloop/`, `bench/scoreboard/`, `docs/benchmarks.md`, `docs/scoreboard.md` | SB | Other lanes add cells in small PRs reviewed by an SB owner. |
| `.github/workflows/*.yml` | SB-04/20, QG, UK-11/12/19, GN-07b/09, H2-07, XD-10, GF-08 | Serialize per workflow file. |
| `docs/grfc.md` | CH, XD, GF | Update in the same PR as the feature. |

## Guardrails

| Gate | Command or source | Applies to |
|---|---|---|
| F1 conformance | `./scripts/conformance.sh` (plugin testee); `upb-conformance` once UK-12 lands | All codec, codegen and UK changes |
| F1 upstream Rust tests | `./scripts/test-rust-out-shared.sh`, then UK-11's unmodified suite | UK and runtime changes |
| F2 interop | `./scripts/grpc-interop.sh`, C++ and tonic peers in CI | All transport changes |
| F3 HTTP/2 | `scripts/grpc-http2-interop.sh`; `h2spec` once H2-07 lands; hostile tests | Transport and pbrs-h2 |
| F6 fuzz/Miri | `fuzz/` targets; scheduled Miri, which QG-01 extends to PRs for new kernels | New `unsafe`, parsers, engines |
| F7 pure Rust | `dep-audit` once QG-04 lands | Every manifest change |
| Security defaults | Existing TLS verification, rapid-reset and CONTINUATION defenses, OB-03 masking; A29 never falls back to insecure credentials | All lanes |

## Decisions required

Each of these is a coordinator card that writes a record in `docs/decisions/`
and needs maintainer approval before its dependents start.

| Card | Decision |
|---|---|
| SB-02, SB-20 | Scoreboard categories, comparators and win rules; blocking thresholds for the perf workflow |
| PK-02 | Split build-time code out of the runtime: features or a new crate |
| PK-05 | Hybrid table-driven parse design and code-size budget |
| PK-12, PK-13 | Arena adoption; borrowed-view model (PK-20 implements it) |
| UK-02 | upb-in-Rust kernel architecture and crate placement |
| UK-17, UK-18 | C ABI for other languages (stretch); plugin/kernel convergence |
| H2-01, H2-03, H2-14 | Custom-engine go/no-go; engine design and crate; default switch |
| RX-03, RX-05, SV-03 | `io_uring`; kTLS; server dispatch model |
| GN-05 | Pure-Rust frontend: contribute upstream or build in-repo |
| TC-01, TC-08, TC-09 | Better-tonic product shape and names (the open `pbrs-tonic` question); parity-gap cards; gRPC-Web/Connect scope |
| CH-01, XD-01, GF-09 | Channel architecture; xDS build versus reuse; final gRFC profile and approved boundaries |

The release rule does not change: new versions of the three current crates
ship together only after qualification
([RELEASE](../../RELEASE.md)). Adding a crate or renaming one needs an
explicit decision.

## Risks

| Risk | Mitigation |
|---|---|
| A custom HTTP/2 engine brings correctness or security bugs | Go/no-go on measured gain (H2-01). h2spec, official negative tests, differential fuzzing against `h2`, 24 h soak, hostile suite. Feature-flag fallback to `h2` for at least two releases. |
| Optimizations overfit the benchmarks | Frozen holdout corpora (SB-05), cross-language peers, the §10.3 anti-gaming rule, and a claim-grade tier kept separate from dev-loop numbers. |
| The kernel's `unsafe` surface (UK, PK arenas and views) | Miri on every PR (QG-01), differential fuzzing against upb C (UK-15), unsafe inventory, upstream black-box tests run unmodified. |
| Google never accepts a third kernel | The kernel is still worth having through the `[patch]` facade and its speed. Upstream engagement is evidence-led (UK-16) and nothing depends on acceptance. |
| `google-protobuf` is beta, and its API or gencode changes | The UK-01 inventory script reruns per upstream release; supported versions are pinned. |
| Table-driven code trades speed for size | Per-message auto strategy with an override (PK-07), plus a code-size budget and dev-loop regression limits. |
| Coordinator bandwidth (25 decision cards) | Decisions are batched per milestone with short templated records. Small cards never make architecture choices. |
| Hardware for claims is not available | Dev-loop evidence lets the work continue; claims wait for SB-15 (operator). |
| gRFC/xDS scope explodes | Build-versus-reuse first (XD-01); xDS starts after core transport wins; boundaries recorded with reasons. |

## Relationship to the existing plan

The [legacy plan](../README.md) has 52 of its 128 cards done. Its protocol
evidence (GT, IO), codegen correctness (CG) and retry safety (RT) remain the
foundation here. The `reconciliation` array in [`tasks.json`](tasks.json)
covers 60 of the 76 open legacy cards:

- **`executed_by` (40 cards)**: the new cards carry the legacy intent. The
  legacy card counts as done for dependency purposes once all of its
  executing cards are done, and the coordinator then marks it `superseded`
  with a pointer. Examples: OP-01 → PK-01, CP-01 → CL-01, SP-01 → SV-01,
  BM-03 → SB-05/08/15, BM-08..11 → SB-10, BM-12/13 → SB-15/SB-22, CG-16..20 → GN/SB, FL-01 → CH-01,
  EX-07..16 → XD, OB-02 → GF-01. As a result, client, server and codec
  optimization no longer waits for BM-13's dedicated hosts.
- **`retained` (20 cards)**: the legacy card stays authoritative, and the
  listed new cards relate to it or unblock it. A `gate` list adds an explicit
  ordering: QL-03 waits for SB-16, and QL-04 waits for GF-09. Examples:
  FL-02..09, CG-14/14b, OP-04/05, RT-07/09, PB-07, IO-08/09, OB-03.

The other 16 open legacy cards (DX-06, DX-07, DX-08, OB-04, OB-05, EX-03, EX-04, EX-05, EX-06, EX-17, EX-18, EX-19, EX-20, QL-01, QL-02, QL-05) continue unchanged
under the legacy plan, and the ready query below surfaces them too.

## Ready query

This lists every card whose dependencies are done, across both files. It
honors `executed_by` and `gate`, hides legacy cards that new cards now carry,
and shows retained or unmapped legacy work:

```bash
python3 - <<'PY'
import json
new = json.load(open("docs/plan/world-class/tasks.json"))
old = json.load(open("docs/plan/tasks.json"))
rec = {r["legacy"]: r for r in new["reconciliation"]}
cards = {t["id"]: t for t in old["tasks"]} | {t["id"]: t for t in new["tasks"]}
done = {i for i, t in cards.items() if t.get("status", "pending") in ("done", "superseded")}
for r in rec.values():
    if r["disposition"] == "executed_by" and all(b in done for b in r["by"]):
        done.add(r["legacy"])
carried = {i for i, r in rec.items() if r["disposition"] == "executed_by"}
rows = []
for plan, src in ((new, "new"), (old, "legacy")):
    for t in plan["tasks"]:
        if t.get("status", "pending") != "pending" or t["id"] in carried:
            continue
        needs = t.get("depends_on", []) + rec.get(t["id"], {}).get("gate", [])
        if all(d in done for d in needs):
            rows.append((t.get("priority", "P1"), src, t["id"], t.get("executor", "small"), t["title"]))
for pri, src, cid, ex, title in sorted(rows):
    print(f"{cid:7} {pri} {src:6} {ex:11} {title}")
PY
```

To print one card with its defaults applied:

```bash
python3 - SB-01 <<'PY'
import json, sys
plan = json.load(open("docs/plan/world-class/tasks.json"))
card = next(t for t in plan["tasks"] if t["id"] == sys.argv[1])
print(json.dumps({**plan["task_defaults"], **card}, indent=2))
PY
```

## Card index

Generated from [`tasks.json`](tasks.json). The JSON is authoritative for scope,
acceptance and checks. Cards marked with † are ready at the snapshot.

### MX: Maintainability splits that unlock parallel workers

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| MX-00 † | Reconcile docs and tests with the landed A6 retry work | none | small | S | P0 |
| MX-01 † | Split src/codegen.rs into a module tree with byte-identical output | none | small | M | P0 |
| MX-02 | Split pbrs-grpc/src/client.rs into a client/ module tree | MX-00 | small | M | P0 |
| MX-03 † | Split pbrs-grpc/src/server.rs into a server/ module tree | none | small | M | P0 |
| MX-04 † | Split src/runtime.rs into module files and a kernel test skeleton | none | small | S | P0 |
| MX-05 † | Split pbrs-grpc/src/wire.rs into a wire/ module tree | none | small | S | P0 |

### SB: Scoreboard and evidence engine

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| SB-01 † | Repair tonic comparator fairness and retract unverified transport claims | none | small | S | P0 |
| SB-02 † | Define the category scoreboard and record current standing | none | coordinator | S | P0 |
| SB-03 | Build the deterministic dev-loop measurement harness | SB-02 | small | M | P0 |
| SB-04 | Add a CI performance-regression lane on dev-loop metrics | SB-03 | small | S | P0 |
| SB-05 | Add realistic corpora and holdout schemas | SB-02 | small | M | P0 |
| SB-06 | Add C++ protobuf and upb C codec peers | SB-03, SB-05 | small | L | P1 |
| SB-07 | Add Go protobuf, vtprotobuf and hyperpb codec peers | SB-06 | small | M | P1 |
| SB-08 | Refresh Rust codec peers and use generated schemas | SB-03, SB-05 | small | M | P1 |
| SB-09 | Build the codegen comparator matrix | SB-05 | small | M | P1 |
| SB-10 † | Honor official QPS scenario thread controls and run official scenarios | none | small | M | P0 |
| SB-11 | Build the cross-stack client/server matrix harness | SB-01, SB-03 | small | M | P0 |
| SB-12 | Measure connection scale, memory per connection and handshake rate | SB-11 | small | M | P1 |
| SB-13 | Attribute copies in large-payload and streaming paths | SB-03 | small | M | P0 |
| SB-14 | Publish the profiling kit and perf-PR evidence template | SB-03 | small | S | P0 |
| SB-15 | Run claim-grade codec and codegen campaigns | SB-05, SB-06, SB-07, SB-08, SB-09 | operator | L | P1 |
| SB-16 | Publish a generated scoreboard with raw artifacts | SB-02, SB-15, SB-22 | small | S | P1 |
| SB-17 | Add a pbrs-grpc entry to the public grpc_bench harness | SB-11 | small | S | P1 |
| SB-18 | Add optional peers to the cross-stack matrix | SB-11 | small | M | P1 |
| SB-19 | Add extended cells to the cross-stack matrix | SB-11 | small | M | P1 |
| SB-20 | Decide dev-loop gating thresholds from observed noise | SB-04 | coordinator | S | P1 |
| SB-21 | Create contract-compliant scenario variants for E1/E2 claims | SB-10, SB-17 | small | S | P1 |
| SB-22 | Run claim-grade client, server and end-to-end campaigns | SB-11, SB-12, SB-13, SB-19, SB-21, RT-07 | operator | L | P1 |

### PK: Protobuf kernel performance (plugin/owned path)

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| PK-01 | Rank codec bottlenecks with dev-loop evidence | SB-03, SB-05, SB-08 | coordinator | M | P0 |
| PK-02 | Decide how to separate build-time code from the runtime | GN-01 | coordinator | S | P0 |
| PK-03 | Implement the runtime/build-time split | PK-02, MX-01 | small | M | P1 |
| PK-04 | Add SWAR/SIMD varint and tag decoding | PK-01, QG-01 | small | M | P1 |
| PK-05 | Design the hybrid table-driven parser | PK-01 | coordinator | M | P0 |
| PK-06 | Implement the table-driven parse engine in the runtime | PK-05, QG-01 | small | L | P1 |
| PK-07 | Emit parse tables and auto-select parse strategy in codegen | PK-06, MX-01, GN-02 | small | L | P1 |
| PK-08 | Use tables for cold size/encode paths | PK-07 | small | M | P1 |
| PK-09 | Add Bytes-owning parse and shared large-field backing | MX-05, SB-13 | small | M | P0 |
| PK-10 | Parse segmented DATA frames without coalescing | MX-05, PK-09 | small | L | P1 |
| PK-11 | Encode shared segments and send scatter-gather | MX-05, PK-09, MX-01 | small | M | P1 |
| PK-12 | Experiment with per-request arena allocation | PK-01 | coordinator | M | P1 |
| PK-13 | Decide the borrowed-view model | PK-01, PK-09 | coordinator | S | P1 |
| PK-14 | Reuse canonical packed payloads and speed large packed encode | PK-01, MX-01 | small | S | P1 |
| PK-15 | Tune medium-string ownership thresholds | PK-01 | small | S | P1 |
| PK-16 | Stream JSON encode and decode, including every well-known type | PK-01, MX-01, SB-06, SB-07 | small | L | P1 |
| PK-17 | Adopt a hybrid map representation | PK-01 | small | S | P1 |
| PK-18 | Make dynamic-message parsing the fastest in class | PK-06, SB-07 | small | L | P1 |
| PK-19 | Speed merge, clone and size, and publish PGO guidance | PK-01, MX-01 | small | M | P1 |
| PK-20 | Generate borrowed views | PK-13, PK-07 | small | L | P1 |
| PK-21 | Stream text format without intermediate trees | PK-01, MX-01 | small | M | P1 |

### GN: Code generator and pure-Rust frontend

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| GN-01 | Attribute downstream compile-time costs | SB-02 | coordinator | S | P0 |
| GN-02 | Shrink generated code | GN-01, MX-01 | small | L | P0 |
| GN-03 | Make descriptor embedding optional and shared | GN-01, MX-01 | small | M | P1 |
| GN-04 | Make the generator itself the fastest | SB-09, MX-01 | small | M | P1 |
| GN-05 † | Decide the pure-Rust .proto frontend route | none | coordinator | S | P0 |
| GN-06 | Implement the frontend lexer and parser for proto2 and proto3 | GN-08 | small | M | P1 |
| GN-06b | Extend the frontend grammar to Editions 2023 and 2024 | GN-06 | small | M | P1 |
| GN-07 | Implement frontend import and name resolution | GN-06b | small | M | P1 |
| GN-07b | Resolve options, features and source info; expose the Rust-only entry | GN-07, MX-01 | small | M | P1 |
| GN-08 | Build the pinned frontend differential corpus and harness | GN-05 | small | M | P1 |
| GN-09 | Prove Rust-only packaged consumers | GN-07b, QG-04 | small | M | P1 |

### UK: upb replacement kernel for official rust_out gencode

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| UK-01 † | Inventory the official rust_out kernel ABI and its gaps | none | small | M | P0 |
| UK-02 | Decide the upb-in-Rust kernel architecture | UK-01 | coordinator | M | P0 |
| UK-03 | Implement a fusing bump arena | UK-02, MX-04, QG-01 | small | M | P1 |
| UK-04 | Complete MiniDescriptor decoding and linking | UK-02, MX-04, QG-01 | small | M | P1 |
| UK-05 | Adopt MiniTable-native message layout | UK-03, UK-04 | small | L | P1 |
| UK-06 | Implement the fast table-driven decoder | UK-05, SB-03 | small | L | P1 |
| UK-07 | Implement a single-pass encoder and exact size | UK-05, SB-03 | small | M | P1 |
| UK-08 | Implement extensions for the official path | UK-06 | small | M | P1 |
| UK-09 | Implement maps and repeated fields with upb semantics | UK-05 | small | M | P1 |
| UK-10 | Implement reflection interop, equality, debug and formats | UK-06, UK-07 | small | M | P1 |
| UK-11 | Run every upstream rust/test/shared suite unmodified in CI | UK-08, UK-09, UK-10 | small | M | P0 |
| UK-12 | Add a conformance testee built from official rust_out gencode | UK-10 | small | M | P0 |
| UK-13 | Beat upb on Google's own messages | UK-11, UK-12, UK-15, SB-06 | small | M | P0 |
| UK-14 | Make pbrs a [patch] drop-in for google-protobuf and legacy protobuf 4.x | UK-11, UK-12 | small | M | P0 |
| UK-15 | Differential-fuzz the kernel against upb C | UK-06, SB-06 | small | M | P1 |
| UK-16 | Prepare the upstream engagement packet | UK-11, UK-12, UK-13, UK-14, UK-15 | operator | S | P1 |
| UK-17 | Study a C ABI for non-Rust upb consumers | UK-13 | coordinator | S | P2 |
| UK-18 | Converge plugin and official paths on shared primitives | PK-07, UK-13 | coordinator | S | P2 |
| UK-19 | Track google-protobuf releases with an ABI drift gate | UK-14 | small | S | P1 |

### H2: gRPC-specialized sans-IO HTTP/2 engine

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| H2-01 | Quantify h2 costs and decide go/no-go on a custom engine | SB-11, CL-01, SV-01 | coordinator | M | P0 |
| H2-02 | Introduce an internal HTTP/2 transport seam | MX-05, MX-02, MX-03, SB-03 | small | M | P0 |
| H2-03 | Design the gRPC-specialized sans-IO HTTP/2 engine | H2-01, H2-02 | coordinator | M | P0 |
| H2-04 | Create pbrs-h2 and implement the frame codec | H2-03, QG-01 | small | L | P1 |
| H2-04b | Implement HPACK with gRPC header templates | H2-04 | small | M | P1 |
| H2-05 | Implement the sans-IO stream state machine | H2-04, H2-04b | small | L | P1 |
| H2-05b | Implement connection and stream flow control | H2-05 | small | M | P1 |
| H2-05c | Implement HTTP/2 security limits | H2-05 | small | M | P1 |
| H2-06 | Implement the single-owner Tokio driver | H2-05, H2-05b, H2-05c, H2-02 | small | L | P1 |
| H2-07 | Run h2spec and RFC conformance in CI | H2-06 | small | S | P0 |
| H2-08 | Pass official gRPC HTTP/2 negative interop on the new engine | H2-06 | small | M | P1 |
| H2-09 | Differential and structure-aware fuzzing of the engine | H2-06 | small | M | P1 |
| H2-10 | Pre-encode gRPC headers from codegen | MX-05, H2-06, MX-01, SV-04 | small | M | P1 |
| H2-11 | Implement the write scheduler and flush policy | H2-06 | small | M | P1 |
| H2-12 | Implement BDP-driven flow control | H2-06 | small | M | P1 |
| H2-13 | Shrink per-connection and per-stream memory | H2-06, SB-12 | small | M | P1 |
| H2-14 | Decide the default HTTP/2 backend | H2-15 | coordinator | S | P0 |
| H2-15 | Qualify the exact pbrs-h2 candidate | H2-07, H2-08, H2-09, H2-10, H2-11, H2-12, H2-13, QG-03 | operator | M | P0 |

### RX: Runtime, IO, TLS and compression

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| RX-01 | Introduce a minimal runtime seam | H2-02 | small | M | P1 |
| RX-02 | Offer thread-per-core server and client modes | RX-01, SB-11 | small | L | P1 |
| RX-03 | Experiment with an io_uring backend | H2-06, RX-01 | coordinator | L | P2 |
| RX-04 | Benchmark TLS providers and handshake paths | SB-11 | small | M | P1 |
| RX-05 | Experiment with kernel TLS offload | RX-04, H2-06 | coordinator | M | P2 |
| RX-06 | Upgrade compression: zlib-rs backend and zstd decision | SB-03, QG-04 | small | M | P1 |
| RX-07 | Add buffer pools and allocator guidance | MX-05, CL-01, SV-01 | small | S | P1 |

### CL: Client fast paths

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| CL-01 | Isolate the native client's limiting costs | SB-01, SB-03, MX-02 | coordinator | M | P0 |
| CL-02 | Specialize the unary call path | CL-01 | small | M | P1 |
| CL-03 | Precompute request headers and timeout encoding | MX-05, CL-01 | small | S | P1 |
| CL-04 | Pick connections without locks and respect peer stream limits | CL-01 | small | M | P1 |
| CL-05 | Remove streaming client pumps | CL-01 | small | M | P1 |
| CL-06 | Minimize cold start and client memory | CL-01, CL-03, SB-12 | small | S | P1 |

### SV: Server fast paths

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| SV-01 | Isolate the native server's limiting costs | SB-01, SB-03, MX-03 | coordinator | M | P0 |
| SV-02 | Remove redundant per-RPC server allocations | SV-01 | small | S | P1 |
| SV-03 | Dispatch without a spawn per RPC where safe | SV-01, SV-02 | small | M | P1 |
| SV-04 | Generate static method routing | SV-01, MX-01 | small | M | P1 |
| SV-05 | Use static trailers and trailers-only responses | MX-05, SV-01, SV-02 | small | S | P1 |
| SV-06 | Optimize bounded streaming batches and fairness | MX-05, SV-01 | small | M | P1 |
| SV-07 | Scale accepts and cut per-connection server memory | SV-01, SB-12 | small | M | P1 |

### TC: Better tonic: ecosystem and migration

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| TC-01 † | Decide the better-tonic product shape and names | none | coordinator | S | P0 |
| TC-02 | Abstract the native codec over message types | MX-05, TC-01, MX-02, MX-03, SB-03 | small | M | P1 |
| TC-03 | Serve prost messages over the native transport | TC-02, MX-01, SB-11 | small | M | P1 |
| TC-04 | Expose native services as tower services | TC-01, MX-03 | small | M | P1 |
| TC-05 | Support tower middleware on the client | TC-01, MX-02, SB-03 | small | M | P1 |
| TC-06 | Offer a tonic-shaped API mode and migration kit | TC-02, MX-01 | small | L | P1 |
| TC-07 | Port tonic's examples and publish migration diffs | TC-03, TC-04, TC-06 | small | M | P1 |
| TC-08 | Audit tonic feature parity and file gap cards | TC-07 | coordinator | S | P1 |
| TC-09 | Decide gRPC-Web and Connect protocol scope | TC-01 | coordinator | S | P2 |
| TC-10 | Speed up the protobuf-tonic adapter | PK-09 | small | S | P1 |

### CH: Channel architecture, resolvers and load balancing

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| CH-01 † | Design the channel architecture | none | coordinator | M | P0 |
| CH-02 | Add target URI schemes and a resolver registry | CH-01, MX-02 | small | M | P1 |
| CH-03 | Deliver service config through resolvers and select LB policies | CH-02, MX-00 | small | M | P1 |
| CH-04 | Race dual-stack connections and shuffle weighted addresses in pick_first | FL-03 | small | S | P1 |
| CH-05 | Integrate client-side health checking with load balancing | FL-04 | small | S | P1 |
| CH-06 | Implement ORCA and weighted round robin | FL-04, CH-03 | small | L | P1 |
| CH-07 | Implement ring hash, least request and subsetting | CH-03 | small | M | P1 |
| CH-08 | Implement priority and outlier detection policies | CH-03 | small | M | P1 |
| CH-09 | Add HTTP CONNECT proxy, TCP user timeout and binary-metadata interop | CH-02 | small | M | P1 |
| CH-10 | Build the picker-cost harness and budget | CH-02, SB-04 | small | S | P1 |
| CH-11 | Qualify picker cost across every shipped policy | FL-03, FL-04, CH-06, CH-07, CH-08, CH-10 | small | S | P1 |

### XD: xDS

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| XD-01 | Decide build versus reuse for xDS machinery | CH-01 | coordinator | S | P1 |
| XD-02 | Generate xDS protos and stand up a local control-plane fixture | XD-01 | small | M | P1 |
| XD-03 | Implement bootstrap, ADS and resource lifecycle | XD-02 | small | M | P1 |
| XD-03b | Harden the xDS client: federation, fallback and failure modes | XD-03 | small | M | P1 |
| XD-04 | Implement LDS/RDS routing | XD-03, CH-03 | small | L | P1 |
| XD-05 | Implement CDS/EDS and cluster policies | XD-03, CH-08 | small | M | P1 |
| XD-05b | Add xDS circuit breaking, outlier detection and endpoint fallback | XD-05, CH-08 | small | M | P1 |
| XD-05c | Add xDS custom LB configuration and session affinity | XD-05, CH-07 | small | M | P1 |
| XD-06 | Implement xDS security, RBAC and HTTP filters | XD-04 | small | M | P1 |
| XD-06b | Implement xDS HTTP filters, RBAC and fault injection | XD-04 | small | M | P1 |
| XD-06c | Implement JWT call credentials | XD-06 | small | S | P1 |
| XD-07 | Serve with xDS-configured listeners | XD-06, XD-06b, MX-03 | small | L | P1 |
| XD-08 | Report load with LRS and ORCA | XD-05, CH-06 | small | M | P1 |
| XD-09 | Expose CSDS and the admin interface | XD-03, GF-03 | small | M | P1 |
| XD-10 | Run the official xDS interop suites | XD-03b, XD-05, XD-05b, XD-05c, XD-06, XD-06b, XD-07 | small | L | P1 |
| XD-11 | Keep xDS scale and hot-path cost bounded | XD-05, CH-11 | small | M | P1 |
| XD-12 | Implement xDS HTTP CONNECT, GrpcService and child-channel options | XD-03, CH-09 | small | M | P1 |

### GF: Observability, security and remaining gRFCs

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| GF-01 | Ship optional OpenTelemetry metrics | SB-03 | small | M | P1 |
| GF-02 | Ship optional OpenTelemetry tracing | GF-01 | small | S | P1 |
| GF-03 | Implement channelz and channel tracing | CH-02, MX-03 | small | M | P1 |
| GF-04 | Implement binary logging | MX-02, MX-03 | small | M | P1 |
| GF-05 | Implement the gRPC authorization policy engine | MX-03 | small | M | P1 |
| GF-06 | Harden TLS: CRL, SPIFFE, telemetry and PQ readiness | RX-04 | small | M | P1 |
| GF-07 | Finish retry coverage: streaming policy retries and retry stats | MX-02 | small | M | P1 |
| GF-08 † | Automate gRFC drift tracking | none | small | S | P2 |
| GF-09 | Reconcile the full gRFC profile | CH-04, CH-05, CH-09, CH-11, XD-06c, XD-08, XD-09, XD-10, XD-11, XD-12, GF-01, GF-02, GF-03, GF-04, GF-06, GF-07, GF-10 | coordinator | S | P1 |
| GF-10 | Implement authorization audit logging | GF-05 | small | S | P1 |

### QG: Quality gates for new kernels and engines

| Card | Title | Depends on | Executor | Size | Priority |
|---|---|---|---|---|---|
| QG-01 † | Apply unsafe and Miri policy to new kernels and engines | none | small | S | P0 |
| QG-02 | Expand fuzzing and prepare OSS-Fuzz | QG-01 | small | M | P1 |
| QG-03 | Soak and chaos-test the new transport | H2-06 | small | M | P1 |
| QG-04 † | Audit the pure-Rust dependency graph per shipping profile | none | small | S | P0 |

## Sources

External facts above were checked on 2026-09-26. Pin exact revisions again
when a card depends on them.

- Google on a pure-Rust kernel: [protobuf#24880](https://github.com/protocolbuffers/protobuf/issues/24880) (maintainer comment, 2025-12-16) and [Rust design decisions](https://protobuf.dev/reference/rust/rust-design-decisions/).
- `google-protobuf` v36.2 release: [protobuf releases](https://github.com/protocolbuffers/protobuf/releases/tag/v36.2).
- `protoc --rust_out` kernels (`cpp` or `upb` only): [context.cc at v36.2](https://github.com/protocolbuffers/protobuf/blob/v36.2/src/google/protobuf/compiler/rust/context.cc).
- tonic 0.14.6 and Google `grpc` 0.9.0: [grpc/grpc-rust releases](https://github.com/grpc/grpc-rust/releases).
- `h2` connection mutex: [hyperium/h2#531](https://github.com/hyperium/h2/issues/531), [hyperium/h2#917](https://github.com/hyperium/h2/pull/917).
- grpc_bench results: [discussion #559](https://github.com/LesnyRumcajs/grpc_bench/discussions/559) (2026-04-22, commit `c4c93b3`).
- Official gRPC benchmarking: [grpc.io guide](https://grpc.io/docs/guides/benchmarking/).
- gRFC catalogue: [grpc/proposal at 6342be7](https://github.com/grpc/proposal/tree/6342be729b96478a2897ceb208a8cddcd832a17b).
- Parser techniques: [upb fasttable select](https://github.com/protocolbuffers/protobuf/blob/b4462c8385061587111af8d1da425c3b683724d4/upb/wire/decode_fast/select.c), [C++ TcParser](https://github.com/protocolbuffers/protobuf/blob/b4462c8385061587111af8d1da425c3b683724d4/src/google/protobuf/generated_message_tctable_impl.h), [hyperpb design](https://github.com/bufbuild/hyperpb-go/blob/main/DESIGN.md), [buffa](https://github.com/anthropics/buffa).
- Transport techniques: [grpc-go loopy writer](https://github.com/grpc/grpc-go/blob/master/internal/transport/controlbuf.go), [h2spec](https://github.com/summerwind/h2spec).
- xDS in Rust: [xds-client](https://docs.rs/crate/xds-client/0.1.0-alpha.4), [tonic-xds](https://docs.rs/crate/tonic-xds/0.1.0-alpha.4), [psm-interop](https://github.com/grpc/psm-interop).
- TLS performance: [rustls report 2026-03-07](https://rustls.dev/perf/2026-03-07-report/).
