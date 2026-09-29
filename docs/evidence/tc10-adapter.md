# TC-10 adapter premise evidence (2026-09-29)

**Finding: the TC-10 code deliverable already landed before the card was
claimed. No code change was needed or made.**

## Deliverable status

Both halves of the deliverable are present in `protobuf-tonic/src/lib.rs`
at base `251a37b5`:

- **Decoder uses PK-09's Bytes parse** (`decode`, ~lines 61-69):
  `src.copy_to_bytes(n)` then `Parse::parse_bytes(bytes)` — one copy into
  an owned buffer (tonic's), then shared parse with no pbrs-side copy.
  Landed by `97eaf5ce` ("PK-09 1b: decode_frame + tonic adapter parse
  shared").
- **Encode writes directly into `EncodeBuf`** (`encode`, ~lines 48-50):
  `Serialize::encode(&item, dst)` with no intermediate buffer. Landed by
  `83c6f6bf` ("Drop the per-message Vec from ProtobufCodec encode and
  decode").

Both commits are ancestors of the wave-1 base (`git merge-base
--is-ancestor`). Any further copy reduction would require changing tonic's
codec API contract (one contiguous buffer per message), which is out of
scope; `docs/zero-copy.md` records why the adapter keeps tonic's single
copy.

## Adapter tests

`cargo test -p protobuf-tonic` at the base SHA: **36 passed, 0 failed**
across 9 test binaries (gzip, health_reflection, interceptor_size,
interop, parse_string, status, streaming, trailers, unary). First accept
item ("adapter tests pass") holds.

## SB-13 accept-item premise correction

The second accept item asks for "large-payload tonic+pbrs cells beating
tonic+prost with Bytes fields on SB-13 evidence". That comparison cannot
be read off SB-13, for a duller reason than a missing run: **SB-13 never
measured tonic+prost.** At the SB-13 source pin `39cf33ca`,
`rpc-bench/build.rs` already generated the tonic arm with pbrs
`emit_tonic_stubs(true)` (pbrs messages over the `protobuf-tonic`
adapter), and `rpc-bench/Cargo.toml` has no prost dependency. The five
tonic rows in `docs/evidence/large-payload-baseline/rpc.json` are
tonic+**pbrs** cells, and the SB-13 doc's pin-table label "tonic (prost
codec)" was wrong; it is corrected to "tonic+pbrs via the protobuf-tonic
adapter (no prost arm in rpc-bench)" with this commit.

So the SB-13 tonic rows serve as the tonic+pbrs large-payload baseline,
and the missing tonic+prost arm is new harness work outside TC-10's write
scope. It is split to card TC-24 rather than held against TC-10.

## Disposition

TC-10 closes as landed-via-PK-09 (code + tests + baseline cells
identified). The prost-arm comparison lives on as TC-24.
