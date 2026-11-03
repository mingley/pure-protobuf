# Checks at the committed source

This packet checks commit
[`08cbcafb3042d5d465fc764ceb7f2578095f510d`](https://github.com/mingley/pure-protobuf/commit/08cbcafb3042d5d465fc764ceb7f2578095f510d).
It binds all **1,307 selected inputs** from the parent validation capsule to
that commit's Git objects.

The local publisher packed core **0.2.1** and both adapters **0.1.0-alpha.3**
in a clean disposable checkout. All **419 shipped source members** match the
commit. The retained final and consumer archives let the checker repeat the
source, normalized manifest, version, VCS and dependency-lock comparisons.
Consumer archives record `a3570158`: core and native gRPC are dirty; the tonic
adapter is clean. Those actual records remain unchanged.

The publisher uses offline `cargo package --no-verify`. Source matches the
archives used by the earlier passing consumer tests; this packet does not
execute the final archives. Recorded VCS identity and adapter core-lock
provenance are the permitted comparison differences.

The release-mode resource smoke passed **19 cycles in 62.06 seconds**, with
finite process limits and TCP/TLS, identity/gzip, slow-reader, overload,
cancellation, deadline and recovery scenarios. Its 24-hour status is
**not run** and `qualified` remains **false**. The report lists the other
unmeasured resource and performance axes.

Run `python3 check.py` from this directory. It checks every packet digest,
binds selected inputs to Git, repeats the package audit and validates the
resource report with the validator retained from the named commit. Resource
executable bytes, caches, compiler binaries and core dumps are omitted;
recorded binary hashes are not replayed. The parent capsule keeps the full
earlier validation and detecting failures. Four rehashed mutation controls
reject changed source bindings, dirty publisher status, production promotion
and an altered resource validator. Their logs and receipt are retained.

This is local packaging and a bounded smoke check. These versions remain
unpublished and the adapters remain previews. Hosted CI and provenance,
verified signing, Trusted Publishers and sustained production qualification
remain separate gates. No tag or registry publication was attempted, and
this packet adds no RPC performance ranking claim or build attestation.
