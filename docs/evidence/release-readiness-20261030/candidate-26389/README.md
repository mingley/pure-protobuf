# Candidate release checks, October 30

These records cover candidate `26389ccf77bf9342fe6e605f1d797853acd6c39a`.
Later production-source changes need their own exact-source checks. The
prepared versions remain unpublished: pbrs 0.2.1 and both adapters
0.1.0-alpha.3. Adapters remain previews.

The local publisher dry run packed all three crates without upload. The
independent archive audit verified versions, adapter requirements, VCS SHA,
licenses and every shipped source blob. Isolated package consumers then
passed both tests and executed all three consumer programs outside the
workspace. The exact core archive matches the rehearsal. Adapter archives
have identical source, manifests and VCS metadata; their Cargo.lock pbrs
source/checksum differs because the test uses Cargo's multi-package registry
overlay. The recorded comparison names that difference.

Required hosted CI has no completed result in the retained observation:
all 11 jobs were queued. A local package run cannot produce the required
GitHub provenance attestation. Sustained qualification, the hosted rehearsal,
Trusted Publisher setup and a verified signed tag remain open. No crate was
uploaded and no tag was created.

`status.json` indexes results and limits. `report.json` and `publisher.log`
record the local package command. `archive-audit.json` lists artifact digests
and per-file source checks. `package-consumers.json` and its log record the
consumer check; the two comparison files explain the archive difference.
`hosted-ci-observed.json` retains the CI observation. `manifest.json` hashes
the retained files. `audit.md` adds the earlier baseline failure context.

Raw crate archives remain in the ignored local directory
`work/release-rehearsal-26389/target/package/`; their digests are recorded,
but they are not published or presented as hosted-attested artifacts.
