# Code Generation and Custom Stubs

Use this guide to generate Rust messages and service stubs with
`pbrs::codegen` or the `protoc-gen-pbrs` plugin. You need `.proto` files or a
checked descriptor set. Bottom line: use `build.rs` for normal crates, choose
native or Tonic stubs explicitly, and use descriptor sets when an earlier stage
already ran `protoc`.

---

## 1. Using `build.rs` (Recommended)

For most crates, generate code from `build.rs`. Add `pbrs` as a build
dependency and add the runtime crates your generated code will use:

```toml
[dependencies]
pbrs = "0.2"
pbrs-grpc = "0.1.0-alpha.2"

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
protobuf-tonic = "0.1.0-alpha.2"
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
- `no_wkt=true`: Suppress Well-Known Types emission.

---

## Parity with tonic-build and prost-build

`pbrs::codegen::Config` follows the `prost-build`/`tonic-build` builder shape
where it applies to pbrs's message model and native transport. Defaults preserve
the existing generated bytes: the new controls below only change output when
explicitly enabled.

| tonic-build / prost-build option | pbrs equivalent | Status |
|---|---|---|
| `out_dir` | `Config::out_dir` | Supported. |
| `compile_protos(protos, includes)` | `Config::compile_protos` / `codegen::compile_protos` | Supported; invokes `protoc` and tracks imports. |
| Descriptor input / `compile_fds` | `Config::compile_descriptor_set` | Supported; no `protoc` needed when the descriptor set is checked in. |
| `file_descriptor_set_path` | `Config::file_descriptor_set_path` | Supported; writes the compiled descriptor bytes. |
| `protoc_arg` | `Config::protoc_arg` | Supported for direct `.proto` compilation. |
| `emit_rerun_if_changed` | `Config::emit_rerun_if_changed` | Supported; defaults on. |
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

For dynamic proxies, gateways, or custom routing, implement `Service` directly
instead of using generated stubs. This gives you raw byte frames, interceptor
context, and path-based dispatch.

```rust
use pbrs_grpc::{Incoming, Request, Response, Rpc, Service, Status};
use std::future::Future;
use std::pin::Pin;

#[derive(Clone)]
struct RawEchoService;

impl Service for RawEchoService {
    fn call(
        &self,
        rpc: Rpc,
    ) -> Pin<Box<dyn Future<Output = Result<(), Status>> + Send + 'static>> {
        Box::pin(async move {
            match rpc.path() {
                "/echo.Echo/Echo" => {
                    rpc.unary(|req: Request<Vec<u8>>| async move {
                        let payload = req.into_inner();
                        Ok(Response::new(payload))
                    }).await
                }
                _ => {
                    rpc.unimplemented().await
                }
            }
        })
    }
}
```
