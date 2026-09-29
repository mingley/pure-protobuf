# GN-08 evidence: pinned frontend differential corpus and harness

Branch: `mingley/gn08-frontend-corpus`, base `1a86bb57`. Card: GN-08
("Build the pinned frontend differential corpus and harness").

This records the corpus and baseline harness, not a passing Rust frontend.
The observed runs used a prepared compiler cache. On a fresh checkout,
`build-pinned-protoc.sh` can fetch upstream sources and build the reference
compiler; the offline comparison boundary begins after that setup.

## Accept verdicts

1. **Corpus pins record upstream commits and licenses; the protoc baseline
   is reproducible.** PASS. `corpus.json` records URL + commit SHA +
   SPDX license per upstream (table below); vendored bytes are hash-pinned
   in the manifest and verified before every run; the baseline compiler is
   `scripts/build-pinned-protoc.sh` (pinned `v35.1`); two consecutive
   baseline runs produce byte-identical `summary.json`; regen via
   `fetch-corpus.sh` leaves the tree unchanged; comparison reuses vendored
   inputs and a prepared pinned compiler.
2. **The harness runs with no frontend present (baseline only).** PASS.
   `./scripts/frontend-diff.sh --corpus pinned` exits 0 with 202 entries x
   3 modes = 606 checks, 0 failures, and no `--frontend` flag.

## Pins (from corpus.json `repos`)

| Leg | URL | Commit | License |
|---|---|---|---|
| protobuf | github.com/protocolbuffers/protobuf | `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03` (`v35.1`) | BSD-3-Clause |
| googleapis | github.com/googleapis/googleapis | `5d2a5100759be0b6fe5a1d3ce5e025d53d8283f4` | Apache-2.0 |
| envoy | github.com/envoyproxy/data-plane-api | `e23a28a41013518863542bdd7e8daba568624bc4` | Apache-2.0 |
| otel | github.com/open-telemetry/opentelemetry-proto | `a8951735f7801e8adfaec5c0ace9262771cfec6e` (`v1.9.0`) | Apache-2.0 |
| grpc | github.com/grpc/grpc-proto | `813330824839bfdd3abc52f41807095c0de2ec19` | Apache-2.0 |
| xds (support) | github.com/cncf/xds | `dba9d589def2cd10099a3a64887d859188c2f57a` | Apache-2.0 |
| pgv (support) | github.com/envoyproxy/protoc-gen-validate | `414042a5ff2e98dc47f8161937316a25b1da5bba` | Apache-2.0 |

Edition 2024 fixtures are in-repo (`tests/fixtures/edition2024/`), hashed
into the manifest, no upstream fetch.

## Observed check outputs (2026-09-29, macOS, this worktree)

```text
$ ./scripts/frontend-diff.sh --corpus pinned
frontend-diff baseline: 202 entries x parse,link,full (606 checks, 0 failures)
protoc: libprotoc 35.1 (v35.1 35cd01f9fe9a)
```

- Entry mix: 194 expect-ok (protobuf 80, googleapis 39, grpc 26, envoy 22,
  otel 10, edition2024 9, xds 6, pgv 1, edition2024-rejected support 1) and
  8 expect-fail (edition2024 rejected set; `visibility_defs.proto` is the
  support file and compiles standalone under both protoc 35.1 and 36.2).
- `visibility_import_local.proto` fails with the true visibility error
  (entry carries repo-root include for its repo-relative import), not a
  file-not-found artifact.
- Determinism: second run to a different `--out` dir, `cmp` of the two
  `summary.json` files: identical.
- Compare mode: a protoc-exact shim frontend passes 606/606; an
  always-exit-0 frontend is reported (`FAIL verdict ... frontend=ok
  protoc=fail`, exit 1).
- Exit codes: mismatch 1; bad `--entry`/missing `--corpus`/bad
  `--frontend` 2; tampered vendored byte trips source-integrity, exit 2.
- `sh -n` + `bash -n` clean on both shell scripts (`shellcheck` is not
  installed here). No Rust touched: clippy/fmt not applicable.
- Full baseline runtime ~90 s serial; `--mode`/`--entry` narrow it.

## Files

- `scripts/frontend-diff.sh` (new): CLI + pinned-protoc build + driver exec.
- `tests/frontend_corpus/corpus.json` (new, generated): pins, roots, entries.
- `tests/frontend_corpus/diff.py` (new): baseline/compare driver.
- `tests/frontend_corpus/fetch-corpus.sh` (new): pinned regen script.
- `tests/frontend_corpus/proto/` (new): 184 vendored `.proto` files.
- `tests/frontend_corpus/README.md`, `tests/frontend_corpus/EVIDENCE.md`.
