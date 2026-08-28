# Test-only PKI

Throwaway ECDSA P-256 certificates for loopback TLS tests. Issued by a CA
whose private key is not in the tree. Not a secret; do not use them for
anything else.

| File | Role |
|---|---|
| `ca.crt` | Trust anchor for the server and client leaves |
| `server.crt` / `server.key` | Server identity; SAN `DNS:localhost`, `IP:127.0.0.1` |
| `client.crt` / `client.key` | Client identity for mTLS |
| `other.crt` | Unrelated CA, used to prove a wrong trust store is refused |
