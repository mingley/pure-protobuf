# Server regression checks

All 1,100 server tests pass serially on source `a9fe4c2d`, using the same Cargo-selected executable that failed four tests in the earlier parallel run. Each of those four tests also passes ten isolated repetitions. Deadlines, assertions and test bodies are unchanged. These results show sensitivity to concurrent execution; they do not establish the cause of the failures. The archive retains the original parallel failures and their timings.

The failures covered server deadline timing, cancellation of spawned work, and mutual-TLS wait-for-ready behavior. Earlier runs also failed assertions that required the old documentation wording. Those prose checks now verify API links. A byte comparison verifies that all other test code in health.rs, reflection.rs and serving.rs is unchanged. Health passes all 180 tests and reflection passes all 188 tests. Strict Clippy, formatting and Rustdoc checks pass; their exact commands and source revisions are retained.

This capsule contains commands, stdout, stderr, source pins, controllers, compiler-selected artifacts, executable hashes, original documentation assertions and code-preservation checks. Executable bytes remain local. Run `python3 check.py` to verify the archive inventory.

The remaining native integration targets are running separately. This evidence does not establish production readiness, complete feature coverage, performance dominance over tonic or a completed 24-hour soak.
