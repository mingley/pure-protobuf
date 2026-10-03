# cc4 native / Go bounded protocol evidence

The cc4 candidate passed all executed frozen interop cases on their first
attempts: 46 cleartext, 46 TLS, and 18 native mTLS cases. The full mTLS
cross-language matrix remains **unqualified**. Its 36 Go rows are explicitly
unsupported, the scoped registry validator fails, the shell exits 1, and the
wrapper correctly retains `failed_or_incomplete`.

| Transport | Native self passes | Native → Go passes | Go → native passes | Unsupported rows | Shell exit |
| --- | ---: | ---: | ---: | ---: | ---: |
| Cleartext | 18 | 14 | 14 | 8 compression | 0 |
| TLS | 18 | 14 | 14 | 8 compression | 0 |
| mTLS | 18 | 0 | 0 | 28 base + 8 compression | 1 |

Each transport has exactly 54 unique case/peer/direction/transport rows. The
110 executed cases passed with one real attempt each, no failed attempts, and
no retry/flaky results. Unsupported rows carry the reporter's synthetic
`attempt_count: 1`, zero duration, and no process log; they are not executions.

The unchanged harness defaults are 15 seconds per attempt, two maximum
attempts, and five seconds for server startup. The wrapper clears inherited
case, timeout, attempt, TLS, `GOFLAGS`, and `GOMAXPROCS` overrides. Its recorded
runtime overrides contain no `GOMAXPROCS`; the build-only `GOMAXPROCS=1` and
`GOFLAGS=-p=1` are recorded separately. Each transport has an additional
1800-second process bound and a 2 GiB global free-space floor. No resource
guard fired.

## Qualification boundary

The native source is
`cc4db05383776ad39e8af34dee12a164b35ffcf2`, with Rust 1.85.0 debug artifacts
(`opt_level=0`). The Go peer is the frozen grpc-go commit
`dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef`, built with Go 1.25.3 and
`-mod=readonly`. The four selected ELFs are preserved exactly once. The
auditor checks their hashes, parses each Go ELF's `.go.buildinfo` section,
compares the retained module-info output, and verifies the exact union of 32
`(module, version, h1)` tuples against the frozen `go.sum`.

Both Go ELFs embed `CGO_ENABLED=1`. Before/after pins cover Go, compile, link,
asm, `go.mod`, and `go.sum`; they do not cover the cgo tool, system C compiler,
or libc. This is bounded compiler provenance and exact runtime-artifact
evidence, not a complete C tool or runtime dependency closure. The separate
`go mod verify` record retains argv, exit, output hashes and tuple checks,
but has no independent UTC/cwd/environment/pre/post pin envelope.

The docs-only branch starts directly at main
`b8a8bd9171f58b5ec67b487d34e6b78f284fb4f7`. cc4 is not its ancestor and differs
in compiled transport/client inputs. This package qualifies the isolated cc4
candidate, not that main revision. The wrapper's source, all four binary
pins, Go inputs, and tracked provider snapshot remained identical across all
three phases and the final snapshot.

In cc4, `client/call.rs` creates `send_request_when_ready`, rechecks cancellation
and deadline, refreshes the relative timeout, and polls admission through
`transport/h2.rs`. That wrapper reaches the embedded `h2_backend` module;
`SendRequestWhenReady::poll` calls the connection's `poll_send_request` before
the request is consumed and a stream is allocated. The archived admission
Rust fixtures describe the stronger zero-emission/wakeup oracles. F2 runs the
18 frozen interop cases; it does not execute those Rust oracle suites.

The unscoped `mtls/logs/report.json` says `overall_passed: true`, because the
separate aggregate command selects no suite/profile/matrix and unsupported
rows alone do not fail it. That original report is retained unchanged as
row diagnostics. The scoped registry validation errors, shell exit 1 and
wrapper status are authoritative for full-matrix qualification. The portable
audit replays that same validator and expects 28 missing Go base directions
and 56 validation errors. No status override is applied.

This package does not qualify release performance, a C++ matrix, a complete
mTLS matrix, TLS rejection scenarios, long soaks, or current-main runtime
behavior. It does not qualify or merge the separate Prost hook experiment.

## Retained evidence and replay

The streamed archive retains raw F2 stdout/stderr, every case/server log,
resource samples and final record; wrapper source; native build 037 metadata,
pins, JSON compiler output and stderr; the failed first Go build, additive
retry, preload and module verification records/logs; module-info outputs;
all 324 native source pins, frozen harness/registry/module inputs, proto/TLS
fixtures, source ancestry and the four selected ELFs. The cached API zip is
hash-pinned in the original preload record but is not duplicated; its
ziphash/mod/info inputs are included. Source members are verification
snapshots: six unrelated rpc-bench proto symlinks retain their plain Git link
payloads rather than becoming live archive links.

The provider advanced to a later documentation/Python-helper merge after F2.
Packaging retains the measured cc4 source label and recovers the original
helper from its exact cc4 Git blob, matching the native/runtime hashes. Three
packaging preflight failures and their original scripts/output are preserved;
all occurred before any archive write. The archive has 931 members, including
the first manifest, and 111,240,517 payload bytes. It was written without
binary staging or any build/cache mutation, then its existing compressed
stream was repartitioned in memory into parts strictly below 25 MiB.

The initial sealed auditor assumed the failed first Go build's pin dictionary
was as broad as the retry's; the first pinned only Go plus module files. Its
original source remains in the immutable archive and its red is retained as
`audit-first.*`. The corrected external auditor checks unchanged first-build
pins as an exact subset of the retry pins and has a separate hash in
`archive.json`. The corrected audit passes with exit 0. No raw build/runtime
record or threshold was changed.

Run the portable audit from any directory, using Python's standard library:

```sh
python3 path/to/final-native-go/audit.py
```

It reads the split archive directly, without extracting or executing the
ELFs, building Rust/Go, or running performance measurements. `SHA256SUMS`
pins the checked-in files; the first tar member pins all 930 remaining
members. The combined gzip SHA-256 is
`5cc136c84f795e31020988af0fac32b9c012da6ea3ab63b4dd1b29e1c9c9f424`.
