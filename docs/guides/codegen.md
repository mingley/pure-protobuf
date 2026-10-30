# Code Generation and Custom Stubs

Generate Rust messages and service stubs with `pbrs::codegen` in `build.rs`,
or use `protoc-gen-pbrs` in an existing compiler pipeline. Inputs can be
`.proto` files or a checked descriptor set. Native gRPC stubs are the default;
Tonic stubs and messages-only output are explicit options.

---

## 1. Using `build.rs` (Recommended)

For most crates, generate code from `build.rs`. Add `pbrs` as a build
dependency and add the runtime crates your generated code will use. The
example uses the published adapter preview; this checkout targets an unreleased
`0.1.0-alpha.3` candidate:

```toml
[dependencies]
pbrs = "0.2"
pbrs-grpc = "0.1.0-alpha.2" # Published preview

[build-dependencies]
pbrs = "0.2"
```

### Cargo features

The default `pbrs` feature set is source-compatible with earlier releases: it
includes `codegen`, `reflect`, `json`, `text`, and the bundled conformance
gencode. Use that default when you run `pbrs::codegen` from `build.rs` and also
compile generated code in the same crate.

For split runtime/build setups, keep code generation on the build-dependency and
trim the runtime dependency:

```toml
[dependencies]
pbrs = { version = "0.2", default-features = false, features = ["json", "text"] }

[build-dependencies]
pbrs = { version = "0.2", default-features = false, features = ["codegen"] }
```

`json` and `text` imply `reflect`, which current generated files need for
descriptor-backed format helpers. A runtime crate that only uses the core
binary wire API can depend on `pbrs = { version = "0.2", default-features =
false }`; that profile does not compile the generator, descriptor pool, JSON,
text, or bundled conformance gencode.

Codegen can also emit a smaller accessor-only surface:

```rust
pbrs::codegen::Config::new()
    .emit_reflection(false)
    .emit_json(false)
    .emit_text(false)
    .compile_protos(&["proto/service.proto"], &["proto"])?;
```

The plugin equivalents are:

```bash
--pbrs_opt=emit_reflection=false,emit_json=false,emit_text=false
```

Defaults stay source-compatible: reflection bytes plus JSON and text methods
are still emitted unless you opt out. `no_reflect=true` remains supported as a
shorthand for disabling reflection and, unless separately requested, generated
JSON/text methods.

### Shared descriptor storage

With `Config::shared_descriptor_set(true)` (plugin
`shared_descriptor_set=true`), reflective multi-file generation emits one
`__pbrs_shared_descriptors.rs` helper. The root registry includes its private
`__pbrs_shared_descriptors` module; each file retains the public
`FILE_DESCRIPTOR_SET` constant as an alias and obtains the same lazily initialized
`Arc<DescriptorPool>` from that helper. Include the registry at crate root:

```rust
include!(concat!(env!("OUT_DIR"), "/mod.rs"));
```

This option is off by default and has no environment-variable equivalent.
Single-file requests remain byte-identical and self-contained, including with
the option enabled. Reflection-free generation emits no helper or descriptor
bytes. For multi-file flat includes, supply the helper module at crate root:

```rust
mod __pbrs_shared_descriptors {
    include!(concat!(env!("OUT_DIR"), "/__pbrs_shared_descriptors.rs"));
}
```

Active sharing requires `include_file` to name a registry file at the output
root (for example, `registry.rs`); nested registry paths are rejected. The
helper file and module names are reserved in this mode; conflicting output
paths, root packages, or emitted root items fail generation. The separate `shared_pool=true` option
uses the bundled conformance pool and cannot be combined with active shared
descriptor-set generation. This option changes descriptor storage only; wire
formats, message APIs, JSON, and text behavior remain the same.

### Owned nested message fields

Use `owned_message_fields(true)` when applications read or mutate most of a
nested message tree:

```rust
pbrs::codegen::Config::new()
    .owned_message_fields(true)
    .compile_protos(&["proto/service.proto"], &["proto"])?;
```

This profile constructs singular nested messages during parsing. The default
is `false`: nested messages are validated first and constructed when accessed,
which can save work and memory when they stay unread. Newly generated lazy
getters reuse the initial validation result instead of validating descendants
again. Regenerate bindings to enable this path; older bindings retain their
checked fallback. Children remain deferred until accessed.

Both profiles keep message accessors, protobuf semantics, formats, validation
and recursion limits. They differ in allocation, partial reads, mutation and
encoding costs. Compare the [latest measurements](../evidence/decode-and-map-speed-20261027/README.md)
for the application's workload. The lazy validation flag can increase each
child allocation by eight bytes; it does not construct the entire subtree on
the first getter.

The option changes generated storage layout. Imported or extern-mapped child
types keep their own layout; generate those schemas with the same profile to
construct the complete tree during parsing. Owned children serialize from their
fields, so untouched child wire may be canonicalized instead of reused.
Its compact size cache leaves sizes at least `u32::MAX` uncached; serialization
still computes and returns their full `u64` sizes.
The plugin equivalent is `owned_message_fields=true`; there is no environment
variable for this option.

### Message names, type URLs and enum prefixes

Enable `pbrs::Name` implementations when generating messages that will be
packed into `Any`:

```rust
pbrs::codegen::Config::new()
    .enable_type_names()
    .type_name_domain(["."], "types.example")
    .retain_enum_prefix()
    .compile_protos(&["proto/service.proto"], &["proto"])?;
```

Type names and prefix retention are independent, opt-in settings. Existing
`MessageName` implementations and `FULL_NAME` constants remain available
without either option. `retain_enum_prefix()` exposes `STATE_READY` as
`State::StateReady`, instead of the default `State::Ready`. Numeric values,
unknown values and protobuf spelling stay unchanged. Aliases remain available
with the same numeric values.

For `example.Outer.Inner`, `Name::NAME` is `"Inner"`, `Name::PACKAGE` is
`"example"`, and `Name::full_name()` returns `"example.Outer.Inner"`.
Without a matching domain rule, `Name::type_url()` returns
`"/example.Outer.Inner"`, matching Prost's generated default. The example
above produces `"types.example/example.Outer.Inner"`. Setting a domain does
not enable trait emission by itself.

Domain selectors use the protobuf name, before any Rust identifier changes.
Selection tries the exact fully-qualified name, suffixes, package or parent
prefixes, then `.`. For example, `.example.Outer.Inner` selects one message,
`Inner` matches a suffix, and `.example` matches that package. The first rule
for the selected matcher wins. Domains are used verbatim, including a trailing
slash. The plugin equivalents are:

```bash
--pbrs_opt=enable_type_names=true,retain_enum_prefix=true,type_name_domain=.=types.example
```

The shared runtime WKT types already implement `Name` with the
`type.googleapis.com` domain. Typed `Any` conversion uses the message's URL:

```rust
use pbrs::wkt::{Any, Timestamp};

let mut timestamp = Timestamp::new();
timestamp.set_seconds(42);
let packed = Any::from_msg(&timestamp)?;
let decoded = packed.to_msg::<Timestamp>()?;
assert_eq!(decoded, timestamp);
```

These helpers require the runtime's default-enabled `conformance` feature,
which provides `pbrs::wkt`. `to_msg` checks the full name after the final slash
and parses the payload; it accepts a different domain for the same type.
For the separate gRPC helper type, use
`pbrs_grpc::pb::Any::pack_with(T::type_url(), &message)`.

Imported and extern-mapped types keep their own trait implementations and
URLs. Generate imported types with `enable_type_names()` when their `Name`
implementation is needed; a rule in the importing configuration does not
change an external type's implementation.

### Basic Compilation

Compile your `.proto` files during the build:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::compile_protos(&["proto/service.proto"], &["proto"])?;
    Ok(())
}
```

Then include the generated Rust file from your crate:

```rust
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/service.rs"));
}
```

### Generating from a checked descriptor set

Use a descriptor set when another build step already compiled the `.proto`
graph. Include imports when creating the descriptor:

```bash
protoc -I proto --include_imports --descriptor_set_out=proto/service.fds proto/service.proto
```

The application `build.rs` can then generate the same layout without `protoc`
on its `PATH`:

```rust
use pbrs::codegen::{Config, Stubs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Config::new()
        .out_dir(std::env::var("OUT_DIR")?)
        .stubs(Stubs::None)
        .compile_descriptor_set("proto/service.fds", &["service.proto"], &["proto"])?;
    Ok(())
}
```

Use `.emit_kernel_stubs(true)` or `.emit_tonic_stubs(true)` instead of
`.stubs(Stubs::None)` when you need native or Tonic service code. Descriptor
targets are include-relative file names.

Important boundaries:

- Missing or malformed descriptors fail explicitly.
- The descriptor and available imported `.proto` sources trigger rebuilds.
- This generator path is `protoc`-free.
- In this checkout, a **cold** build of `pbrs-grpc` or `protobuf-tonic` is also
  `protoc`-free because both adapters use checked descriptor sets for their own
  build scripts.
- Published adapters from `0.1.0-alpha.2` on are `protoc`-free too. The older
  `0.1.0-alpha.1` archives still require `protoc`.
- Creating or updating an application's descriptor set still requires a
  compiler in an earlier stage.
- Direct `.proto` compilation and the `protoc-gen-pbrs` plugin still require
  `protoc`.
- No full-profile Rust-only `.proto` frontend is approved. See the
  [candidate review](../rust-frontend.md) and the
  [support matrix](../../README.md#support-matrix).

---

## 2. Configuring Stub Flavours

By default, `pbrs::codegen::compile_protos` emits native `pbrs-grpc` stubs
(`Stubs::Kernel`). Use `pbrs::codegen::Config` when you want a different
stub shape:

```rust
use pbrs::codegen::{Config, Stubs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Config::new()
        // Default: native pbrs-grpc FooClient / FooServer stubs
        .emit_kernel_stubs(true)
        // Or for tonic services:
        // .emit_tonic_stubs(true)
        // Or messages only (no service stubs):
        // .stubs(Stubs::None)
        .compile_protos(&["proto/service.proto"], &["proto"])?;
    Ok(())
}
```

### Stub Generation Summary

Pick one stub mode per generated service surface:

| Flavour | Builder Setting | Generated Code | Target Stack |
|---|---|---|---|
| **Native Kernel** | `.emit_kernel_stubs(true)` (Default) | `Foo`, `FooServer`, `FooClient` | `pbrs-grpc` |
| **Tonic Adapter** | `.emit_tonic_stubs(true)` | `#[tonic::async_trait]` stubs | `tonic` 0.14+ via `protobuf-tonic` |
| **Tonic-shaped Native** | `.tonic_compat(true)` | Tonic-shaped signatures over native clients/servers | `pbrs-grpc` |
| **Messages Only** | `.stubs(Stubs::None)` | Structs, enums, and WKT traits only | Core `pbrs` |

Generated tonic stubs also reference `http` and `tokio-stream` directly. A
consumer using `.emit_tonic_stubs(true)` needs those direct dependencies, along
with `tonic` and `protobuf-tonic`:

```toml
[dependencies]
pbrs = "0.2"
protobuf-tonic = "0.1.0-alpha.2" # Published preview
tonic = { version = "0.14", default-features = false, features = ["transport", "codegen", "router"] }
http = "1"
tokio-stream = "0.1"
```

---

## 3. Multi-File Protos and Dependency Handling

If a `.proto` file imports other local `.proto` files, point codegen at every
include root and decide whether imported non-Well-Known-Type definitions should
be emitted:

```rust
Config::new()
    .out_dir("src/gen")
    // Include all imported non-WKT definitions in the generated output
    .emit_deps(true)
    // Optional: disable emission of Well-Known Types if handled separately
    .no_wkt(false)
    .compile_protos(
        &["proto/main.proto"],
        &["proto", "vendor/shared_protos"],
    )?;
```

Alternatively, set `PURE_PROTOBUF_EMIT_DEPS=1` in the environment.

### Shared Google protobuf types

Multi-file requests share imported `google.protobuf` definitions by default.
For example, option schemas importing `google/protobuf/descriptor.proto` emit
the descriptor types once in `google/protobuf/descriptor.rs`. Include the
generated `mod.rs` at crate root to make the shared package paths available:

```rust
include!(concat!(env!("OUT_DIR"), "/mod.rs"));
```

A single input keeps its imported WKT definitions inside its own flat file,
so the earlier nested `pub mod pb { include!(...); }` pattern still works.
For multiple inputs, separate flat includes need the generated package paths;
use the root registry or generate each self-contained file separately.

To reuse the runtime's standard WKT bindings explicitly:

```rust
Config::new()
    .extern_path(".google.protobuf", "::pbrs::wkt")
    .compile_protos(&["proto/event.proto"], &["proto"])?;
```

This facade includes the standard Any, Timestamp, Duration, Struct/Value,
FieldMask, Empty and wrapper schemas, with owned and proxy types. It requires
the runtime `conformance` feature, enabled by default. A trimmed runtime must
enable that feature before using the facade. It does not provide tooling
schemas such as `descriptor.proto`: those mappings fail generation clearly.
Generate them in the default multi-file request or map a separately generated
module instead. Do not also request externally mapped `google.protobuf`
sources as generation targets; remove the target or its mapping.

### Descriptor sharing and lazy loading

Every generated file embeds the same `FILE_DESCRIPTOR_SET` bytes, so any
single file keeps compiling standalone via `include!`. Sharing happens at
runtime instead of in the emitted source:

- Each file's `generated_pool()` initializes on first reflection use, so
  binaries that never touch descriptors never parse them.
- `DescriptorPool::from_file_descriptor_set` parses identical bytes once per
  process and shares the parsed pool across all generated files that embed
  them. Repeat loads are cache hits, not re-parses.
- Sharing is invisible to callers: `register_message` and `register_enum`
  copy on write, malformed bytes still fail eagerly, and the `Debug` shape is
  unchanged.

No configuration is needed. The accessor-only profile
(`emit_reflection=false,emit_json=false,emit_text=false`) skips embedding
descriptor bytes entirely; see
[GN-03 evidence](../evidence/gn-03-descriptor-sharing.md) for measurements.

---

## 4. Standalone CLI Plugin (`protoc-gen-pbrs`)

Use the standalone plugin when your build already calls `protoc` directly.

### Installation

Install the plugin binary from this repository:

```bash
cargo install --path . --bin protoc-gen-pbrs
```

### Invocation

Put `protoc-gen-pbrs` on your `$PATH`, then pass `--pbrs_out` and
`--pbrs_opt` to `protoc`:

```bash
protoc \
  --pbrs_out=./gen \
  --pbrs_opt=stubs=kernel,emit_deps=true \
  --proto_path=./proto \
  ./proto/service.proto
```

Supported `--pbrs_opt` options:

- `stubs=kernel`: Emit native `pbrs-grpc` stubs.
- `stubs=tonic`: Emit `tonic` stubs.
- `stubs=none`: Emit messages only.
- `emit_deps=true`: Emit imported non-WKT definitions.
- `owned_message_fields=true`: Construct singular nested messages during parsing.
- `no_wkt=true`: Suppress Well-Known Types emission.

---

## Parity with tonic-build and prost-build

`pbrs::codegen::Config` follows the `prost-build`/`tonic-build` builder shape
where it applies to pbrs's message model and native transport. Single-file
defaults preserve standalone inclusion. Multi-file default WKT ownership
follows the sharing rule above; explicit external mappings remain available.

| tonic-build / prost-build option | pbrs equivalent | Status |
|---|---|---|
| `out_dir` | `Config::out_dir` | Supported. |
| `compile_protos(protos, includes)` | `Config::compile_protos` / `codegen::compile_protos` | Supported; invokes `protoc` and tracks imports. |
| Descriptor input / `compile_fds` | `Config::compile_descriptor_set` | Supported; no `protoc` needed when the descriptor set is checked in. |
| `file_descriptor_set_path` | `Config::file_descriptor_set_path` | Supported; writes the compiled descriptor bytes. |
| `protoc_arg` | `Config::protoc_arg` | Supported for direct `.proto` compilation. |
| `emit_rerun_if_changed` | `Config::emit_rerun_if_changed` | Supported; defaults on. |
| Shared embedded metadata | `Config::shared_descriptor_set` / `--pbrs_opt=shared_descriptor_set=true` | Opt-in for reflective multi-file output using its crate-root registry; defaults off. |
| `include_file` | `Config::include_file` | Supported; defaults to `mod.rs`. |
| `extern_path` | `Config::extern_path` / `--pbrs_opt=extern_path=.pkg=crate::pkg` | Supported with longest-prefix matching. |
| `compile_well_known_types` | `Config::compile_well_known_types`; inverse of `Config::no_wkt` | Supported. Default is to generate pbrs WKTs. |
| `disable_comments` | `Config::disable_comments` / `--pbrs_opt=disable_comments=true` | Supported for source comments; synthetic API docs remain. |
| `skip_debug` | `Config::skip_debug` / `--pbrs_opt=skip_debug=true` | Supported for generated message storage. |
| `type_attribute` | `Config::type_attribute` / `--pbrs_opt=type_attribute=.pkg.Type=#[...]` | Supported for exact `.`-prefixed full names. |
| `message_attribute` | `Config::message_attribute` | Supported for exact message full names. |
| `enum_attribute` | `Config::enum_attribute` | Supported for exact enum full names. |
| `field_attribute` | `Config::field_attribute` | Supported for exact field full names such as `.pkg.Msg.field`. |
| `build_client` | `Config::build_client` / `--pbrs_opt=build_client=false` | Supported for native and Tonic adapter stubs. |
| `build_server` | `Config::build_server` / `--pbrs_opt=build_server=false` | Supported for native and Tonic adapter stubs. |
| `client_attribute` | `Config::client_attribute` | Supported on generated client structs, keyed by service full name. |
| `server_attribute` | `Config::server_attribute` | Supported on generated server structs, keyed by service full name. |
| `client_mod_attribute` | `Config::client_mod_attribute` | Accepted as an alias for `client_attribute`; pbrs does not generate separate client modules. |
| `server_mod_attribute` | `Config::server_mod_attribute` | Accepted as an alias for `server_attribute`; pbrs does not generate separate server modules. |
| `generate_default_stubs` | `Config::generate_default_stubs` | Supported for native stubs; defaults on and returns `UNIMPLEMENTED`. |
| `use_arc_self` | `Config::use_arc_self` | Supported for native service trait receivers. |
| `codec_path` | `Config::codec_path` | Supported for Tonic adapter stubs; defaults to `protobuf_tonic::ProtobufCodec`. |
| Tonic-shaped native mode | `Config::tonic_compat(true)` / `--pbrs_opt=stubs=compat` | Supported; mirrors Tonic handler/client signatures over `pbrs-grpc`. |
| `build_transport` | Not generated by pbrs | Not applicable. Native `pbrs-grpc` clients use `Channel` dialers; Tonic transport users keep Tonic's own builders. |
| Client/server proto-path customization | Generated full gRPC paths from descriptors | Not yet exposed as a separate override; use descriptor/package/method names as source of truth. |

---

<a id="manual-service"></a>
## 5. Writing a Service Without Codegen

For custom routing, implement `Service` directly and dispatch through `Rpc`.
The example reuses bundled protobuf messages but writes the service dispatch
by hand. Message types must implement `CodecMessage`; a plain `Vec<u8>` is
not automatically a protobuf message.

```rust
use pbrs_grpc::hello::{HelloReply, HelloRequest};
use pbrs_grpc::{Request, Response, Rpc, Service};

#[derive(Clone)]
struct RawEchoService;

impl Service for RawEchoService {
    const NAME: &'static str = "echo.Echo";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Echo" => {
                rpc.unary(|req: Request<HelloRequest>| async move {
                    let mut reply = HelloReply::new();
                    reply.set_message(req.get_ref().name());
                    Ok(Response::new(reply))
                }).await;
            }
            _ => rpc.unimplemented(),
        }
    }
}
```
