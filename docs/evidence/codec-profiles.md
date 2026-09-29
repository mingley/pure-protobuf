# Codec profiles (PK-01 / PK-04)

Date: 2026-09-28. Host: local macOS Apple Silicon worktree, shared/contended.
Label every timing below as **dev-loop, contended host**. This is not
claim-grade evidence.

## Scope and pins

- Base/head source: `7aec79e22914cce4cd280f886ced4dae892d169f` plus the local
  PK-04 patch.
- Dev-loop harness: `scripts/devloop.sh`, schema `devloop/1`.
- Google Rust/upb peer requires pinned `libprotoc 35.1` with runtime
  `protobuf = 4.35.1-release`. A default host `protoc 36.2` generated
  incompatible `0.36.2-release` gencode for `v4_tat`; the measurements below
  used `PATH=$PWD/target/pinned-protoc-build:$PATH`.
- Rust peers observed in current manifests:
  - `tonic-bench`: prost `0.14`, protobuf `4.35.1-release`, checked v4 gencode.
  - `bench`: prost `0.13`, buffa `0.9.1`, protobuf `4.35.1-release`.
    Updating `prost_tat`, `v4_tat` or `buffa_*` manifests is outside this
    worker's write scope, so SB-08 is only partially addressed here.

## Baseline artifacts

Commands:

```sh
PATH="$PWD/target/pinned-protoc-build:$PATH" \
  CARGO_BUILD_JOBS=3 ./scripts/devloop.sh \
  --cells codec.pbrs.fresh_encode,codec.pbrs.cached_encode,codec.pbrs.owned_decode,codec.pbrs.parse_touch,codec.prost.fresh_encode,codec.prost.cached_encode,codec.prost.owned_decode,codec.prost.parse_touch,codec.v4.fresh_encode,codec.v4.cached_encode,codec.v4.owned_decode,codec.v4.parse_touch \
  --iters 3000 --repeats 3 --out target/devloop/codec-base.json

PATH="$PWD/target/pinned-protoc-build:$PATH" \
  CARGO_BUILD_JOBS=3 cargo run --manifest-path bench/Cargo.toml --release --offline \
  > target/devloop/bench-base.json

PROTOC="$PWD/target/pinned-protoc-build/protoc" \
  CARGO_BUILD_JOBS=3 cargo run --manifest-path tonic-bench/Cargo.toml --release --offline \
  > target/devloop/tonic-bench-base.md

./scripts/profile.sh --cell codec.pbrs.owned_decode --iters 200000 --skip-build
```

Base profile: `target/profile/codec.pbrs.owned_decode-20260928T233601Z/`.
Top sampled symbols included `_platform_memmove` (21.9%),
`TestAllTypesProto3::merge_inner` (4.9%), `pbrs::wire::decode_varint` (4.2%),
and `Packed<VarintI32>::append_wire` (2.1%). The profile also sampled setup and
peer symbols, so use it for ranking only, not precise attribution.

Base dev-loop pbrs cells:

| cell | wall ns/op | allocs/op | bytes/op |
|---|---:|---:|---:|
| `codec.pbrs.fresh_encode` | 340.583 | 6.8 | 195.267 |
| `codec.pbrs.cached_encode` | 89.444 | 1.0 | 87.0 |
| `codec.pbrs.owned_decode` | 394.597 | 7.0 | 1072.0 |
| `codec.pbrs.parse_touch` | 583.431 | 12.0 | 1216.0 |

Current tonic-bench reproduction of the documented small common-shape rows:

| row | pbrs fresh/cached enc | pbrs parse/touch | prost enc/parse/touch | verdict |
|---|---:|---:|---:|---|
| `empty` | 1.6 / 1.3 | 0.3 / 0.3 | 0.3 / 0.3 / 0.3 | prost still wins; ZST comparator, not worth chasing |
| `id` | 4.8 / 3.6 | 2.4 / 3.8 | 2.8 / 3.6 / 3.6 | current combined row wins despite historical loss |
| `name_80` | 7.5 / 7.6 | 23.5 / 23.7 | 4.9 / 23.5 / 24.1 | decode parity; remaining loss is encode overhead |

## Attempted PK-04 fast path

Hypothesis: packed-varint validation is on the pbrs owned-decode path through
`Packed<VarintI32>::append_wire`; skipping runs of one-byte varints eight bytes
at a time with a portable high-bit mask should help varint-heavy decodes while
preserving truncation/overflow/overlong semantics.

Initial change in `6b32ba7d`:

- `src/wire.rs`: `validate_varints` now skips 8-byte chunks when no byte has
  the continuation bit set. This is safe, portable scalar SWAR and uses no
  `unsafe`.
- `fuzz/fuzz_targets/varint_diff.rs`: new differential fuzz target comparing
  `decode_varint`, `decode_tag`, and packed-varint validation against a scalar
  reference on the same input.

Follow-up Linux/callgrind confirmation on 2026-09-29 disproved the fast-path
hypothesis on the target cells. The final code restores the scalar
`validate_varints` loop and keeps the differential fuzz target plus targeted
dev-loop cells.

Correctness checks run:

```sh
cargo fmt --check
CARGO_BUILD_JOBS=3 cargo test -p pbrs wire::tests::validate_varints_matches_scalar_reference --lib --offline
CARGO_BUILD_JOBS=3 cargo check --manifest-path fuzz/Cargo.toml --bin varint_diff --offline
```

## macOS after evidence

Primary deterministic counts did not change on the broad dev-loop TAT cells:
allocations and allocated bytes are identical. macOS has no `perf`/`strace`
instruction/syscall counters here, so wall time remains secondary evidence.

Best after repeat (`target/devloop/codec-head-repeat.json`, 5 repeats):

| cell | base wall | after wall | delta | allocs | bytes | wall CV |
|---|---:|---:|---:|---:|---:|---:|
| `codec.pbrs.cached_encode` | 89.444 | 87.958 | -1.66% | 1.0 -> 1.0 | 87.0 -> 87.0 | 0.018 |
| `codec.pbrs.fresh_encode` | 340.583 | 363.375 | +6.69% | 6.8 -> 6.8 | 195.267 -> 195.267 | 0.223 |
| `codec.pbrs.owned_decode` | 394.597 | 384.306 | -2.61% | 7.0 -> 7.0 | 1072.0 -> 1072.0 | 0.452 |
| `codec.pbrs.parse_touch` | 583.431 | 578.097 | -0.91% | 12.0 -> 12.0 | 1216.0 -> 1216.0 | 0.057 |

After profile: `target/profile/codec.pbrs.owned_decode-20260928T235942Z/`.
The auto-sized profile captured usable samples; top pbrs-side symbols included
`merge_inner` (2.1%), `decode_varint` (1.2%), map entry decode/push (about
1.0% total), `Packed<VarintI32>::append_wire` (0.6%), `Wire::window` (0.5%),
and `Wire::ensure` (0.3%). Allocator/drop/memmove costs dominate this broad
cell more than varint validation.

This was insufficient to qualify PK-04 because the target cells had no
instruction counts yet.

## Linux/callgrind confirmation

Tooling:

```sh
./scripts/devloop-linux.sh --cells codec.pbrs.packed_256_owned_decode,codec.pbrs.packed_256_parse_touch,codec.pbrs.unpacked_256_owned_decode,codec.pbrs.unpacked_256_parse_touch,codec.pbrs.tags_32_owned_decode,codec.pbrs.tags_32_parse_touch,codec.pbrs.owned_decode,codec.pbrs.cached_encode --iters 500 --repeats 3 --out target/devloop/pk04-linux-swar.json
```

The wrapper runs in Docker Desktop's arm64 Linux VM with valgrind/callgrind.
Artifacts:

- scalar baseline: `target/devloop/pk04-linux-scalar.json`
- original SWAR fast path: `target/devloop/pk04-linux-swar.json`
- direct-array SWAR variant: `target/devloop/pk04-linux-swar-array.json`

Instruction counts are `retired/callgrind-ir per op`, medians of 3 repeats at
500 iterations.

| cell | scalar | original SWAR | delta | verdict |
|---|---:|---:|---:|---|
| `codec.pbrs.packed_256_owned_decode` | 44270.154 | 46289.530 | +4.56% | regression |
| `codec.pbrs.packed_256_parse_touch` | 56332.610 | 58351.986 | +3.59% | regression |
| `codec.pbrs.unpacked_256_owned_decode` | 44264.960 | 46284.336 | +4.56% | regression |
| `codec.pbrs.unpacked_256_parse_touch` | 56331.006 | 58350.382 | +3.59% | regression |
| `codec.pbrs.tags_32_owned_decode` | 63390.220 | 63385.958 | -0.01% | noise |
| `codec.pbrs.tags_32_parse_touch` | 63987.836 | 63983.574 | -0.01% | noise |
| `codec.pbrs.owned_decode` | 33061.184 | 32955.616 | -0.32% | below bar |
| `codec.pbrs.cached_encode` | 27785.454 | 27735.124 | -0.18% | below bar |

The direct-array SWAR variant was worse on the packed cells
(`packed_256_owned_decode` +31.92%, `packed_256_parse_touch` +25.08%), so it
was not kept.

**Verdict:** PK-04 does **not** meet the dev-loop win rule. It does not improve
the target cells by >=2%, and it regresses packed/unpacked target cells by more
than the 1% codec limit. The final code restores scalar validation; this card
should be re-scoped before another SWAR/SIMD attempt.

## Disproved or reverted attempts

- Empty-message generated `Serialize::encode` early return: reverted. It made
  broad fresh/cached encode slower in the dev-loop comparison.
- `decode_varint` one/two-byte fast-path cleanup: reverted. Linux/callgrind
  showed small directionally positive deltas only (-0.00% to -0.27% on target
  cells), below the >=2% rule and with no allocation change.
- Fixed-width packed byte view from decoded vectors: reverted. It required a
  new unsafe block and did not produce a stable large packed-fixed encode win
  in the current noisy bench run.
- Portable packed-varint SWAR validation: reverted after Linux/callgrind showed
  +3.6-4.6% instruction regressions on the packed/unpacked target cells.
- The historical `packed_fixed_5mb` encode loss versus upb did not reproduce
  on this host with pinned `libprotoc 35.1`: base pbrs/v4 encode was
  73.7/142.2 us and final was 69.7/146.6 us. The row remains local smoke
  evidence only because `bench` exited red on unrelated pre-existing gates
  (`map_64` versus buffa view, `unpacked_256` encode versus prost).

## Bottleneck ranking and follow-ups

1. **Owned decode allocations/copies dominate broad TAT decode.** Profile and
   exact counts show 7 allocs/op, 1072 bytes/op, and one 87-byte wire copy/op.
   A future PK card should reduce `Wire::ensure`/cold-box/map allocations
   before further varint micro-optimizations.
2. **`name_80` is encode-overhead, not decode.** Current parse time ties prost;
   pbrs loses on tiny cached encode overhead. Chasing `empty` remains
   low-value because prost's comparator is effectively a zero-sized no-op.
3. **Large packed-fixed encode needs a dedicated dev-loop cell.** The existing
   deterministic harness lacks the 5 MiB packed-fixed row. The historical upb
   encode loss did not reproduce in `bench`, but that result cannot be promoted
   without a deterministic dev-loop row and a green comparator gate.
4. **SB-08 remains partial.** `tonic-bench` already uses generated pbrs Person
   diagnostics and prost 0.14; `bench` still carries older peer helper crates
   and a handwritten `person` gate. Updating those helper crates requires
   write-scope expansion or a follow-up SB card.

## Small-message follow-up

Added dev-loop cells for generated `codec_cases.proto` small shapes:

- `codec.{pbrs,prost}.small_empty_{encode,decode}`
- `codec.{pbrs,prost}.small_id_{encode,decode}`
- `codec.{pbrs,prost}.small_name80_{encode,decode}`

Baseline artifact: `target/devloop/small-linux-baseline.json`.

The original Linux/callgrind baseline counted the whole child process and then
divided by iterations. Those numbers were useful for broad cells, but not for
small cells: every small row had about 25.9k Ir/op of mostly fixed
startup/setup/harness cost. Keep those rows only as **overhead-inclusive
legacy** evidence:

| cell | overhead-inclusive instr | allocs | bytes |
|---|---:|---:|---:|
| `codec.pbrs.small_empty_decode` | 25917.012 | 0.0 | 0.0 |
| `codec.pbrs.small_id_encode` | 25953.644 | 0.0 | 0.0 |
| `codec.pbrs.small_id_decode` | 25951.550 | 0.0 | 0.0 |
| `codec.pbrs.small_name80_encode` | 26092.854 | 0.0 | 0.0 |
| `codec.pbrs.small_name80_decode` | 26502.994 | 1.0 | 80.0 |
| `codec.prost.small_name80_encode` | 26051.588 | 0.0 | 0.0 |
| `codec.prost.small_name80_decode` | 26496.598 | 1.0 | 80.0 |

Task 1 fixed the harness to report loop-only instruction counts. The parent
runs N and 2N measured-loop iterations under `perf`/callgrind with identical
preparation and reports `(Ir(2N) - Ir(N)) / N`. The JSON field
`instruction_method` records the method; the schema stays `devloop/1` so old
readers remain compatible. Corrected artifact:
`target/devloop/loop-instr-after.json` (`instruction_method:
differential_callgrind_2n_minus_n`, 500 iters, 3 repeats, Docker arm64 Linux).

| cell | corrected instr | allocs | bytes | verdict |
|---|---:|---:|---:|---|
| `codec.pbrs.small_empty_decode` | 92.712 | 0.0 | 0.0 | fixed overhead removed |
| `codec.pbrs.small_id_encode` | 122.996 | 0.0 | 0.0 | realistic tiny encode cost |
| `codec.pbrs.small_id_decode` | 121.938 | 0.0 | 0.0 | realistic tiny decode cost |
| `codec.pbrs.small_name80_encode` | 237.858 | 0.0 | 0.0 | pbrs trails prost |
| `codec.pbrs.small_name80_decode` | 584.414 | 1.0 | 80.0 | pbrs roughly ties prost |
| `codec.prost.small_name80_encode` | 203.940 | 0.0 | 0.0 | comparator |
| `codec.prost.small_name80_decode` | 579.116 | 1.0 | 80.0 | comparator |

With corrected counts, the `name_80` decode gap is not an instruction or
allocation-size gap. pbrs and prost both allocate once and copy 80 bytes/op.
The remaining small `name_80` loss is encode overhead: pbrs is 237.858 Ir/op
versus prost at 203.940 Ir/op on this dev-loop run.

Change: `LazyStr::from_parse_span` now stores near-whole medium strings
(`len <= 256`, payload nearly the whole message) as owned `ProtoString` data
after UTF-8 validation. Multi-field medium strings still share the parent
`Wire`, and large near-whole strings still use the existing `Wire` path.

After artifact: `target/devloop/medium-string-linux.json`.

| cell | before instr | after instr | delta | allocs | bytes |
|---|---:|---:|---:|---:|---:|
| `codec.pbrs.small_name80_decode` | 26544.020 | 26502.938 | -0.15% | 1.0 -> 1.0 | 96.0 -> 80.0 |
| `codec.pbrs.small_empty_decode` | 25875.102 | 25917.054 | +0.16% | 0.0 -> 0.0 | 0.0 -> 0.0 |
| `codec.pbrs.small_id_decode` | 25909.830 | 25951.508 | +0.16% | 0.0 -> 0.0 | 0.0 -> 0.0 |
| `codec.pbrs.tags_32_owned_decode` | 63390.220 | 63405.354 | +0.02% | 2.0 -> 2.0 | 1544.0 -> 1544.0 |
| `codec.pbrs.tags_32_parse_touch` | 63987.836 | 64000.570 | +0.02% | 2.0 -> 2.0 | 1544.0 -> 1544.0 |

Verdict: keep the medium-string ownership threshold as a measured allocation
win for `name_80` decode (same allocation count, 16 fewer bytes/op) with no
instruction regression over the 1% codec guard. The remaining `name_80` encode
loss is still open; no safe encode-path change in this pass met the win rule.

PK-04 SWAR remains rejected. This pass did not re-run a cheap
implementation because the previous corrected target-cell comparison already
showed a large absolute regression on the same Linux/callgrind path (about
+2k Ir/op on packed/unpacked target cells), and the final code restored scalar
validation.
