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

## Implemented PK-04 fast path

Hypothesis: packed-varint validation is on the pbrs owned-decode path through
`Packed<VarintI32>::append_wire`; skipping runs of one-byte varints eight bytes
at a time with a portable high-bit mask should help varint-heavy decodes while
preserving truncation/overflow/overlong semantics.

Change:

- `src/wire.rs`: `validate_varints` now skips 8-byte chunks when no byte has
  the continuation bit set. This is safe, portable scalar SWAR and uses no
  `unsafe`.
- `fuzz/fuzz_targets/varint_diff.rs`: new differential fuzz target comparing
  `decode_varint`, `decode_tag`, and packed-varint validation against a scalar
  reference on the same input.

Correctness checks run:

```sh
cargo fmt --check
CARGO_BUILD_JOBS=3 cargo test -p pbrs wire::tests::validate_varints_matches_scalar_reference --lib --offline
CARGO_BUILD_JOBS=3 cargo check --manifest-path fuzz/Cargo.toml --bin varint_diff --offline
```

## After evidence

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

Verdict: keep the no-unsafe SWAR validator and differential target as a small
PK-04 step, but do not mark PK-04 done. The local wall result is directionally
positive for owned decode and parse-touch in the after repeat, but decode CV is
too high for a standalone performance claim without Linux instruction counts.

## Disproved or reverted attempts

- Empty-message generated `Serialize::encode` early return: reverted. It made
  broad fresh/cached encode slower in the dev-loop comparison.
- Fixed-width packed byte view from decoded vectors: reverted. It required a
  new unsafe block and did not produce a stable large packed-fixed encode win
  in the current noisy bench run.
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
