# RX-10a: bounded library-spawn diagnostic

The optional `copy-counts` feature now records four library spawn-expression
invocations, labelled by side and site: client connection driver, client bidi
upload, client server-stream send-half cancellation owner, and server RPC
dispatch. Each note precedes its existing, unchanged spawn expression. These are
attempt counts, including an attempt whose spawn panics; they do not count
successful scheduling, completed tasks, wakeups, or context switches. Reset/read
are process-wide diagnostic operations requiring an exclusive, quiescent
measurement process. Application producers and harness connection tasks are
reported separately.

An independent semantic baseline and the instrumented candidate each passed the
same 14 bounded current-thread duplex cases: four RPC shapes with echo, terminal
error, and cancellation, plus client-stream/bidi early terminal while an upload
is held. Handshake, four warmups, bodies, producer completion and cleanup all
have identical two-second fixture bounds; each diagnostic process has a
45-second outer bound. Setup creates one library connection-driver task. Every
warm case attempts one server dispatch; only bidi attempts a client upload
spawn, and only server-stream attempts the send-half cancellation-owner spawn.
The independent body/reply/terminal/drop oracle and root's separate audit agree.
Cleanup ends at zero live fixture tasks. The baseline explicitly reports library
counts as `not_run`.

| Qualification | Actual source | Result |
| --- | --- | --- |
| Baseline/candidate diagnostic | `1308bd98` / `f2638b90` | 14 cases each |
| Native default/core and enabled counter semantics, Rust 1.85 | `f2638b90` | 107 test executions; default scheduler symbols absent |
| Native strict all-target/all-feature Clippy, Rust 1.88 | `f2638b90` | pass with `-D warnings` |
| Registered RPC bin/worker/fairness, Rust 1.88 | `98272ce6` | 84 + 87 + 1 test executions |
| Devloop unit controls, stable Rust 1.99 | `98272ce6` | 18 test executions |
| Devloop strict all-target Clippy, Rust 1.88 | `6d6678cf` | pass with `-D warnings` |

Later commits corrected only the registered consumer lock/owner wiring and one
inherited assertion-format lint. All runtime/compiler-source changes and exact
source labels are retained; earlier passing captures are not renamed as runs on
later tips. The lock correction adds seven existing local dependency edges and
removes the inactive optional local `h2` edge. All 140 package tuples, including
136 registry tuples, remain fixed; registry `h2` and its edges remain unchanged.
The consumer includes the actual generated Timestamp owner. All 15 generated
output hashes match the earlier failed build. The formatting fix preserves the
assertion and diagnostic text without adding an allow or changing input work.
Both corrections were mirrored on the baseline branch.

The seven genuine Cargo exit-101 captures remain in the proof: missing pinned
upstream prerequisite, original stale RPC lock, three seven-edge-only locked
graph/diagnostic failures, missing generated owner (130 E0433 diagnostics), and
the inherited Rust 1.88 assertion-format lint. Two no-overwrite preflight
attempts did not launch Cargo. The missing-parent cache-retirement preflight and
first OFD fixture failure are also retained. Successful stable unit tests emit
expected caught panic diagnostics for deliberately rejected input fingerprints;
their actual result remains 18 pass, zero fail. No failure is replaced by a
different workload, toolchain, timing bound, or threshold.

The diagnostic is outside the original 20-cell registry. Structural replay
recovers the original spawn files and devloop registry/worker/helper/comparator
bytes after removing only the new feature guards and command wiring. The actual
Rust 1.85 default library has no added scheduler counter/state/API symbols;
existing CopyCounts symbols remain. Manifests, provider/devloop locks, profiles,
workers and original inputs remain fixed. Compiler captures use jobs=1,
offline/locked resolution, no incremental compilation, the frozen DEV/TEST
debug=0 profiles, exact tool hashes/version probes, direct source/tool pre/post
guards, and complete observed two-second resource samples. The fixed summed
owned-cache cap and global reserve are each 2,147,483,648 bytes. Recorded extrema
are samples, not unsampled peak guarantees.

Wakeups, channel sends, handoffs, context switches, instruction attribution,
performance and parent RX-10 budgets remain **not_run/open**. This leaf qualifies
the diagnostic and its bounded semantics; it does not establish a scheduler
budget, a runtime optimization, or a whole-library task attribution.

`records.tar.gz` contains 2,019 hashed metadata/source members: all 37 ordinary
captures, raw exits/stdout/stderr, before/after probes and inputs, periodic
resources, helper versions and Python-only tests, original/modified source
snapshots and patches, artifact selections, independent oracles, root audits,
generated-output evidence, and full cache manifests/removal records. Large
ELFs/rlibs and payload archives remain local with exact paths/hashes in
`index.json`. Directly launched binaries were Cargo-artifact-selected and copied;
the final Cargo unit-test ELF was preserved from its exact raw `Running
unittests` path after the run. That last gate has source/tool/compiler guards but
no separately recorded before/after test-ELF launch guard. Registry-cache bytes
and a complete dependency-header closure were not independently archived.

All four completed RX caches were preserved and retired after content, inode,
mode, ownership, link-topology and fresh process/lock checks. Full archives retain
all 7,989 regular paths, including 373 ELF paths (objects/shared libraries count),
fingerprints, build outputs, generated files, deps and rlibs. Early benchmark and
native1.85 retirements acquired classic POSIX lockf plus dedicated flock; closing
other same-inode descriptors can release classic POSIX locks, so continuous
POSIX protection is not certified. Their executed helpers/results are
immutable. Native1.88 and final benchmark retirements use independently tested
Linux OFD write locks plus dedicated flock. Visibility denials and point-in-time
process limits remain explicit. See the separate historical
[verify-cache preservation proof](../verify-cache-retirement-20261003/README.md).

The portable checker is read-only: it does not extract archives, execute helpers,
launch compilers, or delete files. Its default audit validates capsule hashes,
source/counter/test oracles, all retained failures, fixed resource samples, and
retirement metadata; it explicitly does not reread large payloads:

```sh
python3 docs/evidence/rx-10a/check.py
```

To additionally stream-verify every local cache payload byte, mode, ownership,
hardlink and symlink, run:

```sh
python3 docs/evidence/rx-10a/check.py --payload-root \
  /workspace/scratch/work/rx10/work/qualification/retirements/campaign_002
```
