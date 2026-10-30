# Changelog

## Unreleased

The coordinated release candidates are `pbrs` **0.2.1**, `pbrs-grpc`
**0.1.0-alpha.3**, and `protobuf-tonic` **0.1.0-alpha.3**. These versions
are prepared in source and have not been published. The adapters remain
previews; this version update does not promote them to production-qualified
or stable releases.

### pbrs 0.2.1

- Inline common varint decoding across crate boundaries while keeping the
  longer-value decoder out of line.
- Check canonical one-byte tags directly in generated repeated-field runs.
  Regenerate application bindings to use those loops; compatible existing
  bindings continue to work with the updated runtime.
- Emit reflective descriptor bytes as a Rust byte-string literal, preserving
  the public constant and exact descriptor data while reducing generated
  source and Rust syntax nodes.
- Preserve required-field checks after message merging and retain unknown
  fields and group framing. The [measured results](docs/evidence/runtime-and-codegen-20261029/README.md)
  include remaining losses and the exact source and workload scopes.

### pbrs-grpc 0.1.0-alpha.3

- Require the coordinated `pbrs` 0.2.1 core at runtime and build time.
- Reject overflowing transport byte counters and unrepresentable outbound
  frame lengths without publishing wrapped accounting or invoking a codec
  on an impossible length. Preserve earlier streamed replies and error
  trailers.
- Restore the published cancellation handle's `UnwindSafe` and
  `RefUnwindSafe` traits without asserting unwind safety for internal waiter
  registration. A panicking wake callback leaves the public cancellation
  signal observable and usable.

### protobuf-tonic 0.1.0-alpha.3

- Require the coordinated `pbrs` 0.2.1 core at runtime and build time.
- Preserve tonic 0.14 codec and generated-stub integration, native peer
  connection information in adapters, and checked-descriptor builds without
  a `protoc` dependency for ordinary consumers.

### Release qualification

Publication requires the checks in [the release policy](docs/RELEASE.md),
including required CI on the release SHA, isolated package consumers, a hosted
provenance rehearsal, and a signed, GitHub-verified tag. Each crate also needs
its configured crates.io Trusted Publisher.

Sustained resource and recovery qualification remains open. Known feature
and conformance boundaries remain in [implementation status](docs/status.md)
and [the parity record](docs/parity.md). A green local test run or a version
bump does not close those qualification gates.
