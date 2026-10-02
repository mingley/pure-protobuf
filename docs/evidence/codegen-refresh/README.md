# Integrated codegen correctness acceptance

Reviewed on 2026-10-02 for GN-12, GN-13 and GN-14. The enum-option correction,
scoped implementation lints and safe regeneration are correctness repairs;
these results make no performance claim.

The regenerated runtime source is `bbe9f6fb`. The complete workspace source is
`d10305e4`, which additionally updates bundled-WKT assertions to require the
existing streaming text parser used for fresh output. The acceptance worktree
has the identical Git tree `ce969adb36ff921da5fd7dda4b670b3e36d8e17d`.
Exact commands, source pins and limits are recorded in
[qualification.json](qualification.json).

| Actual check | Result | Raw evidence |
|---|---|---|
| Locked workspace tests | 3,263 passed across 120 successful suites; zero failures, five existing ignores | [output](final-workspace.txt) |
| Strict workspace Clippy, all targets and features | Passed with warnings denied | [output](final-clippy.txt) |
| Strict workspace rustdoc | Passed with warnings denied | [output](final-rustdoc.txt) |
| Workspace formatting | Passed | [output](final-format.txt) |
| Pinned v35.1 conformance, maximum Edition 2023 | Required twice and recommended: each 5,631 binary/JSON plus 909 text successes; no skips or failures | [output](final-conformance.txt), [summary](conformance-summary.json) |
| Non-mutating bundled regeneration check | All 13 bindings match | [output](final-main-regen-check.txt) |
| Original upstream shared consumers | 19 original crates, 233 tests plus six regressions passed; seven documented exclusions retained | [output](final-upstream-shared.txt) |
| Pinned adapter descriptor test, explicitly ignored in the default suite | Passed separately with genuine protoc 35.1 | [output](final-adapter-descriptors.txt) |
| Public semver against published pbrs 0.2.0 and adapters 0.1.0-alpha.2 | Each package: 196 checks passed, 58 tool skips; no semver update required | [pbrs](final-semver-pbrs.txt), [grpc](final-semver-grpc.txt), [tonic](final-semver-tonic.txt) |
| Plan and regeneration safety contracts | 22 tests passed; all 388 cards and 32 check registrations validate without cycles | [tests](final-plan-regeneration-tests.txt), [lint](final-plan-lint.txt) |

Workspace coverage includes strict generated consumers, packaged consumers,
onboarding, core/descriptors, all 17 bundled-WKT tests, generated JSON/text,
native RPC and tonic adapters. The five default-suite ignores are one adapter
descriptor test run explicitly above, two Edition 2024 tests requiring compiler
36.1, and two existing rustdoc snippets. Edition 2024 and excluded upstream
internals are not counted as passing this bounded profile.

The full workspace gate was run outside the checkout's `work/` directory.
In-repository toolchain and worktree scratch otherwise makes the documentation
scanner discover third-party README files. The scanner was preserved and the
same committed tree was tested in a clean external worktree.

The first post-regeneration workspace run exposed four stale bundled-WKT
assertions that expected intermediate-tree parsing. The updated assertions
require the same streaming `TextReader` paths as fresh generation. The final
complete run passes; generated bindings were not hand-edited.

The separate registered legacy codec smoke failed with four losses before
regeneration and three afterward. Both failed measurements are retained
([before](codecbench.txt), [after](final-codecbench.txt),
[final release build](final-codecbench-build.txt)); they are shared-host wall-time
diagnostics and do not qualify performance. A single
[quiet rerun](quiet-codecbench.txt) without concurrent compilation or benchmark
replays also exits 1, retaining two losses: strings versus google-protobuf 0.36
and buffa view. No threshold was changed and no further retries were used to
select a passing result. The registered RPC benchmark checks
passed 93 bin tests, 95 worker tests and one fairness test
([output](rpcbench-tests.txt)).

These are locally verified results. They do not assert a completed GitHub CI
run, a broader Edition 2024 profile, or a qualified map performance improvement.
