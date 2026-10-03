# Read-only conformance descriptor fallback

The conformance build script now recreates its descriptor output with writable
permissions instead of copying the input file's permissions. It also recovers
an output left read-only by an older build script. The vendored descriptor
bytes and permissions remain unchanged, and a missing descriptor remains a
hard build error.

Cargo watches the optional `person.proto` and protobuf SDK only when those
paths exist, or watches the existing `third_party` parent while the SDK is
absent. This avoids perpetual dirty builds caused by missing watched paths.
Installing an SDK into a package with no such parent requires a normal rebuild
trigger.

The regression fixture compiles the actual `build.rs` and uses the actual
74,748-byte vendored descriptor. Qualification on Linux used Rust 1.85.0 and
1.88.0 with identical fixture and descriptor inputs:

| Source | Rust 1.85.0 | Rust 1.88.0 |
|---|---|---|
| Original script, `02fc7229` | Expected failure: copied output is read-only | Same expected failure |
| Fixed script, `63d98148` | One test passes | One test passes |

The passing test covers three fallback runs, a legacy read-only output,
unchanged descriptor input, existing/absent optional input watchers,
missing-vendor rejection without destroying the prior output, and the
conformance-disabled path. All four compilations passed; source, tool and
helper checks stayed unchanged and owned process cleanup completed. The
Windows-specific read-only output recovery branch was not executed on this
Linux host. Main's whole-workspace formatting check passed before publication.

The actual eight-command summary has SHA-256
`c5a08b70abd409ae1e26c5206ecb412aa10d92c37e3e6be6859daafa0aae27a9`;
the independent source/raw-log/four-ELF readback has SHA-256
`488e3d3510fed225e0cf1ee9dc2f0ee8493c8bb766f6a42ca18e0f815022049e`.
These identify retained local qualification records; this note does not bundle
their executable payloads or claim a complete native toolchain capsule.

The two failed frozen GN-03 campaigns remain failed. The first stopped during
consumer lock admission; the second stopped during incremental checking when
the original script attempted to overwrite a read-only descriptor output.
Their raw records and the second attempt's full cache archive remain retained.
Partial timings do not qualify comparative performance. A new current-source
campaign is required; release size, repeated performance and independent-host
acceptance remain open.
