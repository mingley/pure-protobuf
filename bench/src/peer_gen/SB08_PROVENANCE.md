# SB-08 peer provenance: pins, gencode, and why

Card SB-08 ("Refresh Rust codec peers and use generated schemas").
This file is the manifest for the checked-in peer gencode in this directory
and the exact peer pins in `bench/Cargo.toml` / `bench/Cargo.lock`.

## Runtime pins (all `=` exact in `bench/Cargo.toml`)

| JSON id | Package | Version | Lock checksum (sha256) |
|---|---|---|---|
| prost13 | prost | 0.13.5 | `2796faa41db3ec313a31f7624d9286acf277b52de526150b7e69f3debf891ee5` |
| prost14 | prost | 0.14.4 | `528ac67416ff8646872a3c02cad9cc4ee5dc9f9540c9b10771855c95cb2e5ae1` |
| buffa092 | buffa | 0.9.2 | `a92f2f5df67a9d5ccfc65237bfc954c56328aee40a04a9b92381ecba370be246` |
| g36 | google-protobuf | 0.36.2-release | `eb6a2d5d79f0041702be6081437d082721099677d4a708e80bca4e6a831df343` |
| qp | quick-protobuf | 0.8.1 | `9d6da84cc204722a989e01ba2f6e1e276e190f22263d0cb6ce8526fcdb0d2e1f` |

Supporting pins: prost-types 0.14.4
(`f94967dc7688f3054c7fac87473ffae4cc4c3904800e2d9f5b857246d8963b0a`),
buffa-types 0.9.2
(`b15b05c8418f1e200e406e5b19df97400dffa833e2450e4392c506896cbc97ac`),
protobuf-macros 0.36.2-release
(`3efdd431f0efb3b196516e124416be3a3b09bf733ba42397c6cff71ea34539d2`).
The harness embeds `bench/Cargo.lock` at compile time and prints every id,
version, and checksum in its top-level `"peers"` section
(`"peers_schema": "bench-peers/1"`); `peers::tests::current_peers_resolve_in_lockfile`
fails closed when a pin stops resolving.

Retired peers (kept in the `"peers"` output with `"role": "historical"` and
their pre-SB-08 checksums, copied verbatim from the lockfile at 785c5e35):

| JSON id | Package | Version | Checksum |
|---|---|---|---|
| v4 | protobuf | 4.35.1-release | `a169648cc34d6f327fea8919ca63f38261fb26405fde8879745dc0a483db328e` |
| buffa091 | buffa | 0.9.1 | `cf9e6224bc4ee1f189ad257120c156fb05f95b826f5369d620b24984476c200a` |

## Why the old peers cannot stay linked beside the new ones

- `protobuf 4.35.1-release` vs `google-protobuf 0.36.2-release`: both
  declare `links = "upb"`, and cargo rejects two `links = "upb"` packages in
  one dependency graph (verified: the resolver errors out naming both
  packages). The 4.35.1 column therefore cannot share a bench binary with
  the 0.36 column. It stays measured as the historical column in the
  untouched harnesses: tonic-bench (`protobuf = "=4.35.1-release"`) and the
  devloop `codec.v4.*` cells (`protobuf = "=4.35.1-release"`).
- `buffa 0.9.1` vs `buffa 0.9.2`: same package, same 0.9 track. Cargo
  unifies same-track requirements to one version, so the pinned pair
  `=0.9.1` + `=0.9.2` is unresolvable (verified: the resolver errors out).
  Keeping 0.9.1 would have required editing the `buffa_tat` /
  `buffa_person` path crates, which are outside SB-08 write scope, so bench
  moved to 0.9.2 outright and dropped those path dependencies. 0.9.1 survives
  only as pre-SB-08 bench data plus the historical row above.
- `prost 0.13.5` vs `0.14.4` are different 0.x tracks and coexist fine, so
  the historical 0.13 column stays (via the untouched `prost_tat` path
  crate) beside the new 0.14 column.

## Checked-in gencode

Why checked in: no single installed `protoc` can regenerate every peer at
build time (the retired 4.35.1-era gencode needed `protoc --rust_out` 35.x;
the 0.36 gencode needs 36.x), following the tonic-bench checked-in-gencode
precedent. Drift protection is threefold: the source hashes below (tested by
`peers::tests::schemas_match_generation_hashes`), the gencode-to-runtime
version assertions inside the generated files (fail the build on runtime
drift), and the cross-codec equivalence pre-checks before timing (fail the
run on semantic drift).

Sources:

| Schema | sha256 |
|---|---|
| `proto/person.proto` | `13ec16099b636bb46467e28199347c79ba26e72223462c1c3060e28d6c1620c5` |
| `third_party/protobuf/src/google/protobuf/test_messages_proto3.proto` (protobuf v35.1 @ `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`) | `c3c6fd3959fe767f00c19bcb79e71bc4ca2b51769b7f49a14b24e9b24e152873` |

Files (each carries a header with its generator and rewrite):

| Files | Generator | Rewrite |
|---|---|---|
| `prost13_person.rs` | prost-build 0.13.5 + protoc 36.2 | none (bench `prost` is 0.13) |
| `prost14_person.rs`, `prost14_tat.rs` | prost-build 0.14.4 + protoc 36.2, `prost_path=::prost14`, `prost_types_path=::prost_types14` | none (native options; without them the derive defaults to `::prost` = 0.13) |
| `buffa092_person/*`, `buffa092_tat/*` | buffa-build 0.9.2 (`lazy_views=true`, json/text off) | `::buffa::` -> `::buffa092::`, `::buffa_types::` -> `::buffa_types092::` |
| `gpb36_person/*`, `gpb36_tat/*` | protoc 36.2 `--rust_out` (`experimental-codegen=enabled`, `kernel=upb`) | none (bench renames `google-protobuf` to `protobuf`, which is what the gencode references) |
| `qp_person.rs` | pb-rs 0.10.0 (`single_module`) | dropped `use super::*;`, moved `#![allow]`s to the wrapper module |

## Regenerating

`peer_gen/regen/` is a standalone (non-workspace, non-dependency) helper
crate. It is inert to the bench build: cargo never discovers it (the bench
`[workspace]` has no members) and no manifest references it.

```sh
# 1. Fetch schemas (also needed by the prost_tat path crate at build time).
./scripts/fetch-protobuf.sh
# 2. Generate into /tmp/sb08out (needs protoc 36.x for --rust_out and network
#    for the pinned generator crates; see regen/Cargo.toml).
cd bench/src/peer_gen/regen && cargo run --release
# 3. Stage into bench/src/peer_gen/ with headers and rewrites.
python3 stage.py /path/to/worktree
# 4. Rebuild + drift tests + a knobbed smoke run.
cd bench && cargo test && PBRS_BENCH_ITERS=200 ./target/debug/bench >/dev/null
```

After regenerating, update the source-hash table above and the matching
constants in `peers::tests`.

## Column semantics (accept criterion SB-08/1)

- Scoreboard cells (`"role": "scoreboard"`) use generated pbrs types only:
  `pbrs::gencode::TestAllTypesProto3` for TAT cases and the
  compiler-generated `person_generated::Person` for `person_generated`.
- The handwritten `pbrs::testdata::Person` case (`person`) stays as
  `"role": "diagnostic"`; it isolates handwritten-vs-generated layout and is
  not a scoreboard cell. All peer columns in it are generated types too.
- Every owned codec in every case reports fresh, cached, and mutated encode
  plus parse-only and parse-and-touch decode over the same iteration counts,
  with outputs consumed through `black_box`. Views report parse-only and
  parse-and-touch only (they cannot encode).
- quick-protobuf parses always borrow the input, so `qp_decode_ns` is
  borrow-mode and there is deliberately no `qp_decode_owned_ns` alias.
- quick-protobuf 0.8.1 drops proto3-optional presence (`email` is a plain
  string). Absent and explicitly-empty both encode to nothing and both touch
  as zero, so the fixed fixtures stay exactly equivalent; the caveat is
  recorded here and in `peers.rs`.
- quick-protobuf is person-shaped cells only: pb-rs 0.10.0 rejects the TAT
  schema (`TrailingGarbage` on nested messages), and TAT `qp_*` keys are
  null. Its top-level helpers are length-delimited framed, so the harness
  encodes/parses through the bare `MessageWrite`/`MessageRead` trait
  methods (`peers::qp_encode` / `peers::qp_decode`).
- Buffa eager views (`decode_view`) validate the whole tree; lazy views
  (`decode_lazy`) scan the top level and decode nested/repeated message
  fields on access. Both report decode and touch columns.
- Gates are unchanged historical smoke (BM-03): pbrs vs prost 0.13,
  google-protobuf 0.36, and buffa 0.9.2 owned plus the eager view, on the
  same case membership. New columns (prost14, lazy, qp) are reported, not
  gated.
