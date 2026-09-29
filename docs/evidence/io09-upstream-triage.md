# IO-09 upstream probe triage (2026-09-29)

Pinned runner `grpc/grpc@d1487957` `tools/http2_interop`, Go 1.25.3,
native TLS server. Local phases pass (2/2); upstream phases execute
and fail closed with retained logs.

## Framing 5/6: one genuine bug (IO-11)

`TestSoonSmallMaxFrameSize` FAILS: the probe sends
`SETTINGS_MAX_FRAME_SIZE = 16383` (below the RFC 9113 section 6.5
minimum 16384) and expects `GOAWAY`; we close with `EOF` and no
`GOAWAY`. RFC 9113 requires a below-minimum value to be treated as a
connection error of type `PROTOCOL_ERROR`, which means send `GOAWAY`
then close. The connection does terminate, but the reason is
undelivered. Fix belongs in the server connection error path
(`pbrs-grpc/src/server/connection.rs`, owned by in-flight TC-23);
split to IO-11. All other framing subcases pass.

## TLS 0/3: runner drift, correct server behavior (GF-11 follows one thread)

- `TestSoonTLSApplicationProtocol`: probe offers ALPN `h2c` only; we
  abort with `tls: no application protocol`, which is correct (we
  require `h2`). The 2016-era probe expects the strings `EOF` or
  `broken pipe` from old Go error text. Drift, not a bug.
- `TestSoonTLSMaxVersion` (TLS 1.1): Go 1.25 cannot construct the
  attack (`tls: no supported versions satisfy MinVersion and
  MaxVersion`) since TLS 1.0/1.1 removal. The server is never
  contacted. Drift.
- `TestSoonTLSBadCipherSuites`: the weak-only suite list covers TLS
  1.2 ciphers, but modern Go still negotiates TLS 1.3 (separate
  strong suites), so the handshake succeeds and we send `SETTINGS`
  where the pre-1.3 probe demands `GOAWAY`. Drift — with one genuine
  follow-up: when TLS 1.2 *does* negotiate an RFC 9113 section 9.2.2
  blacklisted cipher, nothing in our stack sends
  `GOAWAY(INADEQUATE_SECURITY)`. Split to GF-11.

## Disposition

IO-09 closes: upstream probes execute (not counted from local
tests), failures are visible and triaged, logs retained, CI runs the
local gate blocking and the upstream phase advisory. IO-11 and GF-11
carry the two product threads.
