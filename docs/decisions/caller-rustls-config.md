# Caller-owned rustls configuration (TC-25)

Date: 2026-09-30. Base: `1cfbcd9a82a21568b7c17428ed0a3104a82b4872`.
Status: implementation contract for the authorized TC-25 card; qualification
is recorded separately in `docs/evidence/caller-rustls-config.json`.

The native TLS wrappers currently build rustls configs using Graviola and
PEM identities. Adopters with a shared authentication library need to preserve
their provider, verifier, certificate resolvers and rotation state.

## Configuration ownership

Add these constructors without removing the existing constructors:

```rust,ignore
ServerTls::from_rustls(config: Arc<rustls::ServerConfig>) -> Result<ServerTls, Status>
ClientTls::from_rustls(server_name: impl Into<String>, config: Arc<rustls::ClientConfig>)
    -> Result<ClientTls, Status>
```

Keep the supplied `Arc`. Do not clone and rebuild its config, install a global
provider, replace its verifier/resolver, enable key logging, or modify its
resumption store, ticket producer, protocol versions or client-auth policy.
The crypto provider is selected through rustls's `builder_with_provider` in
the caller. A separate provider-only constructor would duplicate this path.
Graviola remains the provider for the existing convenience constructors and
stays in the default shipping graph. No alternate provider feature is added
to `pbrs-grpc`.

The caller's server certificate resolver remains shared with the running
acceptor. Updating that resolver changes the certificate selected for future
full handshakes without reconstructing the server. Existing connections keep
their authenticated sessions. The caller controls resumption and must rotate
or invalidate tickets when a certificate/trust policy change requires it;
replacing a leaf does not force a resumed handshake to send that new leaf.

## ALPN and authentication

Both constructors reject a config that does not advertise `h2` with
`INVALID_ARGUMENT`. They do not silently append it. Other advertised protocols
may remain present, but the existing post-handshake check still requires the
negotiated protocol to be exactly `h2` before HTTP/2 or RPC processing.
The client still validates the supplied server-name syntax and passes that
name to rustls for SNI and the configured verification policy.

The wrappers await rustls's authenticated handshake and propagate verifier,
signature, client-auth and protocol failures. They never substitute an
accept-all verifier or retry through an unauthenticated configuration.
Server client authentication follows the caller's required/optional/anonymous
policy, just as the existing `mtls`/`optional_mtls`/`new` constructors differ.

A caller config is a **trusted application input**. The pinned rustls
`ClientConfig` stores its verifier privately, and a verifier trait object has
no capability that proves its security policy. This API cannot certify that
an arbitrary caller verifier checks chains, names, validity or signatures.
Callers must supply a policy that authenticates the peer; disabling peer
verification is unsupported. This trust boundary must be explicit in the API
docs and credential contract. The existing built-in constructors continue
to construct verifying WebPKI/CRL/SPIFFE policies themselves. Do not generalize
their verification guarantee to arbitrary opaque configs.

## Qualification and dependency scope

Default native TLS tests cover constructor validation, both supplied configs,
negative verification paths and the existing PEM API. An excluded consumer
fixture under `pbrs-grpc/tests` enables rustls's ring provider and tonic TLS
only in its own dependency graph. Review its manifest and retain its lockfile
before building. It must demonstrate mTLS with ring against Graviola in both
directions, native/tonic interoperability, and live resolver rotation with
distinct leaf certificates observed on new full handshakes.

Negative fixtures must show wrong trust, wrong name, missing required client
identity, and a caller verifier that deliberately rejects an otherwise valid
peer all fail before an RPC succeeds. Rejecting an absent `h2` advertisement
does not replace a negotiated-ALPN test.

Run the shipping `scripts/pure-rust-audit.sh` profiles unchanged and record
their actual graph. Ring, aws-lc-rs and FIPS certification do not enter the
default graph or become claims about the Graviola implementation. The ring
consumer intentionally exercises its own provider's build prerequisites.
No performance, FIPS or production certificate-management qualification is
implied by loopback tests.
