# Consuming shared byte buffers, November 10

`ProtoBytes::into_vec()` explicitly copies its contents. The
[bytes guide](../guides/bytes.md) recommends this for detaching a small selected
field from a potentially much larger input frame. Preserve that behavior when
the retained capacity matters.

An existing alternative is to clone the shared `Bytes` handle, drop the
`ProtoBytes`, then consume the handle:

```rust
let shared = payload.as_shared();
drop(payload);
let output: Vec<u8> = shared.into();
```

This may reuse a unique backing allocation. It may also copy for shared or
static storage, move an offset slice within the backing allocation, or retain
more capacity than the field length. Other live handles prevent unique reuse.
The repository search found a runtime test and the documented detach use, but
no in-repository production caller of `ProtoBytes::into_vec`; external use is
unknown. No public API change follows from this experiment.

## Measurement

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42757307084)
used source `41d94325b8aa48b10b31fb6709c1a41dbc44a72a`, bytes 1.12.1,
Rust 1.99.1 and a hosted AMD EPYC 9V74. It compares the current copy, the
existing public route above, and a [scratch-only direct helper](../evidence/bytes-ownership-conversion-20261110/overlay.patch)
that consumes the internal `Bytes` without the intermediate clone. The helper
was not added to the public API. The [probe source](../evidence/bytes-ownership-conversion-20261110/probe.rs)
uses a generated selected-byte field for the set/get/drop and
parse/get/drop cases. The [raw data](bytes-ownership-consume-20261110.json)
retain all 315 measurements.

The single-thread counting allocator measured successful allocation and
reallocation requests only during conversion; input creation, generated
getters and preparation of any surviving owner happened beforehand. Each row
was repeated five times at 64, 4,096 and 65,536 payload bytes and matched
within that run. The probe checked output contents and that a surviving shared
owner remains independent after output mutation. Output capacity is the
reported `Vec::capacity()`, not peak or resident memory. This is an
instrumented allocation screen, not a CPU or latency benchmark.

For a 4,096-byte payload, each entry is **allocation requests / requested
bytes; output capacity**:

| Input | Current copy | Existing shared route | Scratch direct helper |
|---|---:|---:|---:|
| Unique, exact capacity | 1 / 4,096; 4,096 | 1 / 24; 4,096 | 0 / 0; 4,096 |
| Unique, spare capacity | 1 / 4,096; 4,096 | 0 / 0; 32,768 | 0 / 0; 32,768 |
| Unique offset slice | 1 / 4,096; 4,096 | 0 / 0; 33,024 | 0 / 0; 33,024 |
| Another owner alive | 1 / 4,096; 4,096 | 1 / 4,096; 4,096 | 1 / 4,096; 4,096 |
| Static bytes | 1 / 4,096; 4,096 | 1 / 4,096; 4,096 | 1 / 4,096; 4,096 |
| Generated set, getter, drop message | 1 / 4,096; 4,096 | 0 / 0; 4,096 | 0 / 0; 4,096 |
| Generated parse, getter, drop message | 1 / 4,096; 4,096 | 0 / 0; 33,024 | 0 / 0; 33,024 |

The public and direct reuse routes requested the same allocations in the
generated examples. In the exact-capacity direct input, the intermediate public
clone requested a 24-byte metadata allocation. That difference does not
establish a latency benefit or justify a new method by itself. Generated
getter setup can already have promoted a buffer before measurement.

For unique offset slices the returned vector reused the root pointer but its
payload pointer changed. The pinned bytes implementation moves data back to the
root for this case, so avoiding an allocation does not imply avoiding a copy.
Both reuse routes kept the original capacity in the spare and parsed-frame
examples. The current copy produced a vector with payload-sized capacity in
this screen. Pointer identity and exact capacities are observations for this
version and allocator, not API guarantees.

The [artifact](https://github.com/mingley/pure-protobuf/actions/runs/42757307084/artifacts/13736162195)
expires November 24, 2026 at 04:42:34 UTC; its ZIP SHA-256 is
`7666b99d1e22a910b688c3c39d207cdc0764901ba4cacc8c929ac42c44138135`.
See the earlier [byte ownership measurements](../evidence/byte-ownership-20261105/README.md)
for other conversion and clone costs.
