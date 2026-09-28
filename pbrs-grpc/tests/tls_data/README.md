# Test-only PKI

These throwaway Public Key Infrastructure (PKI) files let loopback Transport Layer Security (TLS) and mutual TLS (mTLS) tests prove certificate loading, trust-store handling, and wrong-CA rejection. Run the focused TLS test with `cargo test -q -p pbrs-grpc --test tls`. They do not cover production certificate management; these keys are not secret, but they must not be used anywhere else.

The Certificate Authority (CA) private key is not in the tree.

| File | Role |
|---|---|
| `ca.crt` | Trust anchor for the server and client leaves |
| `server.crt` / `server.key` | Server identity; Subject Alternative Name (SAN) `DNS:localhost`, `IP:127.0.0.1` |
| `client.crt` / `client.key` | Client identity for mTLS |
| `other.crt` | Unrelated CA, used to prove a wrong trust store is refused |
