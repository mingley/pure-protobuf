# Balanced authority routing: storage costs

Selected-endpoint authority routing has a small measured storage cost in this **native dev/test** configuration. The typed union reduces that cost relative to the first implementation, but ordinary direct calls still request extra bytes versus Git10d baseline. This is not zero overhead, a CPU benchmark, release evidence or an RPC performance claim.

The baseline is production source at `10d94697dc89e3aa89c843720f7150f1186f417a`. **V1** retains the selected balanced Route in a separate optional handle; **union** represents mutually exclusive direct/resolver/balanced identity in one owned enum. Both candidates include the separately tested authority behavior and documentation changes. The report does not isolate all costs to one field or instruction.

## Actual private layout

Native x86_64, Rust 1.99.1 / LLVM 23.1.1, all pbrs-grpc features; opt-level 0, debug 0, incremental off. Sizes and alignments are bytes, measured on the actual private types through an identical temporary unit module.

| Type | Baseline size / align | V1 size / align | Union size / align |
| --- | ---: | ---: | ---: |
| `LiveConn` | 160 / 8 | 168 / 8 | 160 / 8 |
| `Option<LiveConn>` | 160 / 8 | 168 / 8 | 160 / 8 |
| `ChannelLocal` | 184 / 8 | 184 / 8 | 184 / 8 |
| `Channel` | 16 / 8 | 16 / 8 | 16 / 8 |

No field offsets, ConnectionRoute's own size or native 32-bit layout were observed. Equal outer size does not establish equal async-future allocation or execution cost.

## Every constructor case

Triples are **baseline → V1 → union**. Requested bytes sum successful allocation layouts, including each realloc's full requested size; they are not allocator usable size, live/peak memory or RSS. Counts are **cold/warm allocation calls** for each variant. Each fresh process keeps one cold sample, four uncounted warmups and all eight warm samples. All eight warm samples matched exactly within every case and variant.

| Case | Cold requested bytes | Warm requested bytes | Calls cold/warm | Union minus baseline bytes | Union minus V1 bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| `direct.unary` | 6048 → 6072 → 6064 | 5960 → 5984 → 5976 | 2/1 → 2/1 → 2/1 | +16 | -8 |
| `direct.server_streaming` | 4344 → 4368 → 4360 | 4256 → 4280 → 4272 | 2/1 → 2/1 → 2/1 | +16 | -8 |
| `direct.client_streaming` | 4864 → 4864 → 4864 | 4776 → 4776 → 4776 | 4/3 → 4/3 → 4/3 | +0 | +0 |
| `direct.bidi` | 4744 → 4784 → 4776 | 4656 → 4696 → 4688 | 4/3 → 4/3 → 4/3 | +32 | -8 |
| `balanced.unary` | 6048 → 6072 → 6064 | 5960 → 5984 → 5976 | 2/1 → 2/1 → 2/1 | +16 | -8 |
| `balanced.server_streaming` | 4344 → 4368 → 4360 | 4256 → 4280 → 4272 | 2/1 → 2/1 → 2/1 | +16 | -8 |
| `balanced.client_streaming` | 4864 → 4864 → 4864 | 4776 → 4776 → 4776 | 4/3 → 4/3 → 4/3 | +0 | +0 |
| `balanced.bidi` | 4744 → 4784 → 4776 | 4656 → 4696 → 4688 | 4/3 → 4/3 → 4/3 | +32 | -8 |

Thus the union still requests **+16 bytes for unary/server streaming and +32 for bidi on direct calls as well as balanced calls**. Client streaming is unchanged. V1's original +24/+24/+0/+40-byte costs remain visible. The union saves eight bytes versus V1 in unary/server streaming/bidi and none in client streaming. Allocation call counts do not improve.

Every drop window allocated zero bytes. Explicit deallocation counts were 1 for unary/server streaming and 3 for client streaming/bidi; freed bytes equaled the warm requested bytes in the corresponding row. No realloc or constructor deallocation occurred. Complete event sizes/alignments and both construction/drop windows are retained. The unchanged cold difference is an 88-byte cancellation flag retained in a thread-local spare for reuse; that retained state is not evidence of a leak. Explicit deallocation totals exclude old storage retired by realloc, though no such event occurred here.

## What was checked

The baseline and V1 were built sequentially at one private canonical path. Their frozen data and four executable hash declarations were reused for the union comparison; they were not rerun. Only one new union build, one private layout process and eight fresh constructor-case processes followed. Altogether this retains **216 constructor records, 432 separate construction/drop windows and three layouts**, without failed/resource-stopped processes or reruns. All 24 case processes retained cold plus eight warm records. No faster or smaller row was selected from a larger campaign.

The same compiler, canonical source path, original manifests/locks, feature graph, profiles, relevant full rustc arguments, package IDs, generated message types and audit fixtures were checked. Actual generated Hello source matches V1 and union; baseline getter documentation differs and is retained. Source inventories before and after each build matched. The archive includes changed baseline/V1/union source bytes, 205 reviewed source/config hash pins, 499-input inventories, original plans/drivers/fixtures, complete text captures, generated Hello sources and metadata. The complete runtime tree and all executables are omitted. Baseline/common source bytes were materialized later from the named immutable Git objects and checked against the original receipts; they are not described as contemporaneous captures.

Request and channel setup, uncounted constructor warmups and JSON output occur outside the current-thread synchronous windows. No Call was polled and no await/yield, explicit clock or connection dial occurred in those windows. The real native public constructors use the same generated empty HelloRequest/HelloReply types. Static balanced setup does not measure route acquisition. Selected Route lifetime, URI/address/executor retention, reference-count execution, endpoint snapshot Authority allocation/cloning, retries, health watches, serialization and HTTP/2 execution remain unmeasured. Default features, release builds, other payloads and CPU/RPC throughput or latency are also unmeasured.

## Verify the retained evidence

```sh
python3 -B docs/evidence/balanced-authority-storage-20261106/check.py
```

The Python-standard-library checker needs only these four packet files and works from any directory. It uses no Git command, private worktree, compiler or executable. It verifies archive and receipt bindings, historical executable SHA declarations, source pins/common fixtures, metadata/profile/flag/package equality, all sample grids, event formulas and three-way deltas. Historical executable hashes are **receipt bindings only**: the portable check does not rehash or replay the omitted executables, regenerate source, independently rebuild the entire pinned runtime tree or qualify performance. Absolute paths in old records are provenance, not local dependencies. Original private scripts are preserved as text and are not executed by this checker.

Six in-memory corruption controls reseal affected receipt/hash dependencies and must fail at their named semantic assertion: a wrong event total, missing warm sample, clock in a counting row, wrong historical executable binding, hidden remaining cost and omitted case. Normal verification has no bypass. These checks establish that the accounting logic rejects those synthetic errors; they do not substitute for replay or production qualification.
