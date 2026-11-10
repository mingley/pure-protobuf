# Portable TLS caller controls

This standalone test application supplies its own rustls provider. It checks
plaintext gRPC, caller-configured mTLS, certificate and ALPN failures, and
that a failed TLS handshake does not reconnect as plaintext.

The fixture uses ring as a test provider in its own Cargo workspace. The
shipping gRPC dependency graph remains separate, and Cargo excludes this
nested workspace from the published `pbrs-grpc` archive.

On Linux with the i686 Rust target and 32-bit system libraries installed:

```sh
cargo test --locked --manifest-path pbrs-grpc/tests/portable-caller/Cargo.toml \
  --target i686-unknown-linux-gnu
```
