# Caller rustls qualification fixture

This excluded application selects rustls's ring provider while `pbrs-grpc`
keeps its Graviola default. Its own workspace/lockfile prevents provider
feature unification with the shipping profiles. It intentionally requires
ring's C build prerequisites; the native library's default graph does not.

The manifest was reviewed against TC-25 before resolution: all Rust
dependencies already appear in the repository's established graphs; the
ring feature is application-only, and no shipping dependency is changed.
Rustls is pinned to the implementation contract's inspected 0.23.45 version
and tonic to 0.14.6. This is not FIPS or production certification.

The outer `pbrs-grpc` integration test runs this matrix with a separate Cargo
target directory, so workspace testing covers it without feature leakage:

```sh
cargo test -p pbrs-grpc --test caller_rustls
```

For direct inspection:

```sh
cargo test --manifest-path pbrs-grpc/tests/caller-rustls/Cargo.toml --locked
```

The `data/` rotation certificates and leaf keys are copies of the existing
throwaway GF-06 fixtures in `pbrs-grpc/src/tls.rs`, generated 2026-09-29.
The two server leaves carry different serials/SANs and share that fixture's
CA. Full handshakes disable client resumption to observe the changed leaf;
existing connections are also checked after rotation. These public test
keys must not be used for real services. No CA private key is included.
