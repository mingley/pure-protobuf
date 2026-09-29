# Runtime/build-time split (PK-02)

Decision: use Cargo features in the existing `pbrs` crate, not a new build
crate. Default features keep today's source-compatible behavior. Runtime-only
consumers can opt out of the generator, descriptor pool, JSON, text, and
bundled conformance gencode with `default-features = false`.

Decided 2026-09-28 from maintainer direction: keep this a small cohesive
release and use features rather than a new crate name.

## Chosen feature shape

| Feature | Enables | Default? |
|---|---|---|
| `codegen` | `pbrs::codegen` build-time APIs and descriptor parsing used by build scripts. | Yes |
| `reflect` | `DescriptorPool`, descriptors, `DynamicMessage`, and reflection parsing. | Yes |
| `json` | ProtoJSON for `DynamicMessage` and generated field-wise helpers; implies `reflect` and the optional `serde`/`serde_json` dependencies. | Yes |
| `text` | Text-format support for `DynamicMessage` and generated field-wise helpers; implies `reflect`. | Yes |
| `conformance` | Bundled generated TestAllTypes/WKT modules and `pbrs::gencode` for the conformance runner. | Yes |
| `copy-counts` | Bench-only copy counters. | No |

Two non-default profiles matter:

```toml
# Runtime using only the core binary wire API.
pbrs = { version = "0.2", default-features = false }

# Runtime for current generated files without build-time codegen/conformance.
pbrs = { version = "0.2", default-features = false, features = ["json", "text"] }
```

Build scripts that call the generator can keep that cost in
`[build-dependencies]`:

```toml
[build-dependencies]
pbrs = { version = "0.2", default-features = false, features = ["codegen"] }
```

## Rejected options

| Option | Why rejected |
|---|---|
| New `pbrs-build` crate | Requires a new crate-name decision, release wiring, package-consumer changes, and a migration for every existing `build.rs` user. It also conflicts with the maintainer preference for a small compatible release. |
| Move JSON/text to separate crates | Larger public API/release surface for code that is already internal to generated helpers; this would be a second design decision. |
| Break default features | Existing consumers, examples, adapters, tests, and `build.rs` snippets depend on `pbrs::codegen`, `DescriptorPool`, JSON/text helpers, and `pbrs::gencode` being available by default. |

## Migration and semver impact

- Existing users do nothing. `pbrs = "0.2"` keeps the current API and behavior.
- Consumers that want smaller runtime builds set `default-features = false` on
  the runtime dependency and add only the format features their generated code
  needs.
- Existing build scripts can either keep default features or switch their
  build-dependency to `features = ["codegen"]`.
- No public item is renamed or removed from the default feature set, so the
  change is source-compatible.

## Measured estimates

Local measurements on the shared macOS arm64 dev-loop host, `CARGO_BUILD_JOBS=3`.
They are compile-cost estimates, not claim-grade performance results.

| Profile / cell | Command | Result |
|---|---|---|
| Core runtime only | `CARGO_BUILD_JOBS=3 cargo check -p pbrs --lib --no-default-features` | passed; excludes codegen, reflect, JSON, text, conformance gencode, `serde`, and `serde_json` |
| Generated-code runtime | `CARGO_BUILD_JOBS=3 cargo check -p pbrs --lib --no-default-features --features json,text` | passed; excludes codegen and conformance gencode |
| Build-time codegen only | `CARGO_BUILD_JOBS=3 cargo check -p pbrs --lib --no-default-features --features codegen` | passed; excludes JSON/text modules and `serde`/`serde_json` |
| 100-message generated consumer, default runtime | `cargo check --offline --locked --manifest-path target/pk03-feature-split/100-default/Cargo.toml --bin cg19-consumer-100 --timings` | 17.42 s real, 967 MiB max RSS; cargo timing top unit `pbrs` check 7.93 s |
| 100-message generated consumer, `features = ["json", "text"]` runtime | `cargo check --offline --locked --manifest-path target/pk03-feature-split/100-json-text/Cargo.toml --bin cg19-consumer-100 --timings` | 9.48 s real, 560 MiB max RSS; cargo timing top `pbrs` unit 1.45 s |

Downstream 100-message clean-check estimates are recorded in
[`docs/evidence/compile-cost.md`](../evidence/compile-cost.md).

## Consequences

- PK-03 implements cfg gates inside the existing crate and keeps adapters using
  default features unchanged.
- A future generated-output shrink can make `json`/`text` optional for more
  generated-code users, but that requires coordinated emitter changes in the
  codegen-parity lane.
