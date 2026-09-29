# EX-18a: Rust ALTS record-layer review

Date: 2026-09-29. Base SHA:
`016898c13e4d215c6a7056b0b9edd097b18e2754`. Starting point:
[the ALTS contract](../alts-contract.md) (EX-18 boundary decision).

**Verdict: BLOCKER — no maintained pure-Rust ALTS record/handshake
option qualifies.** Nothing is selected, no dependency is added, and
no rewrite is commissioned. EX-18b and EX-19 stay gated.

## 1. Survey method (dated evidence, 2026-09-29)

All queries below were run fresh for this card; the coordinator's
same-day finding (no maintained pure-Rust ALTS provider) is
independently confirmed.

- crates.io API, exact-name probes — `alts`, `grpc-alts`,
  `tonic-alts`, `google-alts`, `alts-tsi`, `alts-record`,
  `alts-handshaker`, `handshaker`, `altsv2`,
  `google-alts-handshaker`, `gcp-alts`, `rust-alts`,
  `alts-client`, `hyper-alts`, `tower-alts`: all 15 return
  HTTP 404 (no such crate).
- crates.io API, keyword tags — `keyword=alts`: 0 crates;
  `keyword=handshaker`: 0 crates.
- crates.io API, fuzzy/keyword queries — `q=alts` (2,842 fuzzy
  hits; top 30 inspected: TUI widgets, key handlers, mail
  clients — all name collisions), `q=alts+handshake`,
  `q=application+layer+transport+security`, `q=grpc+handshaker`
  (top 10 each inspected): zero ALTS-related crates.
- GitHub repository search API — `alts rust grpc
  in:name,description`: 0 results; `handshaker language:rust`:
  top hits are unrelated (TLS proxies, SSB handshakes, Noise
  handshakes); `grpc alts handshaker in:readme language:rust`:
  26 hits, exactly one ALTS-relevant: `vandry/shared-altsd`
  (assessed in section 2). Name-wide `alts in:name` hits are
  collisions (Alt-Tab tools, AltStore, alt:V).
- `tonic`: the `tonic/Cargo.toml` manifest at `hyperium/tonic`
  HEAD contains zero `alts` mentions (fetched HTTP 200) — no
  ALTS feature or crate in the Rust gRPC stack. `grpc-rs`
  wraps the C core and is excluded by the pure-Rust goal.
- Registry reachability: crates.io and GitHub APIs were
  reachable from this environment on 2026-09-29 (crates.io
  required a `User-Agent` header; without it the API returns
  an empty body). No surface had to be skipped for network
  reasons.
- Coverage limits (recorded, not blocking): GitHub **code**
  search requires authentication and was not run; lib.rs
  search renders client-side and was not machine-readable
  (it indexes the same crates.io corpus already queried
  directly). An unlisted personal repo cannot be ruled out
  absolutely, but nothing published, maintained, or
  discoverable through the registry, repo search, or the
  Rust gRPC ecosystem qualifies.

## 2. Only candidate found: `vandry/shared-altsd` — REJECTED

[shared-altsd](https://github.com/vandry/shared-altsd) (Rust,
"Application Layer Transport Security server") is the sole
ALTS-relevant Rust repository surfaced. It fails on every axis:

- **Wrong component and incompatible wire protocol.** It is an
  ALTS *server* (a handshaker-daemon replacement), binary-only
  (`src/main.rs`, no library target), whose inter-daemon
  protocol is, per its own README, "ad-hoc and private" and
  "can only talk to other instances of itself". It cannot
  interop with Google's handshaker or with any other gRPC
  ALTS peer, and it provides no client-side record layer or
  handshake state machine for embedding.
- **Unmaintained.** Created 2022-12-18, last push
  2022-12-29, 1 star, 0 forks, no activity for over three
  years.
- **No license.** No `LICENSE` file; GitHub reports no
  license — all rights reserved, unusable as a dependency.
- **Stale supply chain.** Not published to crates.io; pins
  `tonic 0.8`, `ring 0.16`, `trust-dns 0.22` (pre-rename),
  `prost 0.11`, `x509-parser 0.14`.
- Record framing, nonce/key discipline, handshake state
  machine, memory safety, and MSRV were therefore not
  reviewed further: a component that cannot speak the ALTS
  wire protocol to Google peers cannot be selected
  regardless of its internals.

No other reuse candidate exists to review.

## 3. Reference baseline (what "reviewed" is measured against)

The C++ TSI implementation at the pinned commit
`d1487957db6658bc532b72871775148229836627`
(`src/core/tsi/alts/`) remains the sole reference. It totals
40 C++/proto source files, ~334 KiB, in four groups:
`handshaker/` (streaming `DoHandshake` client plus the TSI
handshaker state machine — `alts_handshaker_client.cc` alone
is ~40 KiB), `frame_protector/` (copying protector, seal/unseal
crypters, frame handler, counter), `zero_copy_frame_protector/`
(iovec record protocol, gRPC privacy+integrity and
integrity-only paths), and `crypt/` (AES-GCM AEAD, key
factory). (`third_party/grpc` is not present in this
checkout; the reference was inspected via the pinned commit
over the GitHub API/raw on 2026-09-29.)

Cryptographic anchors recorded for any future review:

- AEAD is AES-128-GCM (`kAes128GcmKeyLength = 16`); nonce
  length 12 (`kAesGcmNonceLength`); nonces are masked with a
  12-byte mask and rekeying derives keys through a KDF
  counter (`kAes128GcmRekeyKeyLength = 44`: 32-byte KDF key
  + 12-byte mask; `GsecKeyFactory` with `is_rekey`).
- Record framing (zero-copy path): 8-byte header (4-byte
  length + 4-byte message type `0x06`) plus per-frame tag;
  frame-size policy 1 KiB min / 16 KiB default / 1 MiB max;
  frame-count limits `kAltsRecordProtocolFrameLimit = 5`
  and rekey limit `kAltsRecordProtocolRekeyFrameLimit = 8`
  (at most 2^(8·k) frames per key epoch).
- Handshake surface: `handshaker.proto` / `altscontext.proto` /
  `transport_security_common.proto` streaming RPCs against a
  privileged local handshaker (see the contract's handshaker-
  trust section).

Public protocol background: Google's
[ALTS overview](https://cloud.google.com/docs/security/encryption-in-transit/application-layer-transport-security).

## 4. Why no bounded new implementation is commissioned here

A from-scratch pure-Rust port would need, at minimum: both
record-framing paths (privacy+integrity and integrity-only),
the counter/nonce-mask/KDF rekey discipline, the full TSI
handshaker state machine and streaming handshaker client,
and byte-level interop against a real handshaker — with a
dedicated cryptographic audit of exactly the nonce/key/state
axes above. The platform half of that proof (EX-18b: a
GCE-class environment, documented handshaker endpoint,
operator approval) does not exist either, so a rewrite
started now would be unverifiable as well as unbounded.
Per the card's accept clause (2), the correct output is this
dated blocker record, not a rewrite plan.

Re-survey triggers: a maintained, licensed pure-Rust ALTS
crate appearing on crates.io with real repo activity, or
official ALTS support landing in tonic/grpc — at which point
the full seven-axis review (record framing, nonce/key
discipline, handshake state machine, memory safety, MSRV,
license, transitive graph) runs against that option.

## 5. Accept verdicts

- **(1) Selected option fully reviewed — BLOCKED (nothing
  qualifies).** The only surfaced candidate was assessed and
  rejected with rationale above; there is no selected option
  whose framing, nonce/key discipline, state machine,
  memory safety, MSRV, license, and transitive graph could
  be reviewed. The review that exists is recorded here.
- **(2) No dependency added before approval; blocker
  recorded instead of an unbounded rewrite — PASS.** No
  code or manifest change was made; the working tree adds
  only this evidence file. The blocker (no maintained
  pure-Rust ALTS record/handshake provider as of
  2026-09-29, verified by the survey in section 1) is
  recorded with dated evidence.
