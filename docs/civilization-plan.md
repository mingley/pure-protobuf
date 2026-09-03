# Civilization plan

This file is the contract later agents execute. Do not treat it as optional
context. Do not replace it with a Distinct occupancy stretch, a smaller
roadmap, or a slogan.

**This document's job is done when agents can execute it without the
gRPC-kernel Distinct conversation.** Making the repo actually useful to
civilization is the work of those later agents, ticket by ticket below.

## 1. Objective (do not shrink)

Make `mingley/pure-protobuf` something a team that is not the author can
use to encode Protocol Buffers and speak gRPC **without a C toolchain**,
with **official Google protobuf conformance**, **cross-language gRPC
interop**, **always-verify TLS**, and **honest performance claims**.

Civilization usefulness is three things at once:

1. **Adoptable.** A stranger can depend on the crates from git or crates.io,
   generate stubs, and run a service from a short README.
2. **Proven.** Official conformance, grpc-go interop, and benches that name
   host and method remain green. Numbers are captured, not invented.
3. **Still-safe.** No C compiled into the gRPC/TLS build. No skip-verify.
   No optional mTLS. Hostile-peer HTTP/2 limits stay enforced.

It is **not** more Distinct sentences. Occupancy documentation is a ledger
of what this kernel is not; it is not a product.

## 2. What this repo already is

Ground these facts before changing anything. They are true of the tree as
of the commit that added this file.

### Crates

| Crate | Path | Role | Publish |
|---|---|---|---|
| `pbrs` | workspace root | Pure-Rust protobuf kernel. Google protobuf v4 application API (`Parse`, `Serialize`, `Clear`, `proto!`, `ProtoStr`, `DynamicMessage`, …). Not crates.io `protobuf` 4.x (upb/C), not prost, not pb-rs. | **publishable** (`include` list, registry metadata, no `publish = false`) |
| `pbrs-grpc` | `pbrs-grpc/` | HTTP/2 gRPC kernel over pbrs. **No tonic.** TLS: rustls + Graviola (no `aws-lc-rs` / `ring`). `unsafe_code` deny in kernel modules. | `publish = false` |
| `protobuf-tonic` | `protobuf-tonic/` | tonic 0.14 adapter that swaps pbrs message types. **Does not depend on `pbrs-grpc`.** Escape hatch for tower/tonic shops. | `publish = false`; path dep on `pbrs`; rust-version **1.88** (kernel MSRV is **1.85**) |
| `pbrs-grpc-example-greeter` | `examples/greeter/` | Worked user crate: own proto, `build.rs`, health, reflection, four RPC shapes. | `publish = false` |

Workspace members: `.`, `protobuf-tonic`, `pbrs-grpc`, `examples/greeter`.
Excluded benches and throwaways: `bench`, `tonic-bench`, `rpc-bench`,
`v4_*`, `buffa_*`, `prost_tat`, `rust_out_*`, `grpc_remap`, `parse-leftover`.

`pbrs` does not depend on `pbrs-grpc`. `pbrs-grpc` does not depend on
tonic or `protobuf-tonic`. Use one, the other, or neither.

### What already works

- Official protobuf conformance v35.1 (`./scripts/conformance.sh`):
  required ×2 5631 binary+JSON + 909 text, 0 unexpected;
  `--enforce_recommended` same; **no skip list**. CI job `conformance`.
- Official gRPC interop (`./scripts/grpc-interop.sh`): kernel↔kernel,
  kernel client → grpc-go server, grpc-go client → kernel server.
  CI job `grpc-interop`.
- Codegen: `pbrs::codegen::compile_protos` / `protoc-gen-pbrs`.
  **`protoc` must be on PATH.**
- Kernel serving: unary, client-stream, server-stream, bidi; `Router`;
  h2c and TLS+ALPN `h2`; mTLS (required client cert, not optional);
  gzip; health; reflection; interceptors; typed `google.rpc.Status`;
  Unix sockets; `Channel::from_io` / `Server::serve_connection`;
  hostile-peer HTTP/2 tests (`pbrs-grpc/tests/hostile.rs`).
- Codec benches (`./bench`, `docs/benchmarks.md`): line of record **#31:
  52.2 vs 25.8 ns hello combined** on Apple M4 Pro. Do not write other
  VM numbers into `docs/status.md`.
- Loopback RPC: `rpc-bench` process-gates kernel median ns **strictly
  below tonic 0.14** on `empty_unary` / `large_unary`. Streaming gated
  at ~90% of tonic. `scripts/grpc-server-bench.sh` is loopback vs
  grpc-go, not a replacement for the Xeon tables.
- Closed parse-path experiments live in `docs/inventory/`. Do not revive
  them (#39 flatten, #57 heap-copy, and the rest of that table).

### Unique wedge

The reason this repo can matter to civilization is a **rustc-only**
protobuf + gRPC stack:

- No C compiler in the gRPC/TLS build (Graviola, `miniz_oxide`, no
  `aws-lc-rs`, no `ring`, no upb).
- Official Google protobuf conformance with zero unexpected failures.
- Speaks real gRPC with grpc-go, not a same-process toy.
- TLS always verifies; ALPN `h2` is required; there is no skip-verify
  constructor and no `assume_http2`.
- Hostile peers are assumed. Limits are tested with raw HTTP/2.

A second tonic clone, or a prost clone that still needs C crypto, is not
the wedge. Keep the wedge.

## 3. What still blocks a stranger

These are the honest gaps. Later agents close them in ticket order.
They do not close them by documenting that they are Distinct from tonic.

1. **Not on crates.io.** Root `README.md` shows `pbrs = "0.1"` with a git
   fallback. No git tags, no CHANGELOG. `pbrs-grpc` and `protobuf-tonic`
   have `publish = false`. Path deps block publishing the adapter.
   `docs/grpc.md` “What is not here” already names this.
2. **Human onboarding is unreadable.** `pbrs-grpc/README.md` and
   `examples/greeter/README.md` are occupancy ledgers. A newcomer cannot
   start from them. **Hard constraint:** `pbrs-grpc/tests/serving.rs`
   `include_str!`s `pbrs-grpc/README.md` and currently asserts **414**
   `readme.contains(...)` occupancy strings. You cannot delete those
   sentences. You relocate them (see ticket A2).
3. **`protoc` on PATH** for codegen. Conformance uses cmake `protoc` from
   the v35.1 pin; the `test` CI job still apt-installs
   `protobuf-compiler` for plugin / `protobuf-tonic` `build.rs`.
4. **Serving omissions that actually block some production replacements**
   (from `docs/grpc.md` “What is not here”): no load balancing / discovery
   (one authority; hold a `Channel` per backend), no application-transparent
   retries/hedging beyond at-most-once connection-death retry, no
   `GetState` / channelz / binarylog / `grpc.stats` / OpenTelemetry, no
   xDS or dns resolvers, no compressor plugins, no grpc-web / HTTP/1.1.
   Several of these are **deliberate**. Do not implement them just because
   tonic has a setter. See §6 filter.
5. **Proof hygiene.** `docs/benchmarks.md` already contains 4-core Xeon
   tables vs tonic and vs grpc-go. **Do not invent or replace those
   cells.** Reproduce on named hardware if you add a new dated subsection.
   Loopback `rpc-bench` / `grpc-server-bench.sh` numbers are not the Xeon
   unary tables; the file already says so.
6. **Fuzz is light.** `tests/fuzz_parse.rs` is an in-tree corpus, not
   oss-fuzz.

## 4. Invariants (never violate)

If a ticket would break one of these, the ticket is wrong.

- No C compiled into the gRPC/TLS build. Do not add `aws-lc-rs`, `ring`,
  `cc` for crypto, or upb. Graviola only.
- `unsafe_code` stays denied on kernel modules. Do not add `unsafe` to
  `pbrs-grpc/src/`.
- No skip-verify TLS constructor. No optional client auth
  (`client_auth_optional`). No rustls `KeyLogFile` / `SSLKEYLOGFILE`.
  No `assume_http2` (ALPN `h2` is required after TLS).
- Unimplemented methods stay `UNIMPLEMENTED`. Drain always waits. Extra
  RPCs past `max_concurrent_rpcs` are `RESOURCE_EXHAUSTED` via
  `try_acquire` (not wait).
- Do not implement tonic adaptive window.
- `ChannelConfig` and `ServerConfig` stay `Copy`.
- Do not rustfmt markdown.
- Do not invent Xeon (or any other) benchmark cells. Do not overwrite
  the #31 codec line of record with another host's numbers.
- Do not revive closed inventory experiments (`docs/inventory/`).
- Do not add tower as the kernel. Tower lives in `protobuf-tonic`.
- Covering `pbrs-grpc` serving tests `include_str` many sources
  (`client.rs`, `tls.rs`, `health.rs`, `interceptor.rs`, `config.rs`,
  `lib.rs`, `server.rs`, `docs/grpc.md`, `docs/architecture.md`,
  `docs/status.md`, `pbrs-grpc/README.md`, `docs/benchmarks.md`, …).
  **Do not edit those files while a covering serving compile is
  running** (~90–100s). If you change a pinned file, you must keep or
  relocate every asserted substring.

## 5. Non-goals

Do not do these. They look like progress and they are not.

- Continuing the Distinct / occupancy treadmill as success.
- Implementing skip-verify, optional mTLS, key log, `assume_http2`,
  adaptive window, or fake `ChannelConfig` fields that would break `Copy`.
- Publishing `protobuf-tonic` against a path dependency.
- Cookie-cutting tonic/grpc-go DialOptions into this kernel so the
  omission table can shrink.
- Flattening `merge_inner`, heap-copy ProtoString experiments, or any
  other closed inventory item.
- Writing other VM numbers into `docs/status.md`.
- Redefining “world-class performance” around loopback-only numbers
  while claiming Xeon vs tonic **and** grpc-go without a capture.

## 6. How to execute (agent protocol)

1. **Read this file and the current tree.** Conversation memory is not
   authoritative. `git status`, `Cargo.toml`, `docs/grpc.md` “What is not
   here”, and CI are.
2. **Pick the first open ticket in §8** whose prerequisites are met. One
   ticket per change-set unless two files must move together (A2 pin
   relocation).
3. **Apply the Unique + safest filter** before writing protocol code:
   - Unique: this kernel does not already do it, and tonic/grpc-go having
     a setter is not enough.
   - Safest: it does not weaken TLS, ALPN, hostile-peer limits, or
     `UNIMPLEMENTED` / drain / `try_acquire` semantics.
   - If it fails the filter, document it in “What is not here” only if
     that row is genuinely missing; do not plant occupancy on crowded
     planters.
4. **Gates before claiming a ticket done.** Run the commands in that
   ticket. Paste or record the evidence (exit codes, crate versions,
   host strings). Weak evidence (search found a similar string) is not
   done.
5. **Do not mark civilization complete** until §9's audit passes against
   current state. This file existing is not that audit.

### Crowded planters (kernel docs)

If you must touch kernel Distinct docs at all, do not pile more occupancy
onto: `Channel` type, `Channel::connect` / `connect_tls` / `from_io` /
`connect_lazy` / `origin` / `send_compressed` / `intercept`, `ClientTls`
type, `ClientTls::webpki`, `ClientTls::ca`, `ServerTls` type,
`ServerTls::new`, `ServerTls::mtls`, `Server` type, `HealthReporter`,
`ClientInterceptor`, `Target`, `ChannelConfig` type /
`max_connection_idle` / `initial_stream_window_size` /
`max_concurrent_rpcs`, `ServerConfig::max_concurrent_rpcs`,
`Code::is_retryable`. Prefer not to touch them. Civilization tickets
below mostly do not need them.

## 7. Workstreams

### A. Human onboarding

**Why civilization cares.** A rustc-only gRPC kernel that only its author
can start is a research artifact. Greeter and crate READMEs must be
getting-started docs again.

**Do.** Restore short user docs. Keep Distinct occupancy in rustdoc and
`docs/grpc.md` (already serving-pinned). Greeter README is **not**
`include_str`'d by serving and can be rewritten.

**Don't.** Delete occupancy sentences from `pbrs-grpc/README.md` without
relocating every serving assertion. Do not rustfmt markdown.

### B. Distribution

**Why civilization cares.** `pbrs = "0.1"` in the root README is a claim
the registry does not currently back. Path deps mean the kernel cannot
be published as-is.

**Do.** Prepare `pbrs` for crates.io (`cargo publish --dry-run -p pbrs`),
add CHANGELOG and a git tag scheme, then publish **only if** a crates.io
token exists. After `pbrs` is on the registry, switch `pbrs-grpc` from
path-only to a versioned dependency and drop `publish = false` when that
crate is actually publishable (license, readme, include set, no
workspace-only paths). Leave `protobuf-tonic` unpublished until it
depends on a registry `pbrs`, not a path.

**Don't.** `cargo publish -p protobuf-tonic` against `{ path = ".." }`.
Don't publish `pbrs-grpc` while it still cannot be built from crates.io
artifacts alone.

### C. Proof

**Why civilization cares.** Conformance and interop are why a government,
distro, or air-gapped shop can trust the codec and the RPC framing.
Benches without host/method are marketing.

**Do.** Keep CI jobs `test`, `grpc-interop`, and `conformance` green.
When adding numbers, name host, rustc, date, and command. Reproduce Xeon
tables only with real captures; never overwrite #31.

**Don't.** Invent cells. Mix loopback `rpc-bench` with Xeon unary tables.
Skip `--enforce_recommended`. Add a conformance skip list.

### D. Production-shaped gaps

**Why civilization cares.** Some teams cannot replace tonic until they
have a documented multi-backend story, retries at the call site, and
observability they can hook.

**Do.** Cookbook, not fake xDS: hold a `Channel` per backend. Document
application retries using `Code::is_retryable` (`UNAVAILABLE` only).
Interceptors already observe `Outgoing` / `Rpc` / `Status`; that is the
supported observability hook. Implement a missing protocol feature only
if it passes Unique + safest **and** a real caller needs it.

**Don't.** Implement GetState, channelz, binarylog, OTel, xDS, dns
resolvers, compressor plugins, grpc-web, optional mTLS, or tower layers
“for completeness.”

### E. Durability

**Why civilization cares.** The wedge dies if conformance regresses, if
C crypto sneaks back, or if parse experiments that already lost are
retried.

**Do.** Keep `./scripts/conformance.sh` and `./scripts/grpc-interop.sh`
in CI. Keep `tests/hostile.rs`. Optionally graduate `tests/fuzz_parse.rs`
toward cargo-fuzz / oss-fuzz **without** weakening ParseError handling.
Leave `docs/inventory/` closed.

**Don't.** Merge inventory diffs. Relax hostile limits. Add C.

## 8. Ticket queue

Execute in order. Mark a ticket done in a follow-up commit to this file
only after that ticket's **Evidence** exists in the tree or on the
registry, not after intending to do it.

### A1. Restore the greeter as a getting-started

**Files.** `examples/greeter/README.md` (primary). Optionally a short
pointer in root `README.md`. Do not touch `pbrs-grpc/README.md` here.

**Work.** Rewrite the greeter README to a short human guide: what the
crate is, `cargo run -p pbrs-grpc-example-greeter`, proto location,
`build.rs`, health/reflection, tests. Remove occupancy / Distinct walls.
Keep `src/lib.rs` behavior.

**Gate.**

```bash
cargo run -p pbrs-grpc-example-greeter
# prints: hello world
cargo test -p pbrs-grpc-example-greeter
```

**Evidence.** First screen of `examples/greeter/README.md` is a
getting-started (command + expected output) with no “Distinct from”
sentences. The run still prints `hello world`.

**Stop.** Do not “fix” greeter by adding more Distinct paragraphs.

### A2. Relocate `pbrs-grpc` README occupancy so humans can start

**Files.** `pbrs-grpc/README.md`, `pbrs-grpc/tests/serving.rs`, and
whichever destination still keeps every asserted substring
(`pbrs-grpc/src/lib.rs` rustdoc and/or `docs/grpc.md`).

**Work.** Split the crate README into a short quickstart (git/crates.io
dep, `compile_protos`, implement trait, `serve`, `connect`). Move the
occupancy ledger to a destination serving already pins, **or** introduce
a dedicated file (for example `pbrs-grpc/OCCUPANCY.md`) and retarget
`include_str!("../README.md")` assertions to that file. Every one of the
414 `readme.contains` strings must still exist in whatever serving
reads.

**Gate.** After the move, before pushing:

```bash
# Do not edit include_str sources while this runs.
cargo test -p pbrs-grpc --test serving --offline
cargo doc -p pbrs-grpc --offline --no-deps
```

If only README/guide strings moved, a covering serving job is still
required because serving compiles those files into the test binary.

**Evidence.** A newcomer can copy the first ~40 lines of
`pbrs-grpc/README.md` and get a service running. `rg -c 'readme.contains' pbrs-grpc/tests/serving.rs` is not the success metric; human-readable
quickstart **plus** zero dropped pins is.

**Stop.** Do not drop a pin to make the README pretty. Do not plant new
Distincts while relocating.

### B1. Publish-ready `pbrs` (dry-run)

**Files.** Root `Cargo.toml` (`[package]` metadata), `README.md` install
stanza, new `CHANGELOG.md` if you add one, `LICENSE-*` (already present).

**Work.** Confirm `include` covers what the crate needs to build from a
registry tarball (`src/**`, `build.rs`, `vendor/google/conformance_fds.bin`,
licenses). Fix anything `cargo publish --dry-run` reports. Keep
`rust-version = "1.85"`. Do not publish yet.

**Gate.**

```bash
cargo publish --dry-run -p pbrs
```

From a **temporary directory** that is not this checkout, document the
git dep that already works:

```toml
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
```

**Evidence.** Dry-run exits 0. README distinguishes “registry version”
vs “until published, git”. No `publish = false` on `pbrs`.

**Stop.** Do not bump to 1.0. Do not include excluded bench crates.

### B2. crates.io publish of `pbrs` (credentials required)

**Prerequisites.** B1 done. A crates.io API token in the environment.
Owner access to the `pbrs` crate name on crates.io (verify the name is
this crate, not a typo-squatter; abort if the name is taken by someone
else).

**Work.** `cargo publish -p pbrs`. Tag `v0.1.0` (or the version you
actually published) on the exact commit. Update root README to the
registry dep without a lying un-published version.

**Gate.** `cargo info pbrs` (or crates.io page) shows the published
version. A throwaway crate with only `pbrs = "<that version>"` builds.

**Evidence.** Registry version string recorded in `CHANGELOG.md` and
README. Git tag pushed.

**Stop.** If there is no token or the crate name is not ours, **leave B2
open**. Ready-to-publish is B1; do not fake a registry listing.

### B3. Versioned `pbrs-grpc` dependency; publish when actually publishable

**Prerequisites.** B2 done (registry `pbrs`). Otherwise keep git/path
deps and skip publish.

**Work.** Point `pbrs-grpc` at a versioned `pbrs`. Add package `readme`,
`repository`, `include` as needed. Remove `publish = false` only when
`cargo publish --dry-run -p pbrs-grpc` succeeds from that versioned dep.
Inter-crate git checkout of a subdirectory package must still work for
people who are not on crates.io.

**Gate.**

```bash
cargo publish --dry-run -p pbrs-grpc
cargo test -p pbrs-grpc --offline
```

**Evidence.** A stranger's `Cargo.toml` can depend on `pbrs-grpc` from
git **or** crates.io without this monorepo path. `protobuf-tonic` still
must not be published on a path dep.

**Stop.** Do not publish the tonic adapter here. Do not add tonic to
`pbrs-grpc` to make dry-run easier.

### C1. Keep the official gates green

**Work.** No feature work required unless CI is red. If red, fix the
regression; do not skip lists.

**Gate.**

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps   # RUSTDOCFLAGS='-D warnings'
./scripts/grpc-interop.sh
./scripts/conformance.sh
```

CI already runs these (conformance uses cmake `protoc` from the pin, not
apt). Local interop needs a Go toolchain for the grpc-go passes;
`--self-only` skips Go and is **not** a substitute for the CI job.

**Evidence.** `main` CI: `test`, `grpc-interop`, `conformance` all green.

### C2. Honest bench captures only

**Files.** `docs/benchmarks.md` only when you have a real capture.
Never `docs/status.md` for other-host numbers.

**Work.** If you have access to documented hardware, run:

```bash
cd rpc-bench && cargo build --release && ./target/release/rpc-bench
# and/or
./scripts/grpc-server-bench.sh
```

Record host CPU, OS, rustc, date, command. Add a **new dated
subsection**. Do not overwrite existing Xeon tables or the #31 M4 codec
line.

**Evidence.** New subsection names host and method. Process gates still
match what `rpc-bench` actually enforces (kernel empty/large unary
strictly below tonic 0.14 on that run).

**Stop.** Do not fill empty cells from memory. Do not claim “faster than
grpc-go on Xeon” from loopback ping-pong.

### D1. Cookbook: several backends, retries, observability

**Files.** A short `docs/cookbook.md` (new) linked from root README Docs.
Do not implement xDS.

**Work.** Show:

- One `Channel` per resolved backend address (`ChannelConfig::connections`
  pools to one authority).
- Application retry loop using `Status::is_retryable` / `Code::is_retryable`
  (`UNAVAILABLE` only). Packed `RetryInfo` is a wait hint, not a larger
  set. `RESOURCE_EXHAUSTED` from `max_concurrent_rpcs` is not retryable.
- Interceptors as the observability hook (`Outgoing` / `Rpc` / `Status`),
  with an explicit note that this is not OpenTelemetry, not channelz, not
  binarylog.

**Gate.** Cookbook examples compile (doctest or a tiny example). Links
from README.

**Evidence.** A stranger can copy the cookbook without reading occupancy
grammar.

### D2. Protocol features (only if Unique + safest + a real caller)

**Default: skip this ticket.** Open it only when a user of the crate
needs a missing capability that is not already “hold a Channel per
backend” or “retry at the call site.”

If opened, name the caller, the Unique hole, and the safety argument in
the commit. Still forbidden: skip-verify, optional mTLS, key log,
`assume_http2`, adaptive window, C crypto, tower-as-kernel.

### E1. Optional fuzz upgrade

**Prerequisites.** A1–C1 in good shape. Not a blocker for adoptability.

**Work.** If pursued: cargo-fuzz targets around `Parse` of TAT/Person and
gRPC frame prelude, no `unsafe`, crashes are bugs, `ParseError` is not.

**Stop.** Do not replace official conformance with fuzz. Do not disable
hostile tests to “speed CI.”

## 9. Completion audit (civilization usefulness)

Later agents: treat completion as **unproven** until every line below has
current-state evidence. The existence of this plan is not evidence.

| Requirement | Authoritative evidence |
|---|---|
| A stranger can depend on `pbrs` without this checkout | crates.io version **or** a documented git dep that `cargo build`s in a temp crate |
| A stranger can depend on `pbrs-grpc` the same way | git and/or crates.io; `publish = false` removed only if dry-run succeeded |
| `protobuf-tonic` is not published on a path dep | `Cargo.toml` still `publish = false` until versioned `pbrs` |
| Getting-started works | `examples/greeter/README.md` is human; `cargo run -p pbrs-grpc-example-greeter` prints `hello world` |
| Kernel README is human | First screen of `pbrs-grpc/README.md` is quickstart; serving pins relocated, not dropped |
| Conformance | CI `conformance` green; v35.1; 0 unexpected; no skip list |
| Interop | CI `grpc-interop` green (kernel↔kernel and both directions vs grpc-go) |
| Safety wedge | No `aws-lc-rs`/`ring` in `pbrs-grpc` graph; no skip-verify API; ALPN `h2` required; `tests/hostile.rs` still present |
| Honest benches | `docs/benchmarks.md` cells not invented; #31 line of record intact; new numbers named host+method |
| Inventory stays closed | `docs/inventory/` not merged into `src/` |
| Cookbook for real production gaps | `docs/cookbook.md` (or equivalent) covers multi-backend + retry + interceptor observability |

If any row is missing, incomplete, or only “consistent with” completion,
keep working. Do not redefine success as “more Distincts” or “plan was
written.”

## 10. Pointers

| Thing | Where |
|---|---|
| Root install / docs index | `README.md` |
| gRPC guide + omission table | `docs/grpc.md` |
| Architecture | `docs/architecture.md` |
| Codec + RPC benches | `docs/benchmarks.md` |
| Verified status (facts, not plans) | `docs/status.md` |
| Closed parse experiments | `docs/inventory/README.md` |
| Conformance | `./scripts/conformance.sh`, `./scripts/fetch-protobuf.sh` |
| Interop | `./scripts/grpc-interop.sh` |
| grpc-go server bench (loopback) | `./scripts/grpc-server-bench.sh` |
| CI | `.github/workflows/ci.yml` |
| Hostile peer tests | `pbrs-grpc/tests/hostile.rs` |
| Serving pin ledger | `pbrs-grpc/tests/serving.rs` (`include_str` + `contains`) |
| Greeter | `examples/greeter/` |
| tonic escape hatch | `protobuf-tonic/` |
