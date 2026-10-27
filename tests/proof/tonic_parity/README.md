# Tonic wire behavior proof

These tests compare the native transport against tonic 0.14.6 and Prost
0.14.4 on the same raw HTTP/2 traffic. They cover message presence, rather
than relying on a generated client to produce only valid frames.

Run from the repository root:

```sh
cargo test --locked --manifest-path tests/proof/tonic_parity/Cargo.toml -- --nocapture
cargo test --locked --manifest-path tests/interop/tonic/Cargo.toml --tests
```

`protoc` must be on `PATH`. The proof reuses the repository's RouteGuide
schema and independent tonic/prost generator. The committed lockfile pins
the peer versions and dependencies.

The two tests check 18 peer comparisons:

- All four request shapes with no message and with a framed, empty protobuf
  message. Unary and server-streaming requests need one message; an empty
  input stream remains valid for client-streaming and bidirectional calls.
- Unary and client-streaming responses with no message, a framed empty
  message, a peer error, a trailers-only error, and a truncated frame followed
  by a peer error. An OK status without the required response is `INTERNAL`.
  The peer error takes precedence over an absent or truncated payload.

The tests print each peer's status. They deliberately compare status codes;
HTTP header escaping can differ while the decoded status message agrees.
They assert decoded error text on unary error controls.

The existing mixed-peer suite adds example ports in both directions,
compression and rich error details, generated clients on native transport,
and generated services on native transport. It includes TCP, Unix, mTLS,
interceptors, message limits, cancellation, and terminal status controls.

Passing this corpus establishes these behaviors for these versions. It does
not establish compatibility with every tonic API or every possible wire
input, and it does not measure performance.
