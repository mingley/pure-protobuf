# Benchmarks

[Run benchmarks](benchmarks/running.md) · [Earlier results](benchmarks/grpc-20261024.md)
· [Qualification](scoreboard.md)

Choose a result by the operation you need to improve. Each record names its
source, workload, controls and remaining losses. These local diagnostics do not
establish wins on every benchmark, complete feature parity or an overall
ranking.

Historical results retain their dated source and raw captures where available.
Read the [benchmark contract](benchmark-contract.md) before making a broader
performance claim.

## Transport, Web and cancellation

| Read | Scope |
|---|---|
| [TLS portability on x86-64](benchmarks/tls-portability-x86-20261110.md) | Configuration and warmed RPC controls, November 10 |
| [Drop-only cancellation screen](benchmarks/drop-only-cancel-screen-20261110.md) | Unary unpolled-call and endpoint CPU, controls and allocation limits, November 10 |
| [Drop-only construction repeat](benchmarks/drop-only-construction-repeat-20261110.md) | Matched unpolled lifecycle and held-call CPU with controls, November 10 |
| [Receive accounting candidates](benchmarks/recv-budget-rpc-screen-20261110.md) | Unmerged candidates: loopback CPU and layout with a non-default frame budget, November 10 |
| [Small-response budget screens](benchmarks/direct-response-budget-20261110.md) | Shipped reserve and budget: client, native server and cancellation CPU, November 10 |
| [Small-response reserve, native server](benchmarks/response-reserve-native-server.md) | Unmerged repair: native-server CPU and allocation samples, November 10 |
| [Small-response reserve, longer follow-up](benchmarks/response-reserve-screen-long.md) | Historical prototype: longer CPU samples and Miri failure, November 10 |
| [Small-response reserve, revised](benchmarks/response-reserve-screen-revised.md) | Historical prototype: loopback CPU and a later Miri failure, November 10 |
| [Small-response reserve screen](benchmarks/response-reserve-screen.md) | Unmerged prototype: loopback CPU, allocations and functional limits, November 10 |
| [Synthetic Call polling](benchmarks/call-poll-component-screen-20261110.md) | Unmerged prototype: component CPU and controls, November 10 |
| [Streaming Call poll-state screen](benchmarks/streaming-call-poll-screen-20261110.md) | Unmerged prototype; server/bidi `Call` futures end after response headers, November 10 |
| [Streaming sender-failure logging](benchmarks/streaming-sender-failure-20261110.md) | Source-scoped CPU controls and allocation tradeoffs, November 10 |
| [Split streaming observer experiment](benchmarks/streaming-observer-split-20261110.md) | Rejected split futures: fewer requested bytes, slower bidi CPU and a larger fixture, November 10 |
| [Compact streaming sender limits](benchmarks/compact-sender-limits-20261110.md) | Held representation: smaller sender, no measured CPU or allocation gain, November 10 |
| [Unpolled-call cancellation screen](benchmarks/unary-unpolled-drop-screen-20261110.md) | Unmerged prototype: unary lifecycle and endpoint CPU, November 10 |
| [Identity frame staging screen](benchmarks/identity-frame-staging-20261110.md) | Rejected chunk staging, whole-frame timing losses and synthetic backing retention, November 10 |
| [Unary cancellation on 713](benchmarks/unary-cancel-713-screen-20261110.md) | Matched endpoint and unpolled-call CPU, allocations and controls, November 10 |
| [Unary filter state and allocations](benchmarks/unary-filter-layout-20261110.md) | Future and cancellation-flag sizes, warmed and retained-call allocations, November 10 |
| [No-capture unary filter experiment](benchmarks/unary-filter-no-capture-screen-20261110.md) | Unselected early-filter draft: construction, endpoints and controls, November 10 |
| [Rebased unary cancellation screens](benchmarks/rebased-unary-cancel-screen-20261110.md) | Unrefined 1e-based no-logger and excluded-method endpoint results, November 10 |
| [Unary filter refinement screen](benchmarks/unary-filter-refinement-screen-20261110.md) | Unmerged early-filter draft, mixed CPU and higher requested bytes, November 10 |
| [Integrated unary cancellation screen](benchmarks/integrated-unary-cancel-screen-20261109.md) | Unmerged no-logger endpoint draft; mixed CPU results across shared-host screens, November 9 |
| [Unlogged cancellation screen](benchmarks/unlogged-cancel-screen-20261109.md) | Standalone primitive: two unmerged drafts and signal/lifecycle CPU losses, November 9 |
| [gRPC-Web text encoder repeat](benchmarks/web-text-encoder-repeat-20261109.md) | Unmerged variants, allocation gains and aligned CPU losses, November 9 |
| [Buffered gRPC-Web text encoder holdout](benchmarks/web-text-encoder-buffered-20261109.md) | Unmerged candidate gains and short-fragment losses, November 9 |
| [gRPC-Web text encoder screen](benchmarks/web-text-encoder-20261109.md) | Unmerged candidates and CPU controls, November 9 |
| [Status trailer allocations](benchmarks/status-trailers-20261109.md) | Standalone trailer construction, November 9 |
| [Stream-batch capacity diagnostic](benchmarks/stream-batch-capacity-20261108.md) | Historical 16 KiB candidate with paired allocation tradeoffs, November 8 |
| [Balanced authority storage](evidence/balanced-authority-storage-20261106/README.md) | Dev/test layout and constructor allocations for the routing fix; remaining costs included, November 6 |
| [Endpoint origin cost](benchmarks-history.md#endpoint-origin-cost-november-5) | Matched in-memory unary CPU, allocation and layout observations, November 5 |
| [Send-buffer RPC diagnostic](benchmarks-history.md#send-buffer-rpc-diagnostics-november-3) | Selected endpoint CPU comparisons, November 3 |
| [Latest buffer loop](benchmarks-history.md#buffer-chunk-ownership-november-2) | Isolated ownership operations, November 2 |
| [RPC endpoint results](benchmarks-history.md#correctness-and-rejected-candidates-october-28) | Complete earlier transport comparison, October 28 |

## Protobuf and code generation

| Read | Scope |
|---|---|
| [Codegen target-file cache](benchmarks/codegen-target-cache-20261110.md) | Held match-reuse experiment: grouped and interleaved holdouts, November 10 |
| [Compact descriptor literals](benchmarks/compact-descriptor-literals-20261110.md) | Held reflective-output experiment with lean CPU concerns, November 10 |
| [Lean codegen instruction profile](benchmarks/codegen-instruction-profile-20261110.md) | Callgrind caller edges and native controls for 5fe, November 10 |
| [Aligned codegen settings](benchmarks/codegen-aligned-20261110.md) | Descriptor-matched generation CPU, output partition and controls, November 10 |
| [Lazy-message cell seeding](benchmarks/lazy-cell-seeding-20261110.md) | Held Query CPU experiment across Rust 1.85 and 1.99, November 10 |
| [Codegen filename memo](benchmarks/codegen-file-memo-20261110.md) | Held codegen allocation experiment with CPU holdouts, November 10 |
| [Codegen reservation experiment](benchmarks/codegen-reservation-20261110.md) | Rejected per-target estimates: output hashes and CPU holdouts, November 10 |
| [Repeated storage screens](benchmarks/repeated-singleton-20261110.md) | Unmerged singleton and Query-only diagnostics: allocation size tradeoffs and shifting CPU controls, November 10 |
| [Consuming shared byte buffers](benchmarks/bytes-ownership-consume-20261110.md) | Conversion allocations and retained capacity, November 10 |
| [Inline packed-cache experiment](benchmarks/inline-packed-cache-20261110.md) | Rejected candidates: fresh allocation savings and warm CPU losses, November 10 |
| [Query field placement](benchmarks/query-field-placement-20261109.md) | Unmerged Query layout, allocation gains, larger objects and mixed CPU, November 9 |
| [Query and mixed-byte allocations](evidence/query-mixed-allocations-20261109.md) | Component allocation counts, November 9 |
| [Entity allocation sites](evidence/entity-allocation-sites-20261109.md) | Instrumented construction, assignment and packed-cache scopes, November 9 |
| [Packed setter experiment](benchmarks/packed-setter-20261109.md) | Rejected setter candidate and measured losses, November 9 |
| [Rejected medium-string validator](evidence/rejected-medium-core-utf8-20261106/README.md) | An 80-byte name gain with Unicode and boundary losses; all 756 CPU processes, November 6 |
| [Rejected packed readiness routing](evidence/rejected-packed-readiness-routing-20261106/README.md) | Fresh gains with a remaining large encode-then-getter loss; all 432 processes, November 6 |
| [Rejected cache-first packed encoding](evidence/rejected-packed-cache-first-20261106/README.md) | A local fresh TAT win against pinned Prost with large getter and owned regressions, November 6 |
| [Rejected fused packed encoding](evidence/rejected-packed-fused-20261106/README.md) | Fresh and noncanonical gains with cached and getter regressions; all 432 processes, November 6 |
| [Rejected packed-wire cache](evidence/rejected-packed-wire-cache-20261106/README.md) | Fresh-encode gains with cached and noncanonical regressions; all 432 processes, November 6 |
| [Fresh-encode warmup repair](evidence/fresh-encode-warmup-20261106/README.md) | Separate warmup objects and retain the previously hidden cold-allocation cost, November 6 |
| [Map removal](benchmarks-history.md#map-removal-november-6) | Correctness repair, indexed update gains and small-map costs, November 6 |
| [Rejected borrowed queries](evidence/rejected-query-capability-20261106/README.md) | Lookup allocation gains with a repeatable small-map loss, November 6 |
| [Numbered name allocation](benchmarks-history.md#numbered-name-allocation-november-5) | Repeated-prefix generation and ordinary controls, November 5 |
| [Safe generated defaults](benchmarks-history.md#safe-generated-defaults-november-5) | Opt-in construction, encoding, parse and build costs, November 5 |
| [Earlier generation cost](benchmarks-history.md#generation-cost-november-5) | Matched generator wall time after naming repairs, November 5 |
| [Map equality](benchmarks-history.md#map-equality-november-5) | Warm and cold map comparison CPU and allocations, November 5 |
| [Owned byte buffers](evidence/byte-ownership-20261105/README.md) | Conversion allocations and capacity retention, November 5 |
| [Recursive validation](benchmarks-history.md#recursive-validation-november-4) | Selected protobuf parse and read CPU, November 3–4 |
| [Borrowed-map lookup experiment](evidence/rejected-map-lookups-20261103/README.md) | Rejected lookup candidates, November 3 |
| [Rejected candidates](evidence/rejected-runtime-candidates-20261030/README.md) | Unselected repeated-field and map experiments |
| [Earlier codec and codegen](benchmarks-history.md#codec-and-codegen-comparisons) | Earlier comparisons and their recorded sources |

## Comparisons and methods

| Read | Scope |
|---|---|
| [Dev-loop: c8dd to 0685](benchmarks/devloop-0685-20261110.md) | Advisory proxy rows and tonic/reference allocation controls, November 10 |
| [Dev-loop: 452 to 41d](benchmarks/devloop-452-to-41d-20261110.md) | 1,634 common cells, advisory proxy results and selected allocations, November 10 |
| [Dev-loop: b245 to 09d8](benchmarks/devloop-b245-to-09d8-20261110.md) | 1,634 common cells, advisory budget overruns and selected allocations, November 10 |
| [Dev-loop: 656 to 713](benchmarks/devloop-656-to-713-20261110.md) | 1,634 common cells; advisory exit, proxy limits and selected allocations, November 10 |
| [Dev-loop allocation comparison](benchmarks/devloop-001d-to-1e-20261110.md) | 1,634 common cells; advisory comparison failed and selected allocation results, November 10 |
| [1,634-cell hosted run](https://github.com/mingley/pure-protobuf/actions/runs/42685722680) | `4a2d8e26`; advisory comparison exited 1 and instruction counts were unavailable, November 9 |
| [November 6 hosted comparison](evidence/hosted-devloop-10d94697-20261106/README.md) | All 1,634 cells at `10d94697`, with selected wins, remaining Prost losses and instrumentation limits |
| [Earlier full hosted comparison](evidence/hosted-devloop-20261106/README.md) | All 1,634 cells and repaired fresh lifecycle at `05a3cca5`, November 6 |
| [Codec and generator campaign](evidence/runtime-and-codegen-20261029/README.md) | Combined measurements and remaining losses, October 29 |
| [Benchmark input checks](benchmarks-history.md#benchmark-input-checks-november-2) | Canonical input identity and qualification |

## Older direct links

These links preserve previous section URLs. The [historical results](benchmarks-history.md)
keep the complete tables and their original context.

## Map removal, November 6

[Read this historical result](benchmarks-history.md#map-removal-november-6).

## Numbered name allocation, November 5

[Read this historical result](benchmarks-history.md#numbered-name-allocation-november-5).

## Safe generated defaults, November 5

[Read this historical result](benchmarks-history.md#safe-generated-defaults-november-5).

## Endpoint origin cost, November 5

[Read this historical result](benchmarks-history.md#endpoint-origin-cost-november-5).

## Map equality, November 5

[Read this historical result](benchmarks-history.md#map-equality-november-5).

## Generation cost, November 5

[Read this historical result](benchmarks-history.md#generation-cost-november-5).

## Recursive validation, November 4

[Read this historical result](benchmarks-history.md#recursive-validation-november-4).

## Send-buffer RPC diagnostics, November 3

[Read this historical result](benchmarks-history.md#send-buffer-rpc-diagnostics-november-3).

## Buffer chunk ownership, November 2

[Read this historical result](benchmarks-history.md#buffer-chunk-ownership-november-2).

## Benchmark input checks, November 2

[Read this historical result](benchmarks-history.md#benchmark-input-checks-november-2).

## Rejected runtime candidates, October 30

[Read this historical result](benchmarks-history.md#rejected-runtime-candidates-october-30).

## Correctness and rejected candidates, October 28

[Read this historical result](benchmarks-history.md#correctness-and-rejected-candidates-october-28).

## Latest runtime and generator results, October 27

[Read this historical result](benchmarks-history.md#latest-runtime-and-generator-results-october-27).

## Typed map iteration, October 27

[Read this historical result](benchmarks-history.md#typed-map-iteration-october-27).

## Default transport costs, October 27

[Read this historical result](benchmarks-history.md#default-transport-costs-october-27).

## Server scheduling and nested messages, October 27

[Read this historical result](benchmarks-history.md#server-scheduling-and-nested-messages-october-27).

## Integrated changes, October 27

[Read this historical result](benchmarks-history.md#integrated-changes-october-27).

## Compressed streams, October 26

[Read this historical result](benchmarks-history.md#compressed-streams-october-26).

### Allocation bytes

[Read this historical result](benchmarks-history.md#allocation-bytes).

### CPU

[Read this historical result](benchmarks-history.md#cpu).

## Codec and codegen comparisons

[Read this historical result](benchmarks-history.md#codec-and-codegen-comparisons).

