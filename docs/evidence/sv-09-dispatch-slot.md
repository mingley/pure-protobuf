# SV-09 allocation-before-construction diagnostic

The safe `Pin<Box<Option<F>>>` dispatch probe is **rejected before a runtime
experiment**. It replaces the existing 11,536-byte service-future memcpy with
a larger 11,544-byte empty-slot memcpy, then adds an 8-byte wrapper allocation.
The shipping dispatch and response writer remain unchanged. No composition
with either previously rejected runtime experiment is attempted.

## Source and scope

The diagnostic uses restored-runtime source
`e8fc4f2b4acdae53c283e6f6aec3d58add8dabfd`, whose shipping server implementation
matches `cf3eee22c6b06324e299f81b76915651471d8025`. Untracked diagnostic sources
are hashed individually. The frozen binary SHA256 is
`e928a2f48d9eba21b0956c982cd70c935bedecd8b8a94908827ecad0970c5db5`.

The isolated harness copies the original devloop manifest, lock and build
script byte for byte. Its main file adds only a private diagnostic module and
CLI arm. The actual `ProstNativeEchod`, generated prost messages, existing
CountingAlloc, primary benchmark cells and collector remain unchanged.
The added safe modules forbid unsafe code and add no dependencies. Neither
diagnostic constructor is installed into `DynService` or exercised as an RPC
benchmark; function pointers preserve both actual native constructor bodies.

The bounded hypothesis allocates a legal empty `Box<Option<F>>` before the
eager service-future constructor runs, fills it with `Some(make())`, then pins
the slot. A tiny object-safe future owns that slot and polls it through the
standard safe `Option::as_pin_mut` projection. The proposed ordering could
have allowed LLVM to write initialized future fields directly into heap storage.
Actual assembly, rather than a synthetic future size, decides this prerequisite.

## Behavioral and build proof

All five paired std-only shape tests pass with `-D warnings`, using an explicitly
`!Unpin` future. They compare the original and proposed factory shapes for eager
construction/unpolled drop, Pending abandonment, manual Ready retention,
writer-poll panic, and constructor panic. Cancellation precedes the owned writer
drop; the writer drops exactly once. The completed future remains owned until
its wrapper drops, as with the original boxed service future. These generic
shape tests do not constitute production transport qualification.

The source-frozen diagnostic release builds with the pinned env.sh, jobs=1,
the original locked graph, thin LTO and one codegen unit. No RUSTFLAGS or target
CPU/profile overrides are added. Build time is 5m16s; the isolated target remains
942 MB after link. Build time is diagnostic and carries no comparison claim.

## Actual native constructor result

Runtime `size_of` reports the actual service future as 11,536 bytes, its Option
slot as 11,544 bytes, and Rpc as 960 bytes. Release disassembly independently
proves the allocation and normal-path memcpy arguments below:

| Actual constructor | Heap allocation arguments | Normal-path memcpy arguments |
| --- | --- | --- |
| Existing DynService dispatch | 11,536 | 960; 11,536 |
| Original diagnostic wrapper | 11,536 | 960; 11,536 |
| Proposed pinned Option slot | 11,544; 8 | 960; 11,544; 960 |

The existing DynService and original diagnostic constructors both have 199-byte
machine-code bodies. The proposed constructor has a 561-byte body and reserves
12,520 stack bytes, compared with 11,536 for the original wrapper, excluding
pushed registers. Its `Box::new(None)` allocates `0x2d18` bytes and copies the
entire empty Option from the stack at instruction `0x80dbad`. It then constructs
the service future in that heap slot by copying Rpc and writing the service
reference/state tag, and allocates the final 8-byte object-safe wrapper.

Thus the large copy is retained as a larger empty-slot copy. Static constructor
allocation arguments increase by one allocation and 16 bytes; these are assembly
facts, not measured per-RPC allocation totals. The hypothesis fails its source
prerequisite, so RPC screening, full controls and runtime integration are
`not_run`. No performance or targeted-win claim is made. SV09 remains open.

## Retained evidence

`sv-09/dispatch-slot-evidence.tar.gz` retains the exact safe source, copied
manifest/lock/build/main, transparent main-file patch, unchanged-source symlinks
and reproduction notes, final five-test log, initial diagnostic warning log,
release build log, layout output, original/proposed/actual-DynService disassembly,
symbols and inherited source/tool/binary pins. File hashes and the rejection
decision are in `sv-09/dispatch-slot-manifest.json`. Frozen binaries remain in
the isolated worktree and are not bundled. This evidence authorizes no runtime
composition or threshold change.
