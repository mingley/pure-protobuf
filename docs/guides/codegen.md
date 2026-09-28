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
