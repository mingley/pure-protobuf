# TC-30b3a bounded outbound qualification

The leaf supports changed native outbound preferences and real native gzip/deflate levels only with a finite message encoding cap. Default and cap-only response paths retain their established behavior. The two shipping files are byte-identical to frozen 66034085. Final shipping source is 87f031d7; comparable backend/opt-out baseline is 8f009b94. Public fixtures are identical in both lanes.

The qualified functional count is 128 successful execution events across 127 distinct selected tests: 26 private cases at ed2e145, 49 affected native cases + 26 meaningful native regressions + 26 generated transport cases at 44bfcecb, then 1 affected producer-error replay at 87f031d7. Logs and source pins retain these distinctions. Generated tests cover all four RPC shapes over plaintext and mTLS. Genuine baseline semantic reds follow successful builds; original candidate fixture/lint failures remain preserved.

Native and standalone strict Clippy, strict rustdoc, actual 1.85 feature-off MSRV, actual 1.88 tonic MSRV, feature graphs, scoped available-stable rustfmt and whitespace checks passed. No Cargo/compiler or performance commands are needed to audit this proof.

Default IncomingBody and drain source sections are byte-identical, but the top call branch now evaluates the requested outbound settings predicate. Whole-default compiled-path identity, compiler CSE, zero instruction overhead, and matched default performance have not been established. Performance remains not_run.

The wrapper accounts its owned Vec reported capacities and keeps permits through Bytes clones/H2 retention. It cannot preempt or account private Tonic serialization, compressor internals, original/coalesced backing storage, or allocator slack. Force-identity may make Tonic's private cap stricter; the cap is never enlarged. Unsupported budgets/hooks remain fail-closed with status 9. Synchronous compression of one capped message is not preemptible mid-call. Parent TC-30b remains open.

Run `python3 check-proof.py` from this directory for the portable audit. Optionally add `--artifact-root /absolute/path/to/ordinary-qualification/artifacts` to verify every retained ELF, generated file and fingerprint against immutable manifests; `--rustdoc-archive /absolute/path/to/rustdoc-generated.tar.gz` verifies retained strict rustdoc output. Large binaries are retained locally rather than committed. All completed target caches were retired after preservation.

Plan snapshots retain their original prepared `not_run` states; command records and dispositions report actual executions. Formatter version capture used the rustup proxy plus version output, and does not claim a direct formatter binary before/after hash comparison.
