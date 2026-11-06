# Changelog

## Unreleased

The coordinated release candidates are `pbrs` **0.2.1**, `pbrs-grpc`
**0.1.0-alpha.3**, and `protobuf-tonic` **0.1.0-alpha.3**. These versions
are prepared in source and have not been published. The adapters remain
previews; this version update does not promote them to production-qualified
or stable releases.

### pbrs 0.2.1

- Add `Config::disable_comments_for(paths)` to suppress selected protobuf
  source comments while preserving the existing global boolean API. Calls
  replace the selector list; an empty list resets selection. Synthetic docs
  and explicit user attributes stay unchanged.
- Add explicit `alloc-only` runtime and `Config::alloc_only(true)` generation
  for native binary messages. Concrete Cortex consumers keep std host codegen
  separate; std-backed host tests cover five generated profiles. Existing
  no-default std behavior is preserved, and `std` wins when both modes are
  enabled. Lazy cache publication adds boxed candidates; hardware execution
  and performance remain unverified. See the [guide](docs/guides/alloc-only.md).
- Add `Config::btree_map(paths)` with unique, ordered storage and typed map
  views and mutators. Regenerated ancestor paths reach the selected storage;
  older lazy ancestors can still forward their original wire.
- Fix JSON roundtrips for nested map messages, including recursive and
  imported children and singular, repeated, map and oneof parent fields.
- Qualify generated RPC framework references and service-trait paths so
  services named `Send`, `Arc`, `T` and other framework/generic names compile
  without renaming their public identifiers. Resolve configured native and
  adapter prefixes before inserting user paths, attributes and literals.
- Add `Config::bytes(paths)` for selected `ProtoBytes` storage and shared
  getters. Initialize direct selected byte fields with valid defaults, and
  retain shared payload windows through matching generated parents. Older or
  differently configured eager parents can still copy child payloads.
- Forward shared-byte merges through typed message mutators. Both typed and
  arena-backed mutators now honor explicit merges without required-field
  enforcement while preserving ordinary enforcement.
- Preserve explicit JSON/text generation flags through builder serialization.
- Invalidate packed-field encoding caches before conversion, preventing stale
  output when a custom decoder returns an error or panics and the container
  is reused.
- Preserve unrelated duplicate-key winners when removing a map entry and
  repair an existing lookup index without rebuilding it. Removal still
  exposes earlier stored versions of the target key.
- Resolve collisions between singular message/group view methods and other
  field accessors. Previously valid field APIs keep their names; regenerate
  bindings for schemas that formerly produced duplicate methods.
- Add `Config::boxed(path)` selectors for eager singular child storage, with
  unchanged pbrs accessors. Imported children keep their generated profile;
  repeated and map fields keep their storage.
- Add opt-in `safe_message_defaults(true)` for generated consumers that
  forbid unsafe code. Layouts and wire behavior stay unchanged; runtime
  dependencies keep their own unsafe code. Construction and encoding costs
  vary, so the existing default profile is preserved.
- Qualify generator-owned symbols and resolve runtime prefixes before adding
  user paths or literals. Message names that shadow runtime/prelude symbols,
  including lowercase `pbrs` and `std`, compile in the tested profiles.
- Add an optional sixth `default_view` argument to `impl_generated_message!`;
  existing five-argument callers keep their own view `Default` implementation.
- Cache numbered collision suffixes and reuse message companion names,
  avoiding repeated prefix searches while preserving the tested names.
- Transfer owned byte buffers through repeated/map setters and singular
  `Cow::Owned`/boxed-slice setters without copying the payload. Taking a
  `Vec` retains its spare capacity; borrowed inputs still copy their bytes.
- Emit legal Rust names for enum values named `SELF`, including after enum
  prefix removal. Generated names match Prost's `Self_` spelling.
- Reserve message view and mutator names and recheck prefixed type names for
  collisions. Nested aliases and imported references use the same allocation.
  Regeneration may change Rust names in affected collision schemas.
- Compare indexed maps by walking sorted unique keys once, preserving
  duplicate-key last-wins behavior. Small maps keep a scan-based fallback.
- Apply configured type, message, enum and field attributes using global,
  qualified prefix and relative suffix selectors. Field selectors also cover
  cold storage. Builder attributes containing commas now round-trip through
  code generation. Existing configurations may generate more attributes.
- Validate eligible self-recursive messages using borrowed slices. Regenerate
  application bindings to use this path; existing bindings remain compatible.
- Give lazy string and byte storage explicit zero-valued empty tags, repairing
  invalid generated defaults on 32-bit targets. Measured x86_64 wrapper sizes
  remain unchanged; each i686 lazy string or byte wrapper grows by four bytes.
- Reject malformed package and declared-symbol names before adding serialized
  descriptors to the symbol table. Retain the scheduled fuzzer input as a
  deterministic regression and future campaign seed.
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

- Prioritize ready shutdown signals over queued connections in TCP/TLS, Unix
  and custom incoming accept loops. Custom sources also consume Tokio's
  cooperative budget so a ready backlog lets other tasks signal shutdown.
  Existing connections still drain.
- Stop authorization policy watchers when their last provider drops. Serialize
  reload publication, clear recovered errors, and fail construction if the
  polling thread cannot start.
- Preserve per-endpoint executors and full URI identities in static and
  watched balance helpers. Replace changed entries lazily and drain existing
  calls. RPC attempts use the selected endpoint's authority unless an explicit
  `Channel::origin` pins that clone. Health Watches keep their endpoint's
  authority; getters retain configured channel context. Connection settings
  remain shared; per-endpoint origins, TLS and hostname lookup are unsupported.
- Keep one health Watch per current round-robin connection, reject stale
  owners after replacement, and recover after the health transport closes.
- Add `Endpoint::origin(Uri)` and `Channel::request_scheme()` for logical
  request routing without changing dialing, TLS or interceptor security
  context. Origin paths and queries are ignored; static balance helpers
  reject per-endpoint origins.
- Round `grpc-timeout` values upward when a larger unit is needed, preserving
  the requested budget through the wire-format maximum. Check local deadline
  overflow in outgoing interceptors and retry attempts, and reject signed
  timeout headers.
- Check server lifetime and keepalive clock arithmetic, preserve the full
  duration range when jittering connection ages, and keep an outstanding
  probe pending when its timeout exceeds the local clock range.
- Advance large outgoing byte frames with `Bytes::split_to`, removing a
  redundant shared-owner clone and drop per chunk. The isolated chunk-loop
  screen is separate from RPC performance measurements.
- Require the coordinated `pbrs` 0.2.1 core at runtime and build time.
- Reject overflowing transport byte counters and unrepresentable outbound
  frame lengths without publishing wrapped accounting or invoking a codec
  on an impossible length. Preserve earlier streamed replies and error
  trailers.
- Restore the published cancellation handle's `UnwindSafe` and
  `RefUnwindSafe` traits without asserting unwind safety for internal waiter
  registration. A panicking wake callback leaves the public cancellation
  signal observable and usable.
- Release queued and rejected outbound payload owners after the HTTP/2
  engine locks, including reset, EOF, window-error and terminal-response
  paths. Preserve initial headers, reset ordering and accepted DATA ownership.
- Repair cached stream-pointer provenance across debug validation, callbacks
  and slot reborrows. Extend strict Miri coverage to real engine ownership
  and send/receive controls.

### protobuf-tonic 0.1.0-alpha.3

- Require the coordinated `pbrs` 0.2.1 core at runtime and build time.
- Preserve tonic 0.14 codec and generated-stub integration, native peer
  connection information in adapters, and checked-descriptor builds without
  a `protoc` dependency for ordinary consumers.

### Release qualification

Successful main CI can now start an exact-commit package and provenance
rehearsal automatically. This path never uploads crates. Tagged publication
also checks that the verified annotated tag targets the commit tested by CI.

Publication requires the checks in [the release policy](docs/RELEASE.md),
including required CI on the release SHA, isolated package consumers, a hosted
provenance rehearsal, and a signed, GitHub-verified tag. Each crate also needs
its configured crates.io Trusted Publisher.

Sustained resource and recovery qualification remains open. Known feature
and conformance boundaries remain in [implementation status](docs/status.md)
and [the parity record](docs/parity.md). A green local test run or a version
bump does not close those qualification gates.
