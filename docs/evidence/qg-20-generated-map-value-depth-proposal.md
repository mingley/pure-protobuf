# QG-20 generated message-valued map depth proposal

Status: source inspection and test-only preparation. No Cargo, compiler,
generated-consumer execution, protoc, regeneration, native tool-version probe,
performance capture or shipping edit has run. This is separate from QG-18's
unknown-group helpers and QG-19's arena-backed decode. The deliberate shipping
baseline is the frozen QG-18 candidate 5c6e5eeac36e297d0eb67fc1ac9302a77fb451b6
(the 17 shipping files are byte-identical to 1ae03ee1). The isolated sparse
worktree is /workspace/scratch/work/generated-map-value-depth, on branch
mingley/generated-map-value-depth-plan. It has no inherited compiler cache or
historical GN archives. Root owns shipping approval, ordinary leases and main
integration. All proposed compiler/runtime gates remain NOT_RUN.

## Concrete source defect

The actual generator files are src/codegen/parse.rs, src/codegen/messages.rs
and src/codegen/naming.rs; src/codegen/rust.rs does not exist in this tree.
emit_merge_arm already passes parent depth + 1 to decode_map_entry for the
map-entry frame. emit_map_scalar_decode then passes that same entry depth to
the message value's merge_inner. The value's distinct frame is lost. The map
decoder has no entry-depth check before key/value defaults. Its caller creates
a Wire backing/window before entering the decoder. At parent depth 100, even
a scalar or empty map entry can therefore enter depth 101 without rejection.

emit_validate_arm currently walks map entries by skipping every field. A
known tag 2/LEN message value is opaque to that validator. An optional lazy
ancestor can accept an over-depth, truncated or missing-required known value
that direct/eager decoding would reject. QG-18 correctly guards unknown groups
in the entry itself; it cannot inspect an unknown group hidden inside this
known message value. Existing scalar/scalar map tests do not qualify this path.

## Proposed shipping scope, subject to root review

1. In the known-map merge arm, reject parent depth >= 100 before entering the
   map entry, computing its increment, creating its Wire backing/window or
   mutating map storage. The caller may already have consumed the map tag.
   Check the private decoder's supplied entry depth > 100 before key/value
   defaults. Use the unchanged RECURSION_LIMIT and existing ParseError.
2. A present known message value consumes another frame: value depth is entry
   depth + 1. Check entry depth >= 100 before computing that increment and
   before constructing or merging that value. This applies to empty present
   message payloads too, so an empty-child shortcut cannot bypass the frame
   check. Preserve merge_inner's existing required-enforcement argument.
3. Defer the message value's default storage until a present value has passed
   its depth guard, for example with a private Option of the existing value
   type. Repeated value occurrences in one entry must deep-merge into the same
   object, rather than replace it. On an omitted value, apply the same old
   default at entry completion. Keys, scalar defaults, omitted-value defaults,
   duplicate entries, last-wins lookups and full encoded entry order stay the
   same. This is a proposed private implementation choice, not shipping code.
4. The map validator checks the entry frame before visiting it. For a known
   message-valued map, inspect tag 2/LEN, check the value frame, and call that
   actual value type's validate_inner at entry depth + 1. Use the descriptor's
   field-2 type and existing namespace mapping. Visit every value occurrence;
   required checks must match the current eager per-occurrence behavior.
   Unknown numbers, wrong wire types and generic unknown LEN payloads remain
   opaque. Existing scalar-map UTF8, enum, defaults and wire/error semantics
   are retained. This leaf does not add group-number checks to skip validation.
5. Audit and update every affected owned bundled caller in src/generated/,
   retaining exact registry membership and valid encode/size/accessor methods.
   Prefer genuine pinned staged regeneration under a later ordinary lease.
   If prerequisites are unavailable, root must review an exact inventoried
   source migration; call it migrated, never regenerated. This source-only
   preparation makes no regenerated-output or typechecking claim.

The depth model counts wire message frames. A present map entry contributes
one; a present message value contributes one more. An omitted value still
gets the old default without inventing a value payload frame. This distinction
needs explicit source review because serializers can emit the default value
later. The acceptance controls pin omitted values at ordinary root depth and
entry depth 100 separately; they make no round-trip guarantee for a newly
emitted empty value frame beyond the original input's depth budget.

Rejecting before new entry/value construction does not imply a zero-allocation
error path: ParseError strings and already parsed enclosing objects/backings
may allocate. Source ordering can establish the bounded construction guard;
no allocation measurement has run. Existing public generated methods and
runtime exports, option/default selection, manifests/dependencies, unsafe
surface, registry and the limit constant must remain unchanged. Do not change
QG-18 helper behavior, QG-19's arena decoder or frozen GN03 source comparisons.

## Distinct bounded test-only oracles

The new readable proto and independently assembled descriptor describe Node's
optional lazy child, repeated eager child, scalar, unchecked proto2 string,
message-valued nodes map, scalar-valued scores map and required Payload map.
No external compiler, frozen descriptor or existing test fixture is modified.
Every known/group boundary vector has at most 101 actual traversed frames;
opaque unknown LEN controls contain only the same bounded 101-group payload.
No crash, unbounded recursion or deep stress workload is proposed.

tests/generated_map_value_depth.rs builds a fresh actual consumer from the
deliberate source using the existing generator API. Its child copies the root
lock verbatim and adds only the local qg20-map-value-consumer root. Root must
retain both raw locks and compare all registry name/version/source/checksum
tuples; actual offline/locked acceptance must pass before any child red is
called semantic evidence. The child uses jobs 1, no incremental compilation,
offline and locked Cargo, a retained source/log directory when requested, and
root's exact native Cargo and owned CARGO_TARGET_DIR. The test driver itself
does not implement a wall-time/resource monitor; qualification must run under
root's descendant/process-group timeout and cache/global-space helper. No child
or generator has executed during preparation.

The 15 actual generated-consumer tests are independently named cases:

- Present message values at total depth 100 accept through eager/lazy ancestors
  and direct validation (98 known + entry + value; or 97 known + entry + value
  + one unknown group).
- Total 101 present values reject through eager/lazy ancestors, and a separate
  direct-validator test rejects both 99-known/no-group and 98-known/one-group
  cases. The baseline is expected to accept them; outcomes are NOT_RUN.
- Scalar map entries at total 100 accept; empty scalar/message entries at
  total 101 reject. Direct supplied parent depth 99 accepts an empty entry,
  parent 100 rejects, and validation agrees.
- Truncated varint, truncated known LEN child and unterminated unknown group
  inside a known value reject directly, eagerly, lazily and in a separate
  validator case. No large malformed input is used.
- Legacy mismatched group end numbers remain explicit: direct/eager capture
  rejects, skip-based validation/lazy acceptance retains its prior behavior.
  QG-20 does not claim strict validation of these legacy skip paths.
- Unknown LEN data at the node, map entry and known value remains opaque,
  including invalid byte 0xff and a bounded 101-group byte payload.
- Omitted key/value/scalar defaults remain; duplicate keys keep last-wins
  lookup and every original encoded entry; repeated message values within one
  entry deep-merge fields into one canonical value.
- Known required Payload values retain checks through lazy ancestors; proto2
  unchecked UTF8 remains accepted in a known value.

Baseline expected red tests are the total-101 parse/validator, entry-limit,
malformed-value parse/validator and required-value lazy cases. Controls for
total100, opaque LEN, existing end-number asymmetry, defaults, duplicates,
deep merge and unchecked proto2 UTF8 are expected green. Expectations are
source-derived only. A failed matrix assertion stops the rest of that test's
matrix, so unobserved rows must not be relabeled as executed reds/greens.
Infrastructure/compile/locked-resolution failures must remain separate.

## Required later qualification, all NOT_RUN

Preserve this test-only baseline and use identical test/consumer fixtures on
the candidate. Retain genuine bounded baseline assertion failures and raw
candidate results, generated sources, command/env/tool/source identities,
locks and registry tuple checks. Run native 1.85, actual strict 1.88 where the
dependency floor permits, fresh compiled-consumer defaults/reflection-free
profiles, and the existing parser/map compatibility controls under jobs 1 and
root's fixed allocated owned-cache/global-space guards. Retain all failure
dispositions and actual archive-before-retirement proof. No performance/LTO
lease is authorized by this proposal.

Existing controls in tests/plugin.rs, tests/google_shared.rs, tests/dynamic.rs,
tests/differential_binary.rs, tests/table_parse.rs, tests/depth.rs,
tests/unknown_group_depth.rs and src/table.rs must cover map
required/closed-enum/UTF8, wrong wire, duplicate order and dynamic/table
semantics. Actual controls include edition2024_map_decoder_uses_entry_utf8_feature,
edition2024_closed_enum_collections_fail_closed, the fresh edition2024 consumer's
map_key_and_value_validate_entry_utf8/inherited_map_utf8_none_accepts_unverified_entries,
test_required_field_enforced, test_map_entries_last_wins,
pk18_map_entries_last_wins_with_defaults and dynamic_map_entry_direct_matches_temp.
Confirm their exact current source/profile prerequisites before final gate argv;
unsupported closed-enum generated collections must retain their fail-closed diagnostic.
This new proto2 fixture pins unchecked UTF8 only; it does not qualify VERIFY
or closed-enum maps by itself. Known-value validation must preserve their
existing descriptor-driven behavior rather than infer it from this fixture.

Public signature/default/wire compatibility checks need the actual owned
generated delta and genuine or explicitly migrated regeneration comparison.
No complete-parser-security, fastest-implementation, throughput, codegen-time,
release binary-size or repeated-corpus claim follows from this correctness leaf.
