# Selected bytes controls

Run the focused controls without installing an embedded target:

```sh
python3 tests/proof/codegen_parity/run.py --only bytes --output work/parity-bytes
```

The full codegen runner includes the same controls. Each run keeps commands,
compiler versions, source hashes and raw output in a new directory. These are
functional checks against prost/prost-build 0.14.4, not performance results.

`Config::bytes(paths)` selects eager `ProtoBytes` storage. Ordinary borrowed
getters remain unchanged. Selected fields expose an additional `_shared`
getter returning a cheap owned `ProtoBytes` clone; `as_shared()` gives callers
the backing `bytes::Bytes`. Presence fields also have `_shared_opt`, while
repeated/map getters take an index/key. Occupied helper names gain a numeric
suffix. The collision fixture proves that existing field accessors keep their
names.

Selectors follow the pinned generator's actual behavior:

| Field | Fully qualified selector |
| --- | --- |
| Ordinary, optional or repeated bytes | `.package.Message.payload` |
| Bytes in a real oneof | `.package.Message.payload`, without the oneof name |
| A map's bytes value | `.package.Message.value`, using the synthetic `value` name |

`.` selects everything. Qualified prefixes and relative suffixes match whole
name components; `*` is literal. Each builder call replaces the previous bytes
selection. Twenty-one controls compare these paths, nonmatches and replacing
calls with Prost. Builder and descriptor-set entry points must agree, and empty
or unmatched selections must leave current unconfigured output unchanged.

Three compiled profiles run thirteen case groups: selected, globally owned
messages, and selected fields under `forbid(unsafe_code)`. They check valid
hot/cold defaults, proto2 defaults and required presence, empty values, moved
buffers, oneof replacement, shared input, borrowed input, clone/drop/mutation,
indexed duplicate-key lookup, JSON/text, malformed wire and segmented output.
Shared input checks cover 1, 16, 4095 and 4096-byte payloads and compare payload
pointers with the input and the Prost control. Large segmented output uses
the runtime's separate 32 KiB send threshold.

For strict Miri, add `-- --small-formats` to the selected-byte binary. It
keeps the 32 KiB segmented-buffer and all storage/ownership controls, and uses
16-byte payloads for JSON/text roundtrips. The default native run retains the
full-size format fixture. Each run prints both payload sizes.

Owned messages and typed `MessageMut` shared merge retain the same selected
buffer at 1, 16 and 65536 bytes. Partial input errors and required-field checks
preserve the same state and results through both APIs.
Typed and arena-backed mutators also preserve the owned message's explicit
`dont_enforce_required` merge behavior; ordinary merges still enforce required
fields, and completing a partial message preserves its unknown fields.

Four additional consumers compile default, relative-alias, absolute-alias and
local-module runtime prefixes. Literal schema types named `pbrs` and
`runtime_alias`, user attributes and byte defaults keep their original
spelling. Shared helper signatures and typed JSON/text use the configured
runtime without rewriting those user fragments.

The default profile keeps its existing small-field copy policy. A frozen
older parent and a current unconfigured parent both use the selected child
type. Their values agree with Prost; eager repeated/map children copy payloads,
while the lazy singular child can retain shared input. Regenerating only a
child does not promise zero-copy payloads through every older or differently
configured parent. Small selected slices can retain a much larger input frame.

Two existing limits remain explicit:

- Removing a parsed duplicate key reveals an earlier pbrs map entry. Prost's
  unique-key map removes the key. The old/current-parent control and selected
  bytes map record the surviving value `2` after removing the latest value
  `19`, while Prost reports absence. This option does not change map mutation
  semantics.
- With reflection disabled, a complex nested/imported schema outside the
  existing typed-codec subset omits JSON/text methods. A compile-fail consumer
  requires four specific missing-method diagnostics. The flat selected-byte
  container control does compile JSON/text under `forbid(unsafe_code)` without
  reflection. The full fixture uses the normal descriptor-backed fallback.
  This is a pbrs configuration limit, not a claim that Prost supplies JSON
  generation directly.

The frozen parent and its provenance are in [old_bytes_parent](old_bytes_parent/README.md).
Do not format that source or update its hashes without regenerating it with
the recorded compiler. The intentional lightweight compile-fail binary is
run through the driver, not `cargo test --all-targets`.
