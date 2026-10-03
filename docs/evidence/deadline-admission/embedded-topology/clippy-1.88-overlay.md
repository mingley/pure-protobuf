# Supported Clippy import overlay

The topology's strict Clippy gate used stable Rust 1.99 at source 604f4c2e.
The actual Rust 1.88 caller gate at source 5f438159 reports twelve inherited
`uninlined_format_args` style diagnostics in the copied error.rs,
proto/streams/state.rs and frame/util.rs. Their runtime/display format strings
remain unchanged.

A separate additive overlay inserts only `clippy::uninlined_format_args` into
the existing private imported-source lint boundary, under its existing reason.
Critical async/safety restrictions and native/transport unsafe forbids remain
unchanged. No broad warning suppression is introduced.

The original h2_backend/provenance.json and all 604f source/package/gate records
remain immutable historical mappings. clippy-1.88-overlay.json records the exact
old/new mod.rs hashes, original mapping hash, diagnostic source/log hash, and
one-line transformation. Its patch and read-only replay script reproduce the
new bytes. GN13 owns the actual Rust 1.88 and package/consumer qualification of
the resulting caller source; the old topology results are not relabeled.
