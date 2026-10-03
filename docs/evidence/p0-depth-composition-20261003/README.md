# Composed protobuf recursion limits

Production source `68e4d57d67c73ff87ea1404bee18c3c70a9dfb93` is integrated on main
at `9573a7eb`. All 27 changed source and fixture paths match that candidate.
Unknown groups, private arena messages, map entries and present message values
share the existing recursion budget. Bundled generated modules match the generator.

All ten planned native scopes qualify: Rust 1.85 feature-off/core89, Rust 1.88
default parser/integration coverage, strict root and standalone Clippy, linked
upstream254 including15 arena cases, current plugin setup, genuine thirteen-module
byte-equal regeneration, and shared-pool group9/map15. Default generated consumers
also pass group9/map15. Focused and broad tests overlap; these are not unique-test sums.

Run `python3 -B check-payload.py` in this directory. The checker streams all
1,267 full members (42,467,040 logical bytes), validates the ten actual native
records, and checks generated consumer masks and regeneration comparisons.
The archive is 8,689,455 bytes; its SHA-256 and per-member hashes are in the manifest.

Failed coordinator histories remain failed. The original linked native0/wrapper126
is qualified by an independent audit of expected generated-file additions. The
successful group9 helper replacement has its own independent reconciliation.
The original map helper's pre-generation `KeyError('accepted_record')` remains
retained; the successful continuation changes only one metadata serialization
line and uses fresh output paths. Original workloads, locks and limits remain fixed.

The manifest and qualification JSON distinguish selected raw evidence from full
ELF, SDK and generated-source payloads retained locally. These results establish
bounded correctness and compatibility, not benchmark gains, full compiler/linker
closure or complete parser-security coverage. Previously generated external
callers must regenerate to compose known-message and unknown-group depth.
