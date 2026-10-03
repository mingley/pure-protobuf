# Composed protobuf recursion limits

The integrated production source is byte-identical to qualified candidate
`68e4d57d67c73ff87ea1404bee18c3c70a9dfb93` across all 27 changed source and fixture
paths. Unknown groups, private arena messages, map entries and present message
values now share the existing recursion budget. Bundled generated modules are
updated together with the generator.

Rust 1.85 feature-off/core, Rust 1.88 default parser tests, root and standalone
strict Clippy, actual linked upstream tests, genuine thirteen-module byte-equal
regeneration and shared-pool group9 pass. The 254 upstream executions include
the fifteen arena cases. Focused and broad tests overlap.

The linked native result keeps its original wrapper126; an independent audit
qualifies the expected generated-file additions. The group helper's expected
metadata-stub replacement is qualified separately. Failed coordinator histories
remain failed.

The combined shared-pool map replay remains pending because its original helper
raised `KeyError('accepted_record')` before protoc or Cargo. A one-line helper
correction is in progress. Isolated shared-pool map15 and combined default map15
already pass. Complete selected raw evidence will be added after this replay.
The qualification JSON pins current successful records and full local artifact
readback. ELF and SDK payloads are retained locally. These correctness checks do
not establish benchmark gains or complete parser-security coverage.
