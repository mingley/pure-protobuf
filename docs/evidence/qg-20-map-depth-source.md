# QG-20 bounded source implementation

Status: implemented source candidate, awaiting ordinary qualification. No Cargo,
rustc, native version probe, generated consumer, actual generator, protoc,
genuine regeneration, performance or LTO capture has run on this lane. Root
reviewed and authorized the proposal at e12c6c42. The matched metadata-only
baseline is ad04010b166325ec09bb9b403afd47338a38ab1f; its shipping source is the
frozen QG-18 candidate 5c6e5eea, byte-equivalent to 1ae03ee1 in the 17 QG-18
shipping files. These deliberately matched historical source points do not
claim current-main compiler input identity. The original e12 proposal/test
baseline and all previous QG/GN evidence remain unchanged.

The candidate changes src/codegen/parse.rs and exactly seven owned generated
files. Known map merge checks parent depth >= 100 before reading its length,
computing the entry increment or constructing Wire backing/window. Each private
decoder checks supplied entry depth > 100 before key/value defaults. A present
message value checks entry depth >= 100 before reading its body, constructing
or merging its object, or computing value depth + 1. Empty present values also
consume that frame. Existing ParseError allocation and already constructed
enclosing/key storage are outside the bounded construction guarantee.

Message value storage is a private Option, created only after its guard passes.
Every occurrence still calls merge_inner with required enforcement true, and
duplicate occurrences deep-merge into the same value. An omitted value receives
the old default at completion, without inventing a serialized value frame.
The public return type and APIs are unchanged. Scalar/key decoder bodies and
unknown fallback behavior remain unchanged after the new entry guard.

Map validation now checks its entry frame and visits every known tag 2/LEN
message value with that value type's validate_inner at entry depth + 1. All
other unknown/wrong-wire LEN payloads remain opaque. Scalar map validation
retains its old skip behavior after the entry guard. Recursion-limit errors
can now take precedence over malformed body errors at rejected frames. Legacy
skip mismatched group end-number acceptance is retained; complete strict
malformed-group validation is not claimed.

## Exact owned migration, provisional until genuine staging

The source inventory identifies 109 map decoders, callers and validators across
src/generated/struct.rs and the edition2023, edition_unstable, proto2,
proto2_editions, proto3 and proto3_editions test-message files. Seventeen maps
have message values; 92 have scalar/string/bytes values. All 109 callers get a
guard before Wire construction, all 109 decoders get an entry check, and all
109 validators get entry accounting. Seventeen value decoders defer defaults
and add the value frame, and seventeen validators visit the typed value.

The 327 exact inventoried operations reconstruct those seven files from the
matched baseline. Quoted literals and all non-whitespace text outside the
operations remain exact; only documented Rustfmt optional comma cases are
normalized. Six unaffected registered generated files and their module registry
are byte-identical. This is source migration, not regenerated compiler output
or Rust typechecking. Genuine pinned staging and byte comparison of all 13
registered generated outputs are mandatory before integration.

The first direct formatter used edition 2024 for generated files and reordered
imports outside the approved scope. The first textual comparison refused that
delta; its detailed SequenceMatcher expansion was interrupted, not passed.
That actual intermediate source and formatter records are preserved. Final
generated formatting uses edition 2021, matching the frozen QG-18/generated
convention. Final seven-file token/literal reconstruction and diff checks pass.
Handwritten source formatting uses edition 2024. These are source-only checks.

## Matched oracles and driver provenance

All 15 consumer oracle names, source bytes and the readable/in-memory descriptor
are unchanged from the proposal baseline. The baseline is expected to produce
seven red test cases and eight green controls, as listed in the preparation
record; none has executed. A matrix abort leaves later rows unobserved. Actual
15-test child execution and locked resolution must be established before
classifying any failure as semantic; infrastructure/compile failures remain
separate. Candidate runtime behavior is equally NOT_RUN.

The additive ad04010b driver is identical on baseline/candidate. It keeps
per-invocation actual command/cwd/explicit env/exit/raw stream records, original
root seed and after lock, seeded child before/after lock, prior replay lock
when present, and consumer sources/manifest. Byte equality is checked before
the semantic child-success assertion, including expected child reds. The
std-only portable driver does not implement cryptographic hashing: the reviewed
ordinary runner/verifier must compute SHA256 for retained raw lock snapshots,
check equality and compare registry tuples. Source-only lock seed inspection
preserved all 166 registry tuples; actual offline/locked acceptance is NOT_RUN.

Root's external native/process-group/timeout/resource helper remains required.
Prepare native 1.85 actual compiled 15-case baseline/candidate contrasts,
actual 1.88 strict checks and default/map/required/UTF8 controls, plus genuine
13-file staging and default/shared_pool=true actual consumers. Preserve raw
source/tool/lock/generated/ELF/fingerprint evidence and real failure statuses
before guarded retirement. Jobs 1, aggregate allocated owned caches <= 2 GiB
and global free space >= 2 GiB stay fixed. No ordinary compiler lease is active
on this lane, and no performance lease is implied by this source packet.

QG-18 helper source, QG-19 arena decode, unsafe surface, limit constant,
public generated signatures/options, module registry, manifests, dependency
locks, regeneration script, 15 oracle bytes and matched driver are unchanged
from ad04010b. Existing VERIFY/closed-enum and fail-closed collection controls
remain mandatory; the new fixture alone pins proto2 unchecked UTF8. No complete
parser-security, fastest-performance, codegen-time or release-size claim is made.
