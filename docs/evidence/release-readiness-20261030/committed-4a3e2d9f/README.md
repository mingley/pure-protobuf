# Checks at the committed source

This packet checks commit
[`4a3e2d9f82edb8f0a94ae0f9f6a9bc500537607d`](https://github.com/mingley/pure-protobuf/commit/4a3e2d9f82edb8f0a94ae0f9f6a9bc500537607d).
It prepares `pbrs` 0.2.1 and both adapters 0.1.0-alpha.3. These versions
remain unpublished; the adapters remain previews.
The [public registry observation](records/registry-observation.json) at
19:24:15 UTC on October 30 found all three candidate versions absent. That is
a timestamped observation, not a prediction of registry state.

- [Package audit](records/publisher/archive-audit.json): all three archives
  packed successfully in a disposable clean checkout. All 419 shipped source
  members match the commit. Runtime and build requirements, licenses, VCS
  records and package exclusions were checked.
- [Source binding](records/committed-source-binding.json): all 464 selected
  inputs from the earlier validation capsule match the committed Git blobs.
- [Resource report](records/resource/report.json): the bounded release-build
  run passed for 62.06 seconds, with finite process limits and
  TCP/TLS, identity/gzip, overload, cancellation and fault-recovery scenarios.
  It completed 19 cycles. The 24-hour campaign was not run.
- [Documentation check](records/docs-contract/test.log): 27 checks passed after
  the earlier validation capsule was added, before this packet was installed.

The package dry run uses `--no-verify`; it does not execute the final archives.
Earlier isolated consumers passed using the same source and normalized
manifests. Their retained packages differ from these archives only in explicit
VCS identity and the adapters' core dependency lockfile provenance. The
comparison verifies those differences against the actual core archive hashes;
it establishes source equivalence, not archive or executable identity.

Six [disposable mutation controls](records/checker-controls/report.json) were
rejected, including rehashed source, publisher, package and validator changes.

Run `python3 check.py` inside this repository. The checker hashes every packet
member, compares the selected inputs with Git, reruns the archive audit and
validates the resource report with the validator retained from the pinned
commit. The resource executable is identified by SHA-256 but is not retained;
the checker validates its recorded build profile and report rather than
replaying the executable. No cache, temporary tree or core dump is included.

These are selected source and observation records, not a complete compiler
filesystem trace or tool/dependency attestation. They add no performance
ranking evidence. Local packing produced no hosted SBOM or provenance
attestation, and no release tag or publication was attempted. Exact-source
hosted CI, hosted provenance, verified signing, Trusted Publishers and the
remaining production, security and support-policy gates still apply.
