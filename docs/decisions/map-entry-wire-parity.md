# Default-valued map-entry wire parity (PK-30)

Status: proposal for maintainer review. No serializer policy change is approved
by this record. The SB-26d equal-wire gate remains in force.

SB-26d qualifies all 64 adoption specimens at source `22b16557`. Full decoded
equality and complete read checksums agree for all specimens. Map sizes
8/64/512 produce four additional native bytes, so all 24 map/profile/shape
cells reject timing. The [inventory](../../bench/devloop/adoption/evidence/rpc-inventory.json)
and [corpus](../../bench/devloop/adoption/src/corpus.rs) retain this result.

## Source diagnosis

The frozen `MapHeavy` constructor supplies two relevant defaults at every
size: `counts["count-0000"] = 0`, and the entity map has integer key `0`
(the constructor uses keys -4 through 3). The generated native map writer in
[encode.rs](../../src/codegen/encode.rs) computes both key and value sizes and
emits both subfields unconditionally. The runtime map encoder similarly
emits the default key. Prost 0.14.4's `encoding::hash_map::encode_with_default`
compares keys with `K::default()` and values with their configured default,
omitting default subfields while retaining the outer map entry.

| Entry | Extra native subfield | Bytes |
|---|---|---:|
| `counts["count-0000"] = 0` | sint64 value, field 2: `10 00` | 2 |
| Entity at integer key `0` | int64 key, field 1: `08 00` | 2 |

The lengths of the surrounding entry payloads also increase by two. Those
length varints remain the same width for these fixtures, so the total gap is
four bytes. Both representations are legal protobuf and decode to the same
map values. This is a serialized-wire compatibility decision, rather than
evidence of lost values or a reason to remove the fixtures.

## Proposed implementation boundary

To satisfy the existing equal-wire adoption contract, omit default scalar
key/value subfields while retaining every outer map entry. Keep map iteration
and overall field ordering out of this change; PK-28 owns field order.
Synchronize generated size/write paths and the runtime encoder. Preserve
unknown values and the established closed-enum behavior. In particular,
proto2 enum defaults need their declared first value, not a hard-coded zero;
message-valued entries and explicit oneof defaults need separate coverage.

The compatibility cost is changed serialized bytes for default-valued map
entries, which can affect byte-keyed caches, hashes and golden files. The
maintainer must review this policy before implementation. If accepted, file
a small implementation child that owns the exact emitter/runtime paths,
migration note, same-host before/after encode measurements and codegen,
original shared-consumer and pinned conformance checks. Do not close PK-30
without that child and the recorded review.

Acceptance must cover scalar key/value types, proto2 enum defaults, empty and
nonempty message values, explicit typed oneof defaults, and preservation of
the outer default/default map entry. Then regenerate the adoption inventory
and prove equal request/response lengths for all three sizes and all 24 RPC
cells. The existing codec baseline and blocked RPC evidence remain historical
records of their original source; they must not be rewritten as passes.
