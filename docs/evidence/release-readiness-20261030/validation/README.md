# Local transport validation, 30 October

This capsule records bounded ownership and pointer-provenance repairs to the
native HTTP/2 transport. The final source is identified by the recorded hashes
and ordinary-file overlays on commit
`26389ccf77bf9342fe6e605f1d797853acd6c39a`. Earlier attempts use different source
states and remain separate. This is local validation, not a performance result,
build attestation or declaration of full production qualification.

## What changed

Queued outbound DATA owners are released after both transport guards have
been dropped, including reset, failure, shutdown and staging-panic paths.
Newly rejected DATA remains owned by an outer `Option` while validation borrows
it. A rejected owner's destructor can therefore reenter the connection without
trying to take locks still held by the rejecting send. A full `try_send_data`
continues to return the original buffer.

The expanded strict Miri controls found invalid cached stream pointers in
shipping runtime paths. The repair borrows individual slots without reborrowing
neighboring slots, checks stream identity before deriving a raw pointer, and
reacquires pointers after callbacks or same-slot reborrows. End-of-stream,
padded DATA and terminal-trailer controls exercise those paths. The embedded
Miri runner now includes the ownership regression group and still rejects an
empty test selection.

No speed improvement is claimed. Source changes introduce no new payload
clones or heap-allocation operations on accepted sends, but additional pointer
lookups can affect machine code. Arbitrary `Buf::chunk` and `Buf::advance`
callbacks, plus preaccept waker/capacity accounting on panic, retain their
separate boundaries.

## Final checks

The following completed labels use the reconstructed final source inputs.
Counts report Rust tests in each log; they are not a count of distinct tests
across overlapping native and Miri selections.

| Check | Result | Retained report label |
|---|---|---|
| Workspace tests, all features (partitioned coverage) | 3849 native tests passed across 152 targets; 6 ignored; 175 doctests passed; 3 ignored | `partitions-final` |
| Strict all-target Clippy and adapter libraries | Passed | `lint-retry-final` |
| Embedded transport safety kernels, strict Miri | 68 passed | `miri-retry-final` |
| Workspace binary and example builds | Passed | `examples-bins-final` |
| Documentation contracts | 27 passed | `docs-contract-final` |
| Rust formatting | Passed | `fmt-final` |
| Warning-free workspace rustdoc | Passed | `rustdoc-final` |

Ordinary completed check reports retain the command, rustc version text,
start/finish times, log hash and selected source input hashes.
`source_inputs_unchanged` must be true for every final label. The guarded
runner and its source-selection rules are retained in the archive.

The workspace coverage report is a recovered observation. Its outer guarded
wrapper was lost during an environment handover, while the partition runner
continued. The capsule records each completed Cargo command's exit status,
its logs and the runner's final coverage report. It does **not** assign an
exit status, elapsed time or continuous disk-guard monitoring to the missing
wrapper. Its observation timestamp is not a reconstructed launch time.

The final union covers every test executable in the workspace's all-feature
Cargo inventory, then runs all five library doctest targets. Thirty completed
targets / 426 passing tests are reused from the second interrupted run after
checking identical selected source inputs, rustc text and Cargo `fresh=true`.
Binary hashes were not recorded at that earlier execution. The partition
runner hashes the current inventory and every newly executed target, and
rejects binary or feature-graph drift. The checker independently parses the
retained suites, their target union, artifact identities and counts; archived
binaries are omitted, so it checks recorded digest consistency rather than
rehashing those executables. This is partitioned coverage, not a completed
one-shot workspace command.

## Detecting controls and earlier attempts

The initial queued-owner controls failed all three selected tests. The first
candidate attempts also retain compile errors and a fixture teardown failure;
the corrected intermediate candidate passed 16 retirement tests. The rejected
send extension added 12 controls: 11 detected destruction under the locks and
the full-try ownership control passed. Its baseline filter also included three
older passing controls, giving 4 passed / 11 failed in the complete failed log.
Its intermediate candidate passed 28 retirement tests.

Strict Miri then detected invalidation from debug pointer validation,
post-callback close checks, neighboring-slot mutable access, end-of-stream
capacity release, padding-credit release and terminal body discard. The
numbered `ready-provenance-*` logs retain the detecting failures, intermediate
passes and initial compile mistakes. Their accompanying summary and patch
hash report identify the final isolated handoff; the final integrated checks
above are the relevant source binding for this capsule.

The voluntary broad `streams::*` Miri run was interrupted after 92 seconds
inside an unrelated million-iteration budget test. It is **incomplete, not
passed**. Its log and explicit interruption record remain in the archive.
A focused passing run does not turn that interruption into a complete run.

Broader all-target Clippy also found two pre-existing test-only issues. The
retained `ready-test-lint` patch uses Tokio `try_lock` in the executor fixture
and evaluates an invariant assertion in a const block; the assertions and
drop-order controls remain. Initial lint failures are retained. The first
full native attempt stopped at the 750 MiB disk guard while building private
cold consumers and is incomplete. Its second attempt also stopped at the disk
guard, later in the suite, with 30 completed targets. That attempt used a guard that runs
`cargo clean` only on each of two private cold targets after the matching test
has printed `... ok`. The guard and per-case cleanup records are archived;
shared caches and source directories are outside that guard's cleanup allowlist.
Final partition commands clean the shared consumer target with `cargo clean`
only after the preceding Cargo command and all its test children return;
the exact cleanup commands and successful outcomes are retained.

The earlier `examples-final` attempt selected `--examples` alone, which matched
no targets in this workspace. It is retained as a no-op attempt and excluded
from the final checks. The accepted build selects both `--bins` and `--examples`.

Earlier core checks are scoped separately: the core library/runtime strict
Miri run records 125 + 26 passed on commit `26389ccf`; the upb fixture run
records 46 passed, and the shared-consumer run records 20 suites / 248 passed.
The earlier upb log compiled core 0.2.0 before the coordinated version update;
its fixture repair, source-boundary limits and initial compile failure remain
visible. These logs do not qualify the final native transport by themselves.

## Verify the capsule

From a checkout containing the base commit, run:

```sh
python3 docs/evidence/release-readiness-20261030/validation/check.py
```

The checker reads archive members without extracting or executing them. It
verifies every member digest, the three isolated patch digests, selected
report/log bindings, partition coverage and the explicit interrupted status. It reconstructs the
final selected source inputs from the pinned Git object plus archived ordinary
file overlays, then checks the final labels against those hashes.

The archive contains proof logs, reports, patches and the check runner. It
omits caches, executables, crate archives and credentials. Generated outputs,
all filesystem reads, registry source bytes, tool executable digests and the
complete host environment are not attested. The helper's selected input list
is recorded explicitly; it is not a claim to have traced every compiler or
documentation input. A member digest establishes internal integrity, not an
independent signature or trusted build provenance.

Sustained resource/recovery qualification, longer fuzz campaigns, exact-SHA
hosted release gates, signed-tag provenance and registry publishing remain
separate work. The prior `candidate-26389` package capsule is not duplicated
here. Later committed-source package rehearsals and resource smoke results
are recorded separately.
