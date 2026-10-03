# Isolated arena recursion qualification

QG-19's private depth implementation preserves the existing public parse/merge
signatures, recursion limit and unsafe boundaries. The compiled baseline executes
all 15 linked cases: 13 intended depth failures and two passing controls. The
candidate passes the linked cases, broad arena/runtime suites, native Rust 1.85,
strict Rust 1.88, feature controls and regeneration comparisons.

Run `python3 docs/evidence/qg19-isolated-qualification-20261003/check-payload.py`
to verify every included byte. The capsule contains 44 actual command records,
176 raw output/sample/prelaunch files, plans and provenance audits. Its immutable
ledger retains earlier compiler, formatting and fixture failures with their
original source identities. Focused cases overlap broad suites; their counts
must not be added as distinct tests. The final 254 upstream executions include
the 15 depth cases.

The final Rust 1.85 attribution audit records 27 successful protoc invocations,
31 schema inputs and 58 generated outputs equal to the earlier qualified
candidate. Only absolute output-directory paths are normalized. Earlier Rust
1.88 strict checks retain their original source labels; the later assertion
formatting amendment does not relabel them.

The archive includes full selected command evidence. Compiler/test ELFs, complete
SDK/generated-source payloads and their local preservation records are separate
retained artifacts, not included payloads or a complete linker closure here.
`manifest.json` maps each included member back to its original path, hash and
mode; capsule member metadata is normalized for reproducibility.

This is isolated qualification. Combined-current-main validation and source
integration are separate gates. Performance, total byte/arena limits, exhaustive
parser security and official upb extension ABI remain open. `MergeFrom` still
serializes/deep-copies the source before bounded parsing, may leave a partial
destination and discards the parse error through its existing public signature.
Repeated-array storage may be allocated before the child-depth check.
