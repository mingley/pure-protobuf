# Response status and endpoint counter checks

An incomplete response frame previously hid a non-OK server trailer behind
`INTERNAL`. The response reader now returns the peer's status, message,
details, and metadata across all four RPC shapes. Truncation still fails when
the terminal status is OK, missing, or malformed. Request framing is unchanged.
Binary logging also retains the metadata from that error trailer.

The raw capsule keeps both failing regressions, subsequent passing tests,
an intermediate compiler failure, and the benchmark lint failure and fix.
The original status fix is commit `3f7fbe7df9ceaaf0d08b8b2a791e92bde5ba2e53`.
The follow-up is identified by the working-tree patch and per-file hashes in
`manifest.json`; each final check also retains its own source patch.

The initial fix passed 699 native test executions: 14 response-status tests,
528 unit tests, 108 RPC/TLS/hostile/lifecycle/resource tests, and 49 optional
feature tests. These overlap with the follow-up's 46 binary-log/status tests.
The partial-frame fixtures exercise 16 non-OK cases and 24 framing-error
controls. The benchmark suite passed 188 test executions, including duplicate
module tests; its first strict lint run failed on the test's deliberate
blocking sleep, which now has a test-only lint expectation.

The API doc edits remove repeated comparisons and getter/setter prose.
For all 12 edited API files, non-doc-comment lines remain byte-equivalent;
`changes.json` records before/after hashes. Cargo manifest changes are comments.
The all-feature native lint and documentation checks pass with warnings denied.
The final benchmark and quiet resource results are recorded in
`work/readiness/final-status-gates/results.json` inside the capsule: all five
checks passed, including 11 allocation-enabled load-shape tests and 25 quiet
resource tests. The corrected scope passes a further strict benchmark lint,
22 Python guards, and 25 documentation checks. The documentation checks use
a retained prebuilt executable with unchanged test source; its hash and that
attribution limit are recorded. This is not a clean-source rebuild claim.

Optional Linux context-switch capture uses `getrusage(RUSAGE_SELF)` for each
endpoint. It counts voluntary and involuntary switches across live and exited
threads over the process lifetime, including setup. A controlled fork/exec
probe retained all 64 pre-exec sleeps, correcting the initial "since exec"
label before publication. An exited-thread regression and Python guards
check capture scope and validity. OS context switches do not measure Tokio
task wakeups or syscalls, and no scheduling noise band is established here.
The host C compiler confirms the `rusage` size, alignment, and counter offsets.
Miri's direct foreign call is unsupported and its failure is retained; an
isolated C-ABI mock passes the same caller's buffer, layout, conversion, and
failure-path checks. This does not establish Miri coverage of Linux libc.

Five unchanged-limit 30-second diagnostic replays passed after the previous
TLS bidi preview failure. They used a frozen dirty-source diagnostic executable
whose hash is retained; they are not clean-source qualification and do not
establish the original failure's cause. The failed source-pinned preview stays
in its original record. No 24-hour completion or fresh performance result is
claimed by this capsule. Streaming losses, remaining features, the full
per-side matrix, and production qualification remain open.

Run `python3 check.py` in this directory to verify every retained file without
extracting the archive. Executables and rebuildable compiler caches are omitted.
