# RX-04: TLS providers and handshake paths

Date: 2026-09-28. Base SHA:
`251a37b5075f4c9cdd02cc7956f22c8cf47d23cd`. Host: Apple M4 Pro macOS,
14 CPUs, shared/contended laptop, no pinning. All numbers below are
**dev-loop** diagnostics, not claim-grade. Scoreboard: C7, D6, E3.

## What changed (`pbrs-grpc/src/tls.rs` only)

Premise, confirmed in source: the server used rustls's default
`NeverProducesTickets`, which silently disables **all** TLS 1.3
resumption even though `send_tls13_tickets` defaults to 2. Every
(re)connect paid a full handshake.

- Server: install `rustls_graviola::Ticketer` (XChaCha20-Poly1305,
  keys rotated every 6 h). Each `ServerTls` mints its own keys, so
  the rustls cross-config resumption warning (a session from a
  no-client-auth config resumed into an mTLS config) cannot trigger
  between `new` / `mtls` / `optional_mtls` instances.
- Client: state the session store explicitly
  (`Resumption::in_memory_sessions(256)`; same parameters as the
  rustls default, now immune to upstream default drift). Clones of
  one `ClientTls` share the store, so pooled and reconnected
  sockets in a `Channel` resume; a separately built `ClientTls`
  starts empty.
- Verification, ALPN `h2`, cipher/KX policy: unchanged. No new
  dependency, no `Cargo.toml`/`Cargo.lock` change, no public API
  change. Resumption keeps TLS 1.3 PSK + (EC)DHE forward secrecy.

SECURITY-DEFAULT FLAG: this enables stateful-session-ticket
resumption where previously every handshake was full. Tickets are
AEAD-encrypted with server-only keys and rotated every 6 h; peer
authentication still applies to every connection (a ticket only
replaces the KX + certificate flight, never the identity check,
and per-config keys prevent cross-policy resumption). Standard
practice (same defaults as rustls examples), but it is a
handshake-behavior change, so it is called out here.

Tests (in `tls.rs`, the card's write scope): second connection
over shared configs asserts `HandshakeKind::Resumed` on both
ends; a fresh `ClientTls` asserts `Full` twice.

## Provider comparison (benchmark-only harness, /tmp)

rustls 0.23.45 in-memory client/server pairs, ECDSA P-256 certs
(the repo's `pbrs-grpc/tests/tls_data`), TLS 1.3, ALPN `h2`,
per-provider ticketer. Six interleaved rounds with rotating
provider order (the host drifts thermally; round-robin cancels
it); 600 full + 600 resumed handshakes and 16 MiB bulk per
provider per round. Medians across rounds:

| provider | pure Rust | full hs (µs) | resumed (µs) | resumed/full | bulk (MiB/s) |
|---|---|---|---|---|---|
| graviola 0.2.1 (shipping) | yes | 271.7 | 174.1 | 0.64 | 1456 |
| ring 0.17.14 (probe only) | no (C/asm) | 270.4 | 154.0 | 0.57 | 1499 |
| aws-lc-rs 1.18.x (probe only) | no (C/asm) | 278.0 | 169.2 | 0.61 | 1698 |

Reading:

- Full-handshake rate: all three within ~3%, inside run-to-run
  noise (per-round medians overlap; round order rotation shows no
  consistent winner). Handshake rate does **not** discriminate.
- Resumed handshake: ring ~12% faster than graviola, aws-lc-rs
  ~3% faster. Absolute gap ≈ 20 µs.
- Bulk record crypto: aws-lc-rs ~17% faster, ring ~3% faster
  than graviola. At 1456 MiB/s the shipping path spends
  ~0.7 ns/byte, i.e. ~0.7 µs per 1 KiB RPC per side — handshake
  cost dominates connection churn, bulk cost is sub-microsecond
  for small RPCs.
- Caveat: the probe's in-memory pump adds fixed per-flight
  overhead shared by all providers, so absolute crypto gaps are
  understated; relative ordering is what transfers.

Raw per-round CSV (kept with the worker's notes, not committed):
6 rows × 3 providers; full-handshake ranges across rounds were
graviola 229–281 µs, ring 212–272 µs, aws-lc-rs 238–282 µs.

## Real-path resumption gain (shipping code, loopback)

Sequential `Channel::connect_tls` against `Server::serve_tls`
(TCP + TLS + h2 preface, release, 5 rounds × 100 connects,
10% trimmed edges):

| connects | mean connect |
|---|---|
| fresh `ClientTls` each (full hs) | 0.406 ms |
| one shared `ClientTls` (resumes) | 0.348 ms |

Resumption saves ~58 µs (~14%) per connect with zero RPCs
exchanged — tickets are processed during the connect-time h2
preface reads, so even connect-only churn benefits. This is the
D6/C7-relevant win; steady-state QPS on reused connections is
unaffected by design.

## Decision (accept item 1)

**Keep Graviola as the only shipping provider.** It is pure Rust
(QG-04 clean), and the measured gaps — no winner on full
handshakes, ≤12% on resumed, ≤17% on bulk — do not justify
breaking the pure-Rust shipping graph. `ring` and `aws-lc-rs`
were measured via throwaway /tmp probes against cached crates
only; neither is (or may become) a dependency without an
explicit opt-in feature plus maintainer approval, and `aws-lc-rs`
in particular stays **benchmark-only, never shipping**.

## Stack-matrix TLS cells (accept item 2)

`./scripts/stack-matrix.sh --stage smoke --server-peers native
--client-peers native` → 3/3 pass, exit 0, including
`server-native-unary-empty-tls-1cpu` (sustained 7934 qps at the
smoke `max_rate 8000` ceiling, p99 2.3 ms). Smoke cells reuse
long-lived connections, so they validate "TLS still passes with
tickets enabled" rather than measuring the resumption gain; the
gain is on the handshake/connect path above. Full-matrix primary
cells need pinned Linux hosts (SB-15/SB-22), unavailable here.

## Checks run

- `cargo test -p pbrs-grpc --test tls` → 22 passed (22 pre-change).
- `cargo test -p pbrs-grpc --lib` → 402 passed (incl. 2 new).
- `cargo fmt -p pbrs-grpc -- --check` → clean.
- `cargo clippy -p pbrs-grpc --lib` → no warnings.
- `git status` → only `pbrs-grpc/src/tls.rs` + this file.

## Limitations

- Numbers are one contended macOS laptop (dev-loop tier); no
  claim-grade Linux/pinned runs (needs SB-15/SB-22 hosts).
- ECDSA P-256 certs only; RSA handshake gaps may differ.
- No cross-version claim: rustls 0.23.45 / graviola 0.2.1 pins.
- Probe harnesses live in /tmp (`rx04-probe`, `rx04-realpath`)
  and are intentionally uncommitted (card write scope is
  `tls.rs` + this file); rerun recipes are in the sections above.
